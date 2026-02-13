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

pub struct OkxAdapter {
    client: Client,
    base_url: String,
}

impl OkxAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))  // 增加超时时间
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: "https://www.okx.com".to_string(),
        }
    }
}

#[async_trait]
impl ExchangeAdapter for OkxAdapter {
    fn exchange_type(&self) -> ExchangeType {
        ExchangeType::Okx
    }

    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v5/public/instruments", self.base_url);
        let params = vec![("instType", "SPOT")];
        let resp: OkxResponse<Vec<OkxInstrument>> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse OKX instruments")?;

        if resp.code != "0" {
            return Err(anyhow::anyhow!("OKX API error: {}", resp.msg));
        }

        let pairs: Vec<TradingPair> = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter(|inst| inst.state == "live" && inst.inst_id.ends_with("USDT"))
            .map(|inst| {
                let base = inst.base_ccy.clone();
                let quote = inst.quote_ccy.clone();
                TradingPair {
                    symbol: inst.inst_id.clone(),
                    base,
                    quote,
                    exchange: ExchangeType::Okx,
                    is_spot: true,
                    is_futures: false,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>> {
        let url = format!("{}/api/v5/public/instruments", self.base_url);
        let params = vec![("instType", "SWAP")];
        let resp: OkxResponse<Vec<OkxInstrument>> = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse OKX futures instruments")?;

        if resp.code != "0" {
            return Err(anyhow::anyhow!("OKX API error: {}", resp.msg));
        }

        let pairs: Vec<TradingPair> = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .filter(|inst| inst.state == "live" && inst.inst_id.ends_with("USDT-SWAP"))
            .map(|inst| {
                // OKX 期货格式: BTC-USDT-SWAP，需要提取基础资产
                let symbol = inst.inst_id.clone();
                let parts: Vec<&str> = symbol.split('-').collect();
                let base = if parts.len() >= 2 {
                    parts[0].to_string()
                } else {
                    inst.base_ccy.clone()
                };
                let quote = if parts.len() >= 2 {
                    parts[1].to_string()
                } else {
                    inst.quote_ccy.clone()
                };
                TradingPair {
                    symbol: symbol.clone(),
                    base,
                    quote,
                    exchange: ExchangeType::Okx,
                    is_spot: false,
                    is_futures: true,
                }
            })
            .collect();

        Ok(pairs)
    }

    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v5/market/ticker", self.base_url);
        let params = vec![("instId", symbol)];
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if status == 429 {
            return Err(anyhow::anyhow!("OKX API error: Too Many Requests (HTTP 429)"));
        }
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OKX API error: HTTP {} for symbol {} (response: {})",
                status,
                symbol,
                if error_text.len() > 200 {
                    format!("{}...", &error_text[..200])
                } else {
                    error_text
                }
            ));
        }

        let resp: OkxResponse<Vec<OkxTicker>> = response
            .json()
            .await
            .context("Failed to parse OKX ticker")?;

        if resp.code != "0" {
            return Err(anyhow::anyhow!("OKX API error: {}", resp.msg));
        }

        let ticker = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .next()
            .context("No ticker data")?;

        Ok(PriceData {
            exchange: ExchangeType::Okx,
            symbol: symbol.to_string(),
            market_type: MarketType::Spot,
            bid_price: ticker.bid_px.parse()?,
            ask_price: ticker.ask_px.parse()?,
            last_price: ticker.last.parse()?,
            volume_24h: ticker.vol_24h.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData> {
        let url = format!("{}/api/v5/market/ticker", self.base_url);
        let params = vec![("instId", symbol)];
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if status == 429 {
            return Err(anyhow::anyhow!("OKX API error: Too Many Requests (HTTP 429)"));
        }
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OKX API error: HTTP {} for symbol {} (response: {})",
                status,
                symbol,
                if error_text.len() > 200 {
                    format!("{}...", &error_text[..200])
                } else {
                    error_text
                }
            ));
        }

        let resp: OkxResponse<Vec<OkxTicker>> = response
            .json()
            .await
            .context("Failed to parse OKX futures ticker")?;

        if resp.code != "0" {
            return Err(anyhow::anyhow!("OKX API error: {}", resp.msg));
        }

        let ticker = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .next()
            .context("No ticker data")?;

