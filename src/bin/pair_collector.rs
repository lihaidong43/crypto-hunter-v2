use anyhow::Result;
use clap::Parser;
use crypto_hunter::{
    config::AppConfig,
    exchange::{
        BinanceAdapter, BybitAdapter, BitgetAdapter, ExchangeManager, GateioAdapter, OkxAdapter,
    },
    storage::{database::Database, repository::Repository},
    collector::filter_trading_pairs,
    models::exchange::MarketType,
};
#[allow(unused_imports)]
use chrono::{DateTime, Utc, Duration as ChronoDuration}; // DateTime 用于 format 方法
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, error, warn};

const SERVICE_NAME: &str = "pair_collector";

/// 交易对收集服务
/// 独立运行，定期从各交易所同步交易对信息
#[derive(Parser, Debug)]
#[command(name = "pair-collector")]
#[command(about = "交易对收集服务，定期同步各交易所的交易对信息", long_about = None)]
struct Args {
    /// 同步间隔（秒），覆盖配置文件中的设置
    #[arg(short, long)]
    interval: Option<u64>,

    /// 强制立即同步，忽略上次同步时间
    #[arg(short, long)]
    force: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    info!("交易对收集服务启动...");

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

    // 确定同步间隔（命令行参数 > 配置文件）
    let sync_interval_seconds = args.interval.unwrap_or(
        app_config.data_collection.pair_sync_interval_seconds
    );
    info!("同步间隔: {} 秒 ({} 小时)", sync_interval_seconds, sync_interval_seconds as f64 / 3600.0);

    // 显示过滤配置
    info!(
        "交易对过滤配置: 最少交易所数量 = {}, 允许的计价货币 = {:?}, 仅期货 = {}, 仅现货 = {}",
        app_config.pair_filter.min_exchange_count,
        app_config.pair_filter.allowed_quote_currencies,
        app_config.pair_filter.futures_only,
        app_config.pair_filter.spot_only
    );

    // 检查上次同步时间
    if args.force {
        info!("强制同步模式：忽略上次同步时间，立即执行同步");
        // 强制模式下立即执行同步，不等待
        sync_trading_pairs(
            exchange_manager.as_ref(),
            repository.as_ref(),
            &app_config.pair_filter,
        )
        .await;
        
        // 保存同步时间
        repository.save_sync_time(SERVICE_NAME, Utc::now()).await?;
        info!("同步时间已保存");
    } else {
        // 非强制模式：检查上次同步时间
        let next_sync_time = match repository.get_last_sync_time(SERVICE_NAME).await? {
            Some(last_sync) => {
                let next_sync = last_sync + ChronoDuration::seconds(sync_interval_seconds as i64);
                let now = Utc::now();
                
                if next_sync <= now {
                    info!(
                        "上次同步时间: {}，已超过间隔时间，立即同步",
                        last_sync.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    now
                } else {
                    let wait_seconds = (next_sync - now).num_seconds();
                    info!(
                        "上次同步时间: {}，距离下次同步还有 {} 秒（{} 分钟）",
                        last_sync.format("%Y-%m-%d %H:%M:%S UTC"),
                        wait_seconds,
                        wait_seconds / 60
                    );
                    next_sync
                }
            }
            None => {
                info!("未找到上次同步记录，立即执行首次同步");
                Utc::now()
            }
        };

        // 如果下次同步时间在未来，等待
        let now = Utc::now();
        if next_sync_time > now {
            let wait_seconds = (next_sync_time - now).num_seconds();
            info!("等待 {} 秒（{} 分钟）后开始首次同步...", wait_seconds, wait_seconds / 60);
            
            tokio::time::sleep(Duration::from_secs(wait_seconds as u64)).await;
        }

        // 执行首次同步
        sync_trading_pairs(
            exchange_manager.as_ref(),
            repository.as_ref(),
            &app_config.pair_filter,
        )
        .await;
        
        // 保存同步时间
        repository.save_sync_time(SERVICE_NAME, Utc::now()).await?;
        info!("同步时间已保存");
    }

    // 保存同步时间
    repository.save_sync_time(SERVICE_NAME, Utc::now()).await?;
    info!("同步时间已保存");

    // 定期同步
    let mut sync_interval = interval(Duration::from_secs(sync_interval_seconds));
    info!("开始定期同步任务（每 {} 秒执行一次）", sync_interval_seconds);

    loop {
        sync_interval.tick().await;
        
        let sync_start = Utc::now();
        sync_trading_pairs(
            exchange_manager.as_ref(),
            repository.as_ref(),
            &app_config.pair_filter,
        )
        .await;
        
        // 保存同步时间
        if let Err(e) = repository.save_sync_time(SERVICE_NAME, sync_start).await {
            error!("保存同步时间失败: {}", e);
        } else {
            info!("同步时间已更新: {}", sync_start.format("%Y-%m-%d %H:%M:%S UTC"));
        }
    }
}

async fn sync_trading_pairs(
    exchange_manager: &ExchangeManager,
    repository: &Repository,
    filter_config: &crypto_hunter::config::PairFilterConfig,
) {
    let sync_start = std::time::Instant::now();
    info!("========== 开始同步交易对 ==========");
    
    // 使用过滤逻辑收集和过滤交易对
    // 现货交易对（如果没有设置 futures_only）
    if !filter_config.futures_only {
        match filter_trading_pairs(exchange_manager, filter_config, MarketType::Spot).await {
            Ok((filtered_pairs, stats)) => {
                info!("========== 现货交易对过滤完成 ==========");
                stats.log_summary();
                if let Err(e) = repository.save_trading_pairs(&filtered_pairs).await {
                    error!("保存现货交易对失败: {}", e);
                } else {
                    info!("现货交易对同步成功，保存了 {} 个交易对", filtered_pairs.len());
                }
            }
            Err(e) => {
                error!("过滤现货交易对失败: {}", e);
            }
        }
    } else {
        info!("已配置 futures_only，跳过现货交易对同步");
    }

    // 期货交易对（如果没有设置 spot_only）
    if !filter_config.spot_only {
        match filter_trading_pairs(exchange_manager, filter_config, MarketType::Futures).await {
            Ok((filtered_pairs, stats)) => {
                info!("========== 期货交易对过滤完成 ==========");
                stats.log_summary();
                if let Err(e) = repository.save_trading_pairs(&filtered_pairs).await {
                    error!("保存期货交易对失败: {}", e);
                } else {
                    info!("期货交易对同步成功，保存了 {} 个交易对", filtered_pairs.len());
                }
            }
            Err(e) => {
                error!("过滤期货交易对失败: {}", e);
            }
        }
    } else {
        info!("已配置 spot_only，跳过期货交易对同步");
    }
    
    let elapsed = sync_start.elapsed();
    info!("========== 交易对同步完成，耗时: {:.2} 秒 ==========", elapsed.as_secs_f64());
}
