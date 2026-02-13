use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use crypto_hunter::exchange::{
    BinanceWebSocketAdapter, BitgetWebSocketAdapter, BybitWebSocketAdapter, GateioWebSocketAdapter,
    OkxWebSocketAdapter, WebSocketAdapter,
};
use crypto_hunter::models::exchange::MarketType;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Debug, Clone, ValueEnum)]
enum ExchangeArg {
    Binance,
    Okx,
    Bybit,
    Gateio,
    Bitget,
}

#[derive(Debug, Parser)]
#[command(name = "ws-test", about = "单交易所 WebSocket 连接/订阅/推送测试（用于排查无推送问题）")]
struct Args {
    #[arg(long, value_enum)]
    exchange: ExchangeArg,

    /// spot, futures, 或 all（同时测试两者）
    #[arg(long, value_enum, default_value = "futures")]
    market: MarketTypeArg,

    /// 逗号分隔 symbols，例如 BTCUSDT,ETHUSDT（默认：BTCUSDT）
    #[arg(long, default_value = "BTCUSDT")]
    symbols: String,

    /// 运行秒数（统计收到多少条 ticker）
    #[arg(long, default_value_t = 30)]
    seconds: u64,

    /// 连接超时秒数
    #[arg(long, default_value_t = 20)]
    connect_timeout: u64,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum MarketTypeArg {
    Spot,
    Futures,
    All,
}

fn build_adapter(exchange: &ExchangeArg, market: MarketType) -> Box<dyn WebSocketAdapter> {
    match exchange {
        ExchangeArg::Binance => Box::new(BinanceWebSocketAdapter::new(market)),
        ExchangeArg::Okx => Box::new(OkxWebSocketAdapter::new(market)),
        ExchangeArg::Bybit => Box::new(BybitWebSocketAdapter::new(market)),
        ExchangeArg::Gateio => Box::new(GateioWebSocketAdapter::new(market)),
        ExchangeArg::Bitget => Box::new(BitgetWebSocketAdapter::new(market)),
    }
}

/// 单个市场的统计结果
#[derive(Default)]
struct MarketStats {
    ok: u64,
    err: u64,
    intervals: Vec<Duration>,
    symbol_counts: HashMap<String, u64>,
    per_second_counts: Vec<u64>,
}

async fn run_single_market(
    exchange: ExchangeArg,
    market: MarketType,
    symbols: Vec<String>,
    seconds: u64,
    connect_timeout: u64,
) -> Result<MarketStats> {
    let market_name = match market {
        MarketType::Spot => "spot",
        MarketType::Futures => "futures",
    };

    let mut adapter = build_adapter(&exchange, market);

    tracing::info!(
        "[{}] 开始连接...",
        market_name
    );

    timeout(Duration::from_secs(connect_timeout), adapter.connect())
        .await
        .context(format!("[{}] connect timeout", market_name))?
        .context(format!("[{}] connect failed", market_name))?;

    tracing::info!("[{}] 连接成功，开始订阅 {} 个交易对...", market_name, symbols.len());

    adapter
        .subscribe(&symbols, market)
        .await
        .context(format!("[{}] subscribe failed", market_name))?;

    tracing::info!("[{}] 订阅完成，开始接收 {} 秒...", market_name, seconds);

    let start_time = Instant::now();
    let end_at = start_time + Duration::from_secs(seconds);
    
    let mut stats = MarketStats::default();
    let mut last_msg_time: Option<Instant> = None;
    let mut current_second_start = start_time;
    let mut current_second_count = 0u64;

    while Instant::now() < end_at {
        let now = Instant::now();
        
        // 检查是否进入新的一秒
        if now.duration_since(current_second_start) >= Duration::from_secs(1) {
            stats.per_second_counts.push(current_second_count);
            current_second_count = 0;
            current_second_start = now;
        }
        
        match timeout(Duration::from_secs(5), adapter.receive_message()).await {
            Ok(Ok(snapshot)) => {
                stats.ok += 1;
                current_second_count += 1;
                
                // 记录消息间隔
                if let Some(last) = last_msg_time {
                    let interval = now.duration_since(last);
                    stats.intervals.push(interval);
                }
                last_msg_time = Some(now);
                
                // 统计每个 symbol 的消息数
                *stats.symbol_counts.entry(snapshot.symbol.clone()).or_insert(0) += 1;
                
                // 每20条消息输出一次统计
                if stats.ok % 20 == 0 {
                    let elapsed = now.duration_since(start_time);
                    let rate = stats.ok as f64 / elapsed.as_secs_f64();
                    tracing::info!(
                        "[{}] recv #{}: symbol={}, rate={:.2} msg/s",
                        market_name,
                        stats.ok,
                        snapshot.symbol,
                        rate
                    );
                } else {
                    tracing::debug!(
                        "[{}] recv #{}: symbol={}, bid={:?}, ask={:?}",
                        market_name,
                        stats.ok,
                        snapshot.symbol,
                        snapshot.bid_price,
                        snapshot.ask_price
                    );
                }
            }
            Ok(Err(e)) => {
                stats.err += 1;
                let es = e.to_string();
                tracing::warn!("[{}] receive error #{}: {}", market_name, stats.err, es);

                if es.contains("WebSocket stream ended")
                    || es.contains("connection closed")
                    || es.contains("ConnectionClosed")
                {
                    tracing::warn!("[{}] 连接已结束，提前退出", market_name);
                    break;
                }
            }
            Err(_) => {
                tracing::warn!("[{}] receive timeout (5s) - no message", market_name);
            }
        }
    }
    
    // 添加最后一秒的计数
    if current_second_count > 0 {
        stats.per_second_counts.push(current_second_count);
    }

    Ok(stats)
}

fn print_stats(market_name: &str, stats: &MarketStats, total_duration: Duration) {
    let total_rate = stats.ok as f64 / total_duration.as_secs_f64();
    
    tracing::info!("");
    tracing::info!("=== [{}] 消息推送频率统计 ===", market_name);
    tracing::info!("总消息数: {} 条", stats.ok);
    tracing::info!("错误数: {} 条", stats.err);
    tracing::info!("总时长: {:.2} 秒", total_duration.as_secs_f64());
    tracing::info!("平均频率: {:.2} 消息/秒", total_rate);
    
    if !stats.intervals.is_empty() {
        let min_interval = stats.intervals.iter().min().unwrap();
        let max_interval = stats.intervals.iter().max().unwrap();
        let avg_interval = stats.intervals.iter().sum::<Duration>() / stats.intervals.len() as u32;
        let median_interval = {
            let mut sorted = stats.intervals.clone();
            sorted.sort();
            sorted[sorted.len() / 2]
        };
        
        tracing::info!("");
        tracing::info!("消息间隔统计:");
        tracing::info!("  最小间隔: {:.2} ms", min_interval.as_secs_f64() * 1000.0);
        tracing::info!("  最大间隔: {:.2} ms", max_interval.as_secs_f64() * 1000.0);
        tracing::info!("  平均间隔: {:.2} ms", avg_interval.as_secs_f64() * 1000.0);
        tracing::info!("  中位间隔: {:.2} ms", median_interval.as_secs_f64() * 1000.0);
    }
    
    if !stats.per_second_counts.is_empty() {
        let min_per_sec = stats.per_second_counts.iter().min().unwrap();
        let max_per_sec = stats.per_second_counts.iter().max().unwrap();
        let avg_per_sec = stats.per_second_counts.iter().sum::<u64>() as f64 / stats.per_second_counts.len() as f64;
        
        tracing::info!("");
        tracing::info!("每秒消息数统计:");
        tracing::info!("  最小: {} 消息/秒", min_per_sec);
        tracing::info!("  最大: {} 消息/秒", max_per_sec);
        tracing::info!("  平均: {:.2} 消息/秒", avg_per_sec);
    }
    
    if !stats.symbol_counts.is_empty() {
        tracing::info!("");
        tracing::info!("按交易对统计:");
        let mut sorted_symbols: Vec<_> = stats.symbol_counts.iter().collect();
        sorted_symbols.sort_by_key(|(_, &count)| std::cmp::Reverse(count));
        for (symbol, count) in sorted_symbols.iter().take(10) {
            let rate = **count as f64 / total_duration.as_secs_f64();
            tracing::info!("  {}: {} 条 ({:.2} 消息/秒)", symbol, count, rate);
        }
        if sorted_symbols.len() > 10 {
            tracing::info!("  ... 还有 {} 个交易对", sorted_symbols.len() - 10);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let symbols: Vec<String> = args
        .symbols
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,crypto_hunter=debug".to_string()),
        )
        .init();

    let markets: Vec<MarketType> = match args.market {
        MarketTypeArg::Spot => vec![MarketType::Spot],
        MarketTypeArg::Futures => vec![MarketType::Futures],
        MarketTypeArg::All => vec![MarketType::Spot, MarketType::Futures],
    };

    tracing::info!(
        "ws-test: exchange={:?}, markets={:?}, symbols={:?}, seconds={}",
        args.exchange,
        markets,
        symbols,
        args.seconds
    );

    let start_time = Instant::now();

    if markets.len() == 1 {
        // 单市场模式
        let market = markets[0];
        let stats = run_single_market(
            args.exchange,
            market,
            symbols,
            args.seconds,
            args.connect_timeout,
        ).await?;
        
        let total_duration = Instant::now().duration_since(start_time);
        let market_name = match market {
            MarketType::Spot => "spot",
            MarketType::Futures => "futures",
        };
        print_stats(market_name, &stats, total_duration);
    } else {
        // 多市场模式：并行运行
        let spot_stats = Arc::new(Mutex::new(MarketStats::default()));
        let futures_stats = Arc::new(Mutex::new(MarketStats::default()));

        let exchange_spot = args.exchange.clone();
        let exchange_futures = args.exchange.clone();
        let symbols_spot = symbols.clone();
        let symbols_futures = symbols.clone();
        let seconds = args.seconds;
        let connect_timeout = args.connect_timeout;

        let spot_stats_clone = Arc::clone(&spot_stats);
        let futures_stats_clone = Arc::clone(&futures_stats);

        let spot_handle = tokio::spawn(async move {
            match run_single_market(
                exchange_spot,
                MarketType::Spot,
                symbols_spot,
                seconds,
                connect_timeout,
            ).await {
                Ok(stats) => {
                    *spot_stats_clone.lock().await = stats;
                }
                Err(e) => {
                    tracing::error!("[spot] 运行失败: {}", e);
                }
            }
        });

        let futures_handle = tokio::spawn(async move {
            match run_single_market(
                exchange_futures,
                MarketType::Futures,
                symbols_futures,
                seconds,
                connect_timeout,
            ).await {
                Ok(stats) => {
                    *futures_stats_clone.lock().await = stats;
                }
                Err(e) => {
                    tracing::error!("[futures] 运行失败: {}", e);
                }
            }
        });

        // 等待两个任务完成
        let _ = tokio::join!(spot_handle, futures_handle);

        let total_duration = Instant::now().duration_since(start_time);

        // 打印统计
        let spot = spot_stats.lock().await;
        let futures = futures_stats.lock().await;

        print_stats("spot", &spot, total_duration);
        print_stats("futures", &futures, total_duration);

        // 打印汇总
        tracing::info!("");
        tracing::info!("=== 汇总统计 ===");
        tracing::info!("Spot: {} 条消息, {:.2} msg/s", spot.ok, spot.ok as f64 / total_duration.as_secs_f64());
        tracing::info!("Futures: {} 条消息, {:.2} msg/s", futures.ok, futures.ok as f64 / total_duration.as_secs_f64());
        tracing::info!("总计: {} 条消息, {:.2} msg/s", 
            spot.ok + futures.ok,
            (spot.ok + futures.ok) as f64 / total_duration.as_secs_f64()
        );
    }

    tracing::info!("");
    tracing::info!("=== 统计完成 ===");
    Ok(())
}
