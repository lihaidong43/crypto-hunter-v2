# 机器学习训练数据策略

## 需求分析

对于机器学习训练数据，有以下特殊需求：

1. **数据完整性**：需要保留所有历史数据，不能删除
2. **数据导出**：需要能够高效导出大量历史数据
3. **数据格式**：可能需要转换为 CSV、Parquet、JSON 等格式
4. **时间范围**：可能需要查询数月甚至数年的数据
5. **数据采样**：可能需要按时间间隔采样（如每小时、每天）
6. **特征工程**：可能需要计算技术指标、统计特征等

## 推荐方案组合

### 核心方案：TimescaleDB + 数据导出工具

**为什么选择 TimescaleDB：**
1. ✅ **自动压缩**：历史数据自动压缩，节省 90%+ 存储空间
2. ✅ **保留策略**：可以配置保留所有数据，不自动删除
3. ✅ **连续聚合**：预计算小时/日级别聚合，加速训练数据准备
4. ✅ **高效查询**：即使查询数年数据，性能依然优秀
5. ✅ **数据导出**：支持高效的批量数据导出

### 辅助方案：物化视图 + 数据导出脚本

**为什么需要物化视图：**
1. ✅ **预聚合数据**：小时/日级别聚合，减少训练数据量
2. ✅ **特征工程**：可以预计算技术指标（MA、RSI等）
3. ✅ **快速导出**：从聚合表导出比从原始表导出快得多

---

## 实施步骤

### 阶段 1: 安装和配置 TimescaleDB（推荐）

#### 1.1 安装 TimescaleDB

```bash
# macOS
brew install timescaledb

# Ubuntu/Debian
sudo apt install timescaledb-2-postgresql-14

# 在数据库中启用
psql -U postgres -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
```

#### 1.2 转换为超表（Hypertable）

```sql
-- 将 price_data 转换为超表
SELECT create_hypertable('price_data', 'timestamp', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- 将 funding_rates 转换为超表
SELECT create_hypertable('funding_rates', 'timestamp',
    chunk_time_interval => INTERVAL '7 days',
    if_not_exists => TRUE
);
```

#### 1.3 配置压缩策略（保留所有数据，但压缩旧数据）

```sql
-- 压缩超过 7 天的 price_data 数据（节省空间，但保留数据）
SELECT add_compression_policy('price_data', INTERVAL '7 days');

-- 压缩超过 30 天的 funding_rates 数据
SELECT add_compression_policy('funding_rates', INTERVAL '30 days');

-- 重要：不配置保留策略，保留所有数据
-- 如果需要删除数据，手动执行 DELETE
```

#### 1.4 创建连续聚合（用于训练数据准备）

```sql
-- 每小时价格聚合（用于训练）
CREATE MATERIALIZED VIEW price_data_hourly
WITH (timescaledb.continuous) AS
SELECT 
    time_bucket('1 hour', timestamp) AS bucket,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(bid_price) AS avg_bid_price,
    AVG(ask_price) AS avg_ask_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points,
    STDDEV(last_price) AS price_stddev
FROM price_data
GROUP BY bucket, exchange, symbol, market_type;

-- 每日价格聚合（用于长期趋势训练）
CREATE MATERIALIZED VIEW price_data_daily
WITH (timescaledb.continuous) AS
SELECT 
    time_bucket('1 day', timestamp) AS bucket,
    exchange,
    symbol,
    market_type,
    AVG(last_price) AS avg_price,
    MIN(last_price) AS min_price,
    MAX(last_price) AS max_price,
    AVG(volume_24h) AS avg_volume,
    COUNT(*) AS data_points
FROM price_data
GROUP BY bucket, exchange, symbol, market_type;

-- 配置自动刷新
SELECT add_continuous_aggregate_policy('price_data_hourly',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);
```

### 阶段 2: 创建数据导出工具

#### 2.1 导出为 CSV

```sql
-- 导出最近 1 年的价格数据
COPY (
    SELECT 
        timestamp,
        exchange,
        symbol,
        market_type,
        last_price,
        bid_price,
        ask_price,
        volume_24h
    FROM price_data
    WHERE timestamp > NOW() - INTERVAL '1 year'
    ORDER BY timestamp, exchange, symbol
) TO '/tmp/price_data_1year.csv' WITH CSV HEADER;
```

#### 2.2 导出为 Parquet（使用 Python）

```python
# export_to_parquet.py
import psycopg2
import pandas as pd
import pyarrow.parquet as pq

# 连接数据库
conn = psycopg2.connect("postgresql://postgres@localhost:5432/crypto_hunter")

# 查询数据
query = """
    SELECT 
        timestamp,
        exchange,
        symbol,
        market_type,
        last_price,
        bid_price,
        ask_price,
        volume_24h
    FROM price_data
    WHERE timestamp > NOW() - INTERVAL '1 year'
    ORDER BY timestamp, exchange, symbol
"""

df = pd.read_sql(query, conn)

# 转换为 Parquet
df.to_parquet('price_data_1year.parquet', compression='snappy')

conn.close()
```

### 阶段 3: 创建特征工程视图

