use crate::arbitrage::analyzer::ArbitrageAnalyzer;
use crate::models::arbitrage::ArbitrageAnalysis;
use crate::models::exchange::ArbitrageType;
use crate::notification::TelegramNotifier;
use crate::storage::repository::Repository;
use anyhow::Result;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

pub struct ArbitrageMonitor {
    repository: Arc<Repository>,
    analyzer: Arc<ArbitrageAnalyzer>,
    notifier: Arc<TelegramNotifier>,
    min_spread_threshold: Decimal,
    check_interval_seconds: u64,
    last_notified: Arc<dashmap::DashMap<String, chrono::DateTime<chrono::Utc>>>, // 防止重复通知
    notification_cooldown_seconds: i64,
    batch_size: usize, // 每批处理的交易对数量
    is_running: Arc<tokio::sync::Mutex<bool>>, // 防止重叠执行
    ws_snapshot_max_age_seconds: i64, // WS 快照允许的最大延迟（超过则认为数据缺失）
}

impl ArbitrageMonitor {
    pub fn new(
        repository: Arc<Repository>,
        analyzer: Arc<ArbitrageAnalyzer>,
        notifier: Arc<TelegramNotifier>,
        min_spread_threshold: Decimal,
        check_interval_seconds: u64,
        notification_cooldown_seconds: i64,
    ) -> Self {
        Self {
            repository,
            analyzer,
            notifier,
            min_spread_threshold,
            check_interval_seconds,
            last_notified: Arc::new(dashmap::DashMap::new()),
            notification_cooldown_seconds,
            batch_size: 2, // 默认每批处理2个交易对（进一步降低并发，配合限流器使用）
            is_running: Arc::new(tokio::sync::Mutex::new(false)),
            ws_snapshot_max_age_seconds: 45, // 默认：45 秒内没新快照就告警（可按需调整）
        }
    }

    /// 设置每批处理的交易对数量
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// 设置 WS 快照最大允许延迟（秒）
    pub fn with_ws_snapshot_max_age_seconds(mut self, seconds: i64) -> Self {
        self.ws_snapshot_max_age_seconds = seconds;
        self
    }

    /// 启动实时监控
    pub async fn start(&self) -> Result<()> {
        info!(
            "启动套利监控服务 (检查间隔: {}秒, 价差阈值: {}%, 批次大小: {})",
            self.check_interval_seconds,
            self.min_spread_threshold,
            self.batch_size
        );

        let mut check_interval = interval(Duration::from_secs(self.check_interval_seconds));

        loop {
            check_interval.tick().await;

            // 检查是否正在运行，避免重叠执行
            {
                let mut is_running = self.is_running.lock().await;
                if *is_running {
                    warn!("上一次检查尚未完成，跳过本次检查");
                    continue;
                }
                *is_running = true;
            }

            let start_time = std::time::Instant::now();
            match self.check_all_opportunities().await {
                Ok(_) => {
                    let elapsed = start_time.elapsed();
                    info!("套利检查完成，耗时: {:.2}秒", elapsed.as_secs_f64());
                }
                Err(e) => {
                    error!("检查套利机会失败: {}", e);
                }
            }

            // 标记完成
            {
                let mut is_running = self.is_running.lock().await;
                *is_running = false;
            }
        }
    }

