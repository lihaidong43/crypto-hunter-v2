use crate::config::WebSocketConfig;
use crate::exchange::{ExchangeManager, WebSocketAdapter};
use crate::models::exchange::{ExchangeType, MarketType};
use crate::storage::repository::Repository;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Default)]
struct WsLinkStatus {
    symbols: usize,
    reconnects: u32,
    connected: bool,
    last_err: Option<String>,
    recv_ok: u64,
    save_ok: u64,
    recv_timeout: u64,
    recv_err: u64,
    save_err: u64,
    last_msg_age_s: Option<u64>,
}

/// WebSocket数据采集器
/// 管理多个WebSocket连接，实时接收价格数据和资金费率
pub struct WebSocketCollector {
    #[allow(dead_code)]
    exchange_manager: Arc<ExchangeManager>,
    repository: Arc<Repository>,
    config: WebSocketConfig,
}

impl WebSocketCollector {
    pub fn new(
        exchange_manager: Arc<ExchangeManager>,
        repository: Arc<Repository>,
        config: WebSocketConfig,
    ) -> Self {
        Self {
            exchange_manager,
            repository,
            config,
        }
    }

    /// 启动WebSocket采集
    pub async fn start(&mut self) -> Result<()> {
        info!("WebSocket采集器启动...");

        // 获取所有活跃交易对（期货和现货）
        let futures_pairs = self.repository.get_futures_trading_pairs().await?;
        let spot_pairs = self.repository.get_spot_trading_pairs().await?;
        
        debug!("从数据库获取到 {} 个期货交易对，{} 个现货交易对", futures_pairs.len(), spot_pairs.len());
        
        // 按交易所和市场类型分组交易对
        let mut symbols_by_exchange_and_type: HashMap<(ExchangeType, MarketType), Vec<String>> = HashMap::new();
        
        for (exchange, symbol) in futures_pairs {
            symbols_by_exchange_and_type
                .entry((exchange, MarketType::Futures))
                .or_insert_with(Vec::new)
                .push(symbol);
        }
        
        for (exchange, symbol) in spot_pairs {
            symbols_by_exchange_and_type
                .entry((exchange, MarketType::Spot))
                .or_insert_with(Vec::new)
                .push(symbol);
        }

        info!("需要订阅 {} 个交易所/市场类型的WebSocket", symbols_by_exchange_and_type.len());
        for ((exchange, market_type), symbols) in &symbols_by_exchange_and_type {
            debug!("{} {} 需要订阅 {} 个交易对", exchange, market_type, symbols.len());
        }

        // 连接状态表：用于定时输出状态
        let link_status: Arc<Mutex<HashMap<(ExchangeType, MarketType), WsLinkStatus>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // 定时输出所有连接状态（每小时一次，debug 级别）
        {
            let link_status = link_status.clone();
            tokio::spawn(async move {
                const STATUS_INTERVAL_SECS: u64 = 3600; // 1 小时
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(STATUS_INTERVAL_SECS)).await;

                    let mut rows: Vec<((ExchangeType, MarketType), WsLinkStatus)> = {
                        let st = link_status.lock().await;
                        st.iter().map(|(k, v)| (*k, v.clone())).collect()
                    };

                    rows.sort_by_key(|((ex, mt), _)| (format!("{ex}"), format!("{mt}")));

                    debug!("WS_LINK_TABLE:");
                    debug!("+------------+------------+----------+--------+--------+----------+----------+--------+----------+");
                    debug!(
                        "| {:<10} | {:<10} | {:<8} | {:>6} | {:>6} | {:>8} | {:>8} | {:>6} | {:<8} |",
                        "ex", "mkt", "conn", "sym", "rec", "recv", "save", "tout", "age"
                    );
                    debug!("+------------+------------+----------+--------+--------+----------+----------+--------+----------+");
                    
                    let mut error_rows: Vec<(String, String, u32, String)> = Vec::new();
                    for ((exchange, market_type), s) in rows {
                        let last_msg_str = s
                            .last_msg_age_s
                            .map(|age| {
                                if age < 60 {
                                    format!("{}s", age)
                                } else {
                                    format!("{}m", age / 60)
                                }
                            })
                            .unwrap_or_else(|| "-".to_string());
                        
                        let line = format!(
                            "| {:<10} | {:<10} | {:<8} | {:>6} | {:>6} | {:>8} | {:>8} | {:>6} | {:<8} |",
                            format!("{}", exchange),
                            format!("{}", market_type),
                            if s.connected { "Y" } else { "N" },
                            s.symbols,
                            s.reconnects,
                            s.recv_ok,
                            s.save_ok,
                            s.recv_timeout,
                            last_msg_str
                        );
                        debug!("{}", line);
                        
                        if let Some(err) = &s.last_err {
                            let mut msg = err.clone();
                            if msg.len() > 200 {
                                msg.truncate(197);
                                msg.push_str("...");
                            }
                            error_rows.push((
                                format!("{}", exchange),
                                format!("{}", market_type),
                                s.reconnects,
                                msg,
                            ));
                        }
                    }
                    debug!("+------------+------------+----------+--------+--------+----------+----------+--------+----------+");
                    
                    for (ex, mkt, rec, msg) in error_rows {
                        debug!("ERR ex={} mkt={} rec={} msg={}", ex, mkt, rec, msg);
                    }
                }
            });
        }

        // 为每个交易所和市场类型创建独立的WebSocket任务
        let mut tasks = Vec::new();
        for ((exchange_type, market_type), symbols) in symbols_by_exchange_and_type {
            if let Some(adapter) = self.create_ws_adapter(exchange_type, market_type) {
                let symbols_clone = symbols.clone();
                let repository_clone = self.repository.clone();
                let config_clone = self.config.clone();
                let link_status_clone = link_status.clone();

                // 每个任务独立持有自己的 adapter，无锁竞争
                let task = tokio::spawn(async move {
                    Self::handle_exchange_websocket(
                        exchange_type,
                        symbols_clone,
                        adapter,
                        repository_clone,
                        config_clone,
                        market_type,
                        link_status_clone,
                    )
                    .await;
                });

                tasks.push(task);
            } else {
                warn!("不支持 {} {} 的WebSocket适配器", exchange_type, market_type);
            }
        }

        futures_util::future::join_all(tasks).await;
        Ok(())
    }

    /// 创建WebSocket适配器
    fn create_ws_adapter(&self, exchange_type: ExchangeType, market_type: MarketType) -> Option<Box<dyn WebSocketAdapter>> {
        match exchange_type {
            ExchangeType::Binance => {
                Some(Box::new(crate::exchange::BinanceWebSocketAdapter::new(market_type)))
            }
            ExchangeType::Okx => {
                Some(Box::new(crate::exchange::OkxWebSocketAdapter::new(market_type)))
            }
            ExchangeType::Bybit => {
                Some(Box::new(crate::exchange::BybitWebSocketAdapter::new(market_type)))
            }
            ExchangeType::Gateio => {
                Some(Box::new(crate::exchange::GateioWebSocketAdapter::new(market_type)))
            }
            ExchangeType::Bitget => {
                Some(Box::new(crate::exchange::BitgetWebSocketAdapter::new(market_type)))
            }
            _ => None,
        }
    }

    /// 处理单个交易所的WebSocket连接
    /// 注意：每个任务独立持有自己的 adapter，无锁竞争
    async fn handle_exchange_websocket(
        exchange_type: ExchangeType,
        symbols: Vec<String>,
        mut adapter: Box<dyn WebSocketAdapter>,
        repository: Arc<Repository>,
        config: WebSocketConfig,
        market_type: MarketType,
        link_status: Arc<Mutex<HashMap<(ExchangeType, MarketType), WsLinkStatus>>>,
    ) {
        debug!("启动 {} {} WS，{} 交易对", exchange_type, market_type, symbols.len());

        // 初始化状态
        {
            let mut st = link_status.lock().await;
            st.entry((exchange_type, market_type))
                .or_default()
                .symbols = symbols.len();
        }

        let mut reconnect_count = 0u32;
        let mut recv_ok: u64 = 0;
        let mut save_ok: u64 = 0;
        let mut recv_timeout: u64 = 0;
        let mut recv_err: u64 = 0;
        let mut save_err: u64 = 0;
        let mut last_msg_at: Option<std::time::Instant> = None;
        let mut last_stats_log = std::time::Instant::now();

        loop {
            // 连接WebSocket（直接操作 adapter，无锁竞争）
            if let Err(e) = adapter.connect().await {
                error!("{} {} WebSocket连接失败: {}", exchange_type, market_type, e);
                reconnect_count += 1;
                {
                    let mut st = link_status.lock().await;
                    let entry = st.entry((exchange_type, market_type)).or_default();
                    entry.reconnects = reconnect_count;
                    entry.connected = false;
                    entry.last_err = Some(e.to_string());
                }
                if config.max_reconnect_attempts > 0 && reconnect_count >= config.max_reconnect_attempts {
                    error!("{} {} WebSocket达到最大重连次数，停止重连", exchange_type, market_type);
                    break;
                }
                let base_delay = config.reconnect_delay_seconds;
                let delay_seconds = std::cmp::min(base_delay * 2_u64.pow(reconnect_count.saturating_sub(1)), 60);
                warn!("{} {} WebSocket {} 秒后重试（第 {} 次重连）", exchange_type, market_type, delay_seconds, reconnect_count);
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;
                continue;
            }

            // 订阅交易对
            debug!("{} {} 订阅 {} 交易对", exchange_type, market_type, symbols.len());
            if let Err(e) = adapter.subscribe(&symbols, market_type).await {
                error!("{} {} WebSocket订阅失败: {}", exchange_type, market_type, e);
                reconnect_count += 1;
                {
                    let mut st = link_status.lock().await;
                    let entry = st.entry((exchange_type, market_type)).or_default();
                    entry.reconnects = reconnect_count;
                    entry.connected = false;
                    entry.last_err = Some(e.to_string());
                }
                if config.max_reconnect_attempts > 0 && reconnect_count >= config.max_reconnect_attempts {
                    error!("{} {} WebSocket达到最大重连次数，停止重连", exchange_type, market_type);
                    break;
                }
                let base_delay = config.reconnect_delay_seconds;
                let delay_seconds = std::cmp::min(base_delay * 2_u64.pow(reconnect_count.saturating_sub(1)), 60);
                warn!("{} {} WebSocket {} 秒后重试订阅（第 {} 次重连）", exchange_type, market_type, delay_seconds, reconnect_count);
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;
                continue;
            }

            debug!("{} {} WS已连接", exchange_type, market_type);
            reconnect_count = 0;
            {
                let mut st = link_status.lock().await;
                let entry = st.entry((exchange_type, market_type)).or_default();
                entry.reconnects = 0;
                entry.connected = true;
                entry.last_err = None;
            }

            // 批量处理参数
            let (batch_size, batch_timeout_secs): (usize, f64) = match exchange_type {
                ExchangeType::Okx | ExchangeType::Bybit => (300, 0.5),
                ExchangeType::Binance | ExchangeType::Gateio | ExchangeType::Bitget => (50, 2.0),
                ExchangeType::Unknown => (100, 1.0),
            };
            debug!("{} {} batch={}, timeout={}s", exchange_type, market_type, batch_size, batch_timeout_secs);
            
            const RECV_TIMEOUT_SECS: u64 = 10;
            let mut batch: Vec<crate::models::snapshot::MarketSnapshot> = Vec::with_capacity(batch_size);
            let mut last_batch_save = std::time::Instant::now();
            // 节流：每个 symbol 每秒最多 1 条数据
            let mut symbol_last_save: HashMap<String, std::time::Instant> = HashMap::new();
            
            loop {
                // 检查是否需要刷新批次
                let should_flush = !batch.is_empty() && (
                    batch.len() >= batch_size || last_batch_save.elapsed().as_secs_f64() >= batch_timeout_secs
                );
                
                if should_flush {
                    let batch_to_save = std::mem::take(&mut batch);
                    match repository.save_snapshots(&batch_to_save).await {
                        Ok(inserted_count) => {
                            if inserted_count > 0 {
                                save_ok += inserted_count as u64;
                            }
                            {
                                let mut st = link_status.lock().await;
                                let entry = st.entry((exchange_type, market_type)).or_default();
                                entry.recv_ok = recv_ok;
                                entry.save_ok = save_ok;
                                entry.recv_timeout = recv_timeout;
                                entry.recv_err = recv_err;
                                entry.save_err = save_err;
                                entry.last_msg_age_s = last_msg_at.map(|t| t.elapsed().as_secs());
                            }
                        }
                        Err(e) => {
                            save_err += 1;
                            warn!("[{} {}] 批量保存快照失败 (save_err={}, batch_size={}): {}", exchange_type, market_type, save_err, batch_to_save.len(), e);
                        }
                    }
                    last_batch_save = std::time::Instant::now();
                }
                
                // 直接接收消息，无锁竞争！这是关键优化点
                let snapshot_result = tokio::time::timeout(
                    tokio::time::Duration::from_secs(RECV_TIMEOUT_SECS),
                    adapter.receive_message(),
                ).await;

                match snapshot_result {
                    Ok(Ok(snapshot)) => {
                        recv_ok += 1;
                        last_msg_at = Some(std::time::Instant::now());
                        // 过滤无效数据：bid_price、ask_price、last_price 都为空则跳过
                        if snapshot.bid_price.is_none() && snapshot.ask_price.is_none() && snapshot.last_price.is_none() {
                            continue;
                        }
                        // 节流：每个 symbol 每秒最多 1 条数据
                        let now = std::time::Instant::now();
                        if let Some(last_time) = symbol_last_save.get(&snapshot.symbol) {
                            if now.duration_since(*last_time).as_millis() < 1000 {
                                continue; // 跳过，距离上次保存不到 1 秒
                            }
                        }
                        symbol_last_save.insert(snapshot.symbol.clone(), now);
                        batch.push(snapshot);
                    }
                    Ok(Err(e)) => {
                        recv_err += 1;
                        warn!("[{} {}] WebSocket接收/解析失败 (recv_err={}): {}", exchange_type, market_type, recv_err, e);
                        {
                            let mut st = link_status.lock().await;
                            let entry = st.entry((exchange_type, market_type)).or_default();
                            entry.recv_ok = recv_ok;
                            entry.save_ok = save_ok;
                            entry.recv_timeout = recv_timeout;
                            entry.recv_err = recv_err;
                            entry.save_err = save_err;
                            entry.connected = false;
                            entry.last_msg_age_s = last_msg_at.map(|t| t.elapsed().as_secs());
                            entry.last_err = Some(e.to_string());
                        }
                        // 保存批次中的剩余数据
                        if !batch.is_empty() {
                            let batch_to_save = std::mem::take(&mut batch);
                            if let Ok(inserted_count) = repository.save_snapshots(&batch_to_save).await {
                                if inserted_count > 0 {
                                    save_ok += inserted_count as u64;
                                }
                            }
                        }
                        // 尝试重连
                        if let Err(reconnect_err) = adapter.reconnect().await {
                            error!("[{} {}] WebSocket重连失败: {}", exchange_type, market_type, reconnect_err);
                        }
                        break;
                    }
                    Err(_) => {
                        recv_timeout += 1;
                        {
                            let mut st = link_status.lock().await;
                            let entry = st.entry((exchange_type, market_type)).or_default();
                            entry.recv_ok = recv_ok;
                            entry.save_ok = save_ok;
                            entry.recv_timeout = recv_timeout;
                            entry.recv_err = recv_err;
                            entry.save_err = save_err;
                            entry.last_msg_age_s = last_msg_at.map(|t| t.elapsed().as_secs());
                        }
                    }
                }

                // 每 30 秒更新一次统计
                if last_stats_log.elapsed().as_secs() >= 30 {
                    let last_age = last_msg_at.map(|t| t.elapsed().as_secs());
                    {
                        let mut st = link_status.lock().await;
                        let entry = st.entry((exchange_type, market_type)).or_default();
                        entry.recv_ok = recv_ok;
                        entry.save_ok = save_ok;
                        entry.recv_timeout = recv_timeout;
                        entry.recv_err = recv_err;
                        entry.save_err = save_err;
                        entry.last_msg_age_s = last_age;
                    }
                    last_stats_log = std::time::Instant::now();
                }
            }

            // 重连延迟
            tokio::time::sleep(tokio::time::Duration::from_secs(config.reconnect_delay_seconds)).await;
        }
    }
}
