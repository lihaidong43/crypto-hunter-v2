-- 创建 market_snapshots 表（统一快照表）
-- 替代 price_data 和 funding_rates 分离存储
-- 使用 TimescaleDB 超表优化时序数据查询

-- ============================================
-- 1. 创建基础表结构
-- ============================================

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
);

-- ============================================
-- 2. 转换为 TimescaleDB 超表
-- ============================================

-- 前提：已安装 TimescaleDB 扩展
-- CREATE EXTENSION IF NOT EXISTS timescaledb;

SELECT create_hypertable('market_snapshots', 'snapshot_time', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- ============================================
-- 3. 创建索引（优化查询性能）
-- ============================================

-- 按 symbol 和时间查询（最常用）
CREATE INDEX IF NOT EXISTS idx_snapshots_symbol_time 
ON market_snapshots(symbol, snapshot_time DESC);

-- 按交易所、symbol 和时间查询
CREATE INDEX IF NOT EXISTS idx_snapshots_exchange_symbol_time 
ON market_snapshots(exchange, symbol, snapshot_time DESC);

-- 按时间查询（用于时间范围查询）
CREATE INDEX IF NOT EXISTS idx_snapshots_time 
ON market_snapshots(snapshot_time DESC);

-- ============================================
-- 4. 配置压缩策略（7天后的数据自动压缩）
-- ============================================

-- 启用压缩
ALTER TABLE market_snapshots SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'exchange, symbol',
    timescaledb.compress_orderby = 'snapshot_time DESC'
);

-- 添加压缩策略（7天后的数据自动压缩）
SELECT add_compression_policy('market_snapshots', INTERVAL '7 days', if_not_exists => true);

-- ============================================
-- 完成
-- ============================================

SELECT 'market_snapshots 表创建完成，已转换为 TimescaleDB 超表' AS message;
