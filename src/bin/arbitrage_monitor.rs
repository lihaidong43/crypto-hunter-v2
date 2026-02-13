use anyhow::Result;
use clap::Parser;
use crypto_hunter::{
    arbitrage::analyzer::ArbitrageAnalyzer,
    config::AppConfig,
    monitor::arbitrage_monitor::ArbitrageMonitor,
    notification::TelegramNotifier,
    storage::{database::Database, repository::Repository},
};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{error, info};

/// 套利监控独立进程
#[derive(Parser, Debug)]
#[command(name = "arbitrage-monitor")]
#[command(about = "套利监控服务（独立进程，仅从数据库读取快照，不触发HTTP采集）", long_about = None)]
struct Args {}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let _args = Args::parse();

    info!("套利监控服务启动（独立进程）...");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        info!("无法从环境变量加载配置，使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    info!("初始化数据库连接...");
    let database = Database::new(&app_config.database.url).await?;
    database.init_schema().await?;
    info!("数据库初始化完成");

    let repository = Arc::new(Repository::new(&database));

    // 初始化套利分析器（只从 DB 读取快照）
    let arbitrage_analyzer = Arc::new(ArbitrageAnalyzer::new(repository.clone()));

    // 初始化 Telegram 通知服务
    let telegram_notifier = Arc::new(TelegramNotifier::new(
        app_config.notification.telegram_bot_token.clone(),
        app_config.notification.telegram_chat_id.clone(),
    ));

    if telegram_notifier.is_enabled() {
        info!("Telegram 通知服务已启用");
        if let Err(e) = telegram_notifier
            .send_message("🚀 Crypto Hunter 套利监控（独立进程）已启动")
            .await
        {
            error!("发送启动通知失败: {}", e);
        }
    } else {
        info!("Telegram 通知服务未配置，将不会发送通知");
    }

    // 初始化套利监控服务
    let min_spread_threshold =
        Decimal::try_from(app_config.monitoring.spread_threshold).unwrap_or(Decimal::ONE);

    // 监控间隔：沿用主进程里的策略
    let monitor_interval = std::cmp::max(
        app_config.data_collection.price_sync_interval_seconds,
        30, // 最少30秒
    );

    let monitor = Arc::new(
        ArbitrageMonitor::new(
            repository.clone(),
            arbitrage_analyzer.clone(),
            telegram_notifier.clone(),
            min_spread_threshold,
            monitor_interval,
            300, // 通知冷却时间：5分钟
        )
        .with_batch_size(50),
    );

    // 启动套利监控（本进程主任务）
    info!(
        "套利监控进程运行中：interval={}s, spread_threshold={}%",
        monitor_interval, app_config.monitoring.spread_threshold
    );

    if let Err(e) = monitor.start().await {
        error!("套利监控服务错误: {}", e);
    }

    Ok(())
}

