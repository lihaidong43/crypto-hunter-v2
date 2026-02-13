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

/// OKX WebSocket适配器
pub struct OkxWebSocketAdapter {
    exchange_type: ExchangeType,
    market_type: MarketType,
    stream: Option<Arc<Mutex<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    subscribed_symbols: Vec<String>,
    /// 资金费率缓存 (仅 Futures 使用)
    funding_cache: Arc<std::sync::Mutex<HashMap<String, FundingRateCache>>>,
    /// 上次刷新 fundingInfo 的时间
    last_funding_info_fetch: Option<std::time::Instant>,
}

impl OkxWebSocketAdapter {
    pub fn new(market_type: MarketType) -> Self {
        Self {
            exchange_type: ExchangeType::Okx,
            market_type,
            stream: None,
            subscribed_symbols: Vec::new(),
            funding_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_funding_info_fetch: None,
        }
    }

    fn get_ws_url(&self) -> String {
        // OKX 使用统一的 WebSocket 地址
        "wss://ws.okx.com:8443/ws/v5/public".to_string()
    }

    fn parse_ticker_message(&self, msg: &str) -> Result<MarketSnapshot> {
        // OKX WebSocket消息可能是数组格式 [{"arg": {...}, "data": [...]}]
        // 或者单个对象格式 {"arg": {...}, "data": [...]}
        let data: Result<OkxWebSocketMessage, _> = serde_json::from_str(msg);
        let data = match data {
            Ok(msg) => msg,
            Err(_) => {
                // 尝试解析数组格式
                let arr: Vec<OkxWebSocketMessage> = serde_json::from_str(msg)
                    .context("Failed to parse OKX WebSocket message")?;
                if arr.is_empty() {
                    return Err(anyhow::anyhow!("Empty message array"));
                }
                arr[0].clone()
            }
        };

        if data.arg.channel != "tickers" {
            return Err(anyhow::anyhow!("Unexpected channel: {}", data.arg.channel));
        }

        if data.data.is_empty() {
            return Err(anyhow::anyhow!("Empty ticker data"));
        }

        let ticker = &data.data[0];
        let snapshot_time = Utc::now();
        let symbol = ticker.inst_id.clone();

        // 移除 -SWAP 后缀以统一 symbol 格式
        // OKX格式: BTC-USDT-SWAP -> BTCUSDT
        let unified_symbol = symbol.replace("-SWAP", "").replace("-", "");
        
        tracing::debug!("OKX 解析 ticker: symbol={} (原始={}), bid={}, ask={}, last={}", 
            unified_symbol, symbol, ticker.bid_px, ticker.ask_px, ticker.last);

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
            bid_price: ticker.bid_px.parse().ok(),
            ask_price: ticker.ask_px.parse().ok(),
            last_price: ticker.last.parse().ok(),
            volume_24h: ticker.vol_24h.parse().ok(),
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

        // 使用系统代理配置
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        // OKX 需要逐个查询资金费率，分批并行请求避免过载
        let mut tasks = Vec::new();
        let mut success_count = 0;
        let mut error_count = 0;
        
        // 处理所有 symbols（不限制数量）
        for symbol in symbols.iter() {
            // 处理 symbol 格式：支持 BTC-USDT-SWAP 或 BTCUSDT 格式
            let okx_inst_id = if symbol.ends_with("-USDT-SWAP") {
                // 已经是 OKX 格式
                symbol.clone()
            } else if symbol.len() > 4 && symbol.ends_with("USDT") {
                // 标准化格式：BTCUSDT -> BTC-USDT-SWAP
                let base = &symbol[..symbol.len() - 4];
                format!("{}-USDT-SWAP", base)
            } else {
                continue;
            };
            
            let client = client.clone();
            let unified_symbol = symbol.clone();
            let inst_id = okx_inst_id.clone();
            
            tasks.push(tokio::spawn(async move {
                let url = format!(
                    "https://www.okx.com/api/v5/public/funding-rate?instId={}",
                    inst_id
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
                                        .get("fundingTime")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse::<i64>().ok())
                                        .and_then(|ts| DateTime::from_timestamp_millis(ts));
                                    
                                    let rate_limit_upper: Option<Decimal> = item
                                        .get("maxFundingRate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());
                                    
                                    let rate_limit_lower: Option<Decimal> = item
                                        .get("minFundingRate")
                                        .and_then(|x| x.as_str())
                                        .and_then(|s| s.parse().ok());
                                    
                                    return Ok(Some((unified_symbol, FundingRateCache {
                                        funding_rate,
                                        next_funding_time,
                                        funding_interval_hours: Some(8), // OKX 默认 8 小时
                                        rate_limit_upper,
                                        rate_limit_lower,
                                    })));
                                }
                                Ok(None)
                            }
                            Err(e) => Err(format!("{}: parse error: {}", inst_id, e))
                        }
                    }
                    Err(e) => Err(format!("{}: request error: {}", inst_id, e))
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
                                tracing::debug!("OKX funding rate fetch failed: {}", e);
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
                            tracing::debug!("OKX funding rate fetch failed: {}", e);
                            error_count += 1;
                        }
                        _ => {}
                    }
                }
            }
        }

        tracing::info!("OKX fundingInfo 预加载完成: 成功 {}, 失败 {}", success_count, error_count);
        Ok(())
    }
}

