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

pub struct BybitAdapter {
    client: Client,
    base_url: String,
}

impl BybitAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // 增加超时时间
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: "https://api.bybit.com".to_string(),
        }
    }
}

#[async_trait]
impl ExchangeAdapter for BybitAdapter {
    fn exchange_type(&self) -> ExchangeType {
        ExchangeType::Bybit
    }

    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/v5/market/instruments-info", self.base_url);
        let params = vec![("category", "spot")];
        let resp: BybitResponse<BybitInstrumentsInfo> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bybit instruments info")?;

        if resp.ret_code != 0 {
            return Err(anyhow::anyhow!("Bybit API error: {}", resp.ret_msg));
        }

        let pairs: Vec<TradingPair> = resp
            .result
            .list
            .into_iter()
            .filter(|inst| inst.status == "Trading" && inst.symbol.ends_with("USDT"))
            .map(|inst| {
                TradingPair {
                    symbol: inst.symbol.clone(),
                    base: inst.base_coin.clone(),
                    quote: inst.quote_coin.clone(),
                    exchange: ExchangeType::Bybit,
                    is_spot: true,
                    is_futures: false,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/v5/market/instruments-info", self.base_url);
        let params = vec![("category", "linear")];
        let resp: BybitResponse<BybitInstrumentsInfo> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bybit futures instruments info")?;

        if resp.ret_code != 0 {
            return Err(anyhow::anyhow!("Bybit API error: {}", resp.ret_msg));
        }

        let pairs: Vec<TradingPair> = resp
            .result
            .list
            .into_iter()
            .filter(|inst| inst.status == "Trading" && inst.symbol.ends_with("USDT"))
            .map(|inst| {
                TradingPair {
                    symbol: inst.symbol.clone(),
                    base: inst.base_coin.clone(),
                    quote: inst.quote_coin.clone(),
                    exchange: ExchangeType::Bybit,
                    is_spot: false,
                    is_futures: true,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/v5/market/tickers", self.base_url);
        let params = vec![("category", "spot"), ("symbol", symbol)];
        let resp: BybitResponse<BybitTickersInfo> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bybit ticker")?;

        if resp.ret_code != 0 {
            return Err(anyhow::anyhow!("Bybit API error: {}", resp.ret_msg));
        }

        let ticker = resp
            .result
            .list
            .into_iter()
            .next()
            .context("No ticker data")?;

        Ok(PriceData {
            exchange: ExchangeType::Bybit,
            symbol: symbol.to_string(),
            market_type: MarketType::Spot,
            bid_price: ticker.bid1_price.parse()?,
            ask_price: ticker.ask1_price.parse()?,
            last_price: ticker.last_price.parse()?,
            volume_24h: ticker.volume_24h.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/v5/market/tickers", self.base_url);
        let params = vec![("category", "linear"), ("symbol", symbol)];
        let resp: BybitResponse<BybitTickersInfo> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bybit futures ticker")?;

        if resp.ret_code != 0 {
            return Err(anyhow::anyhow!("Bybit API error: {}", resp.ret_msg));
        }

        let ticker = resp
            .result
            .list
            .into_iter()
            .next()
            .context("No ticker data")?;

        Ok(PriceData {
            exchange: ExchangeType::Bybit,
            symbol: symbol.to_string(),
            market_type: MarketType::Futures,
            bid_price: ticker.bid1_price.parse()?,
            ask_price: ticker.ask1_price.parse()?,
            last_price: ticker.last_price.parse()?,
            volume_24h: ticker.volume_24h.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/v5/market/tickers", self.base_url);
        let params = vec![("category", "linear"), ("symbol", symbol)];
        let resp: BybitResponse<BybitTickersInfo> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse Bybit funding rate")?;

        if resp.ret_code != 0 {
            return Err(anyhow::anyhow!("Bybit API error: {}", resp.ret_msg));
        }

        let ticker = resp
            .result
            .list
            .into_iter()
            .next()
            .context("No ticker data")?;

        // 从 API 获取 next_funding_time
        let next_funding_time = if let Some(time_str) = &ticker.next_funding_time {
            time_str
                .parse::<i64>()
                .with_context(|| format!("Failed to parse next_funding_time: '{}'", time_str))
                .and_then(|timestamp_ms| {
                    DateTime::from_timestamp_millis(timestamp_ms)
                        .with_context(|| format!("Invalid timestamp: {}", timestamp_ms))
                })?
                .with_timezone(&Utc)
        } else {
            // 如果没有提供，计算下一个资金费时间（从当前时间开始的8小时周期）
            let now = Utc::now().timestamp();
            let funding_interval = 8 * 3600; // 默认8小时
            DateTime::from_timestamp(
                ((now / funding_interval) + 1) * funding_interval,
                0,
            )
            .context("Invalid next funding time")?
            .with_timezone(&Utc)
        };

        // 从 API 获取资金费结算周期
        let funding_interval_hours = ticker
            .funding_interval_hour
            .as_ref()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .filter(|hours| *hours > 0);

        // 从 API 获取资金费率上限（过滤空字符串）
        let rate_limit_upper = ticker.funding_cap
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.trim().parse::<Decimal>().ok())
            .filter(|d| !d.is_zero());
        // Bybit API 不提供下限，使用上限的负值
        let rate_limit_lower = rate_limit_upper.map(|upper| -upper);

        Ok(FundingRate {
            exchange: ExchangeType::Bybit,
            symbol: symbol.to_string(),
            rate: ticker.funding_rate.parse()?,
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
        let url = format!("{}/v5/market/kline", self.base_url);
        
        // 转换时间间隔格式（Bybit使用数字或字母格式）
        let bybit_interval = match interval {
            "1m" => "1",
            "3m" => "3",
            "5m" => "5",
            "15m" => "15",
            "30m" => "30",
            "1h" => "60",
            "2h" => "120",
            "4h" => "240",
            "6h" => "360",
            "12h" => "720",
            "1d" => "D",
            "1w" => "W",
            "1M" => "M",
            _ => return Err(anyhow::anyhow!("Unsupported interval: {}", interval)),
        };
        
        let mut all_klines = Vec::new();
        let mut current_start = start_time;
        let limit = 200u64; // Bybit最大限制
        
        // Bybit K线API每次最多返回200条，需要分页获取
        while current_start < end_time {
            let start_ms = current_start.timestamp_millis();
            let end_ms = end_time.timestamp_millis();
            let start_ms_str = start_ms.to_string();
            let end_ms_str = end_ms.to_string();
            let limit_str = limit.to_string();
            
            let params = vec![
                ("category", "linear"),
                ("symbol", symbol),
                ("interval", bybit_interval),
                ("start", start_ms_str.as_str()),
                ("end", end_ms_str.as_str()),
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
                    "Bybit Klines API error: HTTP {} for symbol {} (response: {})",
                    status,
                    symbol,
                    if error_text.len() > 200 {
                        format!("{}...", &error_text[..200])
                    } else {
                        error_text
                    }
                ));
            }
            
            let resp: BybitResponse<BybitKlinesInfo> = response
                .json()
                .await
                .context("Failed to parse Bybit klines response")?;
            
            if resp.ret_code != 0 {
                return Err(anyhow::anyhow!("Bybit Klines API error: {}", resp.ret_msg));
            }
            
            let klines = resp.result.list;
            
            if klines.is_empty() {
                break; // 没有更多数据
            }
            
            for kline in klines {
                // Bybit K线数据格式：[开始时间(ms), 开盘价, 最高价, 最低价, 收盘价, 成交量, ...]
                if kline.len() < 6 {
                    continue; // 跳过格式不正确的数据
                }
                
                let timestamp_ms = kline[0]
                    .parse::<i64>()
                    .context("Invalid timestamp")?;
                let timestamp = DateTime::from_timestamp_millis(timestamp_ms)
                    .context("Invalid timestamp")?
                    .with_timezone(&Utc);
                
                all_klines.push(HistoricalPriceData {
                    timestamp,
                    open: kline[1].parse()?,
                    high: kline[2].parse()?,
                    low: kline[3].parse()?,
                    close: kline[4].parse()?,
                    volume: kline[5].parse()?,
                });
            }
            
            // 更新起始时间为最后一条K线的下一个时间点
            if let Some(last_kline) = all_klines.last() {
                // 根据interval计算下一个时间点
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
                    "12h" => last_kline.timestamp + chrono::Duration::hours(12),
                    "1d" => last_kline.timestamp + chrono::Duration::days(1),
                    "1w" => last_kline.timestamp + chrono::Duration::weeks(1),
                    "1M" => last_kline.timestamp + chrono::Duration::days(30),
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

impl Default for BybitAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// Bybit API 响应结构
#[derive(Debug, Deserialize)]
struct BybitResponse<T> {
    #[serde(rename = "retCode")]
    ret_code: i32,
    #[serde(rename = "retMsg")]
    ret_msg: String,
    result: T,
}

#[derive(Debug, Deserialize)]
struct BybitInstrumentsInfo {
    list: Vec<BybitInstrument>,
}

#[derive(Debug, Deserialize)]
struct BybitInstrument {
    symbol: String,
    #[serde(rename = "baseCoin")]
    base_coin: String,
    #[serde(rename = "quoteCoin")]
    quote_coin: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct BybitTickersInfo {
    list: Vec<BybitTicker>,
}

#[derive(Debug, Deserialize)]
struct BybitTicker {
    symbol: String,
    #[serde(rename = "bid1Price")]
    bid1_price: String,
    #[serde(rename = "ask1Price")]
    ask1_price: String,
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "volume24h")]
    volume_24h: String,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "nextFundingTime")]
    next_funding_time: Option<String>, // 毫秒时间戳字符串
    #[serde(rename = "fundingIntervalHour")]
    funding_interval_hour: Option<String>, // 资金费结算周期（小时）
    #[serde(rename = "fundingCap")]
    funding_cap: Option<String>, // 资金费率上限
}

#[derive(Debug, Deserialize)]
struct BybitKlinesInfo {
    list: Vec<Vec<String>>,
}
