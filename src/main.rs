pub mod api;
pub mod arbitrage;
pub mod collector;
pub mod config;
pub mod exchange;
pub mod models;
pub mod monitor;
pub mod notification;
pub mod storage;

use anyhow::Result;
use config::AppConfig;
use exchange::{
    BinanceAdapter, BybitAdapter, BitgetAdapter, ExchangeManager, GateioAdapter, OkxAdapter,
};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, error, warn};

use crate::storage::database::Database;
use crate::storage::repository::Repository;
use crate::collector::{SnapshotCollector, WebSocketCollector};
use crate::arbitrage::analyzer::ArbitrageAnalyzer;
use crate::monitor::ArbitrageMonitor;
use crate::notification::TelegramNotifier;
use crate::models::ExchangeType;
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Crypto Hunter - 数字货币交易所套利平台启动中...");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        info!("使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    info!("初始化数据库连接...");
    let database = Database::new(&app_config.database.url).await?;
    database.init_schema().await?;
    info!("数据库初始化完成");

    let repository = Arc::new(Repository::new(&database));

    // 初始化交易所适配器
    info!("初始化交易所适配器...");
    let mut exchange_manager = ExchangeManager::new();
    
    if app_config.exchanges.iter().any(|e| e.name == "binance" && e.enabled) {
        exchange_manager.add_adapter(Box::new(BinanceAdapter::new()));
        info!("Binance适配器已加载");
    }
    
    if app_config.exchanges.iter().any(|e| e.name == "okx" && e.enabled) {
        exchange_manager.add_adapter(Box::new(OkxAdapter::new()));
        info!("OKX适配器已加载");
    }
    
    if app_config.exchanges.iter().any(|e| e.name == "bybit" && e.enabled) {
        exchange_manager.add_adapter(Box::new(BybitAdapter::new()));
        info!("Bybit适配器已加载");
    }
    
    if app_config.exchanges.iter().any(|e| e.name == "gateio" && e.enabled) {
        exchange_manager.add_adapter(Box::new(GateioAdapter::new()));
        info!("Gate.io适配器已加载");
    }
    
    if app_config.exchanges.iter().any(|e| e.name == "bitget" && e.enabled) {
        exchange_manager.add_adapter(Box::new(BitgetAdapter::new()));
        info!("Bitget适配器已加载");
    }

    // 初始化快照收集器和套利分析器
    let exchange_manager_arc = Arc::new(exchange_manager);
    let repository_clone = repository.clone();
    
    let snapshot_collector = Arc::new(SnapshotCollector::new(
        exchange_manager_arc.clone(),
        repository_clone.clone(),
    ));
    
    let arbitrage_analyzer = Arc::new(ArbitrageAnalyzer::new(repository_clone.clone()));

    // 初始化 Telegram 通知服务
    let telegram_notifier = Arc::new(TelegramNotifier::new(
        app_config.notification.telegram_bot_token.clone(),
        app_config.notification.telegram_chat_id.clone(),
    ));

    if telegram_notifier.is_enabled() {
        info!("Telegram 通知服务已启用");
        if let Err(e) = telegram_notifier
            .send_message("🚀 Crypto Hunter 套利监控服务已启动")
            .await
        {
            error!("发送启动通知失败: {}", e);
        }
    } else {
        info!("Telegram 通知服务未配置，将不会发送通知");
    }

    // 启动数据收集任务（实时行情）
    // 注意：交易对收集已独立为 pair-collector 二进制，不再在此运行
    info!("启动数据收集任务...");
    
    // WebSocket实时数据采集（替代HTTP轮询）
    // 默认使用 WebSocket，避免 HTTP 轮询的速率限制和端口耗尽问题
    if app_config.websocket.enabled {
        info!("启动WebSocket实时数据采集（替代HTTP轮询，无需速率限制）...");
        let mut ws_collector = WebSocketCollector::new(
            exchange_manager_arc.clone(),
            repository_clone.clone(),
            app_config.websocket.clone(),
        );
        tokio::spawn(async move {
            if let Err(e) = ws_collector.start().await {
                error!("WebSocket采集失败: {}", e);
            }
        });
    } else {
        // 如果WebSocket未启用，使用HTTP轮询（向后兼容，但不推荐）
        warn!("WebSocket未启用，使用HTTP轮询采集（不推荐，会有速率限制和端口耗尽问题）...");
        let snapshot_interval = app_config.data_collection.price_sync_interval_seconds;
        tokio::spawn(async move {
            let mut snapshot_interval = interval(Duration::from_secs(snapshot_interval));
            loop {
                snapshot_interval.tick().await;
                if let Err(e) = snapshot_collector.collect_all_snapshots().await {
                    error!("收集快照失败: {}", e);
                }
            }
        });
    }

    // 启动API服务器
    info!("启动API服务器...");
    let api_server = api::server::ApiServer::new(
        repository.clone(),
        arbitrage_analyzer.clone(),
        app_config.api.port,
    );
    api_server.start().await?;

    Ok(())
}

// 注意：交易对收集已独立为 pair-collector 二进制
// 请使用独立的 pair-collector 进程来收集交易对
