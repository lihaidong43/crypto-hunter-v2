# 性能优化实施指南

## 快速开始

### 阶段 1: 立即实施（推荐，1-2 小时）

#### 1.1 添加索引优化

```bash
# 执行索引优化脚本
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql
```

**效果：**
- 查询性能提升 10-100 倍（取决于查询模式）
- 立即生效，无需修改代码

#### 1.2 配置数据清理策略

```bash
# 执行数据清理脚本
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql
```

**配置保留策略：**
- `price_data`: 保留最近 30 天
- `funding_rates`: 保留最近 90 天

**设置自动清理（使用 cron 或应用层）：**

```rust
// 在 main.rs 中添加定期清理任务
use crate::storage::maintenance::Maintenance;

async fn maintenance_task(repository: Arc<Repository>, maintenance: Arc<Maintenance>) {
    let mut interval = interval(Duration::from_secs(3600)); // 每小时执行一次
    
    loop {
        interval.tick().await;
        
        // 清理 30 天前的价格数据
        if let Err(e) = maintenance.cleanup_old_price_data(30).await {
            error!("清理价格数据失败: {}", e);
        }
        
        // 清理 90 天前的资金费率数据
        if let Err(e) = maintenance.cleanup_old_funding_rates(90).await {
            error!("清理资金费率数据失败: {}", e);
        }
    }
}
```

---

### 阶段 2: 短期实施（推荐，1-2 周）

#### 2.1 数据库分区

**前提条件：**
- PostgreSQL 10+ 版本
- 备份现有数据

**实施步骤：**

```bash
# 1. 备份数据
pg_dump -U postgres -d crypto_hunter -t price_data > price_data_backup.sql
pg_dump -U postgres -d crypto_hunter -t funding_rates > funding_rates_backup.sql

# 2. 执行分区脚本
psql -U postgres -d crypto_hunter -f migrations/02_table_partitioning.sql

# 3. 迁移数据（如果需要）
# 按照脚本中的注释执行数据迁移

# 4. 在应用层添加分区维护任务
```

**在应用层添加分区维护：**

```rust
// 每天执行一次，创建未来 3 个月的分区
async fn partition_maintenance_task(maintenance: Arc<Maintenance>) {
    let mut interval = interval(Duration::from_secs(86400)); // 每天执行一次
    
    loop {
        interval.tick().await;
        
        if let Err(e) = maintenance.maintain_partitions().await {
            error!("分区维护失败: {}", e);
        }
    }
}
```

**效果：**
- 查询性能提升 5-50 倍（取决于数据量）
- 数据删除和归档变得简单
- 支持并行查询

---

### 阶段 3: 长期优化（可选，1-2 月）

#### 3.1 TimescaleDB

**前提条件：**
- 安装 TimescaleDB 扩展
- PostgreSQL 12+ 版本

**安装 TimescaleDB：**

```bash
# macOS
brew install timescaledb

# Ubuntu/Debian
sudo apt install timescaledb-2-postgresql-14

# 在数据库中启用
psql -U postgres -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
```

**实施步骤：**

```bash
# 执行 TimescaleDB 配置脚本
psql -U postgres -d crypto_hunter -f migrations/04_timescaledb.sql
```

**效果：**
- 自动分区和压缩
- 查询性能提升 10-1000 倍
- 自动数据管理

#### 3.2 物化视图

**实施步骤：**

```bash
# 执行物化视图脚本
psql -U postgres -d crypto_hunter -f migrations/05_materialized_views.sql
```

**在应用层添加刷新任务：**

```rust
// 每分钟刷新分钟级聚合
async fn refresh_aggregates_task(maintenance: Arc<Maintenance>) {
    let mut interval = interval(Duration::from_secs(60)); // 每分钟执行一次
    
    loop {
        interval.tick().await;
        
        if let Err(e) = maintenance.refresh_materialized_views().await {
            error!("刷新物化视图失败: {}", e);
        }
    }
}
```

**效果：**
- 报表查询性能提升 100-1000 倍
- 支持复杂的时间序列分析

---

## 性能监控

### 查看表统计信息

