-- TimescaleDB 机器学习训练数据配置
-- 前提：已安装 TimescaleDB 扩展

-- ============================================
-- 1. 转换为超表（如果还没有转换）
-- ============================================

-- 将 price_data 转换为超表
SELECT create_hypertable('price_data', 'timestamp', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- 将 funding_rates 转换为超表
SELECT create_hypertable('funding_rates', 'timestamp',
    chunk_time_interval => INTERVAL '7 days',
    if_not_exists => TRUE
);

-- ============================================
-- 2. 配置压缩策略（保留所有数据，但压缩旧数据）
-- ============================================

-- 压缩超过 7 天的 price_data 数据（节省空间，但保留数据）
-- 注意：需要先启用压缩
ALTER TABLE price_data SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'exchange, symbol',
    timescaledb.compress_orderby = 'timestamp DESC'
);
SELECT add_compression_policy('price_data', INTERVAL '7 days', if_not_exists => true);

-- 压缩超过 30 天的 funding_rates 数据
ALTER TABLE funding_rates SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'exchange, symbol',
    timescaledb.compress_orderby = 'timestamp DESC'
);
SELECT add_compression_policy('funding_rates', INTERVAL '30 days', if_not_exists => true);

-- 重要：不配置保留策略，保留所有数据用于训练
-- 如果需要删除数据，手动执行 DELETE

-- ============================================
-- 3. 创建连续聚合（用于训练数据准备）
-- ============================================

-- 每小时价格聚合（用于训练）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_hourly
WITH (timescaledb.continuous) AS
SELECT 
    time_bucket(INTERVAL '1 hour', timestamp) AS bucket,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(bid_price) AS avg_bid_price,
    AVG(ask_price) AS avg_ask_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points,
    STDDEV(last_price) AS price_stddev
FROM price_data
GROUP BY bucket, exchange, symbol, market_type;

-- 每日价格聚合（用于长期趋势训练）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_daily
WITH (timescaledb.continuous) AS
SELECT 
    time_bucket(INTERVAL '1 day', timestamp) AS bucket,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points
FROM price_data
GROUP BY bucket, exchange, symbol, market_type;

-- 配置自动刷新策略
SELECT add_continuous_aggregate_policy('price_data_hourly',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);

SELECT add_continuous_aggregate_policy('price_data_daily',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day'
);

-- ============================================
-- 4. 创建特征工程视图（技术指标）
-- ============================================

-- 创建技术指标视图（用于训练）
CREATE MATERIALIZED VIEW IF NOT EXISTS price_data_features AS
SELECT 
    bucket,
    exchange,
    symbol,
    market_type,
    avg_price,
    -- 移动平均（24小时）
    AVG(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 23 PRECEDING AND CURRENT ROW
    ) AS ma_24h,
    -- 移动平均（7天）
    AVG(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 167 PRECEDING AND CURRENT ROW
    ) AS ma_7d,
    -- 价格变化率（1小时）
    (avg_price - LAG(avg_price, 1) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    )) / NULLIF(LAG(avg_price, 1) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    ), 0) * 100 AS price_change_pct_1h,
    -- 价格变化率（24小时）
    (avg_price - LAG(avg_price, 24) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    )) / NULLIF(LAG(avg_price, 24) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    ), 0) * 100 AS price_change_pct_24h,
    -- 波动率（24小时）
    STDDEV(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 23 PRECEDING AND CURRENT ROW
    ) AS volatility_24h,
    avg_volume,
    price_stddev
FROM price_data_hourly
ORDER BY bucket, exchange, symbol;

-- 创建索引以加速查询
CREATE INDEX IF NOT EXISTS idx_price_data_features_bucket 
ON price_data_features(bucket DESC, exchange, symbol);

-- ============================================
-- 5. 数据导出辅助函数
-- ============================================

