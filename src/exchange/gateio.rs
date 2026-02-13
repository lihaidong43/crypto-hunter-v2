use crate::exchange::adapter::ExchangeAdapter;
use crate::models::{
    exchange::{ExchangeType, MarketType, TradingPair},
    price::{FundingRate, HistoricalPriceData, PriceData},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::time::Duration;

pub struct GateioAdapter {
    client: Client,
    base_url: String,
}

impl GateioAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // 增加超时时间
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: "https://api.gateio.ws".to_string(),
        }
    }
}

#[async_trait]
impl ExchangeAdapter for GateioAdapter {
    fn exchange_type(&self) -> ExchangeType {
        ExchangeType::Gateio
    }

    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v4/spot/currency_pairs", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Gate.io spot API")?;
        
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gate.io spot API error: HTTP {} - {}",
                status,
                text.chars().take(200).collect::<String>()
            ));
        }
        
        let text = response.text().await.context("Failed to read Gate.io response body")?;
        let resp: Vec<GateioCurrencyPair> = serde_json::from_str(&text)
            .context(format!(
                "Failed to parse Gate.io currency pairs, response length: {}, first 200 chars: {}",
                text.len(),
                text.chars().take(200).collect::<String>()
            ))?;

        let pairs: Vec<TradingPair> = resp
            .into_iter()
            .filter(|pair| pair.trade_status == "tradable" && pair.id.ends_with("_USDT"))
            .map(|pair| {
                let parts: Vec<&str> = pair.id.split('_').collect();
                let base = if parts.len() >= 2 {
                    parts[0].to_string()
                } else {
                    pair.base.clone()
                };
                let quote = if parts.len() >= 2 {
                    parts[1].to_string()
                } else {
                    pair.quote.clone()
                };
                TradingPair {
                    symbol: pair.id.clone(),
                    base,
                    quote,
                    exchange: ExchangeType::Gateio,
                    is_spot: true,
                    is_futures: false,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v4/futures/usdt/contracts", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gate.io Futures API error: HTTP {} (response: {})",
                status,
                if error_text.len() > 200 {
                    format!("{}...", &error_text[..200])
                } else {
                    error_text
                }
            ));
        }

        // 获取响应文本用于调试
        let text = response.text().await?;
        
        // 检查是否为空响应
        if text.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Gate.io Futures API error: Empty response"
            ));
        }

        // 尝试解析 JSON
        let resp: Vec<GateioFuturesContract> = serde_json::from_str(&text)
            .context(format!(
                "Failed to parse Gate.io futures contracts (response: {})",
                if text.len() > 500 {
                    format!("{}...", &text[..500])
                } else {
                    text.clone()
                }
            ))?;

        let pairs: Vec<TradingPair> = resp
            .into_iter()
            .filter(|contract| contract.in_delisting == false)
            .filter_map(|contract| {
                // 如果 underlying 为 None，尝试从 name 中提取（格式：BASE_USDT）
                let base = contract.underlying.clone().unwrap_or_else(|| {
                    // 从 name 中提取基础资产（例如：BTC_USDT -> BTC）
                    contract.name
                        .split('_')
                        .next()
                        .unwrap_or("UNKNOWN")
                        .to_string()
                });
                
                Some(TradingPair {
                    symbol: contract.name.clone(),
                    base,
                    quote: "USDT".to_string(),
                    exchange: ExchangeType::Gateio,
                    is_spot: false,
                    is_futures: true,
                })
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v4/spot/tickers", self.base_url);
        let params = vec![("currency_pair", symbol)];
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Gate.io API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }

        // 解析响应
        let resp: Vec<GateioTicker> = response
            .json()
            .await
            .context(format!("Failed to parse Gate.io ticker response for symbol {}", symbol))?;

        // 检查是否为空数组（symbol 不存在）
        if resp.is_empty() {
            return Err(anyhow::anyhow!(
                "Gate.io API error: Symbol {} not found (empty response)",
                symbol
            ));
        }

        let ticker = resp.into_iter().next().expect("Should have at least one ticker");

        Ok(PriceData {
            exchange: ExchangeType::Gateio,
            symbol: symbol.to_string(),
            market_type: MarketType::Spot,
            bid_price: ticker.highest_bid.parse()?,
            ask_price: ticker.lowest_ask.parse()?,
            last_price: ticker.last.parse()?,
            volume_24h: ticker.quote_volume.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v4/futures/usdt/tickers", self.base_url);
        let params = vec![("contract", symbol)];
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Gate.io Futures API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }

        // 解析响应
        let resp: Vec<GateioFuturesTicker> = response
            .json()
            .await
            .context(format!("Failed to parse Gate.io futures ticker response for symbol {}", symbol))?;

        // 检查是否为空数组（symbol 不存在）
        if resp.is_empty() {
            return Err(anyhow::anyhow!(
                "Gate.io Futures API error: Symbol {} not found (empty response)",
                symbol
            ));
        }

        let ticker = resp.into_iter().next().expect("Should have at least one ticker");

        // Gate.io 期货 ticker 字段在不同场景下可能是 bid1/ask1 或 highest_bid/lowest_ask
        let bid = ticker
            .bid()
            .context("Gate.io Futures ticker missing bid (bid1/highest_bid)")?;
        let ask = ticker
            .ask()
            .context("Gate.io Futures ticker missing ask (ask1/lowest_ask)")?;

        Ok(PriceData {
            exchange: ExchangeType::Gateio,
            symbol: symbol.to_string(),
            market_type: MarketType::Futures,
            bid_price: bid.parse()?,
            ask_price: ask.parse()?,
            last_price: ticker.last.parse()?,
            volume_24h: ticker.volume_24h_base.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/api/v4/futures/usdt/contracts/{}", self.base_url, symbol);
        let response = self
            .client
            .get(&url)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Gate.io Futures API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }

        // 解析响应
        let contract: GateioFuturesContract = response
            .json()
            .await
            .context(format!("Failed to parse Gate.io futures contract response for symbol {}", symbol))?;

        // 从 API 获取 next_funding_time
        let next_funding_time = if let Some(timestamp_s) = contract.funding_next_apply {
            DateTime::from_timestamp(timestamp_s, 0)
                .with_context(|| format!("Invalid funding_next_apply timestamp: {}", timestamp_s))?
                .with_timezone(&Utc)
        } else {
            // 如果没有提供，计算下一个资金费时间
            let now = Utc::now().timestamp();
            let funding_interval = contract.funding_interval.unwrap_or(8 * 3600); // 默认8小时
            DateTime::from_timestamp(
                ((now / funding_interval) + 1) * funding_interval,
                0,
            )
            .context("Invalid next funding time")?
            .with_timezone(&Utc)
        };

        // 从 API 获取资金费结算周期（秒转小时）
        // Gate.io API 返回的是秒数，需要转换为小时
        let funding_interval_hours = contract
            .funding_interval
            .map(|seconds| (seconds / 3600) as i32)
            .filter(|hours| *hours > 0);

        // 从 API 获取资金费率上限（过滤空字符串）
        let rate_limit_upper = contract.funding_rate_limit
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.trim().parse::<Decimal>().ok())
            .filter(|d| !d.is_zero());
        // Gate.io API 不提供下限，使用上限的负值
        let rate_limit_lower = rate_limit_upper.map(|upper| -upper);

        // 从 API 获取资金费率
        let rate = contract.funding_rate.parse()
            .context(format!("Failed to parse funding_rate: {}", contract.funding_rate))?;

        Ok(FundingRate {
            exchange: ExchangeType::Gateio,
            symbol: symbol.to_string(),
            rate,
            next_funding_time,
            funding_interval_hours,
            rate_limit_upper,
            rate_limit_lower,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_historical_prices(
        &self,
        symbol: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        interval: &str,
    ) -> Result<Vec<HistoricalPriceData>> {
        // Gate.io 期货K线API
        let url = format!("{}/api/v4/futures/usdt/candlesticks", self.base_url);
        
        // 转换时间间隔格式（Gate.io使用秒数）
        let gateio_interval = match interval {
            "1m" => "60",
            "3m" => "180",
            "5m" => "300",
            "15m" => "900",
            "30m" => "1800",
            "1h" => "3600",
            "2h" => "7200",
            "4h" => "14400",
            "6h" => "21600",
            "8h" => "28800",
            "12h" => "43200",
            "1d" => "86400",
            "1w" => "604800",
            _ => return Err(anyhow::anyhow!("Unsupported interval: {}", interval)),
        };
        
        let mut all_klines = Vec::new();
        let mut current_start = start_time;
        let limit = 1000u64; // Gate.io最大限制
        
        // Gate.io K线API每次最多返回1000条，需要分页获取
        while current_start < end_time {
            let start_secs = current_start.timestamp();
            let end_secs = end_time.timestamp();
            let start_secs_str = start_secs.to_string();
            let end_secs_str = end_secs.to_string();
            let limit_str = limit.to_string();
            
            let params = vec![
                ("contract", symbol),
                ("interval", gateio_interval),
                ("from", start_secs_str.as_str()),
                ("to", end_secs_str.as_str()),
                ("limit", limit_str.as_str()),
            ];
            
            let response = self
                .client
                .get(&url)
                .query(&params)
                .send()
                .await?;
            
            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "Gate.io Klines API error: HTTP {} for symbol {} (response: {})",
                    status,
                    symbol,
                    if error_text.len() > 200 {
                        format!("{}...", &error_text[..200])
                    } else {
                        error_text
                    }
                ));
            }
            
            let klines: Vec<Vec<serde_json::Value>> = response
                .json()
                .await
                .context("Failed to parse Gate.io klines response")?;
            
            if klines.is_empty() {
                break; // 没有更多数据
            }
            
            for kline in klines {
                // Gate.io K线数据格式：[时间戳(s), 成交量, 开盘价, 最高价, 最低价, 收盘价, ...]
                if kline.len() < 6 {
                    continue; // 跳过格式不正确的数据
                }
                
                let timestamp_s = kline[0]
                    .as_i64()
                    .context("Invalid timestamp")?;
                let timestamp = DateTime::from_timestamp(timestamp_s, 0)
                    .context("Invalid timestamp")?
                    .with_timezone(&Utc);
                
                all_klines.push(HistoricalPriceData {
                    timestamp,
                    open: kline[2].as_str().context("Invalid open price")?.parse()?,
                    high: kline[3].as_str().context("Invalid high price")?.parse()?,
                    low: kline[4].as_str().context("Invalid low price")?.parse()?,
                    close: kline[5].as_str().context("Invalid close price")?.parse()?,
                    volume: kline[1].as_str().context("Invalid volume")?.parse()?,
                });
            }
            
            // 更新起始时间为最后一条K线的下一个时间点
            if let Some(last_kline) = all_klines.last() {
                let next_time = match interval {
                    "1m" => last_kline.timestamp + chrono::Duration::minutes(1),
                    "3m" => last_kline.timestamp + chrono::Duration::minutes(3),
                    "5m" => last_kline.timestamp + chrono::Duration::minutes(5),
                    "15m" => last_kline.timestamp + chrono::Duration::minutes(15),
                    "30m" => last_kline.timestamp + chrono::Duration::minutes(30),
                    "1h" => last_kline.timestamp + chrono::Duration::hours(1),
                    "2h" => last_kline.timestamp + chrono::Duration::hours(2),
                    "4h" => last_kline.timestamp + chrono::Duration::hours(4),
                    "6h" => last_kline.timestamp + chrono::Duration::hours(6),
                    "8h" => last_kline.timestamp + chrono::Duration::hours(8),
                    "12h" => last_kline.timestamp + chrono::Duration::hours(12),
                    "1d" => last_kline.timestamp + chrono::Duration::days(1),
                    "1w" => last_kline.timestamp + chrono::Duration::weeks(1),
                    _ => break,
                };
                
                if next_time >= end_time {
                    break; // 已覆盖所有时间范围
                }
                current_start = next_time;
            } else {
                break; // 没有数据，退出循环
            }
            
            // 添加延迟，避免限流
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        Ok(all_klines)
    }
}

impl Default for GateioAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// Gate.io API 响应结构
#[derive(Debug, Deserialize)]
struct GateioCurrencyPair {
    id: String,
    base: String,
    quote: String,
    #[serde(rename = "trade_status")]
    trade_status: String,
}

#[derive(Debug, Deserialize)]
struct GateioFuturesContract {
    name: String,
    underlying: Option<String>, // 可能为 null
    #[serde(rename = "in_delisting")]
    in_delisting: bool,
    #[serde(rename = "funding_rate")]
    funding_rate: String,
    #[serde(rename = "funding_interval")]
    funding_interval: Option<i64>, // 资金费结算周期（秒）
    #[serde(rename = "funding_rate_limit")]
    funding_rate_limit: Option<String>, // 资金费率上限
    #[serde(rename = "funding_next_apply")]
    funding_next_apply: Option<i64>, // 下一个资金费时间（秒时间戳）
}

#[derive(Debug, Deserialize)]
struct GateioTicker {
    #[serde(rename = "currency_pair")]
    currency_pair: String,
    #[serde(rename = "highest_bid")]
    highest_bid: String,
    #[serde(rename = "lowest_ask")]
    lowest_ask: String,
    last: String,
    #[serde(rename = "quote_volume")]
    quote_volume: String,
}

#[derive(Debug, Deserialize)]
struct GateioFuturesTicker {
    contract: String,
    // 有些返回用 bid1/ask1，有些返回用 highest_bid/lowest_ask
    #[serde(default, rename = "bid1", alias = "highest_bid")]
    bid1_or_highest_bid: Option<String>,
    #[serde(default, rename = "ask1", alias = "lowest_ask")]
    ask1_or_lowest_ask: Option<String>,
    last: String,
    #[serde(rename = "volume_24h_base")]
    volume_24h_base: String,
}

impl GateioFuturesTicker {
    fn bid(&self) -> Option<&str> {
        self.bid1_or_highest_bid.as_deref()
    }
    fn ask(&self) -> Option<&str> {
        self.ask1_or_lowest_ask.as_deref()
    }
}