```rust
use crate::storage::maintenance::Maintenance;

let stats = maintenance.get_table_stats().await?;
println!("价格数据: {} 条记录, 总大小: {}", 
    stats.price_data.row_count, 
    stats.price_data.total_size);
println!("资金费率: {} 条记录, 总大小: {}", 
    stats.funding_rates.row_count, 
    stats.funding_rates.total_size);
```

### 查看索引使用情况

```sql
-- 查看索引使用统计
SELECT 
    schemaname,
    tablename,
    indexname,
    idx_scan AS index_scans,
    idx_tup_read AS tuples_read,
    idx_tup_fetch AS tuples_fetched
FROM pg_stat_user_indexes
WHERE tablename IN ('price_data', 'funding_rates')
ORDER BY idx_scan DESC;
```

### 查看慢查询

```sql
-- 启用慢查询日志（在 postgresql.conf 中）
-- log_min_duration_statement = 1000  -- 记录超过 1 秒的查询

-- 查看当前慢查询
SELECT 
    pid,
    now() - pg_stat_activity.query_start AS duration,
    query
FROM pg_stat_activity
WHERE (now() - pg_stat_activity.query_start) > interval '1 second'
AND state = 'active';
```

---

## 性能测试

### 测试查询性能

```sql
-- 测试 1: 查询最近 24 小时的数据
EXPLAIN ANALYZE
SELECT * FROM price_data
WHERE timestamp > NOW() - INTERVAL '24 hours'
AND exchange = 'binance'
AND symbol = 'BTCUSDT'
ORDER BY timestamp DESC
LIMIT 100;

-- 测试 2: 查询特定时间范围的数据
EXPLAIN ANALYZE
SELECT * FROM price_data
WHERE timestamp BETWEEN '2024-01-01' AND '2024-01-02'
AND exchange = 'binance'
AND symbol = 'BTCUSDT';

-- 测试 3: 聚合查询
EXPLAIN ANALYZE
SELECT 
    DATE_TRUNC('hour', timestamp) AS hour,
    AVG(last_price) AS avg_price,
    COUNT(*) AS count
FROM price_data
WHERE timestamp > NOW() - INTERVAL '7 days'
AND exchange = 'binance'
AND symbol = 'BTCUSDT'
GROUP BY hour
ORDER BY hour DESC;
```

---

## 推荐配置

### 小型部署（数据量 < 1000 万条）

1. ✅ 索引优化
2. ✅ 数据清理策略（保留 30 天）
3. ❌ 分区（可选）
4. ❌ TimescaleDB（可选）

### 中型部署（数据量 1000 万 - 1 亿条）

1. ✅ 索引优化
2. ✅ 数据清理策略（保留 30 天）
3. ✅ 数据库分区
4. ❌ TimescaleDB（可选）

### 大型部署（数据量 > 1 亿条）

1. ✅ 索引优化
2. ✅ 数据库分区
3. ✅ TimescaleDB
4. ✅ 物化视图

---

## 故障排查

### 问题 1: 查询仍然很慢

**检查：**
1. 索引是否创建成功
2. 查询是否使用了索引（使用 EXPLAIN ANALYZE）
3. 表是否需要 VACUUM

**解决：**
```sql
-- 更新统计信息
ANALYZE price_data;
ANALYZE funding_rates;

-- 清理表
VACUUM ANALYZE price_data;
VACUUM ANALYZE funding_rates;
```

### 问题 2: 分区创建失败

**检查：**
1. PostgreSQL 版本是否 >= 10
2. 分区函数是否正确创建

**解决：**
```sql
-- 手动创建分区
SELECT create_price_data_partition(CURRENT_DATE);
```

### 问题 3: TimescaleDB 扩展未安装

**检查：**
```sql
SELECT * FROM pg_extension WHERE extname = 'timescaledb';
```

**解决：**
按照 TimescaleDB 官方文档安装扩展。

---

## 总结

**立即实施（今天）：**
1. 执行 `01_add_indexes.sql`
2. 执行 `03_data_retention.sql`
3. 添加数据清理任务

**短期实施（本周）：**
1. 执行 `02_table_partitioning.sql`
2. 添加分区维护任务

**长期优化（本月）：**
1. 安装 TimescaleDB
2. 执行 `04_timescaledb.sql`
3. 执行 `05_materialized_views.sql`
