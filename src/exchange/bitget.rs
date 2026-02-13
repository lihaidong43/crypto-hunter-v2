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

pub struct BitgetAdapter {
    client: Client,
    base_url: String,
}

impl BitgetAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // 增加超时时间
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: "https://api.bitget.com".to_string(),
        }
    }
}

#[async_trait]
impl ExchangeAdapter for BitgetAdapter {
    fn exchange_type(&self) -> ExchangeType {
        ExchangeType::Bitget
    }

    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>> {
        // Bitget v1 API 已废弃，使用 v2 API
        // 正确的端点是 /api/v2/spot/public/symbols
        let url = format!("{}/api/v2/spot/public/symbols", self.base_url);
        let resp: BitgetResponse<Vec<BitgetSpotProduct>> = self
            .client
            .get(&url)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bitget spot symbols")?;

        if resp.code != "00000" {
            return Err(anyhow::anyhow!("Bitget API error: {} (code: {})", resp.msg, resp.code));
        }

        let pairs: Vec<TradingPair> = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter(|prod| prod.status == "online" && prod.quote_coin == "USDT")
            .map(|prod| TradingPair {
                symbol: prod.symbol.clone(),
                base: prod.base_coin.clone(),
                quote: prod.quote_coin.clone(),
                exchange: ExchangeType::Bitget,
                is_spot: true,
                is_futures: false,
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v2/mix/market/contracts", self.base_url);
        let params = vec![("productType", "USDT-FUTURES")];
        let resp: BitgetResponse<Vec<BitgetFuturesContract>> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bitget futures contracts")?;

        if resp.code != "00000" {
            return Err(anyhow::anyhow!("Bitget API error: {}", resp.msg));
        }

        let pairs: Vec<TradingPair> = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter(|contract| contract.symbol_status == "normal" && contract.symbol.ends_with("USDT"))
            .map(|contract| {
                // Bitget 期货格式: BTCUSDT，需要提取基础资产
                let symbol = contract.symbol.clone();
                let base = if symbol.len() > 4 && symbol.ends_with("USDT") {
                    symbol[..symbol.len() - 4].to_string()
                } else {
                    contract.base_coin.clone()
                };
                TradingPair {
                    symbol,
                    base,
                    quote: "USDT".to_string(),
                    exchange: ExchangeType::Bitget,
                    is_spot: false,
                    is_futures: true,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData> {
        // Bitget 现货 ticker API 可能不可用，返回错误或使用其他方式
        // 这里先返回一个占位实现
        Err(anyhow::anyhow!("Bitget spot ticker API not available"))
    }

    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v2/mix/market/ticker", self.base_url);
        let params = vec![("symbol", symbol), ("productType", "USDT-FUTURES")];
        
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;
        
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Bitget Futures API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }
        
        let resp: BitgetResponse<Vec<BitgetFuturesTicker>> = response
            .json()
            .await
            .context(format!("Failed to parse Bitget futures ticker for symbol {}", symbol))?;

        if resp.code != "00000" {
            return Err(anyhow::anyhow!(
                "Bitget API error: {} (code: {}) for symbol {}",
                resp.msg,
                resp.code,
                symbol
            ));
        }

        let ticker = resp
            .data
            .and_then(|mut v| v.pop())
            .context(format!("No ticker data for symbol {}", symbol))?;

        // 优先使用 baseVolume，如果为空则使用 quoteVolume 或 usdtVolume
        let volume = if !ticker.base_volume.is_empty() {
            ticker.base_volume.parse().ok()
        } else if !ticker.quote_volume.is_empty() {
            ticker.quote_volume.parse().ok()
        } else if !ticker.usdt_volume.is_empty() {
            ticker.usdt_volume.parse().ok()
        } else {
            None
        };

        Ok(PriceData {
            exchange: ExchangeType::Bitget,
            symbol: symbol.to_string(),
            market_type: MarketType::Futures,
            bid_price: ticker.bid_pr.parse()?,
            ask_price: ticker.ask_pr.parse()?,
            last_price: ticker.last_pr.parse()?,
            volume_24h: volume.unwrap_or_default(),
            timestamp: Utc::now(),
        })
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/api/v2/mix/market/current-fund-rate", self.base_url);
        let params = vec![("symbol", symbol), ("productType", "USDT-FUTURES")];
        let resp: BitgetResponse<Vec<BitgetFundingRate>> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bitget funding rate")?;

        if resp.code != "00000" {
            return Err(anyhow::anyhow!("Bitget API error: {}", resp.msg));
        }

        let funding = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .next()
            .context("No funding rate data")?;

        // Bitget API 返回的 nextUpdate 是毫秒时间戳
        let next_funding_time = funding.next_update
            .parse::<i64>()
            .with_context(|| format!("Failed to parse nextUpdate as timestamp: '{}' for symbol {}", funding.next_update, symbol))
            .and_then(|timestamp_ms| {
                DateTime::from_timestamp_millis(timestamp_ms)
                    .with_context(|| format!("Invalid timestamp value: {} for symbol {}", timestamp_ms, symbol))
            })?
            .with_timezone(&Utc);

        // 从 API 获取资金费结算周期
        // Bitget API 返回的 funding_rate_interval 可能是字符串格式（如 "8h" 或 "8"）
        let funding_interval_hours = funding.funding_rate_interval
            .trim()
            .trim_end_matches('h')
            .trim_end_matches('H')
            .parse::<i32>()
            .ok()
            .filter(|hours| *hours > 0);

        // 从 API 获取资金费率上下限（过滤空字符串）
        let rate_limit_upper = funding.max_funding_rate
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|d| !d.is_zero());
        let rate_limit_lower = funding.min_funding_rate
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|d| !d.is_zero());

        Ok(FundingRate {
            exchange: ExchangeType::Bitget,
            symbol: symbol.to_string(),
            rate: funding.funding_rate.parse()?,
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
        let url = format!("{}/api/mix/v1/market/candles", self.base_url);
        
        // 转换时间间隔格式（Bitget使用特殊格式）
        let bitget_interval = match interval {
            "1m" => "1m",
            "5m" => "5m",
            "15m" => "15m",
            "30m" => "30m",
            "1h" | "1H" => "1H",
            "4h" | "4H" => "4H",
            "12h" | "12H" => "12H",
            "1d" | "1D" => "1D",
            "1w" | "1W" => "1W",
            _ => return Err(anyhow::anyhow!("Unsupported interval: {}", interval)),
        };
        
        let mut all_klines = Vec::new();
        let mut current_start = start_time;
        
        // Bitget K线API需要分页获取
        while current_start < end_time {
            let start_ms = current_start.timestamp_millis();
            let end_ms = end_time.timestamp_millis();
            let start_ms_str = start_ms.to_string();
            let end_ms_str = end_ms.to_string();
            
            let params = vec![
                ("symbol", symbol),
                ("productType", "USDT-FUTURES"),
                ("granularity", bitget_interval),
                ("startTime", start_ms_str.as_str()),
                ("endTime", end_ms_str.as_str()),
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
                    "Bitget Klines API error: HTTP {} for symbol {} (response: {})",
                    status,
                    symbol,
                    if error_text.len() > 200 {
                        format!("{}...", &error_text[..200])
                    } else {
                        error_text
                    }
                ));
            }
            
            let resp: BitgetResponse<Vec<BitgetKline>> = response
                .json()
                .await
                .context("Failed to parse Bitget klines response")?;
            
            if resp.code != "00000" {
                return Err(anyhow::anyhow!("Bitget Klines API error: {} (code: {})", resp.msg, resp.code));
            }
            
            let klines = resp.data.unwrap_or_default();
            
            if klines.is_empty() {
                break; // 没有更多数据
            }
            
            for kline in klines {
                let timestamp_ms = kline.time
                    .parse::<i64>()
                    .context("Invalid timestamp")?;
                let timestamp = DateTime::from_timestamp_millis(timestamp_ms)
                    .context("Invalid timestamp")?
                    .with_timezone(&Utc);
                
                all_klines.push(HistoricalPriceData {
                    timestamp,
                    open: kline.open.parse()?,
                    high: kline.high.parse()?,
                    low: kline.low.parse()?,
                    close: kline.close.parse()?,
                    volume: kline.vol.parse()?,
                });
            }
            
            // 更新起始时间为最后一条K线的下一个时间点
            if let Some(last_kline) = all_klines.last() {
                let next_time = match interval {
                    "1m" => last_kline.timestamp + chrono::Duration::minutes(1),
                    "5m" => last_kline.timestamp + chrono::Duration::minutes(5),
                    "15m" => last_kline.timestamp + chrono::Duration::minutes(15),
                    "30m" => last_kline.timestamp + chrono::Duration::minutes(30),
                    "1h" => last_kline.timestamp + chrono::Duration::hours(1),
                    "4h" => last_kline.timestamp + chrono::Duration::hours(4),
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

impl Default for BitgetAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// Bitget API 响应结构
#[derive(Debug, Deserialize)]
struct BitgetResponse<T> {
    code: String,
    msg: String,
    #[serde(rename = "requestTime")]
    request_time: Option<i64>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct BitgetSpotProduct {
    symbol: String,
    #[serde(rename = "baseCoin")]
    base_coin: String,
    #[serde(rename = "quoteCoin")]
    quote_coin: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct BitgetFuturesContract {
    symbol: String,
    #[serde(rename = "baseCoin")]
    base_coin: String,
    #[serde(rename = "quoteCoin")]
    quote_coin: String,
    #[serde(rename = "symbolStatus")]
    symbol_status: String,
}

#[derive(Debug, Deserialize)]
struct BitgetSpotTicker {
    symbol: String,
    #[serde(rename = "bestBid")]
    best_bid: String,
    #[serde(rename = "bestAsk")]
    best_ask: String,
    close: String,
    #[serde(rename = "baseVolume24h")]
    base_volume_24h: String,
}

#[derive(Debug, Deserialize)]
struct BitgetFuturesTicker {
    symbol: String,
    #[serde(rename = "bidPr")]
    bid_pr: String,
    #[serde(rename = "askPr")]
    ask_pr: String,
    #[serde(rename = "lastPr")]
    last_pr: String,
    #[serde(rename = "baseVolume")]
    #[serde(default)]
    base_volume: String,
    #[serde(rename = "quoteVolume")]
    #[serde(default)]
    quote_volume: String,
    #[serde(rename = "usdtVolume")]
    #[serde(default)]
    usdt_volume: String,
}

#[derive(Debug, Deserialize)]
struct BitgetFundingRate {
    symbol: String,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "fundingRateInterval")]
    funding_rate_interval: String,
    #[serde(rename = "nextUpdate")]
    next_update: String,
    #[serde(rename = "minFundingRate")]
    min_funding_rate: String,
    #[serde(rename = "maxFundingRate")]
    max_funding_rate: String,
}

#[derive(Debug, Deserialize)]
struct BitgetKline {
    time: String, // 毫秒时间戳
    open: String,
    high: String,
    low: String,
    close: String,
    vol: String, // 成交量
}
