use crate::exchange::ws_adapter::WebSocketAdapter;
use crate::models::exchange::{ExchangeType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use crate::exchange::ws_proxy::connect_websocket_with_proxy;
use tracing;

/// 资金费率缓存
#[derive(Clone, Default)]
struct FundingRateCache {
    funding_rate: Option<Decimal>,
    next_funding_time: Option<DateTime<Utc>>,
    funding_interval_hours: Option<i32>,
    rate_limit_upper: Option<Decimal>,
    rate_limit_lower: Option<Decimal>,
}

/// Bitget WebSocket适配器
pub struct BitgetWebSocketAdapter {
    exchange_type: ExchangeType,
    market_type: MarketType,
    stream: Option<Arc<Mutex<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_symbols: Vec<String>,
    /// 资金费率缓存 (仅 Futures 使用)
    funding_cache: Arc<std::sync::Mutex<HashMap<String, FundingRateCache>>>,
    /// 上次刷新 fundingInfo 的时间
    last_funding_info_fetch: Option<std::time::Instant>,
}

impl BitgetWebSocketAdapter {
    pub fn new(market_type: MarketType) -> Self {
        Self {
            exchange_type: ExchangeType::Bitget,
            market_type,
            stream: None,
            subscribed_symbols: Vec::new(),
            funding_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_funding_info_fetch: None,
        }
    }

    fn get_ws_url(&self) -> String {
        // V1 已下线（日志: code=30032 The V1 API has been decommissioned）
        // 统一迁移到 Bitget WebSocket v2 公共地址
        "wss://ws.bitget.com/v2/ws/public".to_string()
    }

    fn parse_ticker_message(&self, msg: &str) -> Result<MarketSnapshot> {
        tracing::debug!(
            "Bitget 解析 ticker 消息: {}",
            msg.chars().take(200).collect::<String>()
        );

        // v2 返回消息类型较多，且字段随市场类型略有差异，使用 Value 更稳妥
        let v: serde_json::Value =
            serde_json::from_str(msg).context("Failed to parse Bitget WebSocket message")?;

        // 错误消息：直接抛出，触发上层重连/退出
        if v.get("event").and_then(|x| x.as_str()) == Some("error") {
            let code = v.get("code").and_then(|x| x.as_i64()).unwrap_or(-1);
            let m = v
                .get("msg")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow::anyhow!("Bitget ws error (code={}): {}", code, m));
        }

        let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("");
        if action != "snapshot" && action != "update" {
            return Err(anyhow::anyhow!("Unexpected action: {}", action));
        }

        let data_v = v.get("data").cloned().unwrap_or(serde_json::Value::Null);
        let obj = match &data_v {
            serde_json::Value::Array(arr) => arr.first().cloned().unwrap_or(serde_json::Value::Null),
            serde_json::Value::Object(_) => data_v,
            _ => serde_json::Value::Null,
        };
        if obj.is_null() {
            return Err(anyhow::anyhow!("Empty ticker data"));
        }

        let snapshot_time = Utc::now();

        // v2 常见字段：instId；有些消息也会带 arg.instId
        let symbol = obj
            .get("instId")
            .and_then(|x| x.as_str())
            .or_else(|| {
                v.get("arg")
                    .and_then(|a| a.get("instId"))
                    .and_then(|x| x.as_str())
            })
            .unwrap_or("UNKNOWN")
            .to_string();

        // 不同市场字段可能是：bidPr/askPr/last 或 bestBid/bestAsk/lastPr 等
        let bid = obj
            .get("bidPr")
            .or_else(|| obj.get("bestBid"))
            .or_else(|| obj.get("best_bid"))
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let ask = obj
            .get("askPr")
            .or_else(|| obj.get("bestAsk"))
            .or_else(|| obj.get("best_ask"))
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let last = obj
            .get("last")
            .or_else(|| obj.get("lastPr"))
            .or_else(|| obj.get("lastPrice"))
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());
        let vol = obj
            .get("baseVol")
            .or_else(|| obj.get("baseVolume"))
            .or_else(|| obj.get("volume24h"))
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse().ok());

        // 从缓存获取资金费率（仅 Futures）
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

    /// 从 REST API 获取资金费率信息
    /// 使用 current-fund-rate API 获取完整的资金费率信息（包括 nextUpdate, minFundingRate, maxFundingRate）
    async fn fetch_funding_info(&self, symbols: &[String]) -> Result<()> {
        if self.market_type != MarketType::Futures {
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        // Bitget 需要逐个 symbol 查询 funding rate API，分批并行请求
        let mut tasks = Vec::new();
        let mut success_count = 0;
        let mut error_count = 0;
        let funding_cache = self.funding_cache.clone();

        for symbol in symbols.iter() {
            let client = client.clone();
            let symbol = symbol.clone();

            tasks.push(tokio::spawn(async move {
                let url = format!(
                    "https://api.bitget.com/api/v2/mix/market/current-fund-rate?productType=USDT-FUTURES&symbol={}",
                    symbol
                );

                match client.get(&url).send().await {
                    Ok(resp) => {
                        match resp.json::<serde_json::Value>().await {
                            Ok(data) => {
                                if let Some(item) = data.get("data").and_then(|d| d.get(0)) {
                                    let funding_rate: Option<Decimal> = item
                                        .get("fundingRate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());

                                    let next_funding_time: Option<DateTime<Utc>> = item
                                        .get("nextUpdate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse::<i64>().ok())
                                        .and_then(|ts| DateTime::from_timestamp_millis(ts));

                                    let funding_interval_hours: Option<i32> = item
                                        .get("fundingRateInterval")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());

                                    let rate_limit_upper: Option<Decimal> = item
                                        .get("maxFundingRate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());

                                    let rate_limit_lower: Option<Decimal> = item
                                        .get("minFundingRate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());

                                    return Ok(Some((symbol, FundingRateCache {
                                        funding_rate,
                                        next_funding_time,
                                        funding_interval_hours,
                                        rate_limit_upper,
                                        rate_limit_lower,
                                    })));
                                }
                                Ok(None)
                            }
                            Err(e) => Err(format!("{}: parse error: {}", symbol, e))
                        }
                    }
                    Err(e) => Err(format!("{}: request error: {}", symbol, e))
                }
            }));

            // 每 50 个请求后等待完成，避免同时发送过多请求
            if tasks.len() >= 50 {
                let batch_results = futures_util::future::join_all(tasks).await;
                if let Ok(mut cache_guard) = funding_cache.lock() {
                    for result in batch_results {
                        match result {
                            Ok(Ok(Some((symbol, cache)))) => {
                                cache_guard.insert(symbol, cache);
                                success_count += 1;
                            }
                            Ok(Err(e)) => {
                                tracing::debug!("Bitget funding rate fetch failed: {}", e);
                                error_count += 1;
                            }
                            _ => {}
                        }
                    }
                }
                tasks = Vec::new();
                // 短暂等待避免速率限制
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        // 处理剩余的请求
        if !tasks.is_empty() {
            let batch_results = futures_util::future::join_all(tasks).await;
            if let Ok(mut cache_guard) = funding_cache.lock() {
                for result in batch_results {
                    match result {
                        Ok(Ok(Some((symbol, cache)))) => {
                            cache_guard.insert(symbol, cache);
                            success_count += 1;
                        }
                        Ok(Err(e)) => {
                            tracing::debug!("Bitget funding rate fetch failed: {}", e);
                            error_count += 1;
                        }
                        _ => {}
                    }
                }
            }
        }

        tracing::info!("Bitget fundingInfo 加载完成: 成功 {} 个, 失败 {} 个", success_count, error_count);
        Ok(())
    }
}

#[async_trait]
impl WebSocketAdapter for BitgetWebSocketAdapter {
    fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self.get_ws_url();
        tracing::debug!("连接 Bitget WebSocket: {}", url);
        
        // 添加连接超时（更宽松的超时设置，避免网络抖动导致频繁误判超时）
        let connect_future = connect_websocket_with_proxy(&url);
        match tokio::time::timeout(tokio::time::Duration::from_secs(20), connect_future).await {
            Ok(Ok((ws_stream, _))) => {
                self.stream = Some(Arc::new(Mutex::new(ws_stream)));
                tracing::debug!("Bitget WS连接: {}", url);
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to connect to Bitget WebSocket at {}: {}", url, e);
                tracing::error!("{}", error_msg);
                // TLS 错误可能需要特殊处理
                if error_msg.contains("TLS") || error_msg.contains("native-tls") {
                    tracing::warn!("Bitget WebSocket TLS 错误，可能是服务器端 TLS 配置问题或网络问题");
                }
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("Bitget WebSocket connection timeout after 20s: {}", url);
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

        // Bitget v2 instType 枚举：SPOT / USDT-FUTURES 等
        let inst_type = match market_type {
            MarketType::Spot => "SPOT",
            MarketType::Futures => "USDT-FUTURES",
        };

        tracing::debug!("Bitget 订阅 {} 交易对 ({})", symbols.len(), inst_type);

        let total = symbols.len();
        let batch_size = 10usize;

        for (batch_idx, chunk) in symbols.chunks(batch_size).enumerate() {
            let args: Vec<Value> = chunk
                .iter()
                .map(|symbol| {
                    json!({
                        "instType": inst_type,
                        "channel": "ticker",
                        "instId": symbol
                    })
                })
                .collect();

            let subscribe_msg = json!({
                "op": "subscribe",
                "args": args
            });

            if let Some(stream) = &self.stream {
                let mut ws = stream.lock().await;
                let msg_text = subscribe_msg.to_string();
                if let Err(e) = ws.send(Message::Text(msg_text)).await {
                    tracing::error!(
                        "Bitget 发送订阅失败: batch={}/{}, chunk_size={}, err={}",
                        batch_idx + 1,
                        (total + batch_size - 1) / batch_size,
                        chunk.len(),
                        e
                    );
                    return Err(anyhow::anyhow!(e).context("Failed to send subscribe message"));
                }
            }

            let sent = std::cmp::min((batch_idx + 1) * batch_size, total);
            tracing::debug!("Bitget 订阅批次 {}/{} ({}/{})", batch_idx + 1, (total + batch_size - 1) / batch_size, sent, total);

            // 更强节流，避免短时间写爆连接/触发风控（Bitget 经常在订阅阶段 reset）
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        tracing::debug!("Bitget 订阅完成");

        // Futures: 预加载资金费率信息
        if market_type == MarketType::Futures {
            if let Err(e) = self.fetch_funding_info(symbols).await {
                tracing::warn!("Bitget fundingInfo 预加载失败: {}", e);
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
                            tracing::debug!("Bitget 收到 WebSocket 消息: {}", text.chars().take(200).collect::<String>());
                            
                            // 检查是否是 ticker 消息
                            // Bitget 的消息格式是 {"action": "snapshot"/"update", "data": [...]}
                            // 没有 channel 字段，需要检查 action 和 data 字段
                            if (text.contains("\"action\":\"snapshot\"") || text.contains("\"action\":\"update\"")) 
                                && text.contains("\"data\"") {
                                // 忽略订阅确认消息
                                if text.contains("\"event\":\"subscribe\"") || 
                                   (text.contains("\"code\":\"00000\"") && text.contains("\"msg\":\"success\"")) {
                                    tracing::debug!("Bitget 忽略订阅确认消息");
                                    continue;
                                }
                                tracing::debug!("Bitget 解析 ticker 消息");
                                return self.parse_ticker_message(&text);
                            }
                            // 忽略其他消息
                            tracing::debug!("Bitget 忽略非 ticker 消息");
                            
                            // 每小时刷新一次 fundingInfo
                            if self.market_type == MarketType::Futures {
                                let should_refresh = self.last_funding_info_fetch
                                    .map(|t| t.elapsed() >= std::time::Duration::from_secs(3600))
                                    .unwrap_or(true);
                                
                                if should_refresh {
                                    drop(ws);
                                    tracing::debug!("Bitget 开始刷新 fundingInfo (每小时)");
                                    if let Err(e) = self.fetch_funding_info(&self.subscribed_symbols.clone()).await {
                                        tracing::warn!("Bitget fundingInfo 刷新失败: {}", e);
                                    }
                                    self.last_funding_info_fetch = Some(std::time::Instant::now());
                                    ws = stream.lock().await;
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

// Bitget v2 的 ticker 消息字段在 spot/futures 下有差异，且会包含多种事件类型；
// 这里统一用 serde_json::Value 在 parse_ticker_message 中解析，避免强类型结构导致大量解析失败。
