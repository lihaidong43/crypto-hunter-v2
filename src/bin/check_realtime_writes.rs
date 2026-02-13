use anyhow::Result;
use crypto_hunter::{
    config::AppConfig,
    storage::{database::Database, repository::Repository},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("检查数据库实时写入情况...");
    info!("将每10秒检查一次数据库中的最新数据，持续60秒");
    info!("如果数据持续更新，说明有进程在写入");
    info!("按 Ctrl+C 可以提前停止");
    info!("");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        warn!("无法从环境变量加载配置，使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    let database = Database::new(&app_config.database.url).await?;
    let repository = Arc::new(Repository::new(&database));

    // 获取初始状态
    let mut last_max_time = repository.get_latest_snapshot_time().await?;
    let mut last_count: i64 = 0;
    
    if let Some(initial_time) = last_max_time {
        info!("初始状态: 最新快照时间 = {}", initial_time);
        
        // 统计初始数据量
        let initial_stats = repository.count_recent_snapshots(initial_time - chrono::Duration::minutes(1)).await?;
        last_count = initial_stats.iter().map(|(_, _, cnt)| cnt).sum();
        info!("初始状态: 最近1分钟数据量 = {} 条", last_count);
    } else {
        info!("初始状态: 数据库中没有数据");
    }
    
    info!("");
    info!("开始监控（每10秒检查一次）...");
    info!("");

    // 监控循环
    let mut check_count = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        check_count += 1;

        let now = Utc::now();
        
        // 检查最新快照时间
        if let Some(current_max_time) = repository.get_latest_snapshot_time().await? {
            // 检查是否有新数据
            if let Some(last_time) = last_max_time {
                if current_max_time > last_time {
                    let new_data_count = repository
                        .count_recent_snapshots(last_time)
                        .await?
                        .iter()
                        .map(|(_, _, cnt)| cnt)
                        .sum::<i64>();
                    
                    let time_diff = (current_max_time - last_time).num_seconds();
                    warn!(
                        "⚠️  [检查 #{}] 发现新数据写入！",
                        check_count
                    );
                    warn!(
                        "    最新快照时间: {} (比上次新 {} 秒)",
                        current_max_time, time_diff
                    );
                    warn!(
                        "    新增数据量: {} 条",
                        new_data_count
                    );
                    
                    // 统计最近1分钟的数据
                    let one_min_ago = now - chrono::Duration::minutes(1);
                    let recent_stats = repository.count_recent_snapshots(one_min_ago).await?;
                    let recent_count: i64 = recent_stats.iter().map(|(_, _, cnt)| cnt).sum();
                    
                    if recent_count > last_count {
                        let new_in_min = recent_count - last_count;
                        warn!(
                            "    最近1分钟新增: {} 条 (总计: {} 条)",
                            new_in_min, recent_count
                        );
                    }
                    
                    // 按交易所统计
                    warn!("    按交易所统计（最近1分钟）:");
                    for (exchange, market_type, count) in &recent_stats {
                        if *count > 0 {
                            warn!("      {} {}: {} 条", exchange, market_type, count);
                        }
                    }
                } else {
                    info!(
                        "[检查 #{}] 没有新数据写入 (最新时间: {})",
                        check_count, current_max_time
                    );
                }
            } else {
                // 第一次检查
                info!(
                    "[检查 #{}] 当前最新时间: {}",
                    check_count, current_max_time
                );
            }
            
            last_max_time = Some(current_max_time);
        } else {
            info!("[检查 #{}] 数据库中没有数据", check_count);
        }
        
        info!("");
        
        // 检查60秒后停止
        if check_count >= 6 {
            info!("监控完成（已检查60秒）");
            break;
        }
    }

    info!("=== 最终状态 ===");
    if let Some(final_time) = repository.get_latest_snapshot_time().await? {
        info!("最新快照时间: {}", final_time);
        let age = Utc::now() - final_time;
        if age.num_seconds() < 60 {
            warn!("⚠️  数据很新（{} 秒前），说明有进程在持续写入", age.num_seconds());
        } else {
            info!("数据较旧（{} 秒前），可能没有进程在写入", age.num_seconds());
        }
    }

    Ok(())
}
