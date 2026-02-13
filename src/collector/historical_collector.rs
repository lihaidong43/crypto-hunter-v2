use crate::exchange::ExchangeManager;
use crate::models::exchange::{normalize_symbol_for_exchange, ExchangeType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use crate::storage::repository::Repository;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{info, warn};

/// 历史数据采集器
/// 用于批量获取历史K线数据并转换为快照格式保存
pub struct HistoricalCollector {
    exchange_manager: Arc<ExchangeManager>,
    repository: Arc<Repository>,
}

impl HistoricalCollector {
    pub fn new(
        exchange_manager: Arc<ExchangeManager>,
        repository: Arc<Repository>,
    ) -> Self {
        Self {
            exchange_manager,
            repository,
        }
    }

    /// 采集历史数据
    /// 
    /// # 参数
    /// - `symbols`: 交易对列表（None表示从数据库获取全部）
    /// - `start_time`: 开始时间
    /// - `end_time`: 结束时间
    /// - `interval`: K线间隔，如 "1h", "1d"
    pub async fn collect_historical_data(
        &self,
        symbols: Option<Vec<String>>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        interval: &str,
    ) -> Result<()> {
        info!(
            "开始采集历史数据: 时间范围 {} 到 {}, 间隔: {}",
            start_time.format("%Y-%m-%d %H:%M:%S UTC"),
            end_time.format("%Y-%m-%d %H:%M:%S UTC"),
            interval
        );

        // 获取要采集的交易对列表
        let symbols_to_collect = if let Some(symbols) = symbols {
            symbols
        } else {
            // 从数据库获取所有期货交易对
            let pairs = self.repository.get_futures_trading_pairs().await?;
            let mut symbol_set = std::collections::HashSet::new();
            for (_exchange, symbol) in &pairs {
                // 转换为统一格式（Binance格式）用于去重
                let normalized = normalize_symbol_for_exchange(
                    symbol,
                    ExchangeType::Binance,
                    MarketType::Futures,
                );
                symbol_set.insert(normalized);
            }
            symbol_set.into_iter().collect()
        };

        info!("需要采集 {} 个交易对的历史数据", symbols_to_collect.len());

        let mut total_snapshots = 0;
        let mut success_count = 0;
        let mut error_count = 0;

        // 遍历每个交易对
        for (idx, symbol) in symbols_to_collect.iter().enumerate() {
            info!(
                "处理交易对 {}/{}: {}",
                idx + 1,
                symbols_to_collect.len(),
                symbol
            );

            // 遍历每个交易所
            for adapter in self.exchange_manager.get_all_adapters() {
                let exchange_type = adapter.exchange_type();
                
                // 转换symbol格式以匹配交易所
                let exchange_symbol = normalize_symbol_for_exchange(
                    symbol,
                    exchange_type,
                    MarketType::Futures,
                );

                match adapter
                    .fetch_historical_prices(&exchange_symbol, start_time, end_time, interval)
                    .await
                {
                    Ok(historical_data) => {
                        if historical_data.is_empty() {
                            warn!(
                                "{} {} 没有历史数据",
                                exchange_type,
                                exchange_symbol
                            );
                            continue;
                        }

                        // 将历史K线数据转换为MarketSnapshot
                        let snapshots: Vec<MarketSnapshot> = historical_data
                            .into_iter()
                            .map(|kline| {
                                // 使用K线的close价格作为last_price
                                // 使用high和low估算bid_price和ask_price（如果API不提供）
                                MarketSnapshot {
                                    snapshot_time: kline.timestamp,
                                    exchange: exchange_type,
                                    symbol: symbol.clone(),
                                    market_type: MarketType::Futures,
                                    bid_price: Some(kline.low), // 使用low作为bid_price的估算
                                    ask_price: Some(kline.high), // 使用high作为ask_price的估算
                                    last_price: Some(kline.close),
                                    volume_24h: Some(kline.volume),
                                    funding_rate: None,
                                    next_funding_time: None,
                                    funding_interval_hours: None,
                                    rate_limit_upper: None,
                                    rate_limit_lower: None,
                                }
                            })
                            .collect();

                        // 批量保存到数据库
                        if let Err(e) = self.repository.save_snapshots(&snapshots).await {
                            warn!(
                                "保存 {} {} 的历史数据失败: {}",
                                exchange_type,
                                exchange_symbol,
                                e
                            );
                            error_count += 1;
                        } else {
                            info!(
                                "成功保存 {} {} 的 {} 条历史数据",
                                exchange_type,
                                exchange_symbol,
                                snapshots.len()
                            );
                            total_snapshots += snapshots.len();
                            success_count += 1;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "获取 {} {} 的历史数据失败: {}",
                            exchange_type,
                            exchange_symbol,
                            e
                        );
                        error_count += 1;
                    }
                }

                // 添加延迟，避免限流
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }

        info!(
            "历史数据采集完成: 成功 {} 个, 失败 {} 个, 总共保存 {} 条快照",
            success_count,
            error_count,
            total_snapshots
        );

        Ok(())
    }
}