#[async_trait]
impl WebSocketAdapter for OkxWebSocketAdapter {
    fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    async fn connect(&mut self) -> Result<()> {
        let url = self.get_ws_url();
        tracing::debug!("连接 OKX WebSocket: {}", url);
        
        // 添加连接超时（默认 20 秒，避免网络抖动导致频繁误判超时）
        let connect_future = connect_websocket_with_proxy(&url);
        match tokio::time::timeout(tokio::time::Duration::from_secs(20), connect_future).await {
            Ok(Ok((ws_stream, _))) => {
                self.stream = Some(Arc::new(Mutex::new(ws_stream)));
                tracing::debug!("OKX WS连接: {}", url);
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Failed to connect to OKX WebSocket at {}: {}", url, e);
                tracing::error!("{}", error_msg);
                Err(anyhow::anyhow!(error_msg))
            }
            Err(_) => {
                let error_msg = format!("OKX WebSocket connection timeout after 20s: {}", url);
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

        // OKX 使用数组格式订阅，每个订阅都需要单独的消息
        // 格式: {"op": "subscribe", "args": [{"channel": "tickers", "instId": "BTC-USDT-SWAP"}]}
        let args: Vec<serde_json::Value> = symbols
            .iter()
            .map(|symbol| {
                // 转换为 OKX 格式 (BTC-USDT 或 BTC-USDT-SWAP)
                let okx_symbol = if symbol.contains("-") {
                    symbol.clone()
                } else {
                    // 假设是 USDT 交易对，转换格式
                    let base = if symbol.len() > 4 && symbol.ends_with("USDT") {
                        &symbol[..symbol.len() - 4]
                    } else {
                        return json!({"channel": "tickers", "instId": symbol});
                    };
                    match market_type {
                        MarketType::Futures => format!("{}-USDT-SWAP", base),
                        MarketType::Spot => format!("{}-USDT", base),
                    }
                };

                json!({
                    "channel": "tickers",
                    "instId": okx_symbol
                })
            })
            .collect();

        tracing::debug!("OKX 开始订阅 {} 个交易对", symbols.len());

        let subscribe_msg = json!({
            "op": "subscribe",
            "args": args
        });

        tracing::debug!("OKX 发送订阅消息: {}", subscribe_msg.to_string().chars().take(500).collect::<String>());

        if let Some(stream) = &self.stream {
            let mut ws = stream.lock().await;
            ws.send(Message::Text(subscribe_msg.to_string()))
                .await
                .context("Failed to send subscribe message")?;
        }

        tracing::debug!("OKX 订阅完成");

        // Futures: 预加载资金费率信息
        if market_type == MarketType::Futures {
            if let Err(e) = self.fetch_funding_info(symbols).await {
                tracing::warn!("OKX fundingInfo 预加载失败: {}", e);
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
                            tracing::debug!("OKX 收到 WebSocket 消息: {}", text.chars().take(200).collect::<String>());
                            
                            // 检查是否是 ticker 消息
                            if text.contains("\"channel\":\"tickers\"") && text.contains("\"data\"") {
                                // 忽略订阅确认消息
                                if text.contains("\"event\":\"subscribe\"") {
                                    tracing::debug!("OKX 忽略订阅确认消息");
                                    continue;
                                }
                                tracing::debug!("OKX 解析 ticker 消息");
                                return self.parse_ticker_message(&text);
                            }
                            // 忽略其他消息（如订阅确认、心跳等）
                            tracing::debug!("OKX 忽略非 ticker 消息");
                            
                            // 每小时刷新一次 fundingInfo
                            if self.market_type == MarketType::Futures {
                                let should_refresh = self.last_funding_info_fetch
                                    .map(|t| t.elapsed() >= std::time::Duration::from_secs(3600))
                                    .unwrap_or(true);
                                
                                if should_refresh {
                                    drop(ws);
                                    tracing::debug!("OKX 开始刷新 fundingInfo (每小时)");
                                    if let Err(e) = self.fetch_funding_info(&self.subscribed_symbols.clone()).await {
                                        tracing::warn!("OKX fundingInfo 刷新失败: {}", e);
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

#[derive(Debug, Clone, Deserialize)]
struct OkxWebSocketMessage {
    arg: OkxWebSocketArg,
    data: Vec<OkxTickerData>,
}

#[derive(Debug, Clone, Deserialize)]
struct OkxWebSocketArg {
    channel: String,
    #[serde(rename = "instId")]
    inst_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OkxTickerData {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "bidPx")]
    bid_px: String,
    #[serde(rename = "askPx")]
    ask_px: String,
    #[serde(rename = "last")]
    last: String,
    #[serde(rename = "vol24h")]
    vol_24h: String,
}
