-- 方案 5: 物化视图和聚合表
-- 预计算常用查询，提升查询性能

-- ============================================
-- 价格数据聚合表
-- ============================================

-- 1. 每分钟价格聚合（用于实时图表）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_minute AS
SELECT 
    DATE_TRUNC('minute', timestamp) AS minute,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(bid_price) AS min_bid_price,
    MAX(ask_price) AS max_ask_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points,
    MAX(timestamp) AS last_update
FROM price_data
GROUP BY minute, exchange, symbol, market_type;

-- 创建索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_price_data_minute_unique
ON price_data_minute(minute, exchange, symbol, market_type);

CREATE INDEX IF NOT EXISTS idx_price_data_minute_time
ON price_data_minute(minute DESC, exchange, symbol);

-- 2. 每小时价格聚合（用于历史分析）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_hourly AS
SELECT 
    DATE_TRUNC('hour', timestamp) AS hour,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points,
    MAX(timestamp) AS last_update
FROM price_data
GROUP BY hour, exchange, symbol, market_type;

-- 创建索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_price_data_hourly_unique
ON price_data_hourly(hour, exchange, symbol, market_type);

CREATE INDEX IF NOT EXISTS idx_price_data_hourly_time
ON price_data_hourly(hour DESC, exchange, symbol);

-- 3. 每日价格聚合（用于长期趋势）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_daily AS
SELECT 
    DATE_TRUNC('day', timestamp) AS day,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points,
    MAX(timestamp) AS last_update
FROM price_data
GROUP BY day, exchange, symbol, market_type;

-- 创建索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_price_data_daily_unique
ON price_data_daily(day, exchange, symbol, market_type);

CREATE INDEX IF NOT EXISTS idx_price_data_daily_time
ON price_data_daily(day DESC, exchange, symbol);

-- ============================================
-- 资金费率聚合表
-- ============================================

-- 1. 每日资金费率聚合
CREATE MATERIALIZED VIEW IF NOT EXISTS funding_rates_daily AS
SELECT 
    DATE_TRUNC('day', timestamp) AS day,
    exchange,
    symbol,
    AVG(rate) AS avg_rate,
    MIN(rate) AS min_rate,
    MAX(rate) AS max_rate,
    COUNT(*) AS data_points,
    MAX(timestamp) AS last_update
FROM funding_rates
GROUP BY day, exchange, symbol;

-- 创建索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_funding_rates_daily_unique
ON funding_rates_daily(day, exchange, symbol);

CREATE INDEX IF NOT EXISTS idx_funding_rates_daily_time
ON funding_rates_daily(day DESC, exchange, symbol);

-- ============================================
-- 刷新物化视图的函数
-- ============================================

-- 刷新所有价格聚合视图
CREATE OR REPLACE FUNCTION refresh_price_aggregates()
RETURNS VOID AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY price_data_minute;
    REFRESH MATERIALIZED VIEW CONCURRENTLY price_data_hourly;
    REFRESH MATERIALIZED VIEW CONCURRENTLY price_data_daily;
END;
$$ LANGUAGE plpgsql;

-- 刷新资金费率聚合视图
CREATE OR REPLACE FUNCTION refresh_funding_rates_aggregates()
RETURNS VOID AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY funding_rates_daily;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- 增量刷新函数（只刷新最近的数据）
-- ============================================

-- 增量刷新分钟级聚合（只刷新最近 1 小时）
CREATE OR REPLACE FUNCTION refresh_price_data_minute_incremental()
RETURNS VOID AS $$
BEGIN
    -- 删除最近 1 小时的数据
    DELETE FROM price_data_minute
    WHERE minute > NOW() - INTERVAL '1 hour';
    
    -- 重新插入最近 1 小时的数据
    INSERT INTO price_data_minute
    SELECT 
        DATE_TRUNC('minute', timestamp) AS minute,
        exchange,
        symbol,
        market_type,
        AVG(last_price) AS avg_price,
        MIN(bid_price) AS min_bid_price,
        MAX(ask_price) AS max_ask_price,
        AVG(volume_24h) AS avg_volume,
        COUNT(*) AS data_points,
        MAX(timestamp) AS last_update
    FROM price_data
    WHERE timestamp > NOW() - INTERVAL '1 hour'
    GROUP BY minute, exchange, symbol, market_type
    ON CONFLICT (minute, exchange, symbol, market_type) 
    DO UPDATE SET
        avg_price = EXCLUDED.avg_price,
        min_bid_price = EXCLUDED.min_bid_price,
        max_ask_price = EXCLUDED.max_ask_price,
        avg_volume = EXCLUDED.avg_volume,
        data_points = EXCLUDED.data_points,
        last_update = EXCLUDED.last_update;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- 自动刷新任务（使用 pg_cron）
-- ============================================

-- 每分钟刷新分钟级聚合
-- SELECT cron.schedule('refresh-price-minute', '* * * * *', 'SELECT refresh_price_data_minute_incremental();');

-- 每小时刷新小时级聚合
-- SELECT cron.schedule('refresh-price-hourly', '0 * * * *', 'SELECT REFRESH MATERIALIZED VIEW CONCURRENTLY price_data_hourly;');

-- 每天刷新日级聚合
-- SELECT cron.schedule('refresh-price-daily', '0 1 * * *', 'SELECT REFRESH MATERIALIZED VIEW CONCURRENTLY price_data_daily;');

-- ============================================
-- 使用示例
-- ============================================

-- 查询最近 24 小时的价格数据（使用分钟级聚合）
-- SELECT * FROM price_data_minute
-- WHERE minute > NOW() - INTERVAL '24 hours'
-- AND exchange = 'binance'
-- AND symbol = 'BTCUSDT'
-- ORDER BY minute DESC;

-- 查询最近 7 天的价格趋势（使用小时级聚合）
-- SELECT * FROM price_data_hourly
-- WHERE hour > NOW() - INTERVAL '7 days'
-- AND exchange = 'binance'
-- AND symbol = 'BTCUSDT'
-- ORDER BY hour DESC;

-- 查询最近 30 天的价格趋势（使用日级聚合）
-- SELECT * FROM price_data_daily
-- WHERE day > NOW() - INTERVAL '30 days'
-- AND exchange = 'binance'
-- AND symbol = 'BTCUSDT'
-- ORDER BY day DESC;
