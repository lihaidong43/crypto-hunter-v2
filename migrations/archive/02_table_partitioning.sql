-- 方案 1: 数据库分区（按时间范围分区）
-- 将 price_data 和 funding_rates 表改为分区表

-- ============================================
-- 注意：此脚本会重建表，请先备份数据！
-- ============================================

-- ============================================
-- price_data 表分区
-- ============================================

-- 1. 创建新的分区表结构
CREATE TABLE IF NOT EXISTS price_data_partitioned (
    id BIGSERIAL NOT NULL,
    exchange VARCHAR(20) NOT NULL,
    symbol VARCHAR(50) NOT NULL,
    market_type VARCHAR(10) NOT NULL,
    bid_price NUMERIC(20, 8) NOT NULL,
    ask_price NUMERIC(20, 8) NOT NULL,
    last_price NUMERIC(20, 8) NOT NULL,
    volume_24h NUMERIC(30, 8) NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    PRIMARY KEY (id, timestamp)
) PARTITION BY RANGE (timestamp);

-- 2. 创建分区函数（按月分区）
CREATE OR REPLACE FUNCTION create_price_data_partition(partition_date DATE)
RETURNS VOID AS $$
DECLARE
    partition_name TEXT;
    start_date DATE;
    end_date DATE;
BEGIN
    -- 计算分区开始和结束日期（月初到月末）
    start_date := DATE_TRUNC('month', partition_date);
    end_date := start_date + INTERVAL '1 month';
    partition_name := 'price_data_' || TO_CHAR(start_date, 'YYYY_MM');
    
    -- 创建分区（如果不存在）
    EXECUTE format('
        CREATE TABLE IF NOT EXISTS %I PARTITION OF price_data_partitioned
        FOR VALUES FROM (%L) TO (%L)',
        partition_name, start_date, end_date
    );
    
    -- 创建分区索引
    EXECUTE format('
        CREATE INDEX IF NOT EXISTS %I 
        ON %I(exchange, symbol, timestamp DESC)',
        partition_name || '_idx', partition_name
    );
END;
$$ LANGUAGE plpgsql;

-- 3. 创建当前月份和未来 2 个月的分区
SELECT create_price_data_partition(CURRENT_DATE);
SELECT create_price_data_partition(CURRENT_DATE + INTERVAL '1 month');
SELECT create_price_data_partition(CURRENT_DATE + INTERVAL '2 months');

-- 4. 创建自动创建分区的触发器函数
CREATE OR REPLACE FUNCTION create_price_data_partitions_auto()
RETURNS TRIGGER AS $$
DECLARE
    partition_date DATE;
    partition_name TEXT;
BEGIN
    partition_date := DATE_TRUNC('month', NEW.timestamp);
    
    -- 检查分区是否存在，如果不存在则创建
    SELECT create_price_data_partition(partition_date);
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- 注意：分区表不支持 BEFORE INSERT 触发器，需要在应用层处理
-- 或者使用 PostgreSQL 13+ 的 DEFAULT 分区

-- ============================================
-- funding_rates 表分区
-- ============================================

-- 1. 创建新的分区表结构
CREATE TABLE IF NOT EXISTS funding_rates_partitioned (
    id BIGSERIAL NOT NULL,
    exchange VARCHAR(20) NOT NULL,
    symbol VARCHAR(50) NOT NULL,
    rate NUMERIC(20, 8) NOT NULL,
    next_funding_time TIMESTAMP WITH TIME ZONE NOT NULL,
    funding_interval_hours INTEGER NOT NULL,
    rate_limit_upper NUMERIC(20, 8),
    rate_limit_lower NUMERIC(20, 8),
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    PRIMARY KEY (id, timestamp)
) PARTITION BY RANGE (timestamp);

-- 2. 创建分区函数
CREATE OR REPLACE FUNCTION create_funding_rates_partition(partition_date DATE)
RETURNS VOID AS $$
DECLARE
    partition_name TEXT;
    start_date DATE;
    end_date DATE;
BEGIN
    start_date := DATE_TRUNC('month', partition_date);
    end_date := start_date + INTERVAL '1 month';
    partition_name := 'funding_rates_' || TO_CHAR(start_date, 'YYYY_MM');
    
    EXECUTE format('
        CREATE TABLE IF NOT EXISTS %I PARTITION OF funding_rates_partitioned
        FOR VALUES FROM (%L) TO (%L)',
        partition_name, start_date, end_date
    );
    
    EXECUTE format('
        CREATE INDEX IF NOT EXISTS %I 
        ON %I(exchange, symbol, timestamp DESC)',
        partition_name || '_idx', partition_name
    );
END;
$$ LANGUAGE plpgsql;

-- 3. 创建当前月份和未来 2 个月的分区
SELECT create_funding_rates_partition(CURRENT_DATE);
SELECT create_funding_rates_partition(CURRENT_DATE + INTERVAL '1 month');
SELECT create_funding_rates_partition(CURRENT_DATE + INTERVAL '2 months');

-- ============================================
-- 数据迁移（可选）
-- ============================================

-- 如果需要迁移现有数据到分区表：
-- INSERT INTO price_data_partitioned 
-- SELECT * FROM price_data;

-- INSERT INTO funding_rates_partitioned 
-- SELECT * FROM funding_rates;

-- 然后重命名表：
-- ALTER TABLE price_data RENAME TO price_data_old;
-- ALTER TABLE price_data_partitioned RENAME TO price_data;

-- ============================================
-- 维护任务：自动创建未来分区
-- ============================================

-- 创建定期创建分区的函数（可以通过 cron 或应用层调用）
CREATE OR REPLACE FUNCTION maintain_price_data_partitions()
RETURNS VOID AS $$
DECLARE
    months_ahead INTEGER := 3; -- 提前创建 3 个月的分区
    i INTEGER;
BEGIN
    FOR i IN 0..months_ahead LOOP
        PERFORM create_price_data_partition(CURRENT_DATE + (i || ' months')::INTERVAL);
        PERFORM create_funding_rates_partition(CURRENT_DATE + (i || ' months')::INTERVAL);
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- 查看所有分区
-- SELECT 
--     schemaname,
--     tablename,
--     pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS size
-- FROM pg_tables
-- WHERE tablename LIKE 'price_data_%' OR tablename LIKE 'funding_rates_%'
-- ORDER BY tablename;
