// 使用 lib.rs 中的模块
use crypto_hunter::arbitrage::analyzer::ArbitrageAnalyzer;
use crypto_hunter::config::AppConfig;
use crypto_hunter::models::arbitrage::ArbitrageFilters;
use crypto_hunter::models::exchange::ExchangeType;
use crypto_hunter::storage::database::Database;
use crypto_hunter::storage::repository::Repository;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // 加载配置
    dotenv::dotenv().ok();
    let app_config = AppConfig::from_env().unwrap_or_else(|_| {
        tracing::info!("使用默认配置");
        AppConfig::default()
    });

    // 初始化数据库
    let database = Database::new(&app_config.database.url).await?;
    let repository = Arc::new(Repository::new(&database));
    let analyzer = Arc::new(ArbitrageAnalyzer::new(repository.clone()));

    // 解析命令行参数
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "analyze" => {
            if args.len() < 3 {
                eprintln!("错误: 需要指定交易对符号");
                print_usage();
                return Ok(());
            }
            let symbol = &args[2];
            analyze_symbol(&analyzer, symbol).await?;
        }
        "find" => {
            find_opportunities(&analyzer, &args[2..]).await?;
        }
        "stats" => {
            get_stats(&repository).await?;
        }
        _ => {
            eprintln!("错误: 未知命令 {}", args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("用法:");
    println!("  query_arbitrage analyze <symbol>          - 分析指定交易对的套利机会");
    println!("  query_arbitrage find [options]            - 查找符合条件的套利机会");
    println!("  query_arbitrage stats                     - 获取快照统计信息");
    println!();
    println!("查找选项:");
    println!("  --symbol <symbol>                        - 交易对符号");
    println!("  --exchange-a <exchange>                   - 交易所 A");
    println!("  --exchange-b <exchange>                   - 交易所 B");
    println!("  --min-spread <value>                      - 最小价差（百分比）");
    println!("  --max-spread <value>                      - 最大价差（百分比）");
    println!("  --min-net-funding-rate <value>            - 最小净资金费率");
    println!("  --max-net-funding-rate <value>            - 最大净资金费率");
    println!("  --min-volume-24h <value>                  - 最小24小时交易量");
}

async fn analyze_symbol(analyzer: &ArbitrageAnalyzer, symbol: &str) -> Result<()> {
    println!("分析 {} 的套利机会...", symbol);
    
    let analyses = analyzer.analyze_symbol(symbol, None).await?;
    
    if analyses.is_empty() {
        println!("未找到套利机会");
        return Ok(());
    }

    println!("\n找到 {} 个套利机会:\n", analyses.len());
    for (i, analysis) in analyses.iter().enumerate() {
        println!("机会 #{}:", i + 1);
        println!("  交易所: {} <-> {}", analysis.exchange_a, analysis.exchange_b);
        println!("  价格: {} <-> {}", analysis.price_a, analysis.price_b);
        println!("  开仓价差: {:.4}%", analysis.open_spread);
        println!("  清仓价差: {:.4}%", analysis.close_spread);
        if let Some(net_rate) = analysis.net_funding_rate {
            println!("  净资金费率: {:.8}", net_rate);
        }
        if let Some(vol_a) = analysis.volume_24h_a {
            println!("  24h交易量 A: {}", vol_a);
        }
        if let Some(vol_b) = analysis.volume_24h_b {
            println!("  24h交易量 B: {}", vol_b);
        }
        println!();
    }

    Ok(())
}

async fn find_opportunities(analyzer: &ArbitrageAnalyzer, args: &[String]) -> Result<()> {
    let mut symbol = None;
    let mut exchange_a = None;
    let mut exchange_b = None;
    let mut min_spread = None;
    let mut max_spread = None;
    let mut min_net_funding_rate = None;
    let mut max_net_funding_rate = None;
    let mut min_volume_24h = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--symbol" => {
                if i + 1 < args.len() {
                    symbol = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("错误: --symbol 需要参数");
                    return Ok(());
                }
            }
            "--exchange-a" => {
                if i + 1 < args.len() {
                    exchange_a = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("错误: --exchange-a 需要参数");
                    return Ok(());
                }
            }
            "--exchange-b" => {
                if i + 1 < args.len() {
                    exchange_b = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("错误: --exchange-b 需要参数");
                    return Ok(());
                }
            }
            "--min-spread" => {
                if i + 1 < args.len() {
                    min_spread = Some(f64::from_str(&args[i + 1])?);
                    i += 2;
                } else {
                    eprintln!("错误: --min-spread 需要参数");
                    return Ok(());
                }
            }
            "--max-spread" => {
                if i + 1 < args.len() {
                    max_spread = Some(f64::from_str(&args[i + 1])?);
                    i += 2;
                } else {
                    eprintln!("错误: --max-spread 需要参数");
                    return Ok(());
                }
            }
            "--min-net-funding-rate" => {
                if i + 1 < args.len() {
                    min_net_funding_rate = Some(f64::from_str(&args[i + 1])?);
                    i += 2;
                } else {
                    eprintln!("错误: --min-net-funding-rate 需要参数");
                    return Ok(());
                }
            }
            "--max-net-funding-rate" => {
                if i + 1 < args.len() {
                    max_net_funding_rate = Some(f64::from_str(&args[i + 1])?);
                    i += 2;
                } else {
                    eprintln!("错误: --max-net-funding-rate 需要参数");
                    return Ok(());
                }
            }
            "--min-volume-24h" => {
                if i + 1 < args.len() {
                    min_volume_24h = Some(f64::from_str(&args[i + 1])?);
                    i += 2;
                } else {
                    eprintln!("错误: --min-volume-24h 需要参数");
                    return Ok(());
                }
            }
            _ => {
                eprintln!("错误: 未知选项 {}", args[i]);
                return Ok(());
            }
        }
    }

    let filters = ArbitrageFilters {
        symbol,
        exchange_a: exchange_a.map(|s| ExchangeType::from(s.as_str())),
        exchange_b: exchange_b.map(|s| ExchangeType::from(s.as_str())),
        min_open_spread: min_spread.map(|v| Decimal::try_from(v).unwrap_or_default()),
        max_open_spread: max_spread.map(|v| Decimal::try_from(v).unwrap_or_default()),
        min_net_funding_rate: min_net_funding_rate
            .map(|v| Decimal::try_from(v).unwrap_or_default()),
        max_net_funding_rate: max_net_funding_rate
            .map(|v| Decimal::try_from(v).unwrap_or_default()),
        min_volume_24h: min_volume_24h.map(|v| Decimal::try_from(v).unwrap_or_default()),
        snapshot_time: None,
    };

    println!("查找套利机会...");
    let analyses = analyzer.find_opportunities(filters).await?;

    if analyses.is_empty() {
        println!("未找到符合条件的套利机会");
        return Ok(());
    }

    println!("\n找到 {} 个套利机会:\n", analyses.len());
    for (i, analysis) in analyses.iter().enumerate() {
        println!("机会 #{}:", i + 1);
        println!("  交易对: {}", analysis.symbol);
        println!("  交易所: {} <-> {}", analysis.exchange_a, analysis.exchange_b);
        println!("  价格: {} <-> {}", analysis.price_a, analysis.price_b);
        println!("  开仓价差: {:.4}%", analysis.open_spread);
        println!("  清仓价差: {:.4}%", analysis.close_spread);
        if let Some(net_rate) = analysis.net_funding_rate {
            println!("  净资金费率: {:.8}", net_rate);
        }
        println!();
    }

    Ok(())
}

async fn get_stats(repository: &Repository) -> Result<()> {
    println!("获取快照统计信息...\n");
    
    let stats = repository.get_snapshot_stats(None, None, None).await?;
    
    println!("总快照数: {}", stats.total_snapshots);
    if let Some(earliest) = stats.earliest_time {
        println!("最早时间: {}", earliest);
    }
    if let Some(latest) = stats.latest_time {
        println!("最晚时间: {}", latest);
    }
    println!("交易所数量: {}", stats.exchange_count);
    println!("交易对数量: {}", stats.symbol_count);

    Ok(())
}
