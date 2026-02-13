use anyhow::Result;
use crypto_hunter::{
    config::AppConfig,
    storage::{database::Database, repository::Repository},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("检查数据库写入情况...");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        warn!("无法从环境变量加载配置，使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    let database = Database::new(&app_config.database.url).await?;
    let repository = Arc::new(Repository::new(&database));

    let now = Utc::now();
    let one_hour_ago = now - chrono::Duration::hours(1);
    let one_day_ago = now - chrono::Duration::days(1);

    info!("当前时间: {}", now);
    info!("");

    // 1. 检查最近1小时的数据分布
    info!("=== 最近1小时的数据分布 ===");
    let stats_1h = repository.count_recent_snapshots(one_hour_ago).await?;
    for (exchange, market_type, count) in &stats_1h {
        if *count > 0 {
            info!("  {} {}: {} 条", exchange, market_type, count);
        }
    }
    let total_1h: i64 = stats_1h.iter().map(|(_, _, cnt)| cnt).sum();
    info!("  总计: {} 条", total_1h);
    info!("");

    // 2. 检查最近1天的数据分布
    info!("=== 最近1天的数据分布 ===");
    let stats_1d = repository.count_recent_snapshots(one_day_ago).await?;
    for (exchange, market_type, count) in &stats_1d {
        if *count > 0 {
            info!("  {} {}: {} 条", exchange, market_type, count);
        }
    }
    let total_1d: i64 = stats_1d.iter().map(|(_, _, cnt)| cnt).sum();
    info!("  总计: {} 条", total_1d);
    info!("");

    // 3. 检查数据库中的最新快照时间
    info!("=== 数据库最新快照时间 ===");
    if let Some(latest_time) = repository.get_latest_snapshot_time().await? {
        info!("  最新快照时间: {}", latest_time);
        let age = now - latest_time;
        if age.num_seconds() < 60 {
            info!("  数据很新（{} 秒前）", age.num_seconds());
        } else if age.num_minutes() < 60 {
            info!("  数据较新（{} 分钟前）", age.num_minutes());
        } else {
            warn!("  数据较旧（{} 小时前）", age.num_hours());
        }
    } else {
        info!("  数据库中没有快照数据");
    }
    info!("");

    // 4. 检查是否有未来时间戳的数据
    info!("=== 检查未来时间戳的数据 ===");
    let client = database.pool().get().await?;
    let future_rows: Vec<tokio_postgres::Row> = client
        .query(
            r#"
            SELECT exchange, market_type, COUNT(*) as cnt,
                   MIN(snapshot_time) as min_time, MAX(snapshot_time) as max_time
            FROM market_snapshots
            WHERE snapshot_time > NOW()
            GROUP BY exchange, market_type
            ORDER BY exchange, market_type
        "#,
            &[],
        )
        .await?;

    if future_rows.is_empty() {
        info!("  没有未来时间戳的数据");
    } else {
        warn!("  发现未来时间戳的数据：");
        for row in future_rows {
            let exchange: String = row.get("exchange");
            let market_type: String = row.get("market_type");
            let count: i64 = row.get("cnt");
            let min_time: DateTime<Utc> = row.get("min_time");
            let max_time: DateTime<Utc> = row.get("max_time");
            warn!(
                "    {} {}: {} 条, 时间范围: {} ~ {}",
                exchange, market_type, count, min_time, max_time
            );
        }
    }
    info!("");

    // 5. 检查时间戳分布（按分钟统计）
    info!("=== 最近10分钟的数据分布（按分钟） ===");
    let ten_minutes_ago = now - chrono::Duration::minutes(10);
    let time_dist_rows: Vec<tokio_postgres::Row> = client
        .query(
            r#"
            SELECT 
                DATE_TRUNC('minute', snapshot_time)::timestamptz as minute,
                COUNT(*) as cnt
            FROM market_snapshots
            WHERE snapshot_time >= $1
            GROUP BY DATE_TRUNC('minute', snapshot_time)
            ORDER BY minute DESC
            LIMIT 10
        "#,
            &[&ten_minutes_ago],
        )
        .await?;

    if time_dist_rows.is_empty() {
        info!("  最近10分钟没有数据");
    } else {
        for row in time_dist_rows {
            let minute: DateTime<Utc> = row.get("minute");
            let count: i64 = row.get("cnt");
            info!("  {}: {} 条", minute.format("%H:%M:%S"), count);
        }
    }
    info!("");

    // 6. 检查是否有异常的时间戳模式（同一秒内有大量数据）
    info!("=== 检查异常时间戳模式（同一秒内的数据量） ===");
    let one_minute_ago = now - chrono::Duration::minutes(1);
    let same_second_rows: Vec<tokio_postgres::Row> = client
        .query(
            r#"
            SELECT 
                DATE_TRUNC('second', snapshot_time)::timestamptz as second,
                COUNT(*) as cnt
            FROM market_snapshots
            WHERE snapshot_time >= $1
            GROUP BY DATE_TRUNC('second', snapshot_time)
            HAVING COUNT(*) > 100
            ORDER BY cnt DESC
            LIMIT 10
        "#,
            &[&one_minute_ago],
        )
        .await?;

    if same_second_rows.is_empty() {
        info!("  没有发现异常的时间戳模式（同一秒内数据量正常）");
    } else {
        warn!("  发现异常的时间戳模式（同一秒内有大量数据）：");
        for row in same_second_rows {
            let second: DateTime<Utc> = row.get("second");
            let count: i64 = row.get("cnt");
            warn!("    {}: {} 条（可能有多进程写入或数据采集频率异常）", second.format("%H:%M:%S"), count);
        }
    }
    info!("");

    // 7. 检查数据的时间戳是否连续（是否有时间戳跳跃）
    info!("=== 检查时间戳连续性（最近1分钟） ===");
    let time_gaps: Vec<tokio_postgres::Row> = client
        .query(
            r#"
            WITH time_series AS (
                SELECT DISTINCT DATE_TRUNC('second', snapshot_time)::timestamptz as ts
                FROM market_snapshots
                WHERE snapshot_time >= $1
                ORDER BY ts
            ),
            gaps AS (
                SELECT 
                    ts,
                    LAG(ts) OVER (ORDER BY ts) as prev_ts,
                    EXTRACT(EPOCH FROM (ts - LAG(ts) OVER (ORDER BY ts))) as gap_seconds
                FROM time_series
            )
            SELECT ts, prev_ts, gap_seconds
            FROM gaps
            WHERE gap_seconds > 5
            ORDER BY ts DESC
            LIMIT 5
        "#,
            &[&one_minute_ago],
        )
        .await?;

    if time_gaps.is_empty() {
        info!("  时间戳连续（没有发现大于5秒的间隔）");
    } else {
        warn!("  发现时间戳间隔（可能有数据缺失）：");
        for row in time_gaps {
            let ts: DateTime<Utc> = row.get("ts");
            let prev_ts: Option<DateTime<Utc>> = row.get("prev_ts");
            let gap: Option<f64> = row.get("gap_seconds");
            if let (Some(prev), Some(gap_secs)) = (prev_ts, gap) {
                warn!("    {} ~ {}: 间隔 {} 秒", prev.format("%H:%M:%S"), ts.format("%H:%M:%S"), gap_secs);
            }
        }
    }
    info!("");

    info!("=== 检查完成 ===");
    Ok(())
}
