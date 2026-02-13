use async_trait::async_trait;
use crate::models::{
    exchange::{ExchangeType, TradingPair},
    price::{FundingRate, PriceData},
};
use anyhow::Result;
use chrono::{DateTime, Utc};

/// 交易所适配器 trait
/// 所有交易所都需要实现这个接口，便于扩展新的交易所
#[async_trait]
pub trait ExchangeAdapter: Send + Sync {
    /// 获取交易所类型
    fn exchange_type(&self) -> ExchangeType;
    
    /// 获取交易所名称
    fn name(&self) -> String {
        self.exchange_type().to_string()
    }
    
    /// 获取所有交易对（现货）
    async fn fetch_spot_pairs(&self) -> Result<Vec<TradingPair>>;
    
    /// 获取所有交易对（期货）
    async fn fetch_futures_pairs(&self) -> Result<Vec<TradingPair>>;
    
    /// 获取现货价格
    async fn fetch_spot_price(&self, symbol: &str) -> Result<PriceData>;
    
    /// 获取期货价格
    async fn fetch_futures_price(&self, symbol: &str) -> Result<PriceData>;
    
    /// 获取资金费率
    async fn fetch_funding_rate(&self, symbol: &str) -> Result<FundingRate>;
    
    /// 获取历史价格数据（K线数据）
    /// 
    /// # 参数
    /// - `symbol`: 交易对符号
    /// - `start_time`: 开始时间
    /// - `end_time`: 结束时间
    /// - `interval`: K线间隔，如 "1m", "5m", "1h", "1d"
    /// 
    /// # 返回
    /// 历史价格数据列表，按时间升序排列
    async fn fetch_historical_prices(
        &self,
        symbol: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        interval: &str,
    ) -> Result<Vec<crate::models::price::HistoricalPriceData>>;
    
    /// 批量获取现货价格
    async fn fetch_spot_prices_batch(&self, symbols: &[String]) -> Result<Vec<PriceData>> {
        let mut results = Vec::new();
        for symbol in symbols {
            if let Ok(price) = self.fetch_spot_price(symbol).await {
                results.push(price);
            }
        }
        Ok(results)
    }
    
    /// 批量获取期货价格
    async fn fetch_futures_prices_batch(&self, symbols: &[String]) -> Result<Vec<PriceData>> {
        let mut results = Vec::new();
        for symbol in symbols {
            if let Ok(price) = self.fetch_futures_price(symbol).await {
                results.push(price);
            }
        }
        Ok(results)
    }
}

/// 交易所适配器管理器
pub struct ExchangeManager {
    adapters: Vec<Box<dyn ExchangeAdapter>>,
}

impl ExchangeManager {
    pub fn new() -> Self {
        Self {
            adapters: Vec::new(),
        }
    }
    
    pub fn add_adapter(&mut self, adapter: Box<dyn ExchangeAdapter>) {
        self.adapters.push(adapter);
    }
    
    pub fn get_adapter(&self, exchange_type: ExchangeType) -> Option<&dyn ExchangeAdapter> {
        self.adapters
            .iter()
            .find(|a| a.exchange_type() == exchange_type)
            .map(|a| a.as_ref())
    }
    
    pub fn get_all_adapters(&self) -> &[Box<dyn ExchangeAdapter>] {
        &self.adapters
    }
}

impl Default for ExchangeManager {
    fn default() -> Self {
        Self::new()
    }
}