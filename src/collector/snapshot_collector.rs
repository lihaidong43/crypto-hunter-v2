use crate::exchange::ExchangeManager;
use crate::models::exchange::{normalize_symbol_for_exchange, ExchangeType, MarketType};
use crate::models::snapshot::MarketSnapshot;
use crate::storage::repository::Repository;
use anyhow::Result;
use chrono::Utc;
use governor::{
    clock::DefaultClock,
    state::direct::NotKeyed,
    state::InMemoryState,
    Quota, RateLimiter,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

/// 错误类型分类
enum ErrorCategory {
    /// 网络错误（超时、连接失败等）
    NetworkError,
    /// 真正的"交易对不存在"（API 明确返回）
    SymbolNotFound,
    /// 限流错误（429 Too Many Requests）
    RateLimitError,
    /// 其他错误
    Other,
}

/// 根据错误消息分类错误类型
fn categorize_error(error: &anyhow::Error) -> ErrorCategory {
    let error_msg = error.to_string().to_lowercase();
    
    // 网络错误特征
    if error_msg.contains("operation timed out")
        || error_msg.contains("connection closed")
        || error_msg.contains("tcp connect error")
        || error_msg.contains("timeout")
        || error_msg.contains("network error")
        || error_msg.contains("can't assign requested address")
        || error_msg.contains("connection refused")
        || error_msg.contains("connection reset")
    {
        return ErrorCategory::NetworkError;
    }
    
    // 限流错误特征（429）
    if error_msg.contains("429")
        || error_msg.contains("too many requests")
        || error_msg.contains("rate limit")
    {
        return ErrorCategory::RateLimitError;
    }
    
    // 真正的"交易对不存在"特征
    if error_msg.contains("invalid symbol")
        || error_msg.contains("symbol not found")
        || error_msg.contains("not found (empty response)")
        || error_msg.contains("code: -1121") // Binance Invalid symbol
        || error_msg.contains("code: 51000") // OKX Invalid instrument
        || error_msg.contains("code: 130021") // Bybit Invalid symbol
        || error_msg.contains("code: 130150") // Bybit Symbol not found
    {
        return ErrorCategory::SymbolNotFound;
    }
    
    ErrorCategory::Other
}

/// 带重试机制的请求函数
/// 支持 429 限流错误（指数退避）和网络错误（固定延迟）的重试
async fn retry_with_backoff<F, Fut, T>(
    mut f: F,
    max_retries: u32,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_error = None;
    let mut retry_count = 0;
    
    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => {
                if retry_count > 0 {
                    info!("重试成功 (重试次数: {})", retry_count);
                }
                return Ok(result);
            }
            Err(e) => {
                let error_category = categorize_error(&e);
                let should_retry = attempt < max_retries;
                
                // 429 错误：指数退避重试
                if should_retry && matches!(error_category, ErrorCategory::RateLimitError) {
                    retry_count += 1;
                    // 指数退避：1秒、2秒、4秒
                    let backoff_seconds = 2_u64.pow(attempt);
                    warn!(
                        "遇到限流错误 (429)，{} 秒后重试 (尝试 {}/{})",
                        backoff_seconds,
                        attempt + 1,
                        max_retries + 1
                    );
                    tokio::time::sleep(Duration::from_secs(backoff_seconds)).await;
                    last_error = Some(e);
                }
                // 网络错误：固定延迟重试（更短的延迟）
                else if should_retry && matches!(error_category, ErrorCategory::NetworkError) {
                    retry_count += 1;
                    warn!(
                        "遇到网络错误，1 秒后重试 (尝试 {}/{})",
                        attempt + 1,
                        max_retries + 1
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    last_error = Some(e);
                }
                // 其他错误或已达到最大重试次数
                else {
                    if retry_count > 0 {
                        warn!("重试失败，已达到最大重试次数 (重试次数: {})", retry_count);
                    }
                    return Err(e);
                }
            }
        }
    }
    
    // 所有重试都失败
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("重试失败")))
}

