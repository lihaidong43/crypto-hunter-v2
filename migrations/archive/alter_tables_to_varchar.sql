-- 将现有表的 NUMERIC 字段改为 VARCHAR
-- 注意：这会保留现有数据，但需要确保数据格式正确

-- 修改 price_data 表
ALTER TABLE price_data 
    ALTER COLUMN bid_price TYPE VARCHAR(50) USING bid_price::text,
    ALTER COLUMN ask_price TYPE VARCHAR(50) USING ask_price::text,
    ALTER COLUMN last_price TYPE VARCHAR(50) USING last_price::text,
    ALTER COLUMN volume_24h TYPE VARCHAR(50) USING volume_24h::text;

-- 修改 funding_rates 表
ALTER TABLE funding_rates 
    ALTER COLUMN rate TYPE VARCHAR(50) USING rate::text,
    ALTER COLUMN rate_limit_upper TYPE VARCHAR(50) USING rate_limit_upper::text,
    ALTER COLUMN rate_limit_lower TYPE VARCHAR(50) USING rate_limit_lower::text;

-- 修改 arbitrage_opportunities 表
ALTER TABLE arbitrage_opportunities 
    ALTER COLUMN price_a TYPE VARCHAR(50) USING price_a::text,
    ALTER COLUMN price_b TYPE VARCHAR(50) USING price_b::text,
    ALTER COLUMN open_spread TYPE VARCHAR(50) USING open_spread::text,
    ALTER COLUMN close_spread TYPE VARCHAR(50) USING close_spread::text,
    ALTER COLUMN funding_rate_a TYPE VARCHAR(50) USING funding_rate_a::text,
    ALTER COLUMN funding_rate_b TYPE VARCHAR(50) USING funding_rate_b::text,
    ALTER COLUMN net_funding_rate TYPE VARCHAR(50) USING net_funding_rate::text,
    ALTER COLUMN volume_24h_a TYPE VARCHAR(50) USING volume_24h_a::text,
    ALTER COLUMN volume_24h_b TYPE VARCHAR(50) USING volume_24h_b::text;

-- 修改 volatility_events 表
ALTER TABLE volatility_events 
    ALTER COLUMN value TYPE VARCHAR(50) USING value::text,
    ALTER COLUMN previous_value TYPE VARCHAR(50) USING previous_value::text,
    ALTER COLUMN change_percentage TYPE VARCHAR(50) USING change_percentage::text;

-- 完成
SELECT '所有表的 NUMERIC 字段已改为 VARCHAR' AS message;
