-- 方案 3: 索引优化
-- 为 price_data 和 funding_rates 表添加优化索引

-- ============================================
-- price_data 表索引优化
-- ============================================

-- 1. 复合索引：exchange + symbol + timestamp（已存在，但优化顺序）
-- 用于：按交易所和交易对查询最近数据
DROP INDEX IF EXISTS idx_price_data_exchange_symbol_timestamp;
CREATE INDEX idx_price_data_exchange_symbol_timestamp 
ON price_data(exchange, symbol, timestamp DESC);

-- 2. 时间范围查询索引
-- 用于：查询特定时间范围的数据
CREATE INDEX IF NOT EXISTS idx_price_data_timestamp 
ON price_data(timestamp DESC);

-- 3. 部分索引：只索引最近的数据（提升写入性能）
-- 注意：部分索引不能使用 NOW()，改用固定日期或移除条件
-- 如果需要动态时间范围，建议使用普通索引或定期重建索引
-- CREATE INDEX IF NOT EXISTS idx_price_data_recent 
-- ON price_data(exchange, symbol, timestamp DESC)
-- WHERE timestamp > NOW() - INTERVAL '7 days';

-- 4. 覆盖索引：包含常用查询字段
-- 用于：避免回表查询
CREATE INDEX IF NOT EXISTS idx_price_data_covering 
ON price_data(exchange, symbol, timestamp DESC, last_price, volume_24h);

-- 5. 按市场类型查询的索引
-- 用于：区分现货和期货价格
CREATE INDEX IF NOT EXISTS idx_price_data_market_type 
ON price_data(exchange, symbol, market_type, timestamp DESC);

-- ============================================
-- funding_rates 表索引优化
-- ============================================

-- 1. 复合索引：exchange + symbol + timestamp
-- 用于：按交易所和交易对查询最近资金费率
CREATE INDEX IF NOT EXISTS idx_funding_rates_exchange_symbol_timestamp 
ON funding_rates(exchange, symbol, timestamp DESC);

-- 2. 时间范围查询索引
CREATE INDEX IF NOT EXISTS idx_funding_rates_timestamp 
ON funding_rates(timestamp DESC);

-- 3. 部分索引：只索引最近的数据
-- 注意：部分索引不能使用 NOW()，改用固定日期或移除条件
-- 如果需要动态时间范围，建议使用普通索引或定期重建索引
-- CREATE INDEX IF NOT EXISTS idx_funding_rates_recent 
-- ON funding_rates(exchange, symbol, timestamp DESC)
-- WHERE timestamp > NOW() - INTERVAL '30 days';

-- 4. 按 next_funding_time 查询的索引
-- 用于：查询即将结算的资金费率
CREATE INDEX IF NOT EXISTS idx_funding_rates_next_funding_time 
ON funding_rates(next_funding_time);

-- 5. 按资金费率值查询的索引（用于筛选异常费率）
CREATE INDEX IF NOT EXISTS idx_funding_rates_rate 
ON funding_rates(rate);

-- ============================================
-- 索引使用建议
-- ============================================

-- 查看索引使用情况
-- SELECT schemaname, tablename, indexname, idx_scan, idx_tup_read, idx_tup_fetch
-- FROM pg_stat_user_indexes
-- WHERE tablename IN ('price_data', 'funding_rates')
-- ORDER BY idx_scan DESC;

-- 查看表大小和索引大小
-- SELECT 
--     schemaname,
--     tablename,
--     pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS total_size,
--     pg_size_pretty(pg_relation_size(schemaname||'.'||tablename)) AS table_size,
--     pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename) - pg_relation_size(schemaname||'.'||tablename)) AS indexes_size
-- FROM pg_tables
-- WHERE tablename IN ('price_data', 'funding_rates');
