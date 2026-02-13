use crate::exchange::ws_adapter::WebSocketAdapter;
use crate::models::exchange::{ExchangeType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
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

/// Gate.io WebSocket适配器
pub struct GateioWebSocketAdapter {
    exchange_type: ExchangeType,
    market_type: MarketType,
    stream: Option<Arc<Mutex<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_symbols: Vec<String>,
    /// 资金费率缓存 (仅 Futures 使用)
    funding_cache: Arc<std::sync::Mutex<HashMap<String, FundingRateCache>>>,
    /// 上次刷新 fundingInfo 的时间
    last_funding_info_fetch: Option<std::time::Instant>,
}

impl GateioWebSocketAdapter {
    pub fn new(market_type: MarketType) -> Self {
        Self {
            exchange_type: ExchangeType::Gateio,
            market_type,
            stream: None,
            subscribed_symbols: Vec::new(),
            funding_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_funding_info_fetch: None,
        }
    }

    fn get_ws_url(&self) -> String {
        match self.market_type {
            // Gate.io Spot WebSocket v4 API
            MarketType::Spot => "wss://api.gateio.ws/ws/v4/".to_string(),
            // Gate.io Futures WebSocket v4 API
            MarketType::Futures => "wss://fx-ws.gateio.ws/v4/ws/usdt".to_string(),
        }
    }

    fn parse_ticker_message(&self, msg: &str) -> Result<MarketSnapshot> {
        tracing::debug!("Gate.io 解析 ticker 消息: {}", msg.chars().take(200).collect::<String>());
        
        let data: GateioWebSocketMessage = serde_json::from_str(msg)
            .with_context(|| format!("Failed to parse Gate.io WebSocket message: {}", msg))?;

        if data.channel != "spot.tickers" && data.channel != "futures.tickers" {
            return Err(anyhow::anyhow!("Unexpected channel: {}", data.channel));
        }

        // Gate.io 的 result 可能是对象或数组；且由于 untagged，数组可能会被解析成 One(Value::Array)
        // spot / futures 字段也不完全一致，因此这里统一归一化到「单个对象」
        let mut ticker_v = match &data.result {
            GateioResult::One(v) => v.clone(),
            GateioResult::Many(v) => v.first().cloned().unwrap_or(Value::Null),
        };
        if let Value::Array(arr) = &ticker_v {
            ticker_v = arr.first().cloned().unwrap_or(Value::Null);
        }
        if ticker_v.is_null() {
            return Err(anyhow::anyhow!("Empty ticker data"));
        }

        let snapshot_time = Utc::now();
        let symbol = get_string(&ticker_v, "currency_pair")
            .or_else(|| get_string(&ticker_v, "contract"))
            .unwrap_or_else(|| "UNKNOWN".to_string());

        // 移除下划线以统一 symbol 格式 (BTC_USDT -> BTCUSDT)
        let unified_symbol = symbol.replace("_", "");
        
        // futures.tickers 常见字段：bid/ask；spot.tickers 常见字段：highest_bid/lowest_ask
        let bid = get_decimal(&ticker_v, &["highest_bid", "bid", "bid_price", "best_bid", "b"]);
        let ask = get_decimal(&ticker_v, &["lowest_ask", "ask", "ask_price", "best_ask", "a"]);
        let last = get_decimal(&ticker_v, &["last", "c"]);
        
        tracing::debug!("Gate.io 解析 ticker: symbol={} (原始={}), bid={:?}, ask={:?}, last={:?}", 
            unified_symbol, symbol, bid, ask, last);

        // 从缓存获取资金费率（仅 Futures）
        let (funding_rate, next_funding_time, funding_interval_hours, rate_limit_upper, rate_limit_lower) = 
            if self.market_type == MarketType::Futures {
                if let Ok(cache_guard) = self.funding_cache.lock() {
                    if let Some(cache) = cache_guard.get(&unified_symbol) {
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
            symbol: unified_symbol,
            market_type: self.market_type,
            bid_price: bid,
            ask_price: ask,
            last_price: last,
            volume_24h: get_decimal(&ticker_v, &["base_volume", "baseVolume", "volume_24h_base", "volume_24h", "volume"]),
            funding_rate,
            next_funding_time,
            funding_interval_hours,
            rate_limit_upper,
            rate_limit_lower,
        })
    }

    /// 从 REST API 获取资金费率信息
    async fn fetch_funding_info(&self, symbols: &[String]) -> Result<()> {
        if self.market_type != MarketType::Futures {
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        // Gate.io 需要逐个查询合约信息，分批并行请求避免过载
        let mut tasks = Vec::new();
        let mut success_count = 0;
        let mut error_count = 0;
        
        // 处理所有 symbols（不限制数量）
        for symbol in symbols.iter() {
            // 转换 symbol 格式：BTCUSDT -> BTC_USDT
            let gateio_contract = if symbol.len() > 4 && symbol.ends_with("USDT") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}_USDT", base)
            } else {
                continue;
            };
            
            let client = client.clone();
            let unified_symbol = symbol.clone();
            let contract = gateio_contract.clone();
            
            tasks.push(tokio::spawn(async move {
                let url = format!(
                    "https://api.gateio.ws/api/v4/futures/usdt/contracts/{}",
                    contract
                );
                
                match client.get(&url).send().await {
                    Ok(resp) => {
                        match resp.json::<serde_json::Value>().await {
                            Ok(data) => {
                                let funding_rate: Option<Decimal> = data
                                    .get("funding_rate")
                                    .and_then(|x| x.as_str())
                                    .and_then(|s| s.parse().ok());
                                
                                let next_funding_time: Option<DateTime<Utc>> = data
                                    .get("funding_next_apply")
                                    .and_then(|x| x.as_i64())
                                    .and_then(|ts| DateTime::from_timestamp(ts, 0));
                                
                                // funding_interval 是秒，转换为小时
                                let funding_interval_hours: Option<i32> = data
                                    .get("funding_interval")
                                    .and_then(|x| x.as_i64())
                                    .map(|s| (s / 3600) as i32);
                                
                                let rate_limit_upper: Option<Decimal> = data
                                    .get("funding_cap")
                                    .and_then(|x| x.as_str())
                                    .and_then(|s| s.parse().ok());
                                
                                let rate_limit_lower: Option<Decimal> = data
                                    .get("funding_floor")
                                    .and_then(|x| x.as_str())
                                    .and_then(|s| s.parse().ok());
                                
                                Ok(Some((unified_symbol, FundingRateCache {
                                    funding_rate,
                                    next_funding_time,
                                    funding_interval_hours,
                                    rate_limit_upper,
                                    rate_limit_lower,
                                })))
                            }
                            Err(e) => Err(format!("{}: parse error: {}", contract, e))
                        }
                    }
                    Err(e) => Err(format!("{}: request error: {}", contract, e))
                }
            }));
            
            // 每 50 个请求后等待完成，避免同时发送过多请求
            if tasks.len() >= 50 {
                let batch_results = futures_util::future::join_all(tasks).await;
                if let Ok(mut cache_guard) = self.funding_cache.lock() {
                    for result in batch_results {
                        match result {
                            Ok(Ok(Some((symbol, cache)))) => {
                                cache_guard.insert(symbol, cache);
                                success_count += 1;
                            }
                            Ok(Err(e)) => {
                                tracing::debug!("Gate.io funding rate fetch failed: {}", e);
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
            if let Ok(mut cache_guard) = self.funding_cache.lock() {
                for result in batch_results {
                    match result {
                        Ok(Ok(Some((symbol, cache)))) => {
                            cache_guard.insert(symbol, cache);
                            success_count += 1;
                        }
                        Ok(Err(e)) => {
                            tracing::debug!("Gate.io funding rate fetch failed: {}", e);
                            error_count += 1;
                        }
                        _ => {}
                    }
                }
            }
        }

        tracing::info!("Gate.io fundingInfo 加载完成: 成功 {} 个, 失败 {} 个", success_count, error_count);
        Ok(())
    }
}

#[async_trait]
impl WebSocketAdapter for GateioWebSocketAdapter {
    fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self.get_ws_url();
        tracing::debug!("连接 Gate.io WebSocket: {}", url);
        
        // 添加连接超时（默认 10 秒）
        let connect_future = connect_websocket_with_proxy(&url);
        match tokio::time::timeout(tokio::time::Duration::from_secs(10), connect_future).await {
            Ok(Ok((ws_stream, _))) => {
                self.stream = Some(Arc::new(Mutex::new(ws_stream)));
                tracing::debug!("Gate.io WS连接: {}", url);
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to connect to Gate.io WebSocket at {}: {}", url, e);
                tracing::error!("{}", error_msg);
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("Gate.io WebSocket connection timeout after 10s: {}", url);
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

        // Gate.io 支持 payload 为数组：分批订阅，避免逐条发送过慢/卡住
        let channel = match market_type {
            MarketType::Spot => "spot.tickers",
            MarketType::Futures => "futures.tickers",
        };

        tracing::info!(
            "Gate.io 开始订阅 {} 个交易对 (channel: {}, batch_size=100)",
            symbols.len(),
            channel
        );

        // 预先转换格式并过滤非 USDT（当前系统统一用 USDT）
        let mut payloads: Vec<String> = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            // BTC_USDT 直接使用；BTCUSDT -> BTC_USDT
            let gateio_symbol = if symbol.contains("_") {
                symbol.clone()
            } else if symbol.len() > 4 && symbol.ends_with("USDT") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}_USDT", base)
            } else {
                continue;
            };
            payloads.push(gateio_symbol);
        }

        let total = payloads.len();
        let batch_size = 100usize;

        // 逐批发送（每批 100）
        for (batch_idx, chunk) in payloads.chunks(batch_size).enumerate() {
            let subscribe_msg = json!({
                "time": Utc::now().timestamp(),
                "channel": channel,
                "event": "subscribe",
                "payload": chunk
            });

            if let Some(stream) = &self.stream {
                let mut ws = stream.lock().await;
                ws.send(Message::Text(subscribe_msg.to_string()))
                    .await
                    .context("Failed to send subscribe message")?;
            }

            let sent = std::cmp::min((batch_idx + 1) * batch_size, total);
            tracing::debug!("Gate.io 订阅批次 {}/{} ({}/{})", batch_idx + 1, (total + batch_size - 1) / batch_size, sent, total);

            // 轻微节流，避免短时间写爆连接/触发风控（尤其是现货 2000+）
            tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
        }

        tracing::debug!("Gate.io 订阅完成 (共 {} 交易对)", total);

        // Futures: 预加载资金费率信息
        if market_type == MarketType::Futures {
            if let Err(e) = self.fetch_funding_info(symbols).await {
                tracing::warn!("Gate.io fundingInfo 预加载失败: {}", e);
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
                            tracing::debug!("Gate.io 收到 WebSocket 消息: {}", text.chars().take(200).collect::<String>());

                            // 只处理 event=update 的 ticker 推送；subscribe/pong 等都忽略
                            if (text.contains("\"channel\":\"spot.tickers\"")
                                || text.contains("\"channel\":\"futures.tickers\""))
                                && text.contains("\"event\":\"update\"")
                                && text.contains("\"result\"")
                            {
                                match self.parse_ticker_message(&text) {
                                    Ok(s) => return Ok(s),
                                    Err(e) => {
                                        tracing::debug!("Gate.io 忽略无法解析的 update 消息: {}", e);
                                        continue;
                                    }
                                }
                            }
                            
                            // 每小时刷新一次 fundingInfo
                            if self.market_type == MarketType::Futures {
                                let should_refresh = self.last_funding_info_fetch
                                    .map(|t| t.elapsed() >= std::time::Duration::from_secs(3600))
                                    .unwrap_or(true);
                                
                                if should_refresh {
                                    drop(ws);
                                    tracing::debug!("Gate.io 开始刷新 fundingInfo (每小时)");
                                    if let Err(e) = self.fetch_funding_info(&self.subscribed_symbols.clone()).await {
                                        tracing::warn!("Gate.io fundingInfo 刷新失败: {}", e);
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

#[derive(Debug, Deserialize)]
struct GateioWebSocketMessage {
    channel: String,
    #[allow(dead_code)]
    event: Option<String>,
    result: GateioResult,
}

/// Gate.io 的 `result` 有时是对象，有时是数组；并且 spot/futures 字段差异较大。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GateioResult {
    One(Value),
    Many(Vec<Value>),
}

fn get_string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn get_decimal(v: &Value, keys: &[&str]) -> Option<rust_decimal::Decimal> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            if let Ok(d) = s.parse::<rust_decimal::Decimal>() {
                return Some(d);
            }
        } else if let Some(n) = v.get(*k).and_then(|x| x.as_f64()) {
            // 兜底：极少数情况下是数值
            if let Ok(d) = n.to_string().parse::<rust_decimal::Decimal>() {
                return Some(d);
            }
        }
    }
    None
}
