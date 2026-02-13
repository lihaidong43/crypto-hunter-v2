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
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct BinanceAdapter {
    client: Client,
    base_url: String,
    futures_base_url: String,
}

impl BinanceAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // 增加超时时间，exchangeInfo 数据量大
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: "https://api.binance.com".to_string(),
            futures_base_url: "https://fapi.binance.com".to_string(),
        }
    }
}

#[async_trait]
impl ExchangeAdapter for BinanceAdapter {
    fn exchange_type(&self) -> ExchangeType {
        ExchangeType::Binance
    }

    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v3/exchangeInfo", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Binance spot API")?;
        
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Binance spot API error: HTTP {} - {}",
                status,
                text.chars().take(200).collect::<String>()
            ));
        }
        
        let text = response.text().await.context("Failed to read Binance response body")?;
        let resp: BinanceExchangeInfo = serde_json::from_str(&text)
            .context(format!(
                "Failed to parse Binance exchange info, response length: {}, first 200 chars: {}",
                text.len(),
                text.chars().take(200).collect::<String>()
            ))?;

        let pairs: Vec<TradingPair> = resp
            .symbols
            .into_iter()
            .filter(|s| s.status == "TRADING" && s.symbol.ends_with("USDT"))
            .map(|s| TradingPair {
                symbol: s.symbol.clone(),
                base: s.base_asset.clone(),
                quote: s.quote_asset.clone(),
                exchange: ExchangeType::Binance,
                is_spot: true,
                is_futures: false,
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/fapi/v1/exchangeInfo", self.futures_base_url);
        let resp: BinanceExchangeInfo = self
            .client
            .get(&url)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Binance futures exchange info")?;

        let pairs: Vec<TradingPair> = resp
            .symbols
            .into_iter()
            .filter(|s| s.status == "TRADING" && s.symbol.ends_with("USDT"))
            .map(|s| TradingPair {
                symbol: s.symbol.clone(),
                base: s.base_asset.clone(),
                quote: s.quote_asset.clone(),
                exchange: ExchangeType::Binance,
                is_spot: false,
                is_futures: true,
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v3/ticker/24hr", self.base_url);
        let params = vec![("symbol", symbol)];
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
                "Binance API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }

        // 先尝试解析为错误响应
        let text = response.text().await?;
        if let Ok(error) = serde_json::from_str::<BinanceError>(&text) {
            return Err(anyhow::anyhow!(
                "Binance API error: {} (code: {}) for symbol {}",
                error.msg,
                error.code,
                symbol
            ));
        }

        // 解析为 ticker 数据
        let ticker: BinanceTicker = serde_json::from_str(&text)
            .context(format!("Failed to parse Binance ticker response for symbol {}", symbol))?;

        Ok(PriceData {
            exchange: ExchangeType::Binance,
            symbol: symbol.to_string(),
            market_type: MarketType::Spot,
            bid_price: ticker.bid_price
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_else(|| ticker.last_price.parse().unwrap_or_default()),
            ask_price: ticker.ask_price
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_else(|| ticker.last_price.parse().unwrap_or_default()),
            last_price: ticker.last_price.parse()?,
            volume_24h: ticker.volume
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_default(),
            timestamp: Utc::now(),
        })
    }

    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/fapi/v1/ticker/24hr", self.futures_base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("symbol", symbol)])
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Binance Futures API error: HTTP {} for symbol {} (response: {})",
                status,
                symbol,
                if error_text.len() > 200 {
                    format!("{}...", &error_text[..200])
                } else {
                    error_text
                }
            ));
        }

        // 先尝试解析为错误响应
        let text = response.text().await?;
        
        // 检查是否为空响应
        if text.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Binance Futures API error: Empty response for symbol {}",
                symbol
            ));
        }
        
        if let Ok(error) = serde_json::from_str::<BinanceError>(&text) {
            return Err(anyhow::anyhow!(
                "Binance Futures API error: {} (code: {}) for symbol {}",
                error.msg,
                error.code,
                symbol
            ));
        }

        // 解析为 ticker 数据
        let ticker: BinanceTicker = serde_json::from_str(&text)
            .with_context(|| format!(
                "Failed to parse Binance futures ticker response for symbol {} (response: {})",
                symbol,
                if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.clone()
                }
            ))?;

        Ok(PriceData {
            exchange: ExchangeType::Binance,
            symbol: symbol.to_string(),
            market_type: MarketType::Futures,
            bid_price: ticker.bid_price
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_else(|| ticker.last_price.parse().unwrap_or_default()),
            ask_price: ticker.ask_price
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_else(|| ticker.last_price.parse().unwrap_or_default()),
            last_price: ticker.last_price.parse()?,
            volume_24h: ticker.volume
                .as_ref()
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or_default(),
            timestamp: Utc::now(),
        })
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/fapi/v1/premiumIndex", self.futures_base_url);
        let response = self
            .client
            .get(&url)
            .query(&[("symbol", symbol)])
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Binance Futures API error: HTTP {} for symbol {}",
                status,
                symbol
            ));
        }

        // 先尝试解析为错误响应
        let text = response.text().await?;
        if let Ok(error) = serde_json::from_str::<BinanceError>(&text) {
            return Err(anyhow::anyhow!(
                "Binance Futures API error: {} (code: {}) for symbol {}",
                error.msg,
                error.code,
                symbol
            ));
        }

        // 解析为 premium index 数据
        let premium: BinancePremiumIndex = serde_json::from_str(&text)
            .context(format!("Failed to parse Binance premium index response for symbol {}", symbol))?;

        let next_funding_time = DateTime::from_timestamp_millis(premium.next_funding_time)
            .context("Invalid next funding time")?
            .with_timezone(&Utc);

        // Binance 该接口不返回 funding interval，不能硬编码，更不能用 nextFundingTime - now 推断
        let funding_interval_hours = None;

        // Binance API 不直接返回 rate_limit，使用 None（表示未知）
        // 实际应用中可以通过其他接口获取或使用默认值
        Ok(FundingRate {
            exchange: ExchangeType::Binance,
            symbol: symbol.to_string(),
            rate: premium.last_funding_rate.parse()?,
            next_funding_time,
            funding_interval_hours,
            rate_limit_upper: None, // Binance API 不提供此字段
            rate_limit_lower: None, // Binance API 不提供此字段
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
        // Binance K线API支持现货和期货，这里使用期货API（fapi）
        // 如果需要现货，可以使用 base_url + /api/v3/klines
        let url = format!("{}/fapi/v1/klines", self.futures_base_url);
        
        // 转换时间间隔格式（Binance使用特殊格式）
        let binance_interval = match interval {
            "1m" => "1m",
            "3m" => "3m",
            "5m" => "5m",
            "15m" => "15m",
            "30m" => "30m",
            "1h" => "1h",
            "2h" => "2h",
            "4h" => "4h",
            "6h" => "6h",
            "8h" => "8h",
            "12h" => "12h",
            "1d" => "1d",
            "3d" => "3d",
            "1w" => "1w",
            "1M" => "1M",
            _ => return Err(anyhow::anyhow!("Unsupported interval: {}", interval)),
        };
        
        let mut all_klines = Vec::new();
        let mut current_start = start_time;
        let limit = 1000u64; // Binance最大限制
        
        // Binance K线API每次最多返回1000条，需要分页获取
        while current_start < end_time {
            let start_ms = current_start.timestamp_millis();
            let end_ms = end_time.timestamp_millis();
            let start_ms_str = start_ms.to_string();
            let end_ms_str = end_ms.to_string();
            let limit_str = limit.to_string();
            
            let params = vec![
                ("symbol", symbol),
                ("interval", binance_interval),
                ("startTime", start_ms_str.as_str()),
                ("endTime", end_ms_str.as_str()),
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
                    "Binance Klines API error: HTTP {} for symbol {} (response: {})",
                    status,
                    symbol,
                    if error_text.len() > 200 {
                        format!("{}...", &error_text[..200])
                    } else {
                        error_text
                    }
                ));
            }
            
            let text = response.text().await?;
            if let Ok(error) = serde_json::from_str::<BinanceError>(&text) {
                return Err(anyhow::anyhow!(
                    "Binance Klines API error: {} (code: {}) for symbol {}",
                    error.msg,
                    error.code,
                    symbol
                ));
            }
            
            // Binance K线数据格式：[[开盘时间, 开盘价, 最高价, 最低价, 收盘价, 成交量, ...], ...]
            let klines: Vec<Vec<serde_json::Value>> = serde_json::from_str(&text)
                .context(format!("Failed to parse Binance klines response for symbol {}", symbol))?;
            
            if klines.is_empty() {
                break; // 没有更多数据
            }
            
            for kline in klines {
                // 解析K线数据：[开盘时间(ms), 开盘价, 最高价, 最低价, 收盘价, 成交量, ...]
                let open_time_ms = kline[0]
                    .as_i64()
                    .context("Invalid open time")?;
                let timestamp = DateTime::from_timestamp_millis(open_time_ms)
                    .context("Invalid timestamp")?
                    .with_timezone(&Utc);
                
                all_klines.push(HistoricalPriceData {
                    timestamp,
                    open: kline[1].as_str().context("Invalid open price")?.parse()?,
                    high: kline[2].as_str().context("Invalid high price")?.parse()?,
                    low: kline[3].as_str().context("Invalid low price")?.parse()?,
                    close: kline[4].as_str().context("Invalid close price")?.parse()?,
                    volume: kline[5].as_str().context("Invalid volume")?.parse()?,
                });
            }
            
            // 更新起始时间为最后一条K线的下一个时间点
            if let Some(last_kline) = all_klines.last() {
                // 根据interval计算下一个时间点
                let next_time = match interval {
                    "1m" => last_kline.timestamp + chrono::Duration::minutes(1),
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
                    "1M" => {
                        // 月份处理：简单加30天
                        last_kline.timestamp + chrono::Duration::days(30)
                    }
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

impl Default for BinanceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// Binance API 响应结构
#[derive(Debug, Deserialize)]
struct BinanceExchangeInfo {
    symbols: Vec<BinanceSymbol>,
}

#[derive(Debug, Deserialize)]
struct BinanceSymbol {
    symbol: String,
    #[serde(rename = "baseAsset")]
    base_asset: String,
    #[serde(rename = "quoteAsset")]
    quote_asset: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct BinanceTicker {
    #[serde(rename = "symbol")]
    symbol: String,
    #[serde(rename = "bidPrice")]
    #[serde(default)]
    bid_price: Option<String>,
    #[serde(rename = "askPrice")]
    #[serde(default)]
    ask_price: Option<String>,
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(default)]
    volume: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i64,
    msg: String,
}

#[derive(Debug, Deserialize)]
struct BinancePremiumIndex {
    #[serde(rename = "symbol")]
    symbol: String,
    #[serde(rename = "lastFundingRate")]
    last_funding_rate: String,
    #[serde(rename = "nextFundingTime")]
    next_funding_time: i64,
    // Binance API 不直接返回 funding_interval 和 rate_limit
    // 需要从其他接口获取或使用默认值
}