        Ok(PriceData {
            exchange: ExchangeType::Okx,
            symbol: symbol.to_string(),
            market_type: MarketType::Futures,
            bid_price: ticker.bid_px.parse()?,
            ask_price: ticker.ask_px.parse()?,
            last_price: ticker.last.parse()?,
            volume_24h: ticker.vol_24h.parse()?,
            timestamp: Utc::now(),
        })
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate> {
        let url = format!("{}/api/v5/public/funding-rate", self.base_url);
        let params = vec![("instId", symbol)];
        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await?;

        // 检查 HTTP 状态码
        let status = response.status();
        if status == 429 {
            return Err(anyhow::anyhow!("OKX API error: Too Many Requests (HTTP 429)"));
        }
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "OKX API error: HTTP {} for symbol {} (response: {})",
                status,
                symbol,
                if error_text.len() > 200 {
                    format!("{}...", &error_text[..200])
                } else {
                    error_text
                }
            ));
        }

        let resp: OkxResponse<Vec<OkxFundingRate>> = response
            .json()
            .await
            .context("Failed to parse OKX funding rate")?;

        if resp.code != "0" {
            return Err(anyhow::anyhow!("OKX API error: {}", resp.msg));
        }

        let funding = resp
            .data
            .unwrap_or_default()
            .into_iter()
            .next()
            .context("No funding rate data")?;

        // OKX API 返回的 nextFundingTime 是毫秒时间戳（字符串格式）
        let next_funding_time = funding.next_funding_time
            .parse::<i64>()
            .with_context(|| format!("Failed to parse next_funding_time as timestamp: '{}' for symbol {}", funding.next_funding_time, symbol))
            .and_then(|timestamp_ms| {
                DateTime::from_timestamp_millis(timestamp_ms)
                    .with_context(|| format!("Invalid timestamp value: {} for symbol {}", timestamp_ms, symbol))
            })?
            .with_timezone(&Utc);

        // 从 API 获取资金费率上下限（过滤空字符串）
        let rate_limit_upper = funding.max_funding_rate
            .as_ref()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<Decimal>().ok());
        let rate_limit_lower = funding.min_funding_rate
            .as_ref()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<Decimal>().ok());

        // OKX 该接口不返回 funding interval，不能硬编码，更不能用 nextFundingTime - now 推断
        let funding_interval_hours = None;

        Ok(FundingRate {
            exchange: ExchangeType::Okx,
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
        let url = format!("{}/api/v5/market/candles", self.base_url);
        
        // 转换时间间隔格式（OKX使用特殊格式）
        let okx_interval = match interval {
            "1m" => "1m",
            "3m" => "3m",
            "5m" => "5m",
            "15m" => "15m",
            "30m" => "30m",
            "1h" | "1H" => "1H",
            "2h" | "2H" => "2H",
            "4h" | "4H" => "4H",
            "6h" | "6H" => "6H",
            "12h" | "12H" => "12H",
            "1d" | "1D" => "1D",
            "1w" | "1W" => "1W",
            "1M" => "1M",
            _ => return Err(anyhow::anyhow!("Unsupported interval: {}", interval)),
        };
        
        let mut all_klines = Vec::new();
        let mut current_end = end_time;
        let limit = 300u64; // OKX最大限制
        
        // OKX K线API每次最多返回300条，需要分页获取（从后往前）
        while current_end > start_time {
            let before_ms = current_end.timestamp_millis();
            let before_ms_str = before_ms.to_string();
            let limit_str = limit.to_string();
            
            let params = vec![
                ("instId", symbol),
                ("bar", okx_interval),
                ("before", before_ms_str.as_str()),
                ("limit", limit_str.as_str()),
            ];
            
            let response = self
                .client
                .get(&url)
                .query(&params)
                .send()
                .await?;
            
            let status = response.status();
            if status == 429 {
                return Err(anyhow::anyhow!("OKX API error: Too Many Requests (HTTP 429)"));
            }
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "OKX Klines API error: HTTP {} for symbol {} (response: {})",
                    status,
                    symbol,
                    if error_text.len() > 200 {
                        format!("{}...", &error_text[..200])
                    } else {
                        error_text
                    }
                ));
            }
            
            let resp: OkxResponse<Vec<Vec<String>>> = response
                .json()
                .await
                .context("Failed to parse OKX klines response")?;
            
            if resp.code != "0" {
                return Err(anyhow::anyhow!("OKX Klines API error: {}", resp.msg));
            }
            
            let klines = resp.data.unwrap_or_default();
            
            if klines.is_empty() {
                break; // 没有更多数据
            }
            
            for kline in klines {
                // OKX K线数据格式：[时间戳(ms), 开盘价, 最高价, 最低价, 收盘价, 成交量, ...]
                if kline.len() < 6 {
                    continue; // 跳过格式不正确的数据
                }
                
                let timestamp_ms = kline[0]
                    .parse::<i64>()
                    .context("Invalid timestamp")?;
                let timestamp = DateTime::from_timestamp_millis(timestamp_ms)
                    .context("Invalid timestamp")?
                    .with_timezone(&Utc);
                
                if timestamp < start_time {
                    // 已到达开始时间，停止收集
                    break;
                }
                
                all_klines.push(HistoricalPriceData {
                    timestamp,
                    open: kline[1].parse()?,
                    high: kline[2].parse()?,
                    low: kline[3].parse()?,
                    close: kline[4].parse()?,
                    volume: kline[5].parse()?,
                });
            }
            
            // 更新结束时间为最早一条K线的时间
            if let Some(earliest) = all_klines.iter().min_by_key(|k| k.timestamp) {
                if earliest.timestamp <= start_time {
                    break; // 已覆盖所有时间范围
                }
                current_end = earliest.timestamp;
            } else {
                break; // 没有数据，退出循环
            }
            
            // 添加延迟，避免限流
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        
        // OKX返回的数据是倒序的，需要反转
        all_klines.reverse();
        
        Ok(all_klines)
    }
}

impl Default for OkxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// OKX API 响应结构
#[derive(Debug, Deserialize)]
struct OkxResponse<T> {
    code: String,
    msg: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct OkxInstrument {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "baseCcy")]
    base_ccy: String,
    #[serde(rename = "quoteCcy")]
    quote_ccy: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct OkxTicker {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "bidPx")]
    bid_px: String,
    #[serde(rename = "askPx")]
    ask_px: String,
    last: String,
    #[serde(rename = "vol24h")]
    vol_24h: String,
}

#[derive(Debug, Deserialize)]
struct OkxFundingRate {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "fundingRate")]
    funding_rate: String,
    #[serde(rename = "nextFundingTime")]
    next_funding_time: String,
    #[serde(rename = "maxFundingRate")]
    max_funding_rate: Option<String>, // 资金费率上限
    #[serde(rename = "minFundingRate")]
    min_funding_rate: Option<String>, // 资金费率下限
}
