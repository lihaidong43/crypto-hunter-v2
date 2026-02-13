use crate::models::exchange::{ExchangeType, MarketType};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 市场快照（统一价格和资金费率数据）
/// 每次收集都会创建一个新的快照时间点，保存到历史记录中
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSnapshot {
    /// 快照时间（所有数据的时间点）
    pub snapshot_time: DateTime<Utc>,
    
    /// 交易所
    pub exchange: ExchangeType,
    
    /// 交易对符号
    pub symbol: String,
    
    /// 市场类型（现货/期货）
    pub market_type: MarketType,
    
    // ========== 价格数据 ==========
    
    /// 买一价
    pub bid_price: Option<Decimal>,
    
    /// 卖一价
    pub ask_price: Option<Decimal>,
    
    /// 最新成交价
    pub last_price: Option<Decimal>,
    
    /// 24小时交易量
    pub volume_24h: Option<Decimal>,
    
    // ========== 资金费率数据 ==========
    
    /// 资金费率
    pub funding_rate: Option<Decimal>,
    
    /// 下次资金费结算时间
    pub next_funding_time: Option<DateTime<Utc>>,
    
    /// 资金费结算周期（小时）
    pub funding_interval_hours: Option<i32>,
    
    /// 资金费率上限
    pub rate_limit_upper: Option<Decimal>,
    
    /// 资金费率下限
    pub rate_limit_lower: Option<Decimal>,
}

impl MarketSnapshot {
    /// 创建新的快照
    pub fn new(
        snapshot_time: DateTime<Utc>,
        exchange: ExchangeType,
        symbol: String,
        market_type: MarketType,
    ) -> Self {
        Self {
            snapshot_time,
            exchange,
            symbol,
            market_type,
            bid_price: None,
            ask_price: None,
            last_price: None,
            volume_24h: None,
            funding_rate: None,
            next_funding_time: None,
            funding_interval_hours: None,
            rate_limit_upper: None,
            rate_limit_lower: None,
        }
    }
    
    /// 设置价格数据
    pub fn with_price_data(
        mut self,
        bid_price: Decimal,
        ask_price: Decimal,
        last_price: Decimal,
        volume_24h: Decimal,
    ) -> Self {
        self.bid_price = Some(bid_price);
        self.ask_price = Some(ask_price);
        self.last_price = Some(last_price);
        self.volume_24h = Some(volume_24h);
        self
    }
    
    /// 设置资金费率数据
    pub fn with_funding_rate_data(
        mut self,
        rate: Decimal,
        next_funding_time: DateTime<Utc>,
        funding_interval_hours: Option<i32>,
        rate_limit_upper: Option<Decimal>,
        rate_limit_lower: Option<Decimal>,
    ) -> Self {
        self.funding_rate = Some(rate);
        self.next_funding_time = Some(next_funding_time);
        self.funding_interval_hours = funding_interval_hours;
        self.rate_limit_upper = rate_limit_upper;
        self.rate_limit_lower = rate_limit_lower;
        self
    }
}

/// 快照统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotStats {
    /// 总快照数
    pub total_snapshots: i64,
    
    /// 最早时间
    pub earliest_time: Option<DateTime<Utc>>,
    
    /// 最晚时间
    pub latest_time: Option<DateTime<Utc>>,
    
    /// 交易所数量
    pub exchange_count: i64,
    
    /// 交易对数量
    pub symbol_count: i64,
}
