use anyhow::Result;
use clap::{Parser, ValueEnum};
use crypto_hunter::{
    config::AppConfig,
    models::exchange::ExchangeType,
    storage::database::Database,
};
use chrono::{DateTime, Utc};
use std::io::{self, Write};
use tracing::{info, warn};

/// 数据清理工具
/// 用于清理数据库中的历史采集数据
#[derive(Parser, Debug)]
#[command(name = "clean-data")]
#[command(about = "清理数据库中的历史采集数据", long_about = None)]
struct Args {
    /// 清理模式
    #[arg(long, value_enum, default_value = "all")]
    mode: CleanMode,

    /// 开始时间，ISO 8601格式（可选，默认：不限制）
    #[arg(long)]
    start_time: Option<String>,

    /// 结束时间，ISO 8601格式（可选，默认：不限制）
    #[arg(long)]
    end_time: Option<String>,

    /// 交易所（可选，默认：全部）
    #[arg(long)]
    exchange: Option<String>,

    /// 交易对（可选，默认：全部）
    #[arg(long)]
    symbol: Option<String>,

    /// 跳过确认提示（危险操作）
    #[arg(long, short = 'y')]
    yes: bool,

    /// 仅显示统计信息，不执行删除
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Clone, ValueEnum)]
enum CleanMode {
    /// 清理所有快照数据
    All,
    /// 清理指定时间范围之前的数据
    Before,
    /// 清理指定时间范围之后的数据
    After,
    /// 清理指定时间范围的数据
    Range,
    /// 清理指定交易所的数据
    Exchange,
    /// 清理指定交易对的数据
    Symbol,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    info!("数据清理工具启动...");

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        warn!("无法从环境变量加载配置，使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    info!("初始化数据库连接...");
    let database = Database::new(&app_config.database.url).await?;

    // 解析时间参数
    let start_time = if let Some(start_str) = args.start_time {
        Some(
            DateTime::parse_from_rfc3339(&start_str)
                .map_err(|e| anyhow::anyhow!("无法解析开始时间: {}: {}", start_str, e))?
                .with_timezone(&Utc),
        )
    } else {
        None
    };

    let end_time = if let Some(end_str) = args.end_time {
        Some(
            DateTime::parse_from_rfc3339(&end_str)
                .map_err(|e| anyhow::anyhow!("无法解析结束时间: {}: {}", end_str, e))?
                .with_timezone(&Utc),
        )
    } else {
        None
    };

    // 解析交易所
    let exchange_type = if let Some(exchange_str) = args.exchange {
        Some(ExchangeType::from(exchange_str.as_str()))
    } else {
        None
    };

    // 构建清理条件
    let sql_condition = build_clean_condition(
        &args.mode,
        start_time,
        end_time,
        exchange_type,
        args.symbol.as_deref(),
    )?;

    // 先查询统计信息
    info!("查询待清理数据统计...");
    let count = count_snapshots(&database, &sql_condition).await?;
    
    info!("待清理数据统计:");
    info!("  - 快照数量: {}", count);

    if count == 0 {
        info!("没有需要清理的数据");
        return Ok(());
    }

    // 如果不是 dry-run，需要确认
    if !args.dry_run {
        if !args.yes {
            print!("\n⚠️  警告：此操作将删除 {} 条快照数据，是否继续？(yes/no): ", count);
            io::stdout().flush()?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            
            if input.trim().to_lowercase() != "yes" {
                info!("操作已取消");
                return Ok(());
            }
        }

        // 执行清理
        info!("开始清理数据...");
        let deleted = delete_snapshots(&database, &sql_condition).await?;
        info!("成功删除 {} 条快照数据", deleted);
    } else {
        info!("[DRY RUN] 如果执行，将删除 {} 条快照数据", count);
    }

    info!("数据清理完成");
    Ok(())
}

/// 构建清理条件（返回SQL条件）
fn build_clean_condition(
    mode: &CleanMode,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    exchange: Option<ExchangeType>,
    symbol: Option<&str>,
) -> Result<String> {
    let mut conditions = Vec::new();

    match mode {
        CleanMode::All => {
            // 清理所有数据
            return Ok("1=1".to_string());
        }
        CleanMode::Before => {
            if let Some(end) = end_time {
                // 转义单引号，防止SQL注入
                let end_str = end.to_rfc3339().replace('\'', "''");
                conditions.push(format!("snapshot_time < '{}'", end_str));
            } else {
                return Err(anyhow::anyhow!("Before 模式需要指定 end_time"));
            }
        }
        CleanMode::After => {
            if let Some(start) = start_time {
                // 转义单引号，防止SQL注入
                let start_str = start.to_rfc3339().replace('\'', "''");
                conditions.push(format!("snapshot_time > '{}'", start_str));
            } else {
                return Err(anyhow::anyhow!("After 模式需要指定 start_time"));
            }
        }
        CleanMode::Range => {
            if let Some(start) = start_time {
                // 转义单引号，防止SQL注入
                let start_str = start.to_rfc3339().replace('\'', "''");
                conditions.push(format!("snapshot_time >= '{}'", start_str));
            }
            if let Some(end) = end_time {
                // 转义单引号，防止SQL注入
                let end_str = end.to_rfc3339().replace('\'', "''");
                conditions.push(format!("snapshot_time <= '{}'", end_str));
            }
            if conditions.is_empty() {
                return Err(anyhow::anyhow!("Range 模式需要指定 start_time 或 end_time"));
            }
        }
        CleanMode::Exchange => {
            if let Some(ex) = exchange {
                // 转义单引号，防止SQL注入
                let ex_str = ex.to_string().replace('\'', "''");
                conditions.push(format!("exchange = '{}'", ex_str));
            } else {
                return Err(anyhow::anyhow!("Exchange 模式需要指定 exchange"));
            }
        }
        CleanMode::Symbol => {
            if let Some(sym) = symbol {
                // 转义单引号，防止SQL注入
                let sym_str = sym.replace('\'', "''");
                conditions.push(format!("symbol = '{}'", sym_str));
            } else {
                return Err(anyhow::anyhow!("Symbol 模式需要指定 symbol"));
            }
        }
    }

    // 添加额外的过滤条件
    if let Some(ex) = exchange {
        if !matches!(mode, CleanMode::Exchange) {
            // 转义单引号，防止SQL注入
            let ex_str = ex.to_string().replace('\'', "''");
            conditions.push(format!("exchange = '{}'", ex_str));
        }
    }

    if let Some(sym) = symbol {
        if !matches!(mode, CleanMode::Symbol) {
            // 转义单引号，防止SQL注入
            let sym_str = sym.replace('\'', "''");
            conditions.push(format!("symbol = '{}'", sym_str));
        }
    }

    let condition = if conditions.is_empty() {
        "1=1".to_string()
    } else {
        conditions.join(" AND ")
    };

    Ok(condition)
}

/// 统计符合条件的快照数量
async fn count_snapshots(database: &Database, condition: &str) -> Result<i64> {
    let client = database.pool().get().await?;
    
    // 注意：这里使用字符串拼接，实际应该使用参数化查询
    // 但为了简化，这里假设 condition 已经经过验证
    let query = format!("SELECT COUNT(*) FROM market_snapshots WHERE {}", condition);
    let row = client.query_one(&*query, &[]).await?;
    let count: i64 = row.get(0);
    
    Ok(count)
}

/// 删除符合条件的快照
async fn delete_snapshots(database: &Database, condition: &str) -> Result<u64> {
    let client = database.pool().get().await?;
    
    // 注意：这里使用字符串拼接，实际应该使用参数化查询
    // 但为了简化，这里假设 condition 已经经过验证
    let query = format!("DELETE FROM market_snapshots WHERE {}", condition);
    let result = client.execute(&*query, &[]).await?;
    
    Ok(result)
}