/// 快照收集器
/// 统一收集价格和资金费率数据，创建市场快照
pub struct SnapshotCollector {
    exchange_manager: Arc<ExchangeManager>,
    pub(crate) repository: Arc<Repository>,
    /// 每个交易所的并发信号量（控制同时进行的请求数）
    exchange_semaphores: Arc<HashMap<ExchangeType, Arc<Semaphore>>>,
    /// 每个交易所的速率限制器（控制请求速率）
    exchange_rate_limiters: Arc<HashMap<ExchangeType, Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>,
}

impl SnapshotCollector {
    /// 创建新的快照收集器
    pub fn new(
        exchange_manager: Arc<ExchangeManager>,
        repository: Arc<Repository>,
    ) -> Self {
        // 初始化每个交易所的 Semaphore（控制并发数）
        // 进一步降低并发数，解决端口耗尽问题（os error 49）和 Binance IP 封禁问题
        // 根据日志分析：Binance 已被封禁，需要大幅降低并发和 QPS
        let mut semaphores = HashMap::new();
        semaphores.insert(ExchangeType::Gateio, Arc::new(Semaphore::new(1))); // Gate.io: 1 个并发
        semaphores.insert(ExchangeType::Okx, Arc::new(Semaphore::new(1))); // OKX: 1 个并发
        semaphores.insert(ExchangeType::Binance, Arc::new(Semaphore::new(1))); // Binance: 1 个并发（从 3 降到 1，避免 IP 封禁）
        semaphores.insert(ExchangeType::Bybit, Arc::new(Semaphore::new(1))); // Bybit: 1 个并发（从 2 降到 1，进一步缓解端口压力）
        semaphores.insert(ExchangeType::Bitget, Arc::new(Semaphore::new(1))); // Bitget: 1 个并发（从 2 降到 1，缓解端口压力）

        // 初始化每个交易所的速率限制器（控制请求速率）
        let mut rate_limiters = HashMap::new();
        
        // Gate.io: 5 请求/秒（从 10 降到 5，进一步降低）
        rate_limiters.insert(
            ExchangeType::Gateio,
            Arc::new(RateLimiter::<NotKeyed, InMemoryState, DefaultClock>::direct(
                Quota::per_second(NonZeroU32::new(5).unwrap()),
            )),
        );
        
        // OKX: 1 请求/秒（从 3 降到 1，最严格限流，避免 429 错误）
        rate_limiters.insert(
            ExchangeType::Okx,
            Arc::new(RateLimiter::<NotKeyed, InMemoryState, DefaultClock>::direct(
                Quota::per_second(NonZeroU32::new(1).unwrap()),
            )),
        );
        
        // Binance: 10 请求/秒（从 50 大幅降到 10，避免 IP 封禁）
        rate_limiters.insert(
            ExchangeType::Binance,
            Arc::new(RateLimiter::<NotKeyed, InMemoryState, DefaultClock>::direct(
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            )),
        );
        
        // Bybit: 10 请求/秒（从 30 降到 10，缓解端口压力）
        rate_limiters.insert(
            ExchangeType::Bybit,
            Arc::new(RateLimiter::<NotKeyed, InMemoryState, DefaultClock>::direct(
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            )),
        );
        
        // Bitget: 10 请求/秒（从 30 降到 10，缓解端口压力）
        rate_limiters.insert(
            ExchangeType::Bitget,
            Arc::new(RateLimiter::<NotKeyed, InMemoryState, DefaultClock>::direct(
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            )),
        );

        Self {
            exchange_manager,
            repository,
            exchange_semaphores: Arc::new(semaphores),
            exchange_rate_limiters: Arc::new(rate_limiters),
        }
    }

