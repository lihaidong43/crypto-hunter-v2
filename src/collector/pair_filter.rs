use crate::config::PairFilterConfig;
use crate::exchange::ExchangeManager;
use crate::models::exchange::{normalize_symbol_for_exchange, ExchangeType, MarketType, TradingPair};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use tracing::{info, warn};

/// 过滤交易对统计信息
#[derive(Debug, Clone)]
pub struct FilterStats {
    /// 过滤前的总交易对数量
    pub total_before: usize,
    /// 过滤后的交易对数量
    pub total_after: usize,
    /// 按原因分类的过滤统计
    pub filtered_by_reason: HashMap<String, usize>,
}

impl FilterStats {
    pub fn new() -> Self {
        Self {
            total_before: 0,
            total_after: 0,
            filtered_by_reason: HashMap::new(),
        }
    }

    pub fn log_summary(&self) {
        info!(
            "交易对过滤统计: 过滤前 {} 个，过滤后 {} 个，过滤掉 {} 个 ({:.1}%)",
            self.total_before,
            self.total_after,
            self.total_before - self.total_after,
            if self.total_before > 0 {
                (self.total_before - self.total_after) as f64 / self.total_before as f64 * 100.0
            } else {
                0.0
            }
        );

        if !self.filtered_by_reason.is_empty() {
            info!("过滤原因统计:");
            for (reason, count) in &self.filtered_by_reason {
                info!("  - {}: {} 个", reason, count);
            }
        }
    }
}

/// 过滤交易对
/// 
/// 过滤策略：
/// 1. 只保留在至少 N 个交易所都存在的交易对
/// 2. 只保留指定计价货币的交易对
/// 3. 现货和期货分别处理
pub async fn filter_trading_pairs(
    exchange_manager: &ExchangeManager,
    config: &PairFilterConfig,
    market_type: MarketType,
) -> Result<(Vec<TradingPair>, FilterStats)> {
    let mut stats = FilterStats::new();
    let market_type_str = match market_type {
        MarketType::Spot => "现货",
        MarketType::Futures => "期货",
    };

    info!("开始过滤 {} 交易对...", market_type_str);

    // 1. 收集所有交易所的交易对
    let mut all_pairs: Vec<(ExchangeType, TradingPair)> = Vec::new();
    for adapter in exchange_manager.get_all_adapters() {
        let exchange_type = adapter.exchange_type();
        let pairs = match market_type {
            MarketType::Spot => adapter.fetch_spot_pairs().await,
            MarketType::Futures => adapter.fetch_futures_pairs().await,
        };

        match pairs {
            Ok(pairs) => {
                info!("{} {} 交易对数量: {}", exchange_type, market_type_str, pairs.len());
                for pair in pairs {
                    all_pairs.push((exchange_type, pair));
                }
            }
            Err(e) => {
                warn!("{} 获取 {} 交易对失败: {}", exchange_type, market_type_str, e);
            }
        }
    }

    stats.total_before = all_pairs.len();
    info!("收集到 {} 个 {} 交易对", stats.total_before, market_type_str);

    // 2. 按统一格式（Binance 格式）分组，统计每个 symbol 在多少个交易所存在
    let mut symbol_groups: HashMap<String, Vec<(ExchangeType, TradingPair)>> = HashMap::new();

    for (exchange_type, pair) in all_pairs {
        // 转换为统一格式（Binance 格式）进行匹配
        let normalized_symbol = normalize_symbol_for_exchange(
            &pair.symbol,
            ExchangeType::Binance,
            market_type,
        );

        symbol_groups
            .entry(normalized_symbol)
            .or_insert_with(Vec::new)
            .push((exchange_type, pair));
    }

    info!(
        "按统一格式分组后，共有 {} 个不同的 {} symbol",
        symbol_groups.len(),
        market_type_str
    );

    // 3. 过滤：只保留满足条件的交易对
    let mut filtered_pairs = Vec::new();
    let allowed_quotes: HashSet<String> = config
        .allowed_quote_currencies
        .iter()
        .map(|s| s.to_uppercase())
        .collect();

    for (_normalized_symbol, pairs) in symbol_groups.iter() {
        let exchange_count = pairs.len();

        // 检查交易所数量
        if exchange_count < config.min_exchange_count {
            let count = stats
                .filtered_by_reason
                .entry(format!("交易所数量不足 (需要 {}, 实际 {})", config.min_exchange_count, exchange_count))
                .or_insert(0);
            *count += pairs.len();
            continue;
        }

        // 检查计价货币
        let has_allowed_quote = pairs.iter().any(|(_, p)| {
            allowed_quotes.contains(&p.quote.to_uppercase())
        });

        if !has_allowed_quote {
            let count = stats
                .filtered_by_reason
                .entry(format!("计价货币不在允许列表中 (允许: {:?})", config.allowed_quote_currencies))
                .or_insert(0);
            *count += pairs.len();
            continue;
        }

        // 通过过滤，添加到结果中
        // 只保留允许的计价货币的交易对
        for (_exchange_type, pair) in pairs {
            if allowed_quotes.contains(&pair.quote.to_uppercase()) {
                filtered_pairs.push(pair.clone());
            } else {
                let count = stats
                    .filtered_by_reason
                    .entry(format!("计价货币 {} 不在允许列表中", pair.quote))
                    .or_insert(0);
                *count += 1;
            }
        }
    }

    stats.total_after = filtered_pairs.len();
    stats.log_summary();

    Ok((filtered_pairs, stats))
}
