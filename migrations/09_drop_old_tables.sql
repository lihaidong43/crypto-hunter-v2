-- 删除旧表和 TimescaleDB 超表
-- 注意：这会删除所有历史数据，不进行迁移
-- 请确认新系统运行正常后再执行此脚本

-- ============================================
-- 1. 删除相关的连续聚合视图（如果存在）
-- ============================================

DROP MATERIALIZED VIEW IF EXISTS price_data_hourly CASCADE;
DROP MATERIALIZED VIEW IF EXISTS price_data_daily CASCADE;
DROP MATERIALIZED VIEW IF EXISTS funding_rates_hourly CASCADE;
DROP MATERIALIZED VIEW IF EXISTS price_data_features CASCADE;

-- ============================================
-- 2. 删除 TimescaleDB 超表
-- ============================================

-- 删除 price_data 超表（CASCADE 会自动删除相关的压缩策略和索引）
DROP TABLE IF EXISTS price_data CASCADE;

-- 删除 funding_rates 超表（CASCADE 会自动删除相关的压缩策略和索引）
DROP TABLE IF EXISTS funding_rates CASCADE;

-- ============================================
-- 3. 删除其他不再使用的表
-- ============================================

-- 删除套利机会表（实时计算，不需要存储）
DROP TABLE IF EXISTS arbitrage_opportunities CASCADE;

-- 删除价格波动事件表（不再监控波动）
DROP TABLE IF EXISTS volatility_events CASCADE;

-- ============================================
-- 完成
-- ============================================

SELECT '旧表和超表已删除' AS message;