-- 创建导出函数（返回查询结果，供应用层使用）
CREATE OR REPLACE FUNCTION get_training_data(
    p_start_time TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    p_end_time TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    p_exchange VARCHAR DEFAULT NULL,
    p_symbol VARCHAR DEFAULT NULL,
    p_use_aggregate BOOLEAN DEFAULT TRUE
)
RETURNS TABLE (
    timestamp TIMESTAMP WITH TIME ZONE,
    exchange VARCHAR,
    symbol VARCHAR,
    market_type VARCHAR,
    price NUMERIC,
    volume NUMERIC,
    ma_24h NUMERIC,
    ma_7d NUMERIC,
    price_change_pct_1h NUMERIC,
    price_change_pct_24h NUMERIC,
    volatility_24h NUMERIC
) AS $$
BEGIN
    IF p_use_aggregate THEN
        -- 使用聚合数据（更快，数据量更少）
        RETURN QUERY
        SELECT 
            f.bucket AS data_timestamp,
            f.exchange,
            f.symbol,
            f.market_type,
            f.avg_price AS price,
            f.avg_volume AS volume,
            f.ma_24h,
            f.ma_7d,
            f.price_change_pct_1h,
            f.price_change_pct_24h,
            f.volatility_24h
        FROM price_data_features f
        WHERE (p_start_time IS NULL OR f.bucket >= p_start_time)
        AND (p_end_time IS NULL OR f.bucket <= p_end_time)
        AND (p_exchange IS NULL OR f.exchange = p_exchange)
        AND (p_symbol IS NULL OR f.symbol = p_symbol)
        ORDER BY f.bucket, f.exchange, f.symbol;
    ELSE
        -- 使用原始数据（更详细，但数据量大）
        RETURN QUERY
        SELECT 
            p.timestamp AS data_timestamp,
            p.exchange,
            p.symbol,
            p.market_type,
            p.last_price AS price,
            p.volume_24h AS volume,
            NULL::NUMERIC AS ma_24h,
            NULL::NUMERIC AS ma_7d,
            NULL::NUMERIC AS price_change_pct_1h,
            NULL::NUMERIC AS price_change_pct_24h,
            NULL::NUMERIC AS volatility_24h
        FROM price_data p
        WHERE (p_start_time IS NULL OR p.timestamp >= p_start_time)
        AND (p_end_time IS NULL OR p.timestamp <= p_end_time)
        AND (p_exchange IS NULL OR p.exchange = p_exchange)
        AND (p_symbol IS NULL OR p.symbol = p_symbol)
        ORDER BY p.timestamp, p.exchange, p.symbol;
    END IF;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- 6. 使用示例
-- ============================================

-- 查询最近 1 年的聚合数据（用于训练）
-- SELECT * FROM get_training_data(
--     p_start_time => NOW() - INTERVAL '1 year',
--     p_end_time => NOW(),
--     p_use_aggregate => TRUE
-- );

-- 查询特定交易对的原始数据
-- SELECT * FROM get_training_data(
--     p_start_time => '2024-01-01'::TIMESTAMP WITH TIME ZONE,
--     p_end_time => '2024-12-31'::TIMESTAMP WITH TIME ZONE,
--     p_exchange => 'binance',
--     p_symbol => 'BTCUSDT',
--     p_use_aggregate => FALSE
-- );

-- ============================================
-- 7. 监控和维护
-- ============================================

-- 查看超表信息
-- SELECT * FROM timescaledb_information.hypertables;

-- 查看压缩统计
-- SELECT 
--     hypertable_name,
--     pg_size_pretty(before_compression_total_bytes) AS before_size,
--     pg_size_pretty(after_compression_total_bytes) AS after_size,
--     ROUND(100.0 * (1.0 - after_compression_total_bytes::numeric / before_compression_total_bytes), 2) AS compression_ratio
-- FROM timescaledb_information.compression_settings;

-- 查看连续聚合
-- SELECT * FROM timescaledb_information.continuous_aggregates;
