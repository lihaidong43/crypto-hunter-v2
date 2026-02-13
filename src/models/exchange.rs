use serde::{Deserialize, Serialize};
use std::fmt;

/// 交易所类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExchangeType {
    Binance,
    Okx,
    Bybit,
    Gateio,
    Bitget,
    // 预留扩展
    #[serde(other)]
    Unknown,
}

impl fmt::Display for ExchangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExchangeType::Binance => write!(f, "binance"),
            ExchangeType::Okx => write!(f, "okx"),
            ExchangeType::Bybit => write!(f, "bybit"),
            ExchangeType::Gateio => write!(f, "gateio"),
            ExchangeType::Bitget => write!(f, "bitget"),
            ExchangeType::Unknown => write!(f, "unknown"),
        }
    }
}

impl From<&str> for ExchangeType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "binance" => ExchangeType::Binance,
            "okx" | "okex" => ExchangeType::Okx,
            "bybit" => ExchangeType::Bybit,
            "gateio" | "gate.io" => ExchangeType::Gateio,
            "bitget" => ExchangeType::Bitget,
            _ => ExchangeType::Unknown,
        }
    }
}

/// 套利类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArbitrageType {
    /// 现货-期货
    SpotFutures,
    /// 期货-期货
    FuturesFutures,
    /// 期货-现货
    FuturesSpot,
}

impl fmt::Display for ArbitrageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArbitrageType::SpotFutures => write!(f, "spot-futures"),
            ArbitrageType::FuturesFutures => write!(f, "futures-futures"),
            ArbitrageType::FuturesSpot => write!(f, "futures-spot"),
        }
    }
}

/// 交易对信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingPair {
    pub symbol: String,
    pub base: String,
    pub quote: String,
    pub exchange: ExchangeType,
    pub is_spot: bool,
    pub is_futures: bool,
}

/// 市场类型（现货/期货）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketType {
    Spot,
    Futures,
}

impl fmt::Display for MarketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketType::Spot => write!(f, "spot"),
            MarketType::Futures => write!(f, "futures"),
        }
    }
}

impl From<&str> for MarketType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "spot" => MarketType::Spot,
            "futures" => MarketType::Futures,
            _ => MarketType::Futures, // 默认为期货
        }
    }
}

/// 从任意格式的 symbol 中提取 base 和 quote
/// 支持格式：
/// - OKX: BTC-USDT-SWAP, BTC-USDT
/// - Gate.io: BTC_USDT
/// - Binance/Bybit/Bitget: BTCUSDT
fn extract_base_quote(symbol: &str) -> Option<(String, String)> {
    // 先尝试 OKX 格式 (包含 -)
    if symbol.contains('-') {
        let parts: Vec<&str> = symbol.split('-').collect();
        if parts.len() >= 2 {
            // 忽略 SWAP 后缀
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    
    // 再尝试 Gate.io 格式 (包含 _)
    if symbol.contains('_') {
        let parts: Vec<&str> = symbol.split('_').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    
    // 最后尝试 Binance/Bybit/Bitget 格式 (无分隔符)
    // 查找 USDT, USDC, BUSD 等常见 quote
    let quotes = ["USDT", "USDC", "BUSD", "BTC", "ETH"];
    for quote in &quotes {
        if let Some(quote_pos) = symbol.rfind(quote) {
            let base = &symbol[..quote_pos];
            if !base.is_empty() {
                return Some((base.to_string(), quote.to_string()));
            }
        }
    }
    
    None
}

/// Symbol 格式转换工具
/// 不同交易所使用不同的 symbol 格式，需要转换
/// 支持从任意格式（OKX、Gate.io、Binance/Bybit/Bitget）转换为目标格式
pub fn normalize_symbol_for_exchange(symbol: &str, exchange: ExchangeType, market_type: MarketType) -> String {
    // 先提取 base 和 quote
    let (base, quote) = match extract_base_quote(symbol) {
        Some((b, q)) => (b, q),
        None => {
            // 如果无法解析，返回原 symbol
            tracing::warn!("无法解析 symbol 格式: {}", symbol);
            return symbol.to_string();
        }
    };
    
    // 根据目标交易所格式转换
    match exchange {
        ExchangeType::Okx => {
            // OKX 期货格式: BASE-QUOTE-SWAP (如 BTC-USDT-SWAP)
            // OKX 现货格式: BASE-QUOTE (如 BTC-USDT)
            match market_type {
                MarketType::Futures => format!("{}-{}-SWAP", base, quote),
                MarketType::Spot => format!("{}-{}", base, quote),
            }
        }
        ExchangeType::Binance | ExchangeType::Bybit | ExchangeType::Bitget => {
            // Binance/Bybit/Bitget 格式: BASEQUOTE (如 BTCUSDT)
            format!("{}{}", base, quote)
        }
        ExchangeType::Gateio => {
            // Gate.io 格式: BASE_QUOTE (如 BTC_USDT)
            format!("{}_{}", base, quote)
        }
        _ => symbol.to_string(),
    }
}