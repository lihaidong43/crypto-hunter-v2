# 快速开始指南

本指南将帮助你快速搭建和运行 Crypto Hunter 套利平台。

## 前置要求

### 必需

- **Rust 1.70+**: [安装 Rust](https://www.rust-lang.org/tools/install)
- **PostgreSQL 13+**: [安装 PostgreSQL](https://www.postgresql.org/download/)

### 可选

- **Docker & Docker Compose**: 用于容器化部署
- **Telegram Bot Token**: 用于接收通知（[如何创建 Telegram Bot](https://core.telegram.org/bots/tutorial#obtain-your-bot-token)）

## 安装步骤

### 1. 克隆项目（如果从 Git 仓库）

```bash
git clone <repository-url>
cd crypto-hunter-v2
```

### 2. 编译项目

```bash
cargo build --release
```

### 3. 创建数据库

```bash
# 使用 psql 创建数据库
createdb crypto_hunter

# 或者使用 SQL
psql -U postgres -c "CREATE DATABASE crypto_hunter;"
```

### 4. 配置环境变量

复制环境变量示例文件：

```bash
cp .env.example .env
```

编辑 `.env` 文件，配置数据库连接和其他选项：

```env
# 数据库配置（必需）
DATABASE_URL=postgresql://user:password@localhost:5432/crypto_hunter

# Telegram 通知配置（可选）
TELEGRAM_BOT_TOKEN=your_bot_token_here
TELEGRAM_CHAT_ID=your_chat_id_here

# 日志级别
RUST_LOG=info

# API 端口
API_PORT=8080
```

### 5. 运行项目

```bash
# 开发模式
cargo run

# 生产模式（优化编译）
cargo run --release
```

首次运行时会自动创建数据库表结构。

## 验证安装

### 检查日志输出

如果一切正常，你应该看到类似的日志：

```
INFO crypto_hunter: Crypto Hunter - 数字货币交易所套利平台启动中...
INFO crypto_hunter::storage::database: 初始化数据库连接...
INFO crypto_hunter::storage::database: 数据库初始化完成
INFO crypto_hunter: 初始化交易所适配器...
INFO crypto_hunter: Binance适配器已加载
INFO crypto_hunter: OKX适配器已加载
INFO crypto_hunter: Bybit适配器已加载
INFO crypto_hunter: Gate.io适配器已加载
INFO crypto_hunter: 启动数据收集任务...
INFO crypto_hunter: 启动交易对收集任务（独立运行，间隔: 3600秒）...
INFO crypto_hunter: 启动实时数据收集任务（价格: 5秒，资金费率: 300秒）...
INFO crypto_hunter: 交易对收集任务已启动，间隔: 3600秒
INFO crypto_hunter: 开始初始同步交易对...
INFO crypto_hunter: 实时数据收集任务已启动（价格同步: 5秒，资金费率同步: 300秒）
INFO crypto_hunter: 交易对收集任务已就绪，等待下次同步（3600秒后）...
INFO crypto_hunter: 启动API服务器...
INFO crypto_hunter::api::server: API server starting on port 8080
INFO crypto_hunter::api::server: API server is running (placeholder implementation)
```

### 检查数据库表

连接到数据库验证表是否创建成功：

```bash
psql -U postgres -d crypto_hunter -c "\dt"
```

应该看到以下表：
- `trading_pairs`
- `price_data`
- `funding_rates`
- `arbitrage_opportunities`
- `volatility_events`

### 测试 API（如果已实现）

```bash
curl http://localhost:8080/api/v1/health
```

## 配置说明

### 数据收集频率

默认配置在 `src/config/mod.rs` 中：

- `pair_sync_interval_seconds`: 3600（1小时）- 交易对同步间隔
- `price_sync_interval_seconds`: 5（5秒）- 价格同步间隔
- `funding_rate_sync_interval_seconds`: 300（5分钟）- 资金费率同步间隔

可以通过环境变量或配置文件修改。

### 监控阈值

- `spread_threshold`: 1.0（1%）- 价差阈值
- `basis_volatility_threshold`: 0.5（0.5%）- 基差波动阈值
- `funding_rate_volatility_threshold`: 0.1（0.1%）- 资金费率波动阈值

### 启用/禁用交易所

在配置中设置：

```toml
[exchanges]
binance = { enabled = true }
okx = { enabled = true }
bybit = { enabled = false }  # 禁用 Bybit
gateio = { enabled = true }
```

## Telegram 通知设置

### 1. 创建 Telegram Bot

1. 在 Telegram 中搜索 `@BotFather`
2. 发送 `/newbot` 命令
3. 按照提示设置 bot 名称和用户名
4. 获取 Bot Token

### 2. 获取 Chat ID

1. 在 Telegram 中搜索 `@userinfobot`
2. 发送任意消息
3. 获取你的 Chat ID（数字）

### 3. 配置

在 `.env` 文件中设置：

```env
TELEGRAM_BOT_TOKEN=123456789:ABCdefGHIjklMNOpqrsTUVwxyz
TELEGRAM_CHAT_ID=123456789
```

## Docker 部署（可选）

### 使用 Docker Compose

创建 `docker-compose.yml`：

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: crypto_hunter
      POSTGRES_USER: user
      POSTGRES_PASSWORD: password
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

  crypto-hunter:
    build: .
    environment:
      DATABASE_URL: postgresql://user:password@postgres:5432/crypto_hunter
      TELEGRAM_BOT_TOKEN: ${TELEGRAM_BOT_TOKEN}
      TELEGRAM_CHAT_ID: ${TELEGRAM_CHAT_ID}
    depends_on:
      - postgres
    ports:
      - "8080:8080"

volumes:
  postgres_data:
```

运行：

```bash
docker-compose up -d
```

### 构建 Docker 镜像

创建 `Dockerfile`：

```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/crypto-hunter /usr/local/bin/
CMD ["crypto-hunter"]
```

构建：

```bash
docker build -t crypto-hunter .
```

## 故障排查

### 问题 1: 数据库连接失败

**错误**: `Error connecting to database`

**解决方案**:
1. 检查 PostgreSQL 是否运行: `pg_isready`
2. 验证 `DATABASE_URL` 格式是否正确
3. 检查数据库用户权限

### 问题 2: 交易所 API 调用失败

**错误**: `Failed to fetch data from exchange`

**解决方案**:
1. 检查网络连接
2. 验证交易所 API 是否可访问
3. 检查是否有 API 频率限制
4. 查看日志了解具体错误信息

### 问题 3: Telegram 通知不工作

**解决方案**:
1. 验证 Bot Token 是否正确
2. 验证 Chat ID 是否正确（必须是数字）
3. 确认 bot 未被禁用
4. 检查日志中的错误信息

### 问题 4: 编译错误

**错误**: `cargo build` 失败

**解决方案**:
1. 更新 Rust 版本: `rustup update`
2. 清理构建缓存: `cargo clean`
3. 检查依赖是否正确: `cargo update`

## 性能优化建议

### 1. 使用 Release 模式

```bash
cargo run --release
```

### 2. 数据库优化

- 考虑使用 TimescaleDB 扩展进行时序数据优化
- 定期清理历史数据
- 调整 PostgreSQL 配置参数

### 3. 调整数据收集频率

根据实际需求调整收集频率，避免过度调用 API：

```rust
// 在配置中调整
price_sync_interval_seconds: 10  // 增加到10秒
```

### 4. 限制监控的交易对数量

默认监控所有交易对，可以修改代码只监控主要交易对。

## 下一步

- 阅读 [架构文档](./ARCHITECTURE.md) 了解系统设计
- 查看 [README.md](../README.md) 了解完整功能
- 根据需求扩展交易所适配器
- 实现 API 服务端点
- 添加监控和告警

## 获取帮助

如果遇到问题：

1. 查看日志输出: `RUST_LOG=debug cargo run`
2. 检查 GitHub Issues
3. 提交新的 Issue 描述问题

## 常见问题

### Q: 支持哪些交易所？

A: 目前 Binance 已完全实现，OKX、Bybit、Gate.io 接口已设计但待完善实现。

### Q: 如何添加新交易所？

A: 参考 `src/exchange/binance.rs` 实现 `ExchangeAdapter` trait，详见架构文档。

### Q: 数据存储在哪里？

A: 所有数据存储在 PostgreSQL 数据库中。价格数据是时序数据，建议使用 TimescaleDB 扩展。

### Q: 系统资源占用如何？

A: 内存占用约 50-100MB，CPU 占用取决于数据收集频率和监控的交易对数量。

### Q: 可以用于生产环境吗？

A: 当前版本是基础实现，建议在生产环境使用前进行充分测试和优化。