use anyhow::Result;
use clap::Parser;
use crypto_hunter::exchange::{
    BinanceAdapter, BitgetAdapter, BybitAdapter, ExchangeManager, GateioAdapter, OkxAdapter,
};
use crypto_hunter::models::exchange::{normalize_symbol_for_exchange, ExchangeType, MarketType};
use std::sync::Arc;
use tracing::{info, warn};

/// 用于验证“各交易所 symbol 参数格式”的小工具：
/// - 输入任意格式的 symbol（BTCUSDT / BTC_USDT / BTC-USDT-SWAP 等）
/// - 打印各交易所(现货/期货)的转换结果
/// - 直接调用各交易所对应 API（通过 Adapter），用成功/失败来验证格式是否正确
#[derive(Parser, Debug)]
#[command(name = "symbol-tester")]
struct Args {
    /// 输入 symbol（支持 BTCUSDT / BTC_USDT / BTC-USDT / BTC-USDT-SWAP 等）
    #[arg(short, long)]
    symbol: String,

    /// 市场类型：spot / futures（默认 futures）
    #[arg(long, default_value = "futures")]
    market: String,

    /// 仅测试指定交易所（可重复）：binance/okx/bybit/gateio/bitget
    #[arg(long)]
    exchange: Vec<String>,
}

fn parse_market_type(s: &str) -> MarketType {
    match s.to_lowercase().as_str() {
        "spot" => MarketType::Spot,
        _ => MarketType::Futures,
    }
}

fn parse_exchange(s: &str) -> Option<ExchangeType> {
    match s.to_lowercase().as_str() {
        "binance" => Some(ExchangeType::Binance),
        "okx" => Some(ExchangeType::Okx),
        "bybit" => Some(ExchangeType::Bybit),
        "gateio" => Some(ExchangeType::Gateio),
        "bitget" => Some(ExchangeType::Bitget),
        _ => None,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    dotenv::dotenv().ok();
    let args = Args::parse();
    let market_type = parse_market_type(&args.market);

    // 选择要测试的交易所
    let mut selected: Vec<ExchangeType> = Vec::new();
    if args.exchange.is_empty() {
        selected = vec![
            ExchangeType::Binance,
            ExchangeType::Okx,
            ExchangeType::Bybit,
            ExchangeType::Gateio,
            ExchangeType::Bitget,
        ];
    } else {
        for ex in &args.exchange {
            match parse_exchange(ex) {
                Some(et) => selected.push(et),
                None => warn!("未知交易所参数: {}", ex),
            }
        }
    }

    // 简单去重（不依赖 ExchangeType: Ord/Hash）
    let mut uniq: Vec<ExchangeType> = Vec::new();
    for et in selected {
        if !uniq.iter().any(|x| *x == et) {
            uniq.push(et);
        }
    }
    let selected = uniq;

    // 初始化 ExchangeManager（仅用于拿到统一的 Adapter 调用方式）
    let mut manager = ExchangeManager::new();
    manager.add_adapter(Box::new(BinanceAdapter::new()));
    manager.add_adapter(Box::new(OkxAdapter::new()));
    manager.add_adapter(Box::new(BybitAdapter::new()));
    manager.add_adapter(Box::new(GateioAdapter::new()));
    manager.add_adapter(Box::new(BitgetAdapter::new()));
    let manager = Arc::new(manager);

    info!("输入 symbol: {}", args.symbol);
    info!("目标市场: {:?}", market_type);
    info!("====================================");

    // 逐个交易所测试
    for exchange_type in selected {
        let normalized = normalize_symbol_for_exchange(&args.symbol, exchange_type, market_type);
        info!(
            "[{}] normalized({:?}) = {} (raw = {})",
            exchange_type, market_type, normalized, args.symbol
        );

        let adapter = manager
            .get_adapter(exchange_type)
            .expect("Adapter should exist");

        // 用实际 API 调用验证（通过现有 Adapter），输出成功/失败
        let res = match market_type {
            MarketType::Spot => adapter.fetch_spot_price(&normalized).await.map(|p| p.last_price),
            MarketType::Futures => adapter
                .fetch_futures_price(&normalized)
                .await
                .map(|p| p.last_price),
        };

        match res {
            Ok(last_price) => {
                info!(
                    "[{}] OK: last_price={} (param={})",
                    exchange_type, last_price, normalized
                );
            }
            Err(e) => {
                warn!(
                    "[{}] FAIL: param={} err={}",
                    exchange_type, normalized, e
                );
            }
        }

        // 期货额外测一下资金费率（更能验证 OKX / Gateio 等期货 symbol）
        if market_type == MarketType::Futures {
            match adapter.fetch_funding_rate(&normalized).await {
                Ok(fr) => {
                    info!(
                        "[{}] FUNDING OK: rate={} next={} (param={})",
                        exchange_type, fr.rate, fr.next_funding_time, normalized
                    );
                }
                Err(e) => {
                    warn!(
                        "[{}] FUNDING FAIL: param={} err={}",
                        exchange_type, normalized, e
                    );
                }
            }
        }

        info!("------------------------------------");
    }

    Ok(())
}

