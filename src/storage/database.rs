use anyhow::Result;
use deadpool_postgres::{Config, Pool};
use deadpool_postgres::tokio_postgres::NoTls;

/// 数据库连接池
pub struct Database {
    pool: Pool,
}

impl Database {
    /// 创建数据库连接池
    pub async fn new(database_url: &str) -> Result<Self> {
        // 解析数据库 URL 为 tokio_postgres::Config
        let pg_config = database_url.parse::<deadpool_postgres::tokio_postgres::Config>()
            .map_err(|e| anyhow::anyhow!("Failed to parse database URL: {}", e))?;
        
        // 创建 deadpool_postgres::Config
        let mut cfg = Config::default();
        cfg.host = pg_config.get_hosts().first().and_then(|h| match h {
            deadpool_postgres::tokio_postgres::config::Host::Tcp(host) => Some(host.clone()),
            _ => None,
        });
        cfg.port = pg_config.get_ports().first().copied();
        cfg.user = pg_config.get_user().map(|s| s.to_string());
        cfg.password = pg_config.get_password().and_then(|p| std::str::from_utf8(p).ok().map(|s| s.to_string()));
        cfg.dbname = pg_config.get_dbname().map(|s| s.to_string());
        
        let pool = cfg
            .builder(NoTls)
            .map_err(|e| anyhow::anyhow!("Failed to create pool builder: {}", e))?
            .max_size(20)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create pool: {}", e))?;

        Ok(Self { pool })
    }

    /// 获取连接池
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// 初始化数据库表
    pub async fn init_schema(&self) -> Result<()> {
        let client = self.pool.get().await?;

        // 创建交易对表
        client
            .execute(
                r#"
                CREATE TABLE IF NOT EXISTS trading_pairs (
                    id SERIAL PRIMARY KEY,
                    symbol VARCHAR(50) NOT NULL,
                    base VARCHAR(20) NOT NULL,
                    quote VARCHAR(20) NOT NULL,
                    exchange VARCHAR(20) NOT NULL,
                    is_spot BOOLEAN NOT NULL,
                    is_futures BOOLEAN NOT NULL,
                    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
                    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
                    UNIQUE(symbol, exchange, is_spot)
                )
            "#,
                &[],
            )
            .await?;

        // 创建市场快照表（统一价格和资金费率数据）
        // 使用 TimescaleDB 超表优化时序数据查询
        // 注意：TimescaleDB 超表转换和压缩策略在 migrations/08_create_market_snapshots.sql 中配置
        client
            .execute(
                r#"
                CREATE TABLE IF NOT EXISTS market_snapshots (
                    id BIGSERIAL PRIMARY KEY,
                    snapshot_time TIMESTAMP WITH TIME ZONE NOT NULL,
                    exchange VARCHAR(20) NOT NULL,
                    symbol VARCHAR(50) NOT NULL,
                    market_type VARCHAR(10) NOT NULL,
                    
                    -- 价格数据
                    bid_price NUMERIC(20, 8),
                    ask_price NUMERIC(20, 8),
                    last_price NUMERIC(20, 8),
                    volume_24h NUMERIC(30, 8),
                    
                    -- 资金费率数据
                    funding_rate NUMERIC(20, 8),
                    next_funding_time TIMESTAMP WITH TIME ZONE,
                    funding_interval_hours INTEGER,
                    rate_limit_upper NUMERIC(20, 8),
                    rate_limit_lower NUMERIC(20, 8),
                    
                    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
                    
                    UNIQUE(snapshot_time, exchange, symbol, market_type)
                )
            "#,
                &[],
            )
            .await?;

        // 创建索引以优化查询性能
        // 注意：TimescaleDB 超表的索引在 migrations/08_create_market_snapshots.sql 中创建
        client
            .execute(
                r#"
                CREATE INDEX IF NOT EXISTS idx_snapshots_symbol_time 
                ON market_snapshots(symbol, snapshot_time DESC)
            "#,
                &[],
            )
            .await?;

        client
            .execute(
                r#"
                CREATE INDEX IF NOT EXISTS idx_snapshots_exchange_symbol_time 
                ON market_snapshots(exchange, symbol, snapshot_time DESC)
            "#,
                &[],
            )
            .await?;

        // 创建同步时间记录表（用于记录各服务的上次同步时间）
        client
            .execute(
                r#"
                CREATE TABLE IF NOT EXISTS sync_times (
                    service_name VARCHAR(50) PRIMARY KEY,
                    last_sync_time TIMESTAMP WITH TIME ZONE NOT NULL,
                    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
                )
            "#,
                &[],
            )
            .await?;

        Ok(())
    }
}