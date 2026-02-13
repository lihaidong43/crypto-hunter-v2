# Crypto Hunter - 数字货币交易所套利平台

一个高性能的数字货币交易所套利发现平台，使用Rust语言实现，支持多交易所套利机会的实时监控和发现。

## 功能特性

### 1. 套利发现

#### 1.1 基础数据收集
- ✅ 定时同步交易所交易对（支持自定义频率）
- ✅ 实时同步价格数据（支持自定义频率）
- ✅ 计算开仓价差和清仓价差
- ✅ 计算净资金费率
- ✅ 记录基差、资金费率、结算周期、资金费上下限、24小时交易量

#### 1.2 实时监控价格波动
- ✅ Telegram推送大于或小于指定价差的交易对
- ✅ 推送基差或资金费率1/5/60分钟波动较大的交易对
- ✅ API查询接口

#### 1.3 非功能性需求
- ✅ 高性能数据库存储和查询（PostgreSQL）
- ✅ 实时数据获取（低延迟设计）
- ✅ 任务分离架构（低频和实时任务独立运行，优化性能）
- ✅ 价差公式：`[2 × (Price_A - Price_B)] / (Price_A + Price_B) × 100%`

### 2. 套利任务生命周期管理（设计完成，暂不实现）

架构设计已完成，支持：
- 配置套利任务（开仓价差、清仓价差、交易量等）
- 执行套利任务
- 任务状态管理

### 3. 技术架构

- **语言**: Rust
- **异步运行时**: Tokio
- **数据库**: PostgreSQL
- **通知**: Telegram Bot
- **架构**: 模块化设计，易于扩展

## 支持的交易所

- ✅ Binance（币安）
- ⏳ OKX（欧易）- 接口已设计，待实现
- ⏳ Bybit - 接口已设计，待实现
- ⏳ Gate.io - 接口已设计，待实现

所有交易所通过统一的 `ExchangeAdapter` trait 实现，易于扩展新的交易所。

## 套利类型

- **现期套利**: 现货 vs 期货
- **期期套利**: 期货 vs 期货（跨交易所）
- **期现套利**: 期货 vs 现货

## 项目结构

```
crypto-hunter-v2/
├── src/
│   ├── api/              # API服务层
│   │   ├── handlers.rs   # API处理器
│   │   └── server.rs     # API服务器
│   ├── arbitrage/        # 套利计算模块
│   │   ├── calculator.rs # 套利计算器（价差、清仓价差、净资金费率）
│   │   └── detector.rs   # 套利机会检测器
│   ├── config/           # 配置管理
│   │   └── mod.rs        # 配置文件结构
│   ├── exchange/         # 交易所适配器层
│   │   ├── adapter.rs    # 交易所适配器trait
│   │   ├── binance.rs    # Binance适配器
│   │   ├── okx.rs        # OKX适配器
│   │   ├── bybit.rs      # Bybit适配器
│   │   └── gateio.rs     # Gate.io适配器
│   ├── models/           # 数据模型
│   │   ├── arbitrage.rs  # 套利相关模型
│   │   ├── exchange.rs   # 交易所相关模型
│   │   └── price.rs      # 价格相关模型
│   ├── monitor/          # 监控模块
│   │   ├── notification.rs # 通知服务（Telegram）
│   │   └── volatility.rs   # 价格波动监控
│   ├── storage/          # 存储层
│   │   ├── database.rs   # 数据库连接和表结构
│   │   └── repository.rs # 数据访问层
│   └── main.rs           # 主程序入口
├── Cargo.toml            # 项目依赖
├── .env.example          # 环境变量示例
├── config.example.toml   # 配置文件示例
├── docs/                 # 文档目录
│   └── PROCESSES.md      # 进程文档（参数说明）
└── README.md             # 项目文档
```

## 文档

- **[进程文档](docs/PROCESSES.md)** - 所有可执行进程的详细参数说明
- **[README.md](README.md)** - 项目概述和快速开始

## 快速开始

### 1. 环境要求

- Rust 1.75+（推荐最新稳定版）
- PostgreSQL 13+
- 可选的: Telegram Bot Token（用于推送通知）

**注意**: 如果遇到编译问题，请确保 Rust 版本足够新。运行 `rustup update` 更新到最新版本。

### 2. 安装依赖

```bash
cargo build
```

### 3. 配置

复制 `.env.example` 为 `.env` 并修改配置：

```bash
cp .env.example .env
```