    /// 检查所有套利机会（分批处理，避免API限制）
    async fn check_all_opportunities(&self) -> Result<()> {
        info!("开始检查所有套利机会...");

        // 1. 获取所有 symbol，并转换为统一格式（Binance 格式）用于去重
        use crate::models::exchange::{normalize_symbol_for_exchange, ExchangeType, MarketType};
        let pairs = self.repository.get_futures_trading_pairs().await?;
        
        // 将所有 symbol 转换为统一格式（Binance 格式）用于去重
        let mut symbol_set = std::collections::HashSet::new();
        for (exchange, symbol) in &pairs {
            // 将各交易所的 symbol 转换为 Binance 格式（统一格式）
            let normalized = normalize_symbol_for_exchange(symbol, ExchangeType::Binance, MarketType::Futures);
            symbol_set.insert(normalized);
        }
        let symbols: Vec<String> = symbol_set.into_iter().collect();
        let total_symbols = symbols.len();

        info!("检查 {} 个交易对的套利机会（分批处理，每批 {} 个）", total_symbols, self.batch_size);

        // 2. 分批处理交易对
        let mut all_opportunities = Vec::new();
        for (batch_idx, chunk) in symbols.chunks(self.batch_size).enumerate() {
            info!("处理第 {}/{} 批（{} 个交易对）", 
                batch_idx + 1, 
                (total_symbols + self.batch_size - 1) / self.batch_size,
                chunk.len()
            );

            // 注意：不再触发 HTTP 快照采集（已切换为 WS 写入 DB）
            // 这里只从数据库读取最新快照；如果 WS 没数据，不回退 HTTP，而是告警。
            let batch_start = std::time::Instant::now();

            // 分析当前批次的套利机会
            for symbol in chunk {
                // 先做 WS 数据完整性检查（按 futures 交易对期望的交易所集合）
                if let Err(e) = self.alert_if_ws_snapshots_missing(symbol).await {
                    warn!("WS 数据完整性检查失败 {}: {}", symbol, e);
                }

                match self.analyzer.analyze_all_arbitrage_types(symbol).await {
                    Ok(results) => {
                        if results.is_empty() {
                            // 没有套利机会是正常的（可能该 symbol 只在少数交易所存在）
                            continue;
                        }
                        for (arb_type, analysis) in results {
                            // 检查价差阈值（绝对值）
                            if analysis.open_spread.abs() >= self.min_spread_threshold {
                                all_opportunities.push((symbol.clone(), arb_type, analysis));
                            }
                        }
                    }
                    Err(e) => {
                        // 分析失败可能是数据库查询问题，记录警告但不中断
                        warn!("分析 {} 套利机会失败: {}", symbol, e);
                    }
                }
            }

            let batch_elapsed = batch_start.elapsed();
            info!("批次 {} 分析完成，耗时: {:.2}秒", batch_idx + 1, batch_elapsed.as_secs_f64());
        }

        info!("发现 {} 个符合条件的套利机会", all_opportunities.len());

        // 3. 发送通知
        for (symbol, arb_type, analysis) in all_opportunities {
            if let Err(e) = self.notify_if_needed(&symbol, arb_type, &analysis).await {
                warn!("发送通知失败: {}", e);
            }
        }

        Ok(())
    }

    /// WS 无数据不回退 HTTP：如果某 symbol 在时间窗口内缺少应有的 futures 快照，则告警
    async fn alert_if_ws_snapshots_missing(&self, symbol: &str) -> Result<()> {
        use crate::models::exchange::MarketType;
        use chrono::Utc;
        use std::collections::HashSet;

        // 期望：该 symbol 在哪些交易所有 futures（来自 trading_pairs）
        let expected: HashSet<_> = self
            .repository
            .get_futures_exchanges_for_symbol(symbol)
            .await?
            .into_iter()
            .collect();
        if expected.is_empty() {
            return Ok(());
        }

        // 实际：时间窗口内每个交易所最新快照
        let now = Utc::now();
        let since = now - chrono::Duration::seconds(self.ws_snapshot_max_age_seconds);
        let recent = self.repository.get_latest_snapshots_since(symbol, since).await?;

        let mut present = HashSet::new();
        for s in recent {
            if s.market_type == MarketType::Futures {
                present.insert(s.exchange);
            }
        }

        let missing: Vec<_> = expected.difference(&present).cloned().collect();
        if missing.is_empty() {
            return Ok(());
        }

        // 告警（带冷却）
        for ex in missing {
            let key = format!("ws_missing_futures_{}_{}", symbol, ex);
            let now = chrono::Utc::now();
            if let Some(last_time) = self.last_notified.get(&key) {
                let elapsed = (now - *last_time).num_seconds();
                if elapsed < self.notification_cooldown_seconds {
                    continue;
                }
            }

            let msg = format!(
                "告警：WS 数据缺失（不回退HTTP）\n- symbol: {}\n- exchange: {}\n- market: futures\n- 超过 {} 秒未写入快照",
                symbol, ex, self.ws_snapshot_max_age_seconds
            );
            warn!("{}", msg.replace('\n', " | "));
            // 告警只记录日志，不推送 Telegram

            self.last_notified.insert(key, now);
        }

        Ok(())
    }

    /// 如果需要，发送通知（带冷却时间）
    async fn notify_if_needed(
        &self,
        symbol: &str,
        arbitrage_type: ArbitrageType,
        analysis: &ArbitrageAnalysis,
    ) -> Result<()> {
        // 生成唯一键（交易所对 + 套利类型）
        let key = format!(
            "{}_{}_{}_{}",
            symbol,
            analysis.exchange_a,
            analysis.exchange_b,
            arbitrage_type
        );

        // 检查冷却时间
        let now = chrono::Utc::now();
        if let Some(last_time) = self.last_notified.get(&key) {
            let elapsed = (now - *last_time).num_seconds();
            if elapsed < self.notification_cooldown_seconds {
                return Ok(()); // 还在冷却期内，不发送
            }
        }

        // 发送通知
        let message = self.notifier.format_arbitrage_message(&arbitrage_type, analysis);
        self.notifier.send_message(&message).await?;

        // 更新最后通知时间
        self.last_notified.insert(key, now);

        Ok(())
    }
}