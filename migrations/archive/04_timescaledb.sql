-- 方案 2: TimescaleDB 配置
-- 需要先安装 TimescaleDB 扩展：CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ============================================
-- 注意：此脚本需要 TimescaleDB 扩展
-- 安装方法：
-- 1. 安装 TimescaleDB: https://docs.timescale.com/install/latest/
-- 2. 在数据库中启用: CREATE EXTENSION IF NOT EXISTS timescaledb;
-- ============================================

-- ============================================
-- 检查 TimescaleDB 是否已安装
-- ============================================

-- SELECT * FROM pg_extension WHERE extname = 'timescaledb';

-- ============================================
-- 将 price_data 转换为超表（Hypertable）
-- ============================================

-- 1. 如果表已存在，先转换为超表
-- SELECT create_hypertable('price_data', 'timestamp', 
--     chunk_time_interval => INTERVAL '1 day',
--     if_not_exists => TRUE
-- );

-- 2. 如果表不存在，先创建表再转换为超表
-- CREATE TABLE IF NOT EXISTS price_data (
--     id BIGSERIAL,
--     exchange VARCHAR(20) NOT NULL,
--     symbol VARCHAR(50) NOT NULL,
--     market_type VARCHAR(10) NOT NULL,
--     bid_price NUMERIC(20, 8) NOT NULL,
--     ask_price NUMERIC(20, 8) NOT NULL,
--     last_price NUMERIC(20, 8) NOT NULL,
--     volume_24h NUMERIC(30, 8) NOT NULL,
--     timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
--     created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
-- );
-- 
-- SELECT create_hypertable('price_data', 'timestamp', 
--     chunk_time_interval => INTERVAL '1 day'
-- );

-- ============================================
-- 将 funding_rates 转换为超表
-- ============================================

-- SELECT create_hypertable('funding_rates', 'timestamp',
--     chunk_time_interval => INTERVAL '7 days',
--     if_not_exists => TRUE
-- );

-- ============================================
-- 配置压缩策略
-- ============================================

-- 压缩超过 7 天的 price_data 数据
-- SELECT add_compression_policy('price_data', INTERVAL '7 days');

-- 压缩超过 30 天的 funding_rates 数据
-- SELECT add_compression_policy('funding_rates', INTERVAL '30 days');

-- 查看压缩统计
-- SELECT * FROM timescaledb_information.job_stats;

-- ============================================
-- 配置数据保留策略
-- ============================================

-- 自动删除 90 天前的 price_data 数据
-- SELECT add_retention_policy('price_data', INTERVAL '90 days');

-- 自动删除 365 天前的 funding_rates 数据
-- SELECT add_retention_policy('funding_rates', INTERVAL '365 days');

-- ============================================
-- 创建连续聚合（Continuous Aggregates）
-- ============================================

-- 创建每小时价格聚合
-- CREATE MATERIALIZED VIEW price_data_hourly
-- WITH (timescaledb.continuous) AS
-- SELECT 
--     time_bucket('1 hour', timestamp) AS bucket,
--     exchange,
--     symbol,
--     market_type,
--     AVG(last_price) AS avg_price,
--     MIN(last_price) AS min_price,
--     MAX(last_price) AS max_price,
--     AVG(volume_24h) AS avg_volume,
--     COUNT(*) AS data_points
-- FROM price_data
-- GROUP BY bucket, exchange, symbol, market_type;

-- 创建每日价格聚合
-- CREATE MATERIALIZED VIEW price_data_daily
-- WITH (timescaledb.continuous) AS
-- SELECT 
--     time_bucket('1 day', timestamp) AS bucket,
--     exchange,
--     symbol,
--     market_type,
--     AVG(last_price) AS avg_price,
--     MIN(last_price) AS min_price,
--     MAX(last_price) AS max_price,
--     AVG(volume_24h) AS avg_volume,
--     COUNT(*) AS data_points
-- FROM price_data
-- GROUP BY bucket, exchange, symbol, market_type;

-- 添加刷新策略（每小时刷新一次）
-- SELECT add_continuous_aggregate_policy('price_data_hourly',
--     start_offset => INTERVAL '3 hours',
--     end_offset => INTERVAL '1 hour',
--     schedule_interval => INTERVAL '1 hour'
-- );

-- ============================================
-- 查询优化
-- ============================================

-- TimescaleDB 自动优化时间范围查询
-- 以下查询会自动使用时间分区：

-- 查询最近 24 小时的数据
-- SELECT * FROM price_data
-- WHERE timestamp > NOW() - INTERVAL '24 hours'
-- AND exchange = 'binance'
-- AND symbol = 'BTCUSDT';

-- 使用连续聚合查询（更快）
-- SELECT * FROM price_data_hourly
-- WHERE bucket > NOW() - INTERVAL '7 days'
-- AND exchange = 'binance'
-- AND symbol = 'BTCUSDT';

-- ============================================
-- 监控和维护
-- ============================================

-- 查看所有超表
-- SELECT * FROM timescaledb_information.hypertables;

-- 查看所有块（chunks）
-- SELECT * FROM timescaledb_information.chunks
-- WHERE hypertable_name = 'price_data';

-- 查看连续聚合
-- SELECT * FROM timescaledb_information.continuous_aggregates;

-- 查看压缩统计
-- SELECT 
--     hypertable_name,
--     pg_size_pretty(before_compression_total_bytes) AS before_size,
--     pg_size_pretty(after_compression_total_bytes) AS after_size,
--     ROUND(100.0 * (1.0 - after_compression_total_bytes::numeric / before_compression_total_bytes), 2) AS compression_ratio
-- FROM timescaledb_information.compression_settings;
