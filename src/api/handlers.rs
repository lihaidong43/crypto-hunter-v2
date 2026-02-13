use crate::arbitrage::analyzer::ArbitrageAnalyzer;
use crate::models::arbitrage::{ArbitrageAnalysis, ArbitrageFilters};
use crate::storage::Repository;
use anyhow::Result;
use std::sync::Arc;

/// API处理器
/// 提供套利查询接口
pub struct ApiHandlers {
    repository: Arc<Repository>,
    analyzer: Arc<ArbitrageAnalyzer>,
}

impl ApiHandlers {
    pub fn new(repository: Arc<Repository>, analyzer: Arc<ArbitrageAnalyzer>) -> Self {
        Self {
            repository,
            analyzer,
        }
    }

    /// 查询套利机会
    /// 
    /// # Arguments
    /// * `symbol` - 交易对符号（可选）
    /// * `exchange_a` - 交易所 A（可选）
    /// * `exchange_b` - 交易所 B（可选）
    /// * `min_spread` - 最小价差（可选）
    /// * `max_spread` - 最大价差（可选）
    pub async fn get_arbitrage_opportunities(
        &self,
        symbol: Option<String>,
        exchange_a: Option<String>,
        exchange_b: Option<String>,
        min_spread: Option<f64>,
        max_spread: Option<f64>,
        min_net_funding_rate: Option<f64>,
        max_net_funding_rate: Option<f64>,
        min_volume_24h: Option<f64>,
    ) -> Result<Vec<ArbitrageAnalysis>> {
        use crate::models::exchange::ExchangeType;
        use rust_decimal::Decimal;

        let filters = ArbitrageFilters {
            symbol,
            exchange_a: exchange_a.map(|s| ExchangeType::from(s.as_str())),
            exchange_b: exchange_b.map(|s| ExchangeType::from(s.as_str())),
            min_open_spread: min_spread.map(|v| Decimal::try_from(v).unwrap_or_default()),
            max_open_spread: max_spread.map(|v| Decimal::try_from(v).unwrap_or_default()),
            min_net_funding_rate: min_net_funding_rate
                .map(|v| Decimal::try_from(v).unwrap_or_default()),
            max_net_funding_rate: max_net_funding_rate
                .map(|v| Decimal::try_from(v).unwrap_or_default()),
            min_volume_24h: min_volume_24h.map(|v| Decimal::try_from(v).unwrap_or_default()),
            snapshot_time: None,
        };

        self.analyzer.find_opportunities(filters).await
    }

    /// 查询指定 symbol 的套利机会
    pub async fn get_arbitrage_by_symbol(
        &self,
        symbol: &str,
        snapshot_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<ArbitrageAnalysis>> {
        self.analyzer.analyze_symbol(symbol, snapshot_time).await
    }

    /// 获取快照统计信息
    pub async fn get_snapshot_stats(
        &self,
        symbol: Option<&str>,
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<crate::models::snapshot::SnapshotStats> {
        self.repository.get_snapshot_stats(symbol, start, end).await
    }
}

/// 创建API路由（占位实现）
/// 实际实现需要使用 axum 或其他 web 框架
pub fn create_router(
    _repository: Arc<Repository>,
    _analyzer: Arc<ArbitrageAnalyzer>,
) -> () {
    // TODO: 使用axum实现API路由
    // 示例端点：
    // GET /api/v1/arbitrage/opportunities - 查询套利机会
    // GET /api/v1/arbitrage/opportunities/{symbol} - 查询指定交易对的套利机会
    // GET /api/v1/snapshots/stats - 获取快照统计信息
    // GET /api/v1/snapshots/{symbol} - 查询快照数据
}