    /// 收集单个 symbol 在所有交易所的完整快照
    /// 返回收集到的快照列表
    pub async fn collect_symbol_snapshot(&self, symbol: &str) -> Result<Vec<MarketSnapshot>> {
        let snapshot_time = Utc::now();
        let mut snapshots = Vec::new();

        // 仅对“该 symbol 真的有期货”的交易所发请求，避免大量无意义错误（如 OKX Instrument ID doesn't exist）
        let enabled_exchanges = self
            .repository
            .get_futures_exchanges_for_symbol(symbol)
            .await?;

        // 并发获取需要的交易所的数据
        let mut tasks = Vec::new();
        for exchange_type in enabled_exchanges {
            let symbol = symbol.to_string();
            let exchange_manager = self.exchange_manager.clone();
            tasks.push(tokio::spawn(async move {
                let adapter = exchange_manager
                    .get_adapter(exchange_type)
                    .expect("Adapter should exist");
                // 转换 symbol 格式以匹配交易所（期货）
                // 注意：MarketSnapshot 中保存的是“统一后的 symbol”（即上层传入的 symbol），而不是交易所特定格式
                let futures_symbol =
                    normalize_symbol_for_exchange(&symbol, exchange_type, MarketType::Futures);

                // 并发获取价格和资金费率
                let (price_result, funding_result) = tokio::join!(
                    adapter.fetch_futures_price(&futures_symbol),
                    adapter.fetch_funding_rate(&futures_symbol),
                );

                match (price_result, funding_result) {
                    (Ok(price), Ok(funding)) => {
                        Some(MarketSnapshot {
                            snapshot_time,
                            exchange: adapter.exchange_type(),
                            symbol: symbol.clone(),
                            market_type: MarketType::Futures,
                            bid_price: Some(price.bid_price),
                            ask_price: Some(price.ask_price),
                            last_price: Some(price.last_price),
                            volume_24h: Some(price.volume_24h),
                            funding_rate: Some(funding.rate),
                            next_funding_time: Some(funding.next_funding_time),
                            funding_interval_hours: funding.funding_interval_hours,
                            rate_limit_upper: funding.rate_limit_upper,
                            rate_limit_lower: funding.rate_limit_lower,
                        })
                    }
                    (Ok(price), Err(e)) => {
                        // 只有价格数据，没有资金费率（可能是新上线或临时问题）
                        tracing::debug!(
                            "获取 {} {} (原始: {}) 的资金费率失败: {}",
                            adapter.exchange_type(),
                            futures_symbol,
                            symbol,
                            e
                        );
                        Some(MarketSnapshot {
                            snapshot_time,
                            exchange: adapter.exchange_type(),
                            symbol: symbol.clone(),
                            market_type: MarketType::Futures,
                            bid_price: Some(price.bid_price),
                            ask_price: Some(price.ask_price),
                            last_price: Some(price.last_price),
                            volume_24h: Some(price.volume_24h),
                            funding_rate: None,
                            next_funding_time: None,
                            funding_interval_hours: None,
                            rate_limit_upper: None,
                            rate_limit_lower: None,
                        })
                    }
                    (Err(e), _) => {
                        // 根据错误类型记录不同级别的日志
                        match categorize_error(&e) {
                            ErrorCategory::NetworkError => {
                                // 网络错误：warn 级别，说明是网络问题而非交易对不存在
                                warn!(
                                    "{} {} (原始: {}) 网络请求失败: {}",
                                    adapter.exchange_type(),
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::RateLimitError => {
                                // 限流错误：warn 级别，说明请求过于频繁
                                warn!(
                                    "{} {} (原始: {}) 请求被限流 (429): {}",
                                    adapter.exchange_type(),
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::SymbolNotFound => {
                                // 真正的"交易对不存在"：debug 级别，这是正常情况
                                debug!(
                                    "{} 没有 {} (原始: {}) 的期货交易对: {}",
                                    adapter.exchange_type(),
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::Other => {
                                // 其他错误：warn 级别
                                warn!(
                                    "{} {} (原始: {}) 获取期货数据失败: {}",
                                    adapter.exchange_type(),
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                        }
                        None
                    }
                }
            }));
        }

        // 收集结果
        for task in tasks {
            if let Ok(Some(snapshot)) = task.await {
                snapshots.push(snapshot);
            }
        }

        // 数据完整性检查：统计交易所数量
        let exchange_count: HashSet<ExchangeType> = snapshots
            .iter()
            .map(|s| s.exchange)
            .collect();
        let exchange_count = exchange_count.len();
        
        // 如果只有少于 2 个交易所的数据，记录警告（可能影响套利分析）
        if exchange_count > 0 && exchange_count < 2 {
            warn!(
                "Symbol {} 只有 {} 个交易所的数据，可能影响套利分析",
                symbol,
                exchange_count
            );
        } else if exchange_count == 0 {
            warn!(
                "Symbol {} 没有收集到任何交易所的数据",
                symbol
            );
        }

        // 批量保存
        if !snapshots.is_empty() {
            self.repository.save_snapshots(&snapshots).await?;
            info!(
                "成功收集 {} 个交易所的 {} 快照",
                exchange_count,
                symbol
            );
        }

        Ok(snapshots)
    }

    /// 收集所有 symbol 的快照
    /// 每次调用都会创建一个新的 snapshot_time，保存到历史记录中
    pub async fn collect_all_snapshots(&self) -> Result<()> {
        let pairs = self.repository.get_futures_trading_pairs().await?;
        // symbol -> exchanges
        let mut exchanges_by_symbol: HashMap<String, HashSet<ExchangeType>> = HashMap::new();
        for (ex, sym) in pairs {
            exchanges_by_symbol
                .entry(sym)
                .or_insert_with(HashSet::new)
                .insert(ex);
        }

        info!("开始收集 {} 个交易对的快照", exchanges_by_symbol.len());

        // 并发收集所有 symbol
        let mut tasks = Vec::new();
        for (symbol, _exchanges) in exchanges_by_symbol {
            let exchange_manager = self.exchange_manager.clone();
            let repository = self.repository.clone();
            let semaphores = self.exchange_semaphores.clone();
            let rate_limiters = self.exchange_rate_limiters.clone();
            let symbol_clone = symbol.clone();
            tasks.push(tokio::spawn(async move {
                let collector = SnapshotCollector {
                    exchange_manager,
                    repository,
                    exchange_semaphores: semaphores,
                    exchange_rate_limiters: rate_limiters,
                };
                collector.collect_symbol_snapshot(&symbol_clone).await
            }));
        }

        // 等待所有任务完成
        let mut success_count = 0;
        let mut error_count = 0;
        for task in tasks {
            match task.await {
                Ok(Ok(_)) => success_count += 1,
                Ok(Err(e)) => {
                    error!("收集快照失败: {}", e);
                    error_count += 1;
                }
                Err(e) => {
                    error!("任务执行失败: {}", e);
                    error_count += 1;
                }
            }
        }

        info!(
            "快照收集完成: 成功 {}, 失败 {}",
            success_count, error_count
        );

        Ok(())
    }

    /// 收集单个 symbol 的完整快照（包括现货和期货）
    /// 用于实时套利监控，同时收集现货和期货数据
    pub async fn collect_symbol_snapshot_full(
        &self,
        symbol: &str,
    ) -> Result<Vec<MarketSnapshot>> {
        let snapshot_time = Utc::now();
        let mut snapshots = Vec::new();

        // 关键：只对“该 symbol 在该交易所该市场确实存在”的组合发请求
        // 否则会出现 Binance spot HTTP 400、OKX instId 不存在等噪音，并浪费请求额度
        let enabled_spot_exchanges: HashSet<ExchangeType> = self
            .repository
            .get_spot_exchanges_for_symbol(symbol)
            .await?
            .into_iter()
            .collect();
        let enabled_futures_exchanges: HashSet<ExchangeType> = self
            .repository
            .get_futures_exchanges_for_symbol(symbol)
            .await?
            .into_iter()
            .collect();

        // 并发获取所有交易所的现货和期货数据
        // 使用 Semaphore 和 RateLimiter 精确控制每个交易所的请求速率
        let mut tasks: Vec<tokio::task::JoinHandle<Result<Vec<MarketSnapshot>>>> = Vec::new();
        for adapter in self.exchange_manager.get_all_adapters() {
            let symbol = symbol.to_string();
            let exchange_type = adapter.exchange_type();
            let enable_spot = enabled_spot_exchanges.contains(&exchange_type);
            let enable_futures = enabled_futures_exchanges.contains(&exchange_type);
            if !enable_spot && !enable_futures {
                continue;
            }
            let exchange_manager = self.exchange_manager.clone();
            let semaphores = self.exchange_semaphores.clone();
            let rate_limiters = self.exchange_rate_limiters.clone();
            
            tasks.push(tokio::spawn(async move {
                // 1. 获取 Semaphore 许可（控制并发数）
                let semaphore = semaphores
                    .get(&exchange_type)
                    .expect("Semaphore should exist for exchange");
                let _permit = match semaphore.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        warn!("Failed to acquire semaphore for {}: {}", exchange_type, e);
                        return Ok(Vec::new()); // 返回空结果，不阻塞其他任务
                    }
                };
                
                // 2. 等待速率限制器许可（控制请求速率）
                let rate_limiter = rate_limiters
                    .get(&exchange_type)
                    .expect("Rate limiter should exist for exchange");
                rate_limiter.until_ready().await;
                
                // OKX 专用：在速率限制后额外延迟 500ms，进一步降低限流风险
                if exchange_type == ExchangeType::Okx {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                
                let adapter = exchange_manager
                    .get_adapter(exchange_type)
                    .expect("Adapter should exist");
                
                // 转换 symbol 格式以匹配交易所（仅在该市场启用时才需要）
                let spot_symbol = if enable_spot {
                    normalize_symbol_for_exchange(&symbol, exchange_type, MarketType::Spot)
                } else {
                    String::new()
                };
                let futures_symbol = if enable_futures {
                    normalize_symbol_for_exchange(&symbol, exchange_type, MarketType::Futures)
                } else {
                    String::new()
                };
                
                // 调试日志：记录转换前后的 symbol（仅在转换发生变化时）
                if spot_symbol != symbol {
                    tracing::debug!(
                        "Symbol 格式转换 (现货): {} -> {} ({})",
                        symbol,
                        spot_symbol,
                        exchange_type
                    );
                }
                if futures_symbol != symbol {
                    tracing::debug!(
                        "Symbol 格式转换 (期货): {} -> {} ({})",
                        symbol,
                        futures_symbol,
                        exchange_type
                    );
                }
                
                // 3. 使用重试机制并发获取现货价格、期货价格和资金费率（按需）
                let spot_task = async {
                    if !enable_spot {
                        return Err(anyhow::anyhow!("spot_disabled"));
                    }
                    retry_with_backoff(|| adapter.fetch_spot_price(&spot_symbol), 5).await
                };
                let futures_task = async {
                    if !enable_futures {
                        return Err(anyhow::anyhow!("futures_disabled"));
                    }
                    retry_with_backoff(|| adapter.fetch_futures_price(&futures_symbol), 5).await
                };
                let funding_task = async {
                    if !enable_futures {
                        return Err(anyhow::anyhow!("futures_disabled"));
                    }
                    retry_with_backoff(|| adapter.fetch_funding_rate(&futures_symbol), 5).await
                };

                let (spot_result, futures_result, funding_result) =
                    tokio::join!(spot_task, futures_task, funding_task);

                let mut results = Vec::new();

                // 现货快照（如果该交易所有现货交易对）
                if enable_spot {
                    match spot_result {
                    Ok(price) => {
                        results.push(MarketSnapshot {
                            snapshot_time,
                            exchange: exchange_type,
                            symbol: symbol.clone(),
                            market_type: MarketType::Spot,
                            bid_price: Some(price.bid_price),
                            ask_price: Some(price.ask_price),
                            last_price: Some(price.last_price),
                            volume_24h: Some(price.volume_24h),
                            funding_rate: None,
                            next_funding_time: None,
                            funding_interval_hours: None,
                            rate_limit_upper: None,
                            rate_limit_lower: None,
                        });
                    }
                    Err(e) => {
                        // 由于 enable_spot=true，这里不应出现 "spot_disabled"
                        // 根据错误类型记录不同级别的日志
                        match categorize_error(&e) {
                            ErrorCategory::NetworkError => {
                                // 网络错误：warn 级别
                                warn!(
                                    "{} {} (原始: {}) 现货网络请求失败: {}",
                                    exchange_type,
                                    spot_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::SymbolNotFound => {
                                // 真正的"交易对不存在"：debug 级别
                                debug!(
                                    "{} 没有 {} (原始: {}) 的现货交易对: {}",
                                    exchange_type,
                                    spot_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::RateLimitError => {
                                // 限流错误：warn 级别，说明请求过于频繁
                                warn!(
                                    "{} {} (原始: {}) 现货请求被限流 (429): {}",
                                    exchange_type,
                                    spot_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::Other => {
                                // 其他错误：warn 级别
                                warn!(
                                    "{} {} (原始: {}) 获取现货数据失败: {}",
                                    exchange_type,
                                    spot_symbol,
                                    symbol,
                                    e
                                );
                            }
                        }
                    }
                    }
                }

                // 期货快照（如果该交易所有期货交易对）
                if enable_futures {
                    match (futures_result, funding_result) {
                    (Ok(price), Ok(funding)) => {
                        results.push(MarketSnapshot {
                            snapshot_time,
                            exchange: exchange_type,
                            symbol: symbol.clone(),
                            market_type: MarketType::Futures,
                            bid_price: Some(price.bid_price),
                            ask_price: Some(price.ask_price),
                            last_price: Some(price.last_price),
                            volume_24h: Some(price.volume_24h),
                            funding_rate: Some(funding.rate),
                            next_funding_time: Some(funding.next_funding_time),
                            funding_interval_hours: funding.funding_interval_hours,
                            rate_limit_upper: funding.rate_limit_upper,
                            rate_limit_lower: funding.rate_limit_lower,
                        });
                    }
                    (Ok(price), Err(e)) => {
                        // 只有期货价格，没有资金费率（可能是新上线或临时问题）
                        tracing::debug!(
                            "{} {} 获取资金费率失败: {}",
                            exchange_type,
                            symbol,
                            e
                        );
                        results.push(MarketSnapshot {
                            snapshot_time,
                            exchange: exchange_type,
                            symbol: symbol.clone(),
                            market_type: MarketType::Futures,
                            bid_price: Some(price.bid_price),
                            ask_price: Some(price.ask_price),
                            last_price: Some(price.last_price),
                            volume_24h: Some(price.volume_24h),
                            funding_rate: None,
                            next_funding_time: None,
                            funding_interval_hours: None,
                            rate_limit_upper: None,
                            rate_limit_lower: None,
                        });
                    }
                    (Err(e), _) => {
                        // 由于 enable_futures=true，这里不应出现 "futures_disabled"
                        // 根据错误类型记录不同级别的日志
                        match categorize_error(&e) {
                            ErrorCategory::NetworkError => {
                                // 网络错误：warn 级别
                                warn!(
                                    "{} {} (原始: {}) 期货网络请求失败: {}",
                                    exchange_type,
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::RateLimitError => {
                                // 限流错误：warn 级别，说明请求过于频繁
                                warn!(
                                    "{} {} (原始: {}) 期货请求被限流 (429): {}",
                                    exchange_type,
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::SymbolNotFound => {
                                // 真正的"交易对不存在"：debug 级别
                                debug!(
                                    "{} 没有 {} (原始: {}) 的期货交易对: {}",
                                    exchange_type,
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                            ErrorCategory::Other => {
                                // 其他错误：warn 级别
                                warn!(
                                    "{} {} (原始: {}) 获取期货数据失败: {}",
                                    exchange_type,
                                    futures_symbol,
                                    symbol,
                                    e
                                );
                            }
                        }
                    }
                    }
                }

                Ok(results)
            }));
        }

        // 收集所有结果
        for task in tasks {
            match task.await {
                Ok(Ok(mut results)) => {
                    snapshots.append(&mut results);
                }
                Ok(Err(e)) => {
                    warn!("快照收集任务失败: {}", e);
                }
                Err(e) => {
                    warn!("任务执行失败: {}", e);
                }
            }
        }

        // 数据完整性检查：统计每个交易所的数据数量
        let exchange_count: HashSet<ExchangeType> = snapshots
            .iter()
            .map(|s| s.exchange)
            .collect();
        let exchange_count = exchange_count.len();
        
        // 如果只有少于 2 个交易所的数据，记录警告（可能影响套利分析）
        if exchange_count > 0 && exchange_count < 2 {
            warn!(
                "Symbol {} 只有 {} 个交易所的数据（总共 {} 个快照），可能影响套利分析",
                symbol,
                exchange_count,
                snapshots.len()
            );
        } else if exchange_count == 0 {
            warn!(
                "Symbol {} 没有收集到任何交易所的数据",
                symbol
            );
        } else {
            info!(
                "Symbol {} 成功收集 {} 个交易所的数据（总共 {} 个快照）",
                symbol,
                exchange_count,
                snapshots.len()
            );
        }

        // 批量保存
        if !snapshots.is_empty() {
            self.repository.save_snapshots(&snapshots).await?;
        }

        Ok(snapshots)
    }
}
