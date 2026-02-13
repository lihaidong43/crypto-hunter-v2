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

/// 资金费率缓存
#[derive(Clone, Default)]
struct FundingRateCache {
    funding_rate: Option<Decimal>,
    next_funding_time: Option<DateTime<Utc>>,
    funding_interval_hours: Option<i32>,
    rate_limit_upper: Option<Decimal>,
    rate_limit_lower: Option<Decimal>,
}

/// Binance WebSocket适配器
pub struct BinanceWebSocketAdapter {
    exchange_type: ExchangeType,
    market_type: MarketType,
    stream: Option<Arc<Mutex<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_symbols: Vec<String>,
    /// 资金费率缓存 (仅 Futures 使用) - 使用 std::sync::Mutex 避免异步借用问题
    funding_cache: Arc<std::sync::Mutex<HashMap<String, FundingRateCache>>>,
    /// 上次刷新 fundingInfo 的时间
    last_funding_info_fetch: Option<std::time::Instant>,
}

impl BinanceWebSocketAdapter {
    pub fn new(market_type: MarketType) -> Self {
        Self {
            exchange_type: ExchangeType::Binance,
            market_type,
            stream: None,
            subscribed_symbols: Vec::new(),
            funding_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_funding_info_fetch: None,
        }
    }

    fn get_ws_url(&self) -> String {
        match self.market_type {
            MarketType::Spot => "wss://stream.binance.com:9443/stream".to_string(),
            MarketType::Futures => "wss://fstream.binance.com/stream".to_string(),
        }
    }

