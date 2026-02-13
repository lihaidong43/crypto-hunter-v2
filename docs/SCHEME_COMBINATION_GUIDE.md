# 性能优化方案组合指南

## 方案兼容性分析

### 方案列表

1. **索引优化**（方案3）
2. **数据库分区**（方案1）
3. **数据清理策略**（方案4/6）
4. **TimescaleDB**（方案2）
5. **物化视图**（方案5）

---

## 兼容性矩阵

| 方案组合 | 兼容性 | 说明 |
|---------|--------|------|
| 索引优化 + 数据库分区 | ✅ 完全兼容 | 分区表也需要索引 |
| 索引优化 + TimescaleDB | ✅ 完全兼容 | TimescaleDB 支持所有索引 |
| 索引优化 + 物化视图 | ✅ 完全兼容 | 物化视图也需要索引 |
| 数据库分区 + TimescaleDB | ⚠️ 部分兼容 | TimescaleDB 已内置分区，手动分区多余 |
| 数据清理 + TimescaleDB | ✅ 完全兼容 | 但 ML 训练数据不建议删除 |
| 物化视图 + TimescaleDB | ✅ 完全兼容 | TimescaleDB 的连续聚合更强大 |

---

## 推荐组合方案

### 组合 1: ML 训练数据场景（推荐）

**目标：** 保留所有历史数据，用于机器学习训练

**方案组合：**
1. ✅ **索引优化** - 基础优化，必须
2. ✅ **TimescaleDB** - 核心方案，替代手动分区
3. ✅ **连续聚合**（TimescaleDB 内置）- 替代普通物化视图
4. ✅ **数据归档**（可选）- 归档到冷存储，不删除

**不推荐：**
- ❌ 数据库分区 - TimescaleDB 已内置
- ❌ 数据清理（删除）- 会丢失训练数据
- ❌ 普通物化视图 - TimescaleDB 连续聚合更好

**实施顺序：**
```bash
# 1. 索引优化（基础）
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql

# 2. TimescaleDB 配置（核心）
psql -U postgres -d crypto_hunter -f migrations/06_timescaledb_ml_setup.sql

# 3. 数据归档（可选，如果需要）
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql
```

---

### 组合 2: 生产环境（不需要 ML 训练）

**目标：** 高性能查询，定期清理旧数据

**方案组合：**
1. ✅ **索引优化** - 基础优化，必须
2. ✅ **数据库分区** - 手动分区（如果不用 TimescaleDB）
3. ✅ **数据清理策略** - 定期删除旧数据
4. ✅ **物化视图** - 预聚合数据

**不推荐：**
- ❌ TimescaleDB - 如果不需要保留所有数据，手动分区更简单

**实施顺序：**
```bash
# 1. 索引优化
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql

# 2. 数据库分区
psql -U postgres -d crypto_hunter -f migrations/02_table_partitioning.sql

# 3. 数据清理策略
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql

# 4. 物化视图
psql -U postgres -d crypto_hunter -f migrations/05_materialized_views.sql
```

---

### 组合 3: 混合场景（推荐）

**目标：** 既需要 ML 训练数据，又需要高性能查询

**方案组合：**
1. ✅ **索引优化** - 基础优化，必须
2. ✅ **TimescaleDB** - 核心方案
3. ✅ **连续聚合** - TimescaleDB 内置
4. ✅ **数据归档** - 归档旧数据到冷存储
5. ✅ **普通物化视图** - 用于特定查询场景

**实施顺序：**
```bash
# 1. 索引优化
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql

# 2. TimescaleDB 配置
psql -U postgres -d crypto_hunter -f migrations/06_timescaledb_ml_setup.sql

# 3. 普通物化视图（用于特定场景）
psql -U postgres -d crypto_hunter -f migrations/05_materialized_views.sql

# 4. 数据归档（可选）
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql
```

---

## 方案冲突说明

### 冲突 1: 数据库分区 vs TimescaleDB

**问题：** TimescaleDB 已经内置了分区功能（chunks），手动分区是多余的。

**解决方案：**
- **选择 TimescaleDB**：如果安装 TimescaleDB，不要执行 `02_table_partitioning.sql`
- **选择手动分区**：如果不安装 TimescaleDB，使用 `02_table_partitioning.sql`

**推荐：** 优先选择 TimescaleDB（功能更强大）

---

### 冲突 2: 连续聚合 vs 普通物化视图

**问题：** TimescaleDB 的连续聚合（Continuous Aggregates）和普通物化视图功能重叠。

