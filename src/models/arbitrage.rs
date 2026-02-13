use crate::models::exchange::ExchangeType;
use crate::models::snapshot::MarketSnapshot;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 套利分析结果（用于查询，不存储到数据库）
/// 基于两个交易所的快照计算套利机会
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageAnalysis {
    /// 交易对符号
    pub symbol: String,
    
    /// 交易所 A
    pub exchange_a: ExchangeType,
    
    /// 交易所 B
    pub exchange_b: ExchangeType,
    
    /// 快照时间
    pub snapshot_time: DateTime<Utc>,
    
    // ========== 价格数据 ==========
    
    /// 交易所 A 的最新成交价
    pub price_a: Decimal,
    
    /// 交易所 B 的最新成交价
    pub price_b: Decimal,
    
    /// 交易所 A 的买一价
    pub bid_price_a: Option<Decimal>,
    
    /// 交易所 A 的卖一价
    pub ask_price_a: Option<Decimal>,
    
    /// 交易所 B 的买一价
    pub bid_price_b: Option<Decimal>,
    
    /// 交易所 B 的卖一价
    pub ask_price_b: Option<Decimal>,
    
    // ========== 价差 ==========
    
    /// 开仓价差 (2 × (Price_A - Price_B)) / (Price_A + Price_B) × 100%
    pub open_spread: Decimal,
    
    /// 清仓价差
    pub close_spread: Decimal,
    
    // ========== 资金费率 ==========
    
    /// 交易所 A 的资金费率
    pub funding_rate_a: Option<Decimal>,
    
    /// 交易所 B 的资金费率
    pub funding_rate_b: Option<Decimal>,
    
    /// 净资金费率（A + B）
    pub net_funding_rate: Option<Decimal>,
    
    /// 交易所 A 的资金费结算周期（小时）
    pub funding_interval_a: Option<i32>,
    
    /// 交易所 B 的资金费结算周期（小时）
    pub funding_interval_b: Option<i32>,
    
    // ========== 交易量 ==========
    
    /// 交易所 A 的 24 小时交易量
    pub volume_24h_a: Option<Decimal>,
    
    /// 交易所 B 的 24 小时交易量
    pub volume_24h_b: Option<Decimal>,
}

impl ArbitrageAnalysis {
    /// 从两个快照创建套利分析
    pub fn from_snapshots(
        snapshot_a: &MarketSnapshot,
        snapshot_b: &MarketSnapshot,
        open_spread: Decimal,
        close_spread: Decimal,
    ) -> Self {
        // 计算净资金费率
        let net_funding_rate = if let (Some(rate_a), Some(rate_b)) =
            (snapshot_a.funding_rate, snapshot_b.funding_rate)
        {
            Some(rate_a + rate_b)
        } else {
            None
        };

        Self {
            symbol: snapshot_a.symbol.clone(),
            exchange_a: snapshot_a.exchange,
            exchange_b: snapshot_b.exchange,
            snapshot_time: snapshot_a.snapshot_time,
            price_a: snapshot_a.last_price.unwrap_or_default(),
            price_b: snapshot_b.last_price.unwrap_or_default(),
            bid_price_a: snapshot_a.bid_price,
            ask_price_a: snapshot_a.ask_price,
            bid_price_b: snapshot_b.bid_price,
            ask_price_b: snapshot_b.ask_price,
            open_spread,
            close_spread,
            funding_rate_a: snapshot_a.funding_rate,
            funding_rate_b: snapshot_b.funding_rate,
            net_funding_rate,
            funding_interval_a: snapshot_a.funding_interval_hours,
            funding_interval_b: snapshot_b.funding_interval_hours,
            volume_24h_a: snapshot_a.volume_24h,
            volume_24h_b: snapshot_b.volume_24h,
        }
    }
}

/// 套利查询过滤器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageFilters {
    /// 交易对符号（可选）
    pub symbol: Option<String>,
    
    /// 交易所 A（可选）
    pub exchange_a: Option<ExchangeType>,
    
    /// 交易所 B（可选）
    pub exchange_b: Option<ExchangeType>,
    
    /// 最小开仓价差（百分比）
    pub min_open_spread: Option<Decimal>,
    
    /// 最大开仓价差（百分比）
    pub max_open_spread: Option<Decimal>,
    
    /// 最小净资金费率
    pub min_net_funding_rate: Option<Decimal>,
    
    /// 最大净资金费率
    pub max_net_funding_rate: Option<Decimal>,
    
    /// 最小交易量（24小时）
    pub min_volume_24h: Option<Decimal>,
    
    /// 快照时间（可选，用于查询历史数据）
    pub snapshot_time: Option<DateTime<Utc>>,
}