    fn parse_ticker_message(&self, msg: &str) -> Result<MarketSnapshot> {
        // Binance futures 的 24hrTicker 有时不带 b/a（日志里已出现），用 Value 提取更稳妥
        let v: serde_json::Value =
            serde_json::from_str(msg).context("Failed to parse Binance ticker json")?;

        // 兼容组合流 {"stream": "...", "data": {...}} 和直接 {...}
        let data_v = v.get("data").cloned().unwrap_or(v);

        let symbol = data_v
            .get("s")
            .and_then(|x| x.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let bid = data_v
            .get("b")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let ask = data_v
            .get("a")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let last = data_v
            .get("c")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let vol = data_v
            .get("v")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        // 从缓存获取资金费率及相关信息（仅 Futures）
        let (funding_rate, next_funding_time, funding_interval_hours, rate_limit_upper, rate_limit_lower) = 
            if self.market_type == MarketType::Futures {
                if let Ok(cache_guard) = self.funding_cache.lock() {
                    if let Some(cache) = cache_guard.get(&symbol) {
                        (cache.funding_rate, cache.next_funding_time, 
                         cache.funding_interval_hours, cache.rate_limit_upper, cache.rate_limit_lower)
                    } else {
                        (None, None, None, None, None)
                    }
                } else {
                    (None, None, None, None, None)
                }
            } else {
                (None, None, None, None, None)
            };

        Ok(MarketSnapshot {
            snapshot_time: Utc::now(),
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

    /// 解析 markPrice 消息并更新资金费率缓存
    fn parse_mark_price_message(&self, msg: &str) -> Result<()> {
        let v: serde_json::Value =
            serde_json::from_str(msg).context("Failed to parse Binance markPrice json")?;

        let data_v = v.get("data").cloned().unwrap_or(v);

        let symbol = data_v
            .get("s")
            .and_then(|x| x.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let funding_rate: Option<Decimal> = data_v
            .get("r")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        let next_funding_time: Option<DateTime<Utc>> = data_v
            .get("T")
            .and_then(|x| x.as_i64())
            .and_then(|ts| DateTime::from_timestamp_millis(ts));

        if funding_rate.is_some() || next_funding_time.is_some() {
            if let Ok(mut cache_guard) = self.funding_cache.lock() {
                let cache = cache_guard.entry(symbol).or_default();
                if let Some(rate) = funding_rate {
                    cache.funding_rate = Some(rate);
                }
                if let Some(time) = next_funding_time {
                    cache.next_funding_time = Some(time);
                }
            }
        }

        Ok(())
    }

    /// 从 REST API 获取资金费率静态信息（fundingIntervalHours, rateLimitCap/Floor）
    /// 在订阅后调用一次，预加载这些不会频繁变化的信息到缓存
    async fn fetch_funding_info(&self, symbols: &[String]) -> Result<()> {
        if self.market_type != MarketType::Futures {
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let resp = client
            .get("https://fapi.binance.com/fapi/v1/fundingInfo")
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::warn!("Binance fundingInfo API 请求失败: {}", resp.status());
            return Ok(());
        }

        let data: Vec<serde_json::Value> = resp.json().await?;
        
        // 构建 symbol 集合用于快速查找
        let symbol_set: std::collections::HashSet<&str> = symbols.iter().map(|s| s.as_str()).collect();

        if let Ok(mut cache_guard) = self.funding_cache.lock() {
            for item in data {
                let symbol = item.get("symbol").and_then(|x| x.as_str()).unwrap_or("");
                if !symbol_set.contains(symbol) {
                    continue;
                }

                let funding_interval_hours: Option<i32> = item
                    .get("fundingIntervalHours")
                    .and_then(|x| x.as_i64())
                    .map(|v| v as i32);

                let rate_limit_upper: Option<Decimal> = item
                    .get("adjustedFundingRateCap")
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());

                let rate_limit_lower: Option<Decimal> = item
                    .get("adjustedFundingRateFloor")
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());

                let cache = cache_guard.entry(symbol.to_string()).or_default();
                cache.funding_interval_hours = funding_interval_hours;
                cache.rate_limit_upper = rate_limit_upper;
                cache.rate_limit_lower = rate_limit_lower;
            }
        }

        tracing::debug!("Binance fundingInfo 预加载完成");
        Ok(())
    }
}

#[async_trait]
impl WebSocketAdapter for BinanceWebSocketAdapter {
    fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self.get_ws_url();
        tracing::debug!("连接 Binance WebSocket: {}", url);
        
        // 添加连接超时（默认 20 秒，避免网络抖动导致频繁误判超时）
        let connect_future = connect_websocket_with_proxy(&url);
        match tokio::time::timeout(tokio::time::Duration::from_secs(20), connect_future).await {
            Ok(Ok((ws_stream, _))) => {
                self.stream = Some(Arc::new(Mutex::new(ws_stream)));
                tracing::debug!("Binance WS连接: {}", url);
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to connect to Binance WebSocket at {}: {}", url, e);
                tracing::error!("{}", error_msg);
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("Binance WebSocket connection timeout after 20s: {}", url);
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

        // Binance使用组合流订阅多个symbol
        // 注意：Binance 限制每个连接最多订阅 1024 个流，且单次请求消息不能过大
        // Futures 同时订阅 ticker 和 markPrice 以获取资金费率
        let max_symbols = if market_type == MarketType::Futures { 500 } else { 1024 };
        
        let mut streams: Vec<String> = Vec::new();
        for symbol in symbols.iter().take(max_symbols) {
            let symbol_lower = symbol.to_lowercase();
            streams.push(format!("{}@ticker", symbol_lower));
            // Futures 额外订阅 markPrice 获取资金费率
            if market_type == MarketType::Futures {
                streams.push(format!("{}@markPrice", symbol_lower));
            }
        }

        if streams.is_empty() {
            return Err(anyhow::anyhow!("No valid streams to subscribe"));
        }

        let total = streams.len();
        let batch_size = 20usize; // 每批 20 个，避免消息过长和限流

        tracing::debug!("Binance 订阅 {} 个流 (batch_size={})", total, batch_size);

        for (batch_idx, chunk) in streams.chunks(batch_size).enumerate() {
            let subscribe_msg = json!({
                "method": "SUBSCRIBE",
                "params": chunk,
                "id": batch_idx + 1
            });

            if let Some(stream) = &self.stream {
                let mut ws = stream.lock().await;
                if let Err(e) = ws.send(Message::Text(subscribe_msg.to_string())).await {
                    tracing::warn!("Binance 订阅批次 {} 失败: {}", batch_idx + 1, e);
                    return Err(anyhow::anyhow!(e).context("Failed to send subscribe message"));
                }
            }

            let sent = std::cmp::min((batch_idx + 1) * batch_size, total);
            tracing::debug!("Binance 订阅批次 {}/{} ({}/{})", batch_idx + 1, (total + batch_size - 1) / batch_size, sent, total);

            // 节流，避免触发限流（Binance 对订阅频率敏感）
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        }

        tracing::debug!("Binance 订阅完成 (共 {} 流)", total);

        // Futures: 预加载资金费率静态信息（fundingIntervalHours, rateLimitCap/Floor）
        if market_type == MarketType::Futures {
            if let Err(e) = self.fetch_funding_info(symbols).await {
                tracing::warn!("Binance fundingInfo 预加载失败: {}", e);
            }
            self.last_funding_info_fetch = Some(std::time::Instant::now());
        }

        Ok(())
    }

    async fn receive_message(&mut self) -> Result<MarketSnapshot> {
        if let Some(stream) = &self.stream {
            let mut ws = stream.lock().await;
            loop {
                if let Some(msg) = ws.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            tracing::debug!("Binance 收到 WebSocket 消息: {}", text.chars().take(200).collect::<String>());
                            
                            // 检查是否是订阅确认消息，忽略
                            // Binance 订阅确认格式: {"result":null,"id":1} 或 {"id":1,"result":null}
                            if (text.contains("\"result\"") || text.contains("\"id\"")) 
                                && !text.contains("\"stream\"") 
                                && !text.contains("@ticker")
                                && !text.contains("@markPrice")
                                && !text.contains("24hrTicker") {
                                tracing::debug!("Binance 忽略订阅确认消息: {}", text.chars().take(100).collect::<String>());
                                continue;
                            }
                            
                            // 处理 markPrice 消息（更新资金费率缓存，不返回快照）
                            if text.contains("@markPrice") {
                                if let Err(e) = self.parse_mark_price_message(&text) {
                                    tracing::debug!("Binance 解析 markPrice 失败: {}", e);
                                }
                                
                                // 每小时刷新一次 fundingInfo
                                if self.market_type == MarketType::Futures {
                                    let should_refresh = self.last_funding_info_fetch
                                        .map(|t| t.elapsed() >= std::time::Duration::from_secs(3600))
                                        .unwrap_or(true);
                                    
                                    if should_refresh {
                                        // 释放 ws 锁，执行 REST API 调用
                                        drop(ws);
                                        tracing::debug!("Binance 开始刷新 fundingInfo (每小时)");
                                        if let Err(e) = self.fetch_funding_info(&self.subscribed_symbols.clone()).await {
                                            tracing::warn!("Binance fundingInfo 刷新失败: {}", e);
                                        }
                                        self.last_funding_info_fetch = Some(std::time::Instant::now());
                                        // 重新获取锁
                                        ws = stream.lock().await;
                                    }
                                }
                                
                                continue;
                            }
                            
                            // 检查是否是ticker消息
                            // 组合流消息格式: {"stream":"btcusdt@ticker","data":{...}}
                            if text.contains("\"stream\"") && text.contains("@ticker") {
                                tracing::trace!("Binance ticker: {}", text.chars().take(100).collect::<String>());
                                match self.parse_ticker_message(&text) {
                                    Ok(s) => return Ok(s),
                                    Err(e) => {
                                        tracing::warn!("Binance 解析 ticker 消息失败: {}", e);
                                        continue;
                                    }
                                }
                            } else if text.contains("\"e\"") && text.contains("24hrTicker") {
                                // 直接ticker消息格式（较少见）
                                tracing::trace!("Binance direct ticker: {}", text.chars().take(100).collect::<String>());
                                match self.parse_ticker_message(&text) {
                                    Ok(s) => return Ok(s),
                                    Err(e) => {
                                        tracing::warn!("Binance 解析直接 ticker 消息失败: {}", e);
                                        continue;
                                    }
                                }
                            } else {
                                // 记录其他消息类型，便于调试
                                tracing::debug!("Binance 收到非 ticker 消息: {}", text.chars().take(200).collect::<String>());
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
struct BinanceTickerMessage {
    data: BinanceTickerData,
}

#[derive(Debug, Deserialize)]
struct BinanceTickerData {
    #[serde(rename = "s")]
    s: String, // symbol
    #[serde(rename = "b")]
    b: String, // bid price
    #[serde(rename = "a")]
    a: String, // ask price
    #[serde(rename = "c")]
    c: String, // last price
    #[serde(rename = "v")]
    v: String, // volume
}

#[derive(Debug, Deserialize)]
struct BinanceStreamMessage {
    stream: String,
    data: BinanceTickerData, // JSON object
}
