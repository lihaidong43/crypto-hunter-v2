-- TimescaleDB 完整初始化脚本
-- 先创建表，再转换为超表

-- ============================================
-- 1. 创建基础表结构
-- ============================================

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
);

CREATE TABLE IF NOT EXISTS price_data (
    id BIGSERIAL,
    exchange VARCHAR(20) NOT NULL,
    symbol VARCHAR(50) NOT NULL,
    market_type VARCHAR(10) NOT NULL,
    bid_price NUMERIC(20, 8) NOT NULL,
    ask_price NUMERIC(20, 8) NOT NULL,
    last_price NUMERIC(20, 8) NOT NULL,
    volume_24h NUMERIC(30, 8) NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS funding_rates (
    id BIGSERIAL,
    exchange VARCHAR(20) NOT NULL,
    symbol VARCHAR(50) NOT NULL,
    rate NUMERIC(20, 8) NOT NULL,
    next_funding_time TIMESTAMP WITH TIME ZONE NOT NULL,
    funding_interval_hours INTEGER NOT NULL,
    rate_limit_upper NUMERIC(20, 8),
    rate_limit_lower NUMERIC(20, 8),
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS arbitrage_opportunities (
    id VARCHAR(50) PRIMARY KEY,
    arbitrage_type VARCHAR(20) NOT NULL,
    symbol VARCHAR(50) NOT NULL,
    exchange_a VARCHAR(20) NOT NULL,
    exchange_b VARCHAR(20) NOT NULL,
    symbol_a VARCHAR(100) NOT NULL,
    symbol_b VARCHAR(100) NOT NULL,
    price_a NUMERIC(20, 8) NOT NULL,
    price_b NUMERIC(20, 8) NOT NULL,
    open_spread NUMERIC(10, 4) NOT NULL,
    close_spread NUMERIC(10, 4) NOT NULL,
    funding_rate_a NUMERIC(20, 8),
    funding_rate_b NUMERIC(20, 8),
    net_funding_rate NUMERIC(20, 8),
    volume_24h_a NUMERIC(30, 8) NOT NULL,
    volume_24h_b NUMERIC(30, 8) NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS volatility_events (
    id VARCHAR(50) PRIMARY KEY,
    symbol VARCHAR(50) NOT NULL,
    exchange VARCHAR(20) NOT NULL,
    event_type VARCHAR(20) NOT NULL,
    value NUMERIC(20, 8) NOT NULL,
    previous_value NUMERIC(20, 8) NOT NULL,
    change_percentage NUMERIC(10, 4) NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- ============================================
-- 2. 转换为超表
-- ============================================

SELECT create_hypertable('price_data', 'timestamp', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

SELECT create_hypertable('funding_rates', 'timestamp',
    chunk_time_interval => INTERVAL '7 days',
    if_not_exists => TRUE
);

-- ============================================
-- 3. 创建索引
-- ============================================

CREATE INDEX IF NOT EXISTS idx_price_data_exchange_symbol_timestamp 
ON price_data(exchange, symbol, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_price_data_timestamp 
ON price_data(timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_funding_rates_exchange_symbol_timestamp 
ON funding_rates(exchange, symbol, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_funding_rates_timestamp 
ON funding_rates(timestamp DESC);

-- ============================================
-- 4. 配置压缩（可选，需要先启用压缩）
-- ============================================

-- 注意：压缩需要先启用，然后才能添加策略
-- ALTER TABLE price_data SET (
--     timescaledb.compress,
--     timescaledb.compress_segmentby = 'exchange, symbol',
--     timescaledb.compress_orderby = 'timestamp DESC'
-- );
-- SELECT add_compression_policy('price_data', INTERVAL '7 days', if_not_exists => true);

-- ============================================
-- 完成
-- ============================================

SELECT 'TimescaleDB 初始化完成！' AS message;
