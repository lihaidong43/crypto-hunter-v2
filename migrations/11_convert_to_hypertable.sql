-- 将 market_snapshots 转换为 TimescaleDB Hypertable
-- 注意：此操作会迁移现有数据，可能需要一些时间

-- ============================================
-- 1. 检查当前状态
-- ============================================

SELECT 'Current table size:' AS info;
SELECT 
    count(*) as total_rows,
    pg_size_pretty(pg_total_relation_size('market_snapshots')) as total_size
FROM market_snapshots;

-- ============================================
-- 2. 修改表结构（TimescaleDB 要求分区列在主键中）
-- ============================================

-- 删除现有主键
ALTER TABLE market_snapshots DROP CONSTRAINT IF EXISTS market_snapshots_pkey;

-- 删除 id 列（不再需要，使用时间作为分区键）
ALTER TABLE market_snapshots DROP COLUMN IF EXISTS id;

-- 使用 UNIQUE 约束代替主键（已存在则跳过）
-- 注意：已有 UNIQUE(snapshot_time, exchange, symbol, market_type) 约束

-- ============================================
-- 3. 转换为 Hypertable（迁移现有数据）
-- ============================================

-- 注意：migrate_data => true 会将现有数据迁移到 chunks
-- chunk_time_interval 设置为 1 天
SELECT create_hypertable(
    'market_snapshots', 
    'snapshot_time',
    chunk_time_interval => INTERVAL '1 day',
    migrate_data => true,
    if_not_exists => true
);

-- ============================================
-- 3. 配置压缩策略（3天后压缩，节省空间）
-- ============================================

-- 启用压缩
ALTER TABLE market_snapshots SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'exchange, symbol',
    timescaledb.compress_orderby = 'snapshot_time DESC'
);

-- 先删除旧的压缩策略
SELECT remove_compression_policy('market_snapshots', if_exists => true);

-- 1天后的数据自动压缩（3天保留，1天后压缩可节省空间）
SELECT add_compression_policy('market_snapshots', INTERVAL '1 day', if_not_exists => true);

-- ============================================
-- 4. 添加数据保留策略（3天）
-- ============================================

-- 先删除旧的保留策略
SELECT remove_retention_policy('market_snapshots', if_exists => true);

-- 添加新的3天保留策略
SELECT add_retention_policy('market_snapshots', INTERVAL '3 days', if_not_exists => true);

-- ============================================
-- 5. 验证配置
-- ============================================

SELECT 'Hypertable info:' AS info;
SELECT * FROM timescaledb_information.hypertables 
WHERE hypertable_name = 'market_snapshots';

SELECT 'Chunks info:' AS info;
SELECT 
    chunk_name,
    range_start::date as start_date,
    range_end::date as end_date,
    is_compressed
FROM timescaledb_information.chunks
WHERE hypertable_name = 'market_snapshots'
ORDER BY range_start DESC
LIMIT 10;

SELECT 'Jobs (compression & retention):' AS info;
SELECT job_id, application_name, schedule_interval, config 
FROM timescaledb_information.jobs 
WHERE hypertable_name = 'market_snapshots';

-- ============================================
-- 完成
-- ============================================

SELECT 'market_snapshots 已转换为 TimescaleDB Hypertable，保留策略：3天' AS message;