```sql
-- 创建技术指标视图（用于训练）
CREATE MATERIALIZED VIEW price_data_features AS
SELECT 
    bucket,
    exchange,
    symbol,
    market_type,
    avg_price,
    -- 移动平均
    AVG(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 23 PRECEDING AND CURRENT ROW
    ) AS ma_24h,
    AVG(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 167 PRECEDING AND CURRENT ROW
    ) AS ma_7d,
    -- 价格变化率
    (avg_price - LAG(avg_price, 1) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    )) / LAG(avg_price, 1) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket
    ) * 100 AS price_change_pct,
    -- 波动率
    STDDEV(avg_price) OVER (
        PARTITION BY exchange, symbol, market_type 
        ORDER BY bucket 
        ROWS BETWEEN 23 PRECEDING AND CURRENT ROW
    ) AS volatility_24h,
    avg_volume
FROM price_data_hourly
ORDER BY bucket, exchange, symbol;
```

---

## 数据导出最佳实践

### 1. 按时间范围导出

```sql
-- 导出特定时间范围的数据
COPY (
    SELECT * FROM price_data
    WHERE timestamp BETWEEN '2024-01-01' AND '2024-12-31'
    ORDER BY timestamp, exchange, symbol
) TO '/tmp/price_data_2024.csv' WITH CSV HEADER;
```

### 2. 按交易所和交易对导出

```sql
-- 导出特定交易对的数据
COPY (
    SELECT * FROM price_data
    WHERE exchange = 'binance'
    AND symbol = 'BTCUSDT'
    ORDER BY timestamp
) TO '/tmp/binance_btcusdt.csv' WITH CSV HEADER;
```

### 3. 导出聚合数据（减少数据量）

```sql
-- 导出小时级别聚合数据（用于训练）
COPY (
    SELECT * FROM price_data_hourly
    WHERE bucket > NOW() - INTERVAL '1 year'
    ORDER BY bucket, exchange, symbol
) TO '/tmp/price_data_hourly_1year.csv' WITH CSV HEADER;
```

### 4. 批量导出多个交易对

```sql
-- 导出所有交易对的数据（按交易对分组）
DO $$
DECLARE
    pair RECORD;
BEGIN
    FOR pair IN 
        SELECT DISTINCT exchange, symbol 
        FROM price_data
    LOOP
        EXECUTE format('
            COPY (
                SELECT * FROM price_data
                WHERE exchange = %L AND symbol = %L
                ORDER BY timestamp
            ) TO %L WITH CSV HEADER',
            pair.exchange,
            pair.symbol,
            '/tmp/' || pair.exchange || '_' || pair.symbol || '.csv'
        );
    END LOOP;
END $$;
```

---

## 存储空间估算

### 原始数据（未压缩）

- 价格数据：每条记录约 200 字节
  - 1 年：3,456,000 条 × 200 字节 = 690 MB
  - 5 年：约 3.5 GB
  
- 资金费率：每条记录约 150 字节
  - 1 年：18,000 条 × 150 字节 = 2.7 MB
  - 5 年：约 13.5 MB

### TimescaleDB 压缩后

- 压缩率：通常 90%+
- 1 年价格数据：约 70 MB（压缩后）
- 5 年价格数据：约 350 MB（压缩后）

### 聚合数据（用于训练）

- 小时级别：1 年约 8,760 条/交易对
- 日级别：1 年约 365 条/交易对
- 数据量减少 99%+

---

## 推荐配置

### 小型训练项目（< 1 年数据）

1. ✅ TimescaleDB（可选，但推荐）
2. ✅ 物化视图（小时级别聚合）
3. ✅ 直接导出 CSV/Parquet

### 中型训练项目（1-3 年数据）

1. ✅ **TimescaleDB（必需）**
2. ✅ 物化视图（小时/日级别）
3. ✅ 特征工程视图
4. ✅ 批量导出工具

### 大型训练项目（> 3 年数据）

1. ✅ **TimescaleDB（必需）**
2. ✅ 物化视图（多时间粒度）
3. ✅ 特征工程视图
4. ✅ 数据管道（ETL）
5. ✅ 分布式存储（S3、HDFS等）

---

## 实施优先级

### 立即实施（今天）

1. ✅ 安装 TimescaleDB
2. ✅ 转换为超表
3. ✅ 配置压缩策略
4. ✅ 创建连续聚合

### 短期实施（本周）

1. ✅ 创建特征工程视图
2. ✅ 开发数据导出脚本
3. ✅ 测试数据导出性能

### 长期优化（本月）

1. ✅ 自动化数据管道
2. ✅ 数据质量检查
3. ✅ 数据版本管理

---

## 注意事项

1. **数据完整性**：不要配置自动删除策略，保留所有数据
2. **压缩策略**：配置压缩以节省空间，但保留查询能力
3. **导出性能**：使用 COPY 命令或批量查询，避免逐条查询
4. **数据格式**：根据训练框架选择合适格式（CSV、Parquet、JSON）
5. **数据采样**：使用聚合视图减少数据量，加速训练

---

## 总结

**对于机器学习训练数据，强烈推荐：**

1. **TimescaleDB** - 核心方案
   - 保留所有历史数据
   - 自动压缩节省空间
   - 高效查询和导出

2. **连续聚合** - 辅助方案
   - 预计算小时/日级别数据
   - 减少训练数据量
   - 加速数据准备

3. **数据导出工具** - 必需
   - 支持多种格式
   - 批量导出
   - 自动化流程
