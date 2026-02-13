-- 方案 4 & 6: 数据归档和清理策略
-- 定期清理旧数据，只保留最近的数据

-- ============================================
-- 创建归档表
-- ============================================

-- price_data 归档表
CREATE TABLE IF NOT EXISTS price_data_archive (
    LIKE price_data INCLUDING ALL
);

-- funding_rates 归档表
CREATE TABLE IF NOT EXISTS funding_rates_archive (
    LIKE funding_rates INCLUDING ALL
);

-- ============================================
-- 数据归档函数
-- ============================================

-- 归档 price_data 的旧数据
CREATE OR REPLACE FUNCTION archive_old_price_data(days_to_keep INTEGER DEFAULT 30)
RETURNS INTEGER AS $$
DECLARE
    archived_count INTEGER;
    cutoff_date TIMESTAMP WITH TIME ZONE;
BEGIN
    cutoff_date := NOW() - (days_to_keep || ' days')::INTERVAL;
    
    -- 将旧数据移动到归档表
    WITH moved AS (
        DELETE FROM price_data
        WHERE timestamp < cutoff_date
        RETURNING *
    )
    INSERT INTO price_data_archive
    SELECT * FROM moved;
    
    GET DIAGNOSTICS archived_count = ROW_COUNT;
    
    -- 清理归档表中超过 1 年的数据
    DELETE FROM price_data_archive
    WHERE timestamp < NOW() - INTERVAL '1 year';
    
    RETURN archived_count;
END;
$$ LANGUAGE plpgsql;

-- 归档 funding_rates 的旧数据
CREATE OR REPLACE FUNCTION archive_old_funding_rates(days_to_keep INTEGER DEFAULT 90)
RETURNS INTEGER AS $$
DECLARE
    archived_count INTEGER;
    cutoff_date TIMESTAMP WITH TIME ZONE;
BEGIN
    cutoff_date := NOW() - (days_to_keep || ' days')::INTERVAL;
    
    WITH moved AS (
        DELETE FROM funding_rates
        WHERE timestamp < cutoff_date
        RETURNING *
    )
    INSERT INTO funding_rates_archive
    SELECT * FROM moved;
    
    GET DIAGNOSTICS archived_count = ROW_COUNT;
    
    -- 清理归档表中超过 2 年的数据
    DELETE FROM funding_rates_archive
    WHERE timestamp < NOW() - INTERVAL '2 years';
    
    RETURN archived_count;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- 数据清理函数（直接删除，不归档）
-- ============================================

-- 清理 price_data 的旧数据（直接删除）
CREATE OR REPLACE FUNCTION cleanup_old_price_data(days_to_keep INTEGER DEFAULT 30)
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM price_data
    WHERE timestamp < NOW() - (days_to_keep || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    
    -- 执行 VACUUM 回收空间
    VACUUM ANALYZE price_data;
    
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

-- 清理 funding_rates 的旧数据（直接删除）
CREATE OR REPLACE FUNCTION cleanup_old_funding_rates(days_to_keep INTEGER DEFAULT 90)
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM funding_rates
    WHERE timestamp < NOW() - (days_to_keep || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    
    VACUUM ANALYZE funding_rates;
    
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

-- ============================================
-- 自动维护任务（使用 pg_cron 扩展）
-- ============================================

-- 如果安装了 pg_cron 扩展，可以设置自动任务：
-- SELECT cron.schedule('archive-price-data', '0 2 * * *', 'SELECT archive_old_price_data(30);');
-- SELECT cron.schedule('archive-funding-rates', '0 3 * * *', 'SELECT archive_old_funding_rates(90);');

-- ============================================
-- 使用示例
-- ============================================

-- 手动归档 30 天前的价格数据
-- SELECT archive_old_price_data(30);

-- 手动清理 30 天前的价格数据（不归档）
-- SELECT cleanup_old_price_data(30);

-- 查看表大小
-- SELECT 
--     'price_data' AS table_name,
--     pg_size_pretty(pg_total_relation_size('price_data')) AS size,
--     (SELECT COUNT(*) FROM price_data) AS row_count
-- UNION ALL
-- SELECT 
--     'funding_rates' AS table_name,
--     pg_size_pretty(pg_total_relation_size('funding_rates')) AS size,
--     (SELECT COUNT(*) FROM funding_rates) AS row_count;
