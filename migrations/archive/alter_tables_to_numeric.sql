-- 将表的 VARCHAR 字段改回 NUMERIC
-- 注意：这会保留现有数据，但需要确保数据格式正确

-- 修改 price_data 表
ALTER TABLE price_data 
    ALTER COLUMN bid_price TYPE NUMERIC(20, 8) USING bid_price::numeric,
    ALTER COLUMN ask_price TYPE NUMERIC(20, 8) USING ask_price::numeric,
    ALTER COLUMN last_price TYPE NUMERIC(20, 8) USING ask_price::numeric,
    ALTER COLUMN volume_24h TYPE NUMERIC(30, 8) USING volume_24h::numeric;

-- 修改 funding_rates 表
ALTER TABLE funding_rates 
    ALTER COLUMN rate TYPE NUMERIC(20, 8) USING rate::numeric,
    ALTER COLUMN rate_limit_upper TYPE NUMERIC(20, 8) USING rate_limit_upper::numeric,
    ALTER COLUMN rate_limit_lower TYPE NUMERIC(20, 8) USING rate_limit_lower::numeric;

-- 修改 arbitrage_opportunities 表
ALTER TABLE arbitrage_opportunities 
    ALTER COLUMN price_a TYPE NUMERIC(20, 8) USING price_a::numeric,
    ALTER COLUMN price_b TYPE NUMERIC(20, 8) USING price_b::numeric,
    ALTER COLUMN open_spread TYPE NUMERIC(10, 4) USING open_spread::numeric,
    ALTER COLUMN close_spread TYPE NUMERIC(10, 4) USING close_spread::numeric,
    ALTER COLUMN funding_rate_a TYPE NUMERIC(20, 8) USING funding_rate_a::numeric,
    ALTER COLUMN funding_rate_b TYPE NUMERIC(20, 8) USING funding_rate_b::numeric,
    ALTER COLUMN net_funding_rate TYPE NUMERIC(20, 8) USING net_funding_rate::numeric,
    ALTER COLUMN volume_24h_a TYPE NUMERIC(30, 8) USING volume_24h_a::numeric,
    ALTER COLUMN volume_24h_b TYPE NUMERIC(30, 8) USING volume_24h_b::numeric;

-- 修改 volatility_events 表
ALTER TABLE volatility_events 
    ALTER COLUMN value TYPE NUMERIC(20, 8) USING value::numeric,
    ALTER COLUMN previous_value TYPE NUMERIC(20, 8) USING previous_value::numeric,
    ALTER COLUMN change_percentage TYPE NUMERIC(10, 4) USING change_percentage::numeric;

-- 完成
SELECT '所有表的 VARCHAR 字段已改回 NUMERIC' AS message;
