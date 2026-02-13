use anyhow::Result;
use clap::Parser;
use crypto_hunter::{
    config::AppConfig,
    exchange::{
        BinanceAdapter, BitgetAdapter, BybitAdapter, ExchangeManager, GateioAdapter, OkxAdapter,
    },
    storage::{database::Database, repository::Repository},
    collector::HistoricalCollector,
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use std::sync::Arc;
use tracing::{error, info, warn};

const SERVICE_NAME: &str = "historical_collector";

/// 历史数据采集工具
/// 独立运行，用于批量获取历史K线数据
#[derive(Parser, Debug)]
#[command(name = "historical-collector")]
#[command(about = "历史数据采集工具，用于批量获取历史K线数据", long_about = None)]
struct Args {
    /// 交易对列表，逗号分隔（可选，默认：从数据库获取全部交易对）
    #[arg(long)]
    symbols: Option<String>,

    /// 开始时间，ISO 8601格式（可选，默认：今年1月1日 00:00:00 UTC）
    #[arg(long)]
    start_time: Option<String>,

    /// 结束时间，ISO 8601格式（可选，默认：当前时间）
    #[arg(long)]
    end_time: Option<String>,

    /// K线间隔，如 "1m", "5m", "1h", "1d"（可选，默认：1h）
    #[arg(long, default_value = "1h")]
    interval: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    info!("历史数据采集工具启动...");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        warn!("无法从环境变量加载配置，使用默认配置");
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

    let exchange_manager = Arc::new(exchange_manager);

    // 解析时间参数
    let start_time = if let Some(start_str) = args.start_time {
        DateTime::parse_from_rfc3339(&start_str)
            .map_err(|e| anyhow::anyhow!("无法解析开始时间: {}: {}", start_str, e))?
            .with_timezone(&Utc)
    } else {
        // 默认：今年1月1日 00:00:00 UTC
        let now = Utc::now();
        let year = now.year();
        let date = NaiveDate::from_ymd_opt(year, 1, 1)
            .ok_or_else(|| anyhow::anyhow!("无法创建日期"))?;
        let time = NaiveTime::from_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("无法创建时间"))?;
        let naive_dt = date.and_time(time);
        DateTime::from_naive_utc_and_offset(naive_dt, Utc)
    };

    let end_time = if let Some(end_str) = args.end_time {
        DateTime::parse_from_rfc3339(&end_str)
            .map_err(|e| anyhow::anyhow!("无法解析结束时间: {}: {}", end_str, e))?
            .with_timezone(&Utc)
    } else {
        Utc::now()
    };

    // 解析交易对列表
    let symbols: Option<Vec<String>> = args.symbols.map(|s| {
        s.split(',')
            .map(|sym| sym.trim().to_string())
            .filter(|sym| !sym.is_empty())
            .collect()
    });

    info!(
        "采集配置: 开始时间 = {}, 结束时间 = {}, 间隔 = {}, 交易对 = {}",
        start_time.format("%Y-%m-%d %H:%M:%S UTC"),
        end_time.format("%Y-%m-%d %H:%M:%S UTC"),
        args.interval,
        if let Some(ref syms) = symbols {
            format!("指定 {} 个", syms.len())
        } else {
            "全部".to_string()
        }
    );

    // 创建历史数据采集器
    let collector = HistoricalCollector::new(exchange_manager, repository);

    // 执行采集
    if let Err(e) = collector
        .collect_historical_data(symbols, start_time, end_time, &args.interval)
        .await
    {
        error!("历史数据采集失败: {}", e);
        return Err(e);
    }

    info!("历史数据采集完成");
    Ok(())
}
