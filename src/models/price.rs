use crate::models::exchange::{ExchangeType, MarketType};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 价格数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub exchange: ExchangeType,
    pub symbol: String,
    pub market_type: MarketType,
    pub bid_price: Decimal,
    pub ask_price: Decimal,
    pub last_price: Decimal,
    pub volume_24h: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// 资金费率数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    pub exchange: ExchangeType,
    pub symbol: String,
    pub rate: Decimal,
    pub next_funding_time: DateTime<Utc>,
    /// 资金费结算周期（小时）
    ///
    /// 注意：不要通过 next_funding_time - now 来“推断”结算周期，这只代表“距离下次结算还有多久”，
    /// 会产生 3/5/7 这类明显不合理的值。
    ///
    /// 仅当交易所 API 明确返回 interval 时才写入 Some(hours)，否则为 None（未知）。
    pub funding_interval_hours: Option<i32>,
    pub rate_limit_upper: Option<Decimal>, // 资金费率上限
    pub rate_limit_lower: Option<Decimal>, // 资金费率下限
    pub timestamp: DateTime<Utc>,
}

/// 基差数据（期货价格 - 现货价格）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Basis {
    pub exchange: ExchangeType,
    pub symbol: String,
    pub futures_price: Decimal,
    pub spot_price: Decimal,
    pub basis: Decimal, // 基差 = futures_price - spot_price
    pub basis_percentage: Decimal, // 基差百分比
    pub timestamp: DateTime<Utc>,
}

/// 历史价格数据（K线数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalPriceData {
    pub timestamp: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}