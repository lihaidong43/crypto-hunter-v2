# 交易对收集服务 (Pair Collector)

独立的交易对收集服务，定期从各交易所同步交易对信息到数据库。

## 功能特性

- ✅ **独立进程**：与主服务分离，不影响实时监控性能
- ✅ **配置化同步间隔**：默认 4 小时，可通过配置文件或命令行参数调整
- ✅ **持久化同步时间**：系统重启后自动恢复上次同步时间
- ✅ **强制同步**：支持命令行参数强制立即同步
- ✅ **智能等待**：启动时根据上次同步时间自动计算等待时间

## 使用方法

### 基本使用

```bash
# 使用默认配置（4小时同步一次）
cargo run --bin pair-collector

# 或编译后运行
./target/release/pair-collector
```

### 命令行参数

```bash
# 自定义同步间隔（秒）
cargo run --bin pair-collector -- --interval 7200

# 强制立即同步（忽略上次同步时间）
cargo run --bin pair-collector -- --force

# 组合使用
cargo run --bin pair-collector -- --interval 3600 --force
```

### 配置说明

同步间隔可通过以下方式配置（优先级从高到低）：

1. **命令行参数** `--interval`：直接指定秒数
2. **环境变量** `DATA_COLLECTION__PAIR_SYNC_INTERVAL_SECONDS`：配置文件中的间隔
3. **默认值**：14400 秒（4 小时）

### 环境变量配置

```bash
# 数据库连接
export DATABASE__URL="postgresql://postgres@localhost:5432/crypto_hunter"

# 同步间隔（秒）
export DATA_COLLECTION__PAIR_SYNC_INTERVAL_SECONDS=14400

# 交易所配置
export EXCHANGES__0__NAME="binance"
export EXCHANGES__0__ENABLED=true
# ... 其他交易所
```

### 作为系统服务运行

#### systemd 服务文件示例

创建 `/etc/systemd/system/crypto-hunter-pair-collector.service`：

```ini
[Unit]
Description=Crypto Hunter Pair Collector Service
After=network.target postgresql.service

[Service]
Type=simple
User=crypto-hunter
WorkingDirectory=/opt/crypto-hunter
ExecStart=/opt/crypto-hunter/pair-collector
Restart=always
RestartSec=10
Environment="RUST_LOG=info"
Environment="DATABASE__URL=postgresql://postgres@localhost:5432/crypto_hunter"

[Install]
WantedBy=multi-user.target
```

启用并启动服务：

```bash
sudo systemctl enable crypto-hunter-pair-collector
sudo systemctl start crypto-hunter-pair-collector
sudo systemctl status crypto-hunter-pair-collector
```

## 工作原理

### 同步时间管理

服务会在数据库中记录每次同步的时间：

- **表名**：`sync_times`
- **服务名**：`pair_collector`
- **字段**：`last_sync_time`（最后同步时间）

### 启动逻辑

1. **检查上次同步时间**：
   - 如果存在记录，计算距离下次同步的剩余时间
   - 如果已超过间隔时间，立即同步
   - 如果未到时间，等待至下次同步时间

2. **强制同步模式**：
   - 使用 `--force` 参数时，忽略上次同步时间，立即执行同步

3. **定期同步**：
   - 按照配置的间隔，定期执行同步任务

### 数据库表结构

```sql
CREATE TABLE IF NOT EXISTS sync_times (
    service_name VARCHAR(50) PRIMARY KEY,
    last_sync_time TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
```

## 日志输出示例

```
2024-01-15T10:00:00.000Z INFO  交易对收集服务启动...
2024-01-15T10:00:00.100Z INFO  初始化数据库连接...
2024-01-15T10:00:00.200Z INFO  数据库初始化完成
2024-01-15T10:00:00.300Z INFO  初始化交易所适配器...
2024-01-15T10:00:00.400Z INFO  Binance适配器已加载
2024-01-15T10:00:00.500Z INFO  OKX适配器已加载
2024-01-15T10:00:00.600Z INFO  同步间隔: 14400 秒 (4 小时)
2024-01-15T10:00:00.700Z INFO  上次同步时间: 2024-01-15 06:00:00 UTC，距离下次同步还有 7200 秒（120 分钟）
2024-01-15T12:00:00.000Z INFO  等待 0 秒（0 分钟）后开始首次同步...
2024-01-15T12:00:00.100Z INFO  ========== 开始同步交易对 ==========
2024-01-15T12:00:01.000Z INFO  Binance 现货交易对数量: 1500
2024-01-15T12:00:01.100Z INFO  Binance 现货交易对同步成功
...
2024-01-15T12:00:10.000Z INFO  ========== 交易对同步完成，耗时: 9.85 秒 ==========
2024-01-15T12:00:10.100Z INFO  同步时间已更新: 2024-01-15 12:00:00 UTC
2024-01-15T12:00:10.200Z INFO  开始定期同步任务（每 14400 秒执行一次）
```

## 注意事项

1. **数据库连接**：确保数据库服务正在运行且可访问
2. **网络连接**：需要能够访问各交易所的 API
3. **频率控制**：同步间隔过短可能导致 API 限流
4. **资源消耗**：首次同步可能耗时较长，建议在低峰期执行

## 故障排查

### 无法连接到数据库

检查数据库连接字符串和网络连接：

```bash
psql "postgresql://postgres@localhost:5432/crypto_hunter" -c "SELECT 1"
```

### 同步失败

查看详细日志：

```bash
RUST_LOG=debug cargo run --bin pair-collector
```

### 查看同步时间记录

```sql
SELECT * FROM sync_times WHERE service_name = 'pair_collector';
```

### 手动重置同步时间

```sql
DELETE FROM sync_times WHERE service_name = 'pair_collector';
```

删除后，服务将在下次启动时立即执行同步。
