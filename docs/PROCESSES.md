# Crypto Hunter 进程文档

本文档描述了 Crypto Hunter 系统中的所有可执行进程及其参数。

> **维护提示**：当进程参数发生变化时，请同步更新此文档。

---

## 目录

1. [核心服务进程](#核心服务进程)
   - [crypto-hunter](#crypto-hunter-主进程)
   - [pair-collector](#pair-collector-交易对收集服务)
   - [arbitrage-monitor](#arbitrage-monitor-套利监控服务)
2. [工具进程](#工具进程)
   - [query_arbitrage](#query_arbitrage-套利查询工具)
   - [historical-collector](#historical-collector-历史数据采集)
   - [clean-data](#clean-data-数据清理工具)
   - [ws-test](#ws-test-websocket-测试工具)
   - [symbol-tester](#symbol-tester-交易对格式测试)
3. [环境变量配置](#环境变量配置)
4. [数据库策略](#数据库策略)

---

## 核心服务进程

### crypto-hunter (主进程)

**功能**：主服务进程，启动 WebSocket 数据采集和 REST API 快照采集。

**运行命令**：
```bash
RUST_LOG=info ./target/release/crypto-hunter
```

**参数**：无命令行参数，通过环境变量配置。

**依赖服务**：
- PostgreSQL + TimescaleDB
- 需要先运行 `pair-collector` 同步交易对

**输出**：
- WebSocket 实时数据写入 `market_snapshots` 表
- 日志输出到 stdout

---

### pair-collector (交易对收集服务)

**功能**：定期从各交易所同步交易对信息，是其他服务的前置依赖。

**运行命令**：
```bash
RUST_LOG=info ./target/release/pair-collector [OPTIONS]
```

**参数**：

| 参数 | 短参数 | 类型 | 默认值 | 说明 |
|------|--------|------|--------|------|
| `--interval` | `-i` | u64 | 3600 | 同步间隔（秒） |
| `--force` | `-f` | bool | false | 强制立即同步，忽略上次同步时间 |

**示例**：
```bash
# 强制立即同步
./target/release/pair-collector --force

# 设置同步间隔为 2 小时
./target/release/pair-collector --interval 7200
```

**过滤配置**（通过代码配置，默认值）：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `min_exchange_count` | 2 | 交易对至少在 N 个交易所存在 |
| `allowed_quote_currencies` | ["USDT"] | 允许的计价货币 |
| `futures_only` | false | 是否只收集期货 |
| `spot_only` | false | 是否只收集现货 |

---

### arbitrage-monitor (套利监控服务)

**功能**：独立的套利监控进程，从数据库读取快照数据分析套利机会。

**运行命令**：
```bash
RUST_LOG=info ./target/release/arbitrage-monitor
```

**参数**：无命令行参数。

**特点**：
- 只读数据库，不触发 HTTP 采集
- 独立于主进程运行

---

## 工具进程

### query_arbitrage (套利查询工具)

**功能**：查询和分析套利机会的命令行工具。

**运行命令**：
```bash
./target/release/query_arbitrage <COMMAND> [OPTIONS]
```

**命令**：

| 命令 | 说明 | 示例 |
|------|------|------|
| `analyze <SYMBOL>` | 分析指定交易对 | `query_arbitrage analyze BTCUSDT` |
| `find [OPTIONS]` | 查找套利机会 | `query_arbitrage find --min-spread 0.5` |
| `stats` | 显示数据统计 | `query_arbitrage stats` |

**find 命令选项**：
- `--min-spread <PERCENT>` - 最小价差百分比
- `--exchange <NAME>` - 限定交易所
- `--limit <N>` - 返回结果数量

---

### historical-collector (历史数据采集)

**功能**：批量获取历史 K 线数据。

**运行命令**：
```bash
RUST_LOG=info ./target/release/historical-collector [OPTIONS]
```

**参数**：

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--symbols` | String | 全部 | 交易对列表，逗号分隔 |
| `--start-time` | String | 今年1月1日 | 开始时间，ISO 8601 格式 |
| `--end-time` | String | 当前时间 | 结束时间，ISO 8601 格式 |
| `--interval` | String | "1h" | K线间隔：1m, 5m, 1h, 1d |

**示例**：
```bash
# 获取 BTC 和 ETH 过去一个月的小时 K 线
./target/release/historical-collector \
  --symbols BTCUSDT,ETHUSDT \
  --start-time 2026-01-01T00:00:00Z \
  --end-time 2026-02-01T00:00:00Z \
  --interval 1h
```

---

### clean-data (数据清理工具)

**功能**：清理数据库中的历史采集数据。

**运行命令**：
```bash
./target/release/clean-data [OPTIONS]
```

**参数**：

| 参数 | 短参数 | 类型 | 默认值 | 说明 |
|------|--------|------|--------|------|
| `--mode` | | enum | all | 清理模式：all/snapshots/klines |
| `--start-time` | | String | 无 | 开始时间，ISO 8601 格式 |
| `--end-time` | | String | 无 | 结束时间，ISO 8601 格式 |
| `--exchange` | | String | 全部 | 限定交易所 |
| `--symbol` | | String | 全部 | 限定交易对 |
| `--yes` | `-y` | bool | false | 跳过确认提示 |
| `--dry-run` | | bool | false | 仅显示统计，不执行删除 |

**示例**：
```bash
# 预览将删除的数据
./target/release/clean-data --dry-run

# 删除指定时间范围的数据
./target/release/clean-data \
  --start-time 2026-01-01T00:00:00Z \
  --end-time 2026-01-15T00:00:00Z \
  --yes
```

---

### ws-test (WebSocket 测试工具)

**功能**：测试单个交易所的 WebSocket 连接和数据推送。

**运行命令**：
```bash
RUST_LOG=info ./target/release/ws-test [OPTIONS]
```

**参数**：

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `--exchange` | enum | 必填 | 交易所：binance/okx/bybit/gateio/bitget |
| `--market` | enum | futures | 市场类型：spot/futures/all |
| `--symbols` | String | BTCUSDT | 交易对列表，逗号分隔 |
| `--seconds` | u64 | 30 | 测试运行时间（秒） |
| `--connect-timeout` | u64 | 20 | 连接超时（秒） |

**示例**：
```bash
# 测试 Binance 期货 WebSocket
./target/release/ws-test --exchange binance --market futures --seconds 60

# 测试 OKX 现货多个交易对
./target/release/ws-test \
  --exchange okx \
  --market spot \
  --symbols BTCUSDT,ETHUSDT,SOLUSDT \
  --seconds 30
```

---

### symbol-tester (交易对格式测试)

**功能**：测试交易对符号在不同交易所的格式转换。

**运行命令**：
```bash
./target/release/symbol-tester [OPTIONS]
```

**参数**：

| 参数 | 短参数 | 类型 | 默认值 | 说明 |
|------|--------|------|--------|------|
| `--symbol` | `-s` | String | 必填 | 输入交易对符号 |
| `--market` | | String | futures | 市场类型：spot/futures |
| `--exchange` | | String[] | 全部 | 限定测试的交易所 |

**示例**：
```bash
# 测试 BTCUSDT 在所有交易所的格式
./target/release/symbol-tester --symbol BTCUSDT

# 只测试 Binance 和 OKX
./target/release/symbol-tester --symbol BTCUSDT --exchange binance --exchange okx
```

---

## 环境变量配置

所有进程都可以通过环境变量进行配置，支持 `.env` 文件。

### 数据库配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `DATABASE__URL` | postgresql://postgres@localhost:5432/crypto_hunter | 数据库连接 URL |

### 日志配置

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `RUST_LOG` | error | 日志级别：trace/debug/info/warn/error |

### 网络代理

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `HTTP_PROXY` | 无 | HTTP 代理地址 |
| `HTTPS_PROXY` | 无 | HTTPS 代理地址 |

---

## 数据库策略

### TimescaleDB 自动任务

| 任务 | 执行间隔 | 配置 | 说明 |
|------|----------|------|------|
| 压缩策略 | 每 12 小时 | 3 天后压缩 | 自动压缩 3 天前的数据 |
| 保留策略 | 每天 | 7 天保留 | 自动删除 7 天前的数据 |

### 查看任务状态

```sql
SELECT job_id, application_name, schedule_interval, next_start, config
FROM timescaledb_information.jobs 
WHERE hypertable_name = 'market_snapshots';
```

### 手动触发任务

```sql
-- 手动执行压缩
CALL run_job(<compression_job_id>);

-- 手动执行清理
CALL run_job(<retention_job_id>);
```

---

## 常用运维命令

### 启动所有服务

```bash
# 1. 先同步交易对
./target/release/pair-collector --force

# 2. 启动主服务（包含 WebSocket 采集）
RUST_LOG=info ./target/release/crypto-hunter

# 3. （可选）启动套利监控
RUST_LOG=info ./target/release/arbitrage-monitor
```

### 查看数据状态

```bash
# 查询套利统计
./target/release/query_arbitrage stats

# 测试 WebSocket 连接
./target/release/ws-test --exchange binance --seconds 10
```

### 数据维护

```bash
# 预览清理数据
./target/release/clean-data --dry-run

# 执行清理
./target/release/clean-data --yes
```

---

*最后更新：2026-02-03*
