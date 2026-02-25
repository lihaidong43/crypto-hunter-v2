use serde::{Deserialize, Serialize};
use std::time::Duration;
use rust_decimal::Decimal;

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub exchanges: Vec<ExchangeConfig>,
    pub data_collection: DataCollectionConfig,
    pub monitoring: MonitoringConfig,
    pub notification: NotificationConfig,
    pub api: ApiConfig,
    pub pair_filter: PairFilterConfig,
    pub websocket: WebSocketConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeConfig {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionConfig {
    /// 交易对同步间隔（秒）
    pub pair_sync_interval_seconds: u64,
    /// 价格同步间隔（秒）
    pub price_sync_interval_seconds: u64,
    /// 资金费率同步间隔（秒）
    pub funding_rate_sync_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// 价差阈值（百分比）
    pub spread_threshold: f64,
    /// 基差波动阈值（百分比）
    pub basis_volatility_threshold: f64,
    /// 资金费率波动阈值（百分比）
    pub funding_rate_volatility_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub port: u16,
}

/// 交易对过滤配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairFilterConfig {
    /// 至少需要的交易所数量（默认 3，套利需要多交易所）
    pub min_exchange_count: usize,
    /// 允许的计价货币列表（默认 ["USDT"]）
    pub allowed_quote_currencies: Vec<String>,
    /// 流动性阈值（可选，用于第二阶段过滤）
    pub min_volume_24h: Option<Decimal>,
    /// 是否只收集期货（默认 false）
    #[serde(default)]
    pub futures_only: bool,
    /// 是否只收集现货（默认 false）
    #[serde(default)]
    pub spot_only: bool,
}

/// WebSocket配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// 是否启用WebSocket采集
    pub enabled: bool,
    /// 重连延迟（秒）
    pub reconnect_delay_seconds: u64,
    /// 心跳间隔（秒）
    pub heartbeat_interval_seconds: u64,
    /// 最大重连次数（0表示无限重试）
    pub max_reconnect_attempts: u32,
    /// 连接超时（秒）
    pub connect_timeout_seconds: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig {
                // 优先从环境变量读取，否则使用默认值（包含密码）
                url: std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/crypto_hunter".to_string()),
            },
            exchanges: vec![
                ExchangeConfig {
                    name: "binance".to_string(),
                    enabled: true,
                },
                ExchangeConfig {
                    name: "okx".to_string(),
                    enabled: true,
                },
                ExchangeConfig {
                    name: "bybit".to_string(),
                    enabled: true,
                },
                ExchangeConfig {
                    name: "gateio".to_string(),
                    enabled: true,
                },
                ExchangeConfig {
                    name: "bitget".to_string(),
                    enabled: true,
                },
            ],
            data_collection: DataCollectionConfig {
                pair_sync_interval_seconds: 3600,    // 1小时同步一次交易对
                price_sync_interval_seconds: 5,      // 5秒同步一次价格
                funding_rate_sync_interval_seconds: 300, // 5分钟同步一次资金费率
            },
            monitoring: MonitoringConfig {
                spread_threshold: 1.0,           // 1%价差阈值
                basis_volatility_threshold: 0.5, // 0.5%基差波动阈值
                funding_rate_volatility_threshold: 0.1, // 0.1%资金费率波动阈值
            },
            notification: NotificationConfig {
                // 从常用环境变量名读取 Telegram 配置
                telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN").ok(),
                telegram_chat_id: std::env::var("TELEGRAM_CHAT_ID").ok(),
            },
            api: ApiConfig { port: 8080 },
            pair_filter: PairFilterConfig {
                min_exchange_count: 2,  // 至少2个交易所才有套利价值
                allowed_quote_currencies: vec![
                    "USDT".to_string(),  // 只保留 USDT 计价，流动性最好
                ],
                min_volume_24h: None,
                futures_only: false,  // 同时收集期货和现货
                spot_only: false,
            },
            websocket: WebSocketConfig {
                enabled: true, // 默认启用 WebSocket
                reconnect_delay_seconds: 5,
                heartbeat_interval_seconds: 30,
                max_reconnect_attempts: 0, // 无限重试
                connect_timeout_seconds: 10, // 连接超时 10 秒
            },
        }
    }
}

impl AppConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Result<Self, config::ConfigError> {
        let mut builder = config::Config::builder();

        // 从环境变量读取
        builder = builder.add_source(config::Environment::default().separator("__"));

        let config = builder.build()?;
        config.try_deserialize()
    }

    /// 从文件加载配置
    pub fn from_file(path: &str) -> Result<Self, config::ConfigError> {
        let mut builder = config::Config::builder();

        // 尝试从文件加载
        if std::path::Path::new(path).exists() {
            builder = builder.add_source(config::File::with_name(path));
        }

        // 环境变量可以覆盖文件配置
        builder = builder.add_source(config::Environment::default().separator("__"));

        let config = builder.build()?;
        config.try_deserialize()
    }
}