编辑 `.env` 文件：

```env
DATABASE_URL=postgresql://user:password@localhost:5432/crypto_hunter
TELEGRAM_BOT_TOKEN=your_telegram_bot_token
TELEGRAM_CHAT_ID=your_chat_id
```

### 4. 初始化数据库

```bash
# 创建数据库
createdb crypto_hunter

# 运行程序会自动初始化表结构
cargo run
```

### 5. 运行

**如果需要在代理环境下运行（可选）：**

```bash
# 设置代理环境变量（根据实际情况调整）
export http_proxy=http://127.0.0.1:59502
export https_proxy=http://127.0.0.1:59502
export all_proxy=socks5://127.0.0.1:59502

# 运行程序（reqwest 会自动使用上述代理设置）
cargo run --release
```

**直接运行（无需代理）：**

```bash
cargo run --release
```

## 配置说明

### 数据收集频率配置

在代码中或配置文件中可以设置：

- `pair_sync_interval_seconds`: 交易对同步间隔（默认3600秒，1小时）
- `price_sync_interval_seconds`: 价格同步间隔（默认5秒）
- `funding_rate_sync_interval_seconds`: 资金费率同步间隔（默认300秒，5分钟）

### 监控阈值配置

- `spread_threshold`: 价差阈值（默认1.0%，即1%）
- `basis_volatility_threshold`: 基差波动阈值（默认0.5%）
- `funding_rate_volatility_threshold`: 资金费率波动阈值（默认0.1%）

## API接口

API服务将在 `http://localhost:8080` 启动（默认端口）。

计划中的API端点：

- `GET /api/v1/arbitrage/opportunities` - 查询套利机会
- `GET /api/v1/arbitrage/opportunities/{id}` - 查询特定套利机会
- `GET /api/v1/prices/{exchange}/{symbol}` - 查询价格数据
- `GET /api/v1/funding-rates/{exchange}/{symbol}` - 查询资金费率
- `GET /api/v1/volatility-events` - 查询波动事件

## 扩展新交易所

1. 在 `src/exchange/` 目录下创建新的适配器文件（如 `huobi.rs`）
2. 实现 `ExchangeAdapter` trait
3. 在 `src/exchange/mod.rs` 中导出新适配器
4. 在 `src/main.rs` 中注册新适配器

示例：

```rust
// src/exchange/huobi.rs
use crate::exchange::adapter::ExchangeAdapter;
use async_trait::async_trait;

pub struct HuobiAdapter {
    // ...
}

#[async_trait]
impl ExchangeAdapter for HuobiAdapter {
    // 实现所有必需的方法
}
```

## 数据库设计

### 主要表结构

1. **trading_pairs** - 交易对信息
2. **price_data** - 价格数据（时序数据）
3. **funding_rates** - 资金费率
4. **arbitrage_opportunities** - 套利机会
5. **volatility_events** - 价格波动事件

### 性能优化

**任务分离架构：**
- 交易对收集任务独立运行（1小时一次），不影响实时任务
- 价格同步和资金费率同步在独立任务中运行，确保及时性
- 避免低频耗时任务阻塞高频实时任务

**数据库优化：**
- 价格数据表按时间戳建立索引
- 考虑使用TimescaleDB进行时序数据优化
- 定期清理历史数据

**性能提升：**
- 价格同步延迟从可能延迟5-30秒降低到稳定的5秒间隔
- 更好的任务隔离和故障隔离

## 开发计划

### 已完成
- ✅ 项目架构设计
- ✅ 交易所适配器框架（Binance已实现）
- ✅ 套利计算模块
- ✅ 监控和通知模块
- ✅ 数据库存储层
- ✅ 任务分离架构优化（低频和实时任务独立运行）

### 待实现
- ⏳ 完善其他交易所适配器（OKX, Bybit, Gate.io）
- ⏳ 实现API服务（使用axum或类似框架）
- ⏳ 实现套利任务生命周期管理
- ⏳ 完整的单元测试和集成测试
- ⏳ DEX支持（Uniswap, PancakeSwap等）
- ⏳ 压力测试和进一步性能优化

## 注意事项

1. **API限制**: 注意各交易所的API调用频率限制
2. **数据准确性**: 价格数据有延迟，套利前需验证
3. **风险提示**: 套利有风险，需谨慎操作
4. **合规性**: 确保遵守相关法律法规

## 许可证

MIT License

## 贡献

欢迎提交Issue和Pull Request！