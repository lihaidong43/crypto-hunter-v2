# 交易所 Symbol 格式说明

本文档详细说明每个交易所的 symbol 格式要求，用于确保 symbol 格式转换的正确性。

## 格式总结

| 交易所 | 现货格式 | 期货格式 | 示例（现货） | 示例（期货） |
|--------|---------|---------|--------------|--------------|
| **Binance** | `BASEQUOTE` | `BASEQUOTE` | `BTCUSDT` | `BTCUSDT` |
| **OKX** | `BASE-QUOTE` | `BASE-QUOTE-SWAP` | `BTC-USDT` | `BTC-USDT-SWAP` |
| **Bybit** | `BASEQUOTE` | `BASEQUOTE` | `BTCUSDT` | `BTCUSDT` |
| **Gate.io** | `BASE_QUOTE` | `BASE_QUOTE` | `BTC_USDT` | `BTC_USDT` |
| **Bitget** | `BASEQUOTE` | `BASEQUOTE` | `BTCUSDT` | `BTCUSDT` |

## 详细说明

### 1. Binance

**API 端点：**
- 现货: `/api/v3/ticker/24hr?symbol=BTCUSDT`
- 期货: `/fapi/v1/ticker/24hr?symbol=BTCUSDT`

**格式：**
- 现货和期货都使用无分隔符格式：`BASEQUOTE`
- 示例：`BTCUSDT`, `ETHUSDT`, `1000SATSUSDT`

**特点：**
- 无分隔符
- 直接拼接 base 和 quote
- 支持数字前缀（如 `1000SATSUSDT`）

### 2. OKX

**API 端点：**
- 现货: `/api/v5/market/ticker?instId=BTC-USDT`
- 期货: `/api/v5/market/ticker?instId=BTC-USDT-SWAP`

**格式：**
- 现货：`BASE-QUOTE`（用 `-` 分隔）
- 期货：`BASE-QUOTE-SWAP`（用 `-` 分隔，末尾有 `-SWAP`）

**示例：**
- 现货：`BTC-USDT`, `ETH-USDT`
- 期货：`BTC-USDT-SWAP`, `ETH-USDT-SWAP`

**特点：**
- 使用 `-` 作为分隔符
- 期货需要添加 `-SWAP` 后缀

### 3. Bybit

**API 端点：**
- 现货: `/v5/market/tickers?category=spot&symbol=BTCUSDT`
- 期货: `/v5/market/tickers?category=linear&symbol=BTCUSDT`

**格式：**
- 现货和期货都使用无分隔符格式：`BASEQUOTE`
- 示例：`BTCUSDT`, `ETHUSDT`

**特点：**
- 无分隔符
- 直接拼接 base 和 quote
- 与 Binance 格式相同

### 4. Gate.io

**API 端点：**
- 现货: `/api/v4/spot/tickers?currency_pair=BTC_USDT`
- 期货: `/api/v4/futures/usdt/tickers?contract=BTC_USDT`

**格式：**
- 现货和期货都使用下划线分隔格式：`BASE_QUOTE`
- 示例：`BTC_USDT`, `ETH_USDT`

**特点：**
- 使用 `_` 作为分隔符
- 现货和期货格式相同

### 5. Bitget

**API 端点：**
- 现货: `/api/v2/public/symbols` (获取列表，ticker API 可能不可用)
- 期货: `/api/v2/mix/market/ticker?symbol=BTCUSDT&productType=USDT-FUTURES`

**格式：**
- 现货和期货都使用无分隔符格式：`BASEQUOTE`
- 示例：`BTCUSDT`, `ETHUSDT`

**特点：**
- 无分隔符
- 直接拼接 base 和 quote
- 与 Binance/Bybit 格式相同

## Symbol 转换逻辑

系统使用 `normalize_symbol_for_exchange` 函数进行格式转换：

1. **提取 base 和 quote**：从任意格式的 symbol 中提取基础资产和报价资产
   - 支持 OKX 格式（`-` 分隔）
   - 支持 Gate.io 格式（`_` 分隔）
   - 支持 Binance/Bybit/Bitget 格式（无分隔符）

2. **转换为目标格式**：
   - **OKX**: 现货 `BASE-QUOTE`，期货 `BASE-QUOTE-SWAP`
   - **Gate.io**: `BASE_QUOTE`
   - **Binance/Bybit/Bitget**: `BASEQUOTE`

## 常见问题

### 问题 1: 数字前缀的 symbol

某些 symbol 有数字前缀，如 `1000SATSUSDT`、`1000FLOKIUSDT`。

**处理方式：**
- Binance/Bybit/Bitget: 直接使用，如 `1000SATSUSDT`
- OKX: 转换为 `1000SATS-USDT` 或 `1000SATS-USDT-SWAP`
- Gate.io: 转换为 `1000SATS_USDT`

### 问题 2: 特殊字符

某些 symbol 可能包含特殊字符，如 `-`、`_`。

**处理方式：**
- `extract_base_quote` 函数会优先识别分隔符（`-` 或 `_`）
- 如果无法识别，会尝试从末尾查找常见的 quote（如 `USDT`、`USDC`）

### 问题 3: 格式转换失败

如果无法解析 symbol 格式，系统会：
1. 记录警告日志
2. 返回原始 symbol（可能导致 API 调用失败）

## 验证方法

可以通过以下方式验证格式转换是否正确：

1. **查看日志**：转换后的 symbol 会在错误日志中显示
2. **API 测试**：直接调用各交易所 API，使用转换后的 symbol
3. **数据库查询**：查看 `trading_pairs` 表中存储的原始 symbol 格式

## 代码位置

- Symbol 转换函数：`src/models/exchange.rs::normalize_symbol_for_exchange`
- Base/Quote 提取函数：`src/models/exchange.rs::extract_base_quote`
- 使用位置：`src/collector/snapshot_collector.rs::collect_symbol_snapshot_full`
