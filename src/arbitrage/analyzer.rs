use crate::arbitrage::calculator::ArbitrageCalculator;
use crate::models::arbitrage::{ArbitrageAnalysis, ArbitrageFilters};
use crate::models::exchange::{ArbitrageType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use crate::storage::repository::Repository;
use anyhow::Result;
use chrono::DateTime;
use std::sync::Arc;

/// 套利分析服务（核心查询功能）
/// 基于快照数据计算套利机会
pub struct ArbitrageAnalyzer {
    repository: Arc<Repository>,
}

impl ArbitrageAnalyzer {
    /// 创建新的套利分析器
    pub fn new(repository: Arc<Repository>) -> Self {
        Self { repository }
    }

    /// 分析指定 symbol 在所有交易所之间的套利机会
    /// 
    /// # Arguments
    /// * `symbol` - 交易对符号
    /// * `snapshot_time` - 快照时间（可选，如果为 None 则使用最新快照）
    pub async fn analyze_symbol(
        &self,
        symbol: &str,
        snapshot_time: Option<DateTime<chrono::Utc>>,
    ) -> Result<Vec<ArbitrageAnalysis>> {
        // 1. 获取指定时间点的所有交易所快照（或最新快照）
        let snapshots = if let Some(time) = snapshot_time {
            self.repository.get_snapshots_at_time(symbol, time).await?
        } else {
            self.repository.get_latest_snapshots(symbol).await?
        };

        if snapshots.len() < 2 {
            return Ok(vec![]);
        }

        // 2. 计算所有交易所对之间的套利机会
        let mut analyses = Vec::new();
        for i in 0..snapshots.len() {
            for j in (i + 1)..snapshots.len() {
                let snapshot_a = &snapshots[i];
                let snapshot_b = &snapshots[j];

                // 确保两个快照都有价格数据
                if snapshot_a.last_price.is_none() || snapshot_b.last_price.is_none() {
                    continue;
                }

                match self.calculate_arbitrage(snapshot_a, snapshot_b) {
                    Ok(analysis) => analyses.push(analysis),
                    Err(e) => {
                        tracing::warn!(
                            "计算套利机会失败 {} {}: {}",
                            snapshot_a.exchange,
                            snapshot_b.exchange,
                            e
                        );
                    }
                }
            }
        }

        Ok(analyses)
    }

    /// 计算两个快照之间的套利机会
    fn calculate_arbitrage(
        &self,
        snapshot_a: &MarketSnapshot,
        snapshot_b: &MarketSnapshot,
    ) -> Result<ArbitrageAnalysis> {
        let price_a = snapshot_a
            .last_price
            .ok_or_else(|| anyhow::anyhow!("Snapshot A missing last_price"))?;
        let price_b = snapshot_b
            .last_price
            .ok_or_else(|| anyhow::anyhow!("Snapshot B missing last_price"))?;

        // 计算价差
        let open_spread = ArbitrageCalculator::calculate_spread(price_a, price_b)?;
        let close_spread = ArbitrageCalculator::calculate_close_spread(open_spread);

        // 创建套利分析结果
        Ok(ArbitrageAnalysis::from_snapshots(
            snapshot_a,
            snapshot_b,
            open_spread,
            close_spread,
        ))
    }

    /// 查询符合条件的套利机会
    /// 
    /// # Arguments
    /// * `filters` - 查询过滤器
    pub async fn find_opportunities(
        &self,
        filters: ArbitrageFilters,
    ) -> Result<Vec<ArbitrageAnalysis>> {
        // 获取快照
        let snapshots = if let Some(time) = filters.snapshot_time {
            if let Some(symbol) = &filters.symbol {
                self.repository.get_snapshots_at_time(symbol, time).await?
            } else {
                // 如果没有指定 symbol，需要查询所有 symbol
                // 这里简化处理，只返回空结果
                return Ok(vec![]);
            }
        } else {
            if let Some(symbol) = &filters.symbol {
                self.repository.get_latest_snapshots(symbol).await?
            } else {
                return Ok(vec![]);
            }
        };

        // 过滤交易所
        let filtered_snapshots: Vec<&MarketSnapshot> = snapshots
            .iter()
            .filter(|s| {
                if let Some(ex_a) = filters.exchange_a {
                    if s.exchange != ex_a {
                        return false;
                    }
                }
                if let Some(ex_b) = filters.exchange_b {
                    if s.exchange != ex_b {
                        return false;
                    }
                }
                true
            })
            .collect();

        if filtered_snapshots.len() < 2 {
            return Ok(vec![]);
        }

        // 计算所有交易所对之间的套利机会
        let mut analyses = Vec::new();
        for i in 0..filtered_snapshots.len() {
            for j in (i + 1)..filtered_snapshots.len() {
                let snapshot_a = filtered_snapshots[i];
                let snapshot_b = filtered_snapshots[j];

                if snapshot_a.last_price.is_none() || snapshot_b.last_price.is_none() {
                    continue;
                }

                match self.calculate_arbitrage(snapshot_a, snapshot_b) {
                    Ok(analysis) => {
                        // 应用过滤器
                        if let Some(min_spread) = filters.min_open_spread {
                            if analysis.open_spread < min_spread {
                                continue;
                            }
                        }
                        if let Some(max_spread) = filters.max_open_spread {
                            if analysis.open_spread > max_spread {
                                continue;
                            }
                        }
                        if let Some(min_net_rate) = filters.min_net_funding_rate {
                            if let Some(net_rate) = analysis.net_funding_rate {
                                if net_rate < min_net_rate {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        if let Some(max_net_rate) = filters.max_net_funding_rate {
                            if let Some(net_rate) = analysis.net_funding_rate {
                                if net_rate > max_net_rate {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        if let Some(min_volume) = filters.min_volume_24h {
                            if let Some(vol_a) = analysis.volume_24h_a {
                                if vol_a < min_volume {
                                    continue;
                                }
                            }
                            if let Some(vol_b) = analysis.volume_24h_b {
                                if vol_b < min_volume {
                                    continue;
                                }
                            }
                        }

                        analyses.push(analysis);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "计算套利机会失败 {} {}: {}",
                            snapshot_a.exchange,
                            snapshot_b.exchange,
                            e
                        );
                    }
                }
            }
        }

        Ok(analyses)
    }

    /// 分析指定 symbol 的所有类型套利机会（现期/期期/期现）
    /// 
    /// # Arguments
    /// * `symbol` - 交易对符号
    /// 
    /// # Returns
    /// 返回 (套利类型, 套利分析结果) 的列表
    /// 
    /// # Note
    /// 只分析在至少 2 个交易所都存在的 symbol
    pub async fn analyze_all_arbitrage_types(
        &self,
        symbol: &str,
    ) -> Result<Vec<(ArbitrageType, ArbitrageAnalysis)>> {
        let snapshots = self.repository.get_latest_snapshots(symbol).await?;
        
        // 需要至少 2 个快照（即至少 2 个交易所有该 symbol）才能进行套利分析
        if snapshots.len() < 2 {
            tracing::debug!(
                "{} 在 {} 个交易所存在，需要至少 2 个交易所才能进行套利分析",
                symbol,
                snapshots.len()
            );
            return Ok(vec![]);
        }

        let mut results = Vec::new();

        // 遍历所有快照对
        for i in 0..snapshots.len() {
            for j in (i + 1)..snapshots.len() {
                let snapshot_a = &snapshots[i];
                let snapshot_b = &snapshots[j];

                // 确保都有价格数据
                if snapshot_a.last_price.is_none() || snapshot_b.last_price.is_none() {
                    continue;
                }

                // 确定套利类型
                let arbitrage_type = match (snapshot_a.market_type, snapshot_b.market_type) {
                    (MarketType::Spot, MarketType::Futures) => ArbitrageType::SpotFutures,
                    (MarketType::Futures, MarketType::Spot) => ArbitrageType::FuturesSpot,
                    (MarketType::Futures, MarketType::Futures) => ArbitrageType::FuturesFutures,
                    (MarketType::Spot, MarketType::Spot) => continue, // 现货-现货不计算
                };

                match self.calculate_arbitrage(snapshot_a, snapshot_b) {
                    Ok(analysis) => {
                        results.push((arbitrage_type, analysis));
                    }
                    Err(e) => {
                        tracing::warn!(
                            "计算套利机会失败 {} {}: {}",
                            snapshot_a.exchange,
                            snapshot_b.exchange,
                            e
                        );
                    }
                }
            }
        }

        Ok(results)
    }

    /// 分析所有 symbol 的所有套利类型
    /// 
    /// # Returns
    /// 返回 (symbol, 套利类型, 套利分析结果) 的列表
    pub async fn analyze_all_symbols_all_types(
        &self,
    ) -> Result<Vec<(String, ArbitrageType, ArbitrageAnalysis)>> {
        // 获取所有交易对
        let pairs = self.repository.get_futures_trading_pairs().await?;
        let symbols: std::collections::HashSet<String> = 
            pairs.iter().map(|(_, s)| s.clone()).collect();

        let mut all_results = Vec::new();

        for symbol in symbols {
            match self.analyze_all_arbitrage_types(&symbol).await {
                Ok(results) => {
                    for (arb_type, analysis) in results {
                        all_results.push((symbol.clone(), arb_type, analysis));
                    }
                }
                Err(e) => {
                    tracing::warn!("分析 {} 套利机会失败: {}", symbol, e);
                }
            }
        }

        Ok(all_results)
    }
}
