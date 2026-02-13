-- 删除所有表（按依赖顺序）
-- 注意：这会删除所有数据，请确保已备份重要数据！

-- 删除索引（如果存在）
DROP INDEX IF EXISTS idx_price_data_exchange_symbol_timestamp;
DROP INDEX IF EXISTS idx_arbitrage_opportunities_symbol_timestamp;
DROP INDEX IF EXISTS idx_volatility_events_symbol_timestamp;

-- 删除表（按依赖顺序）
DROP TABLE IF EXISTS volatility_events CASCADE;
DROP TABLE IF EXISTS arbitrage_opportunities CASCADE;
DROP TABLE IF EXISTS funding_rates CASCADE;
DROP TABLE IF EXISTS price_data CASCADE;
DROP TABLE IF EXISTS trading_pairs CASCADE;

-- 完成
SELECT '所有表已删除，请重新运行程序以创建新表结构' AS message;
