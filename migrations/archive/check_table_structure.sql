-- 检查表结构，确认字段类型
-- 执行方式：psql -U postgres -d crypto_hunter -f migrations/check_table_structure.sql

-- 检查 price_data 表结构
SELECT 
    column_name, 
    data_type, 
    character_maximum_length
FROM information_schema.columns 
WHERE table_name = 'price_data' 
ORDER BY ordinal_position;

-- 检查 funding_rates 表结构
SELECT 
    column_name, 
    data_type, 
    character_maximum_length
FROM information_schema.columns 
WHERE table_name = 'funding_rates' 
ORDER BY ordinal_position;

-- 检查 arbitrage_opportunities 表结构
SELECT 
    column_name, 
    data_type, 
    character_maximum_length
FROM information_schema.columns 
WHERE table_name = 'arbitrage_opportunities' 
ORDER BY ordinal_position;

-- 检查 volatility_events 表结构
SELECT 
    column_name, 
    data_type, 
    character_maximum_length
FROM information_schema.columns 
WHERE table_name = 'volatility_events' 
ORDER BY ordinal_position;