**解决方案：**
- **TimescaleDB 连续聚合**：自动刷新，性能更好，推荐用于时间序列数据
- **普通物化视图**：可以用于非时间序列的复杂聚合查询

**推荐：** 两者可以共存，但优先使用 TimescaleDB 连续聚合

---

### 冲突 3: 数据清理 vs ML 训练数据

**问题：** ML 训练需要保留所有历史数据，但数据清理会删除旧数据。

**解决方案：**
- **ML 训练场景**：使用数据归档（移动到归档表），不删除
- **生产环境**：使用数据清理（直接删除），节省空间

**推荐：** 根据场景选择，ML 训练用归档，生产环境用清理

---

## 最佳实践组合

### 场景 A: 小型项目（数据量 < 1000 万条）

**推荐组合：**
1. ✅ 索引优化
2. ✅ 数据清理策略（保留 30 天）

**不推荐：**
- TimescaleDB（过度设计）
- 数据库分区（数据量小，不需要）

---

### 场景 B: 中型项目（数据量 1000 万 - 1 亿条）

**推荐组合：**
1. ✅ 索引优化
2. ✅ 数据库分区 或 TimescaleDB（二选一）
3. ✅ 数据清理策略

**选择建议：**
- 需要保留所有数据 → TimescaleDB
- 可以定期清理 → 手动分区

---

### 场景 C: 大型项目（数据量 > 1 亿条）

**推荐组合：**
1. ✅ 索引优化
2. ✅ TimescaleDB（必需）
3. ✅ 连续聚合
4. ✅ 数据归档（不删除）

**不推荐：**
- 手动分区（TimescaleDB 更好）
- 数据清理（会丢失数据）

---

### 场景 D: ML 训练数据（任何规模）

**推荐组合：**
1. ✅ 索引优化
2. ✅ TimescaleDB（必需）
3. ✅ 连续聚合
4. ✅ 特征工程视图
5. ✅ 数据导出工具

**不推荐：**
- 数据清理（会丢失训练数据）
- 手动分区（TimescaleDB 已内置）

---

## 实施检查清单

### 基础优化（所有场景）

- [ ] 执行 `01_add_indexes.sql` - 索引优化
- [ ] 验证索引创建成功
- [ ] 监控索引使用情况

### ML 训练场景

- [ ] 安装 TimescaleDB
- [ ] 执行 `06_timescaledb_ml_setup.sql`
- [ ] 验证超表创建成功
- [ ] 验证连续聚合工作正常
- [ ] 配置数据导出工具

### 生产环境（非 ML）

- [ ] 选择：TimescaleDB 或手动分区
- [ ] 如果手动分区：执行 `02_table_partitioning.sql`
- [ ] 执行 `03_data_retention.sql` - 数据清理
- [ ] 执行 `05_materialized_views.sql` - 物化视图
- [ ] 配置自动清理任务

---

## 性能监控

### 检查索引使用情况

```sql
SELECT 
    schemaname,
    tablename,
    indexname,
    idx_scan AS index_scans,
    idx_tup_read AS tuples_read
FROM pg_stat_user_indexes
WHERE tablename IN ('price_data', 'funding_rates')
ORDER BY idx_scan DESC;
```

### 检查 TimescaleDB 状态

```sql
-- 查看超表
SELECT * FROM timescaledb_information.hypertables;

-- 查看压缩统计
SELECT 
    hypertable_name,
    pg_size_pretty(before_compression_total_bytes) AS before_size,
    pg_size_pretty(after_compression_total_bytes) AS after_size,
    ROUND(100.0 * (1.0 - after_compression_total_bytes::numeric / before_compression_total_bytes), 2) AS compression_ratio
FROM timescaledb_information.compression_settings;

-- 查看连续聚合
SELECT * FROM timescaledb_information.continuous_aggregates;
```

### 检查分区状态

```sql
-- 查看所有分区
SELECT 
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS size
FROM pg_tables
WHERE tablename LIKE 'price_data_%' OR tablename LIKE 'funding_rates_%'
ORDER BY tablename;
```

---

## 总结

### 可以一起使用的方案

✅ **索引优化** + 任何其他方案
✅ **TimescaleDB** + 索引优化 + 连续聚合 + 数据归档
✅ **物化视图** + 索引优化 + 数据清理

### 不建议一起使用的方案

❌ **数据库分区** + **TimescaleDB**（功能重复）
❌ **数据清理（删除）** + **ML 训练数据**（会丢失数据）

### 推荐组合

**ML 训练数据：** 索引优化 + TimescaleDB + 连续聚合 + 数据归档

**生产环境：** 索引优化 + 数据库分区 + 数据清理 + 物化视图
