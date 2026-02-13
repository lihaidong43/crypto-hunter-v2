use crate::exchange::ws_adapter::WebSocketAdapter;
use crate::models::exchange::{ExchangeType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use crate::exchange::ws_proxy::connect_websocket_with_proxy;
use rust_decimal::Decimal;
use tracing;

/// 资金费率缓存
#[derive(Clone, Default)]
struct FundingRateCache {
    funding_rate: Option<Decimal>,
    next_funding_time: Option<DateTime<Utc>>,
    funding_interval_hours: Option<i32>,
    rate_limit_upper: Option<Decimal>,  // fundingCap
    rate_limit_lower: Option<Decimal>,  // Bybit 只有 fundingCap，lower 取负值
}

/// Bybit WebSocket适配器
pub struct BybitWebSocketAdapter {
    exchange_type: ExchangeType,
    market_type: MarketType,
    stream: Option<Arc<Mutex<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_symbols: Vec<String>,
    /// 资金费率缓存 (仅 Futures 使用)
    funding_cache: Arc<std::sync::Mutex<HashMap<String, FundingRateCache>>>,
}

impl BybitWebSocketAdapter {
    pub fn new(market_type: MarketType) -> Self {
        Self {
            exchange_type: ExchangeType::Bybit,
            market_type,
            stream: None,
            subscribed_symbols: Vec::new(),
            funding_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    fn get_ws_url(&self) -> String {
        match self.market_type {
            MarketType::Spot => "wss://stream.bybit.com/v5/public/spot".to_string(),
            MarketType::Futures => "wss://stream.bybit.com/v5/public/linear".to_string(),
        }
    }

    fn parse_ticker_message(&self, msg: &str) -> Result<MarketSnapshot> {
        tracing::debug!(
            "Bybit 解析 ticker 消息: {}",
            msg.chars().take(200).collect::<String>()
        );

        // Bybit 返回的消息类型比较多（订阅 ack / snapshot / delta），用 Value 更稳妥
        let v: serde_json::Value =
            serde_json::from_str(msg).context("Failed to parse Bybit WebSocket message")?;

        let topic = v
            .get("topic")
            .and_then(|x| x.as_str())
            .unwrap_or_default();
        if !topic.starts_with("tickers.") {
            return Err(anyhow::anyhow!("Unexpected topic: {}", topic));
        }

        // data 可能是对象或数组
        let data_v = v.get("data").cloned().unwrap_or(serde_json::Value::Null);
        let obj = match &data_v {
            serde_json::Value::Object(_) => data_v,
            serde_json::Value::Array(arr) => arr.first().cloned().unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Null,
        };
        if obj.is_null() {
            return Err(anyhow::anyhow!("Empty ticker data"));
        }

        let snapshot_time = Utc::now();

        let symbol = obj
            .get("symbol")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .or_else(|| topic.strip_prefix("tickers.").map(|s| s.to_string()))
            .unwrap_or_else(|| "UNKNOWN".to_string());

        let bid = obj
            .get("bid1Price")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let ask = obj
            .get("ask1Price")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let last = obj
            .get("lastPrice")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let vol = obj
            .get("volume24h")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        // Bybit Linear Perpetual: snapshot 消息包含完整的资金费率信息，delta 消息只包含变化字段
        // 策略：从 snapshot 提取并缓存，delta 使用缓存值
        let funding_rate_from_msg: Option<Decimal> = obj
            .get("fundingRate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        
        let next_funding_time_from_msg: Option<DateTime<Utc>> = obj
            .get("nextFundingTime")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|ts| DateTime::from_timestamp_millis(ts));

        // fundingIntervalHour: 资金费率结算间隔（小时）
        let funding_interval_from_msg: Option<i32> = obj
            .get("fundingIntervalHour")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        // fundingCap: 资金费率上限（Bybit 只有上限，下限取负值）
        let funding_cap_from_msg: Option<Decimal> = obj
            .get("fundingCap")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        // 更新或读取缓存
        let (funding_rate, next_funding_time, funding_interval_hours, rate_limit_upper, rate_limit_lower) = 
            if self.market_type == MarketType::Futures {
                if let Ok(mut cache_guard) = self.funding_cache.lock() {
                    // 如果消息中包含资金费率相关字段，更新缓存
                    let has_funding_info = funding_rate_from_msg.is_some() 
                        || next_funding_time_from_msg.is_some()
                        || funding_interval_from_msg.is_some()
                        || funding_cap_from_msg.is_some();
                    
                    if has_funding_info {
                        let cache = cache_guard.entry(symbol.clone()).or_default();
                        if let Some(rate) = funding_rate_from_msg {
                            cache.funding_rate = Some(rate);
                        }
                        if let Some(time) = next_funding_time_from_msg {
                            cache.next_funding_time = Some(time);
                        }
                        if let Some(interval) = funding_interval_from_msg {
                            cache.funding_interval_hours = Some(interval);
                        }
                        if let Some(cap) = funding_cap_from_msg {
                            cache.rate_limit_upper = Some(cap);
                            cache.rate_limit_lower = Some(-cap);  // Bybit 对称，下限取负值
                        }
                        (cache.funding_rate, cache.next_funding_time, cache.funding_interval_hours, 
                         cache.rate_limit_upper, cache.rate_limit_lower)
                    } else {
                        // delta 消息：从缓存读取
                        if let Some(cache) = cache_guard.get(&symbol) {
                            (cache.funding_rate, cache.next_funding_time, cache.funding_interval_hours,
                             cache.rate_limit_upper, cache.rate_limit_lower)
                        } else {
                            (None, None, None, None, None)
                        }
                    }
                } else {
                    (funding_rate_from_msg, next_funding_time_from_msg, funding_interval_from_msg,
                     funding_cap_from_msg, funding_cap_from_msg.map(|c| -c))
                }
            } else {
                (None, None, None, None, None)
            };

        Ok(MarketSnapshot {
            snapshot_time,
            exchange: self.exchange_type,
            symbol,
            market_type: self.market_type,
            bid_price: bid,
            ask_price: ask,
            last_price: last,
            volume_24h: vol,
            funding_rate,
            next_funding_time,
            funding_interval_hours,
            rate_limit_upper,
            rate_limit_lower,
        })
    }
}

#[async_trait]
impl WebSocketAdapter for BybitWebSocketAdapter {
    fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self.get_ws_url();
        tracing::debug!("连接 Bybit WebSocket: {}", url);
        
        // 添加连接超时（默认 20 秒，避免网络抖动导致频繁误判超时）
        let connect_future = connect_websocket_with_proxy(&url);
        match tokio::time::timeout(tokio::time::Duration::from_secs(20), connect_future).await {
            Ok(Ok((ws_stream, _))) => {
                self.stream = Some(Arc::new(Mutex::new(ws_stream)));
                tracing::debug!("Bybit WS连接: {}", url);
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to connect to Bybit WebSocket at {}: {}", url, e);
                tracing::error!("{}", error_msg);
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("Bybit WebSocket connection timeout after 20s: {}", url);
                tracing::error!("{}", error_msg);
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    async fn subscribe(&mut self, symbols: &[String], market_type: MarketType) -> Result<()> {
        if self.stream.is_none() {
            return Err(anyhow::anyhow!("WebSocket not connected"));
        }

        self.subscribed_symbols = symbols.to_vec();
        self.market_type = market_type;

        // Bybit 使用数组格式订阅多个交易对
        // 格式: tickers.{symbol}
        // 注意：Bybit 对单次订阅消息大小有限制，需要分批订阅
        let topics: Vec<String> = symbols
            .iter()
            .map(|symbol| format!("tickers.{}", symbol))
            .collect();

        let total = topics.len();
        let batch_size = 10usize; // 每批 10 个，避免消息过长

        tracing::debug!("Bybit 开始订阅 {} 个交易对 (batch_size={})", total, batch_size);

        for (batch_idx, chunk) in topics.chunks(batch_size).enumerate() {
            let subscribe_msg = json!({
                "op": "subscribe",
                "args": chunk
            });

            if let Some(stream) = &self.stream {
                let mut ws = stream.lock().await;
                if let Err(e) = ws.send(Message::Text(subscribe_msg.to_string())).await {
                    tracing::warn!("Bybit 订阅批次 {} 失败: {}", batch_idx + 1, e);
                    return Err(anyhow::anyhow!(e).context("Failed to send subscribe message"));
                }
            }

            let sent = std::cmp::min((batch_idx + 1) * batch_size, total);
            tracing::debug!("Bybit 订阅批次 {}/{} ({}/{})", batch_idx + 1, (total + batch_size - 1) / batch_size, sent, total);

            // 节流，避免触发限流
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        tracing::debug!("Bybit 订阅完成 (共 {} 交易对)", total);

        Ok(())
    }

    async fn receive_message(&mut self) -> Result<MarketSnapshot> {
        if let Some(stream) = &self.stream {
            let mut ws = stream.lock().await;
            loop {
                if let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            tracing::debug!("Bybit 收到 WebSocket 消息: {}", text.chars().take(200).collect::<String>());
                            
                            // 先快速忽略订阅确认
                            if text.contains("\"op\":\"subscribe\"") && text.contains("\"success\":true") {
                                tracing::debug!("Bybit 忽略订阅确认消息");
                                continue;
                            }

                            // 只处理 tickers.* 且包含 data 的消息
                            if text.contains("\"topic\":\"tickers.") && text.contains("\"data\"") {
                                match self.parse_ticker_message(&text) {
                                    Ok(s) => return Ok(s),
                                    Err(e) => {
                                        // Bybit 会推送多种 tickers.* 子类型（或字段缺失），解析失败不要打断接收循环
                                        tracing::debug!("Bybit 忽略无法解析的 tickers 消息: {}", e);
                                        continue;
                                    }
                                }
                            }
                        }
                        Ok(Message::Ping(data)) => {
                            ws.send(Message::Pong(data)).await?;
                        }
                        Ok(Message::Close(_)) => {
                            return Err(anyhow::anyhow!("WebSocket connection closed"));
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("WebSocket error: {}", e));
                        }
                        _ => {}
                    }
                } else {
                    return Err(anyhow::anyhow!("WebSocket stream ended"));
                }
            }
        } else {
            Err(anyhow::anyhow!("WebSocket not connected"))
        }
    }

    async fn reconnect(&mut self) -> Result<()> {
        let symbols = self.subscribed_symbols.clone();
        let market_type = self.market_type;
        
        self.stream = None;
        self.connect().await?;
        
        // 重新订阅
        if !symbols.is_empty() {
            self.subscribe(&symbols, market_type).await?;
        }
        
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}

#[derive(Debug, Deserialize)]
struct BybitWebSocketMessage {
    topic: String,
    data: BybitTickerData,
}

#[derive(Debug, Deserialize)]
struct BybitTickerData {
    symbol: String,
    #[serde(rename = "bid1Price")]
    bid1_price: String,
    #[serde(rename = "ask1Price")]
    ask1_price: String,
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "volume24h")]
    volume_24h: String,
}
