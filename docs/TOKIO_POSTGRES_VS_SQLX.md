# tokio_postgres vs sqlx 比较

## 概述

当前项目使用了 `deadpool-postgres`（基于 `tokio_postgres`），但 `Cargo.toml` 中也包含了 `sqlx` 依赖（未使用）。本文档比较这两个库的差异，特别关注当前遇到的序列化问题。

## 核心差异

| 特性 | tokio_postgres | sqlx |
|------|---------------|------|
| **异步支持** | ✅ 是 | ✅ 是 |
| **编译时 SQL 检查** | ❌ 否（运行时检查） | ✅ 是（可选特性） |
| **类型安全** | ⚠️ 运行时检查 | ✅ 编译时检查 |
| **连接池管理** | ❌ 需要 `deadpool-postgres` | ✅ 内置 `PgPool` |
| **ORM 功能** | ❌ 无 | ⚠️ 轻量级（macros） |
| **灵活性** | ✅ 底层控制 | ⚠️ 更高级抽象 |
| **依赖大小** | 较小 | 较大 |
| **性能** | ✅ 优秀（底层） | ✅ 优秀（优化） |

## 当前问题：序列化错误

### tokio_postgres 的问题

当前遇到的 `error serializing parameter 2` 问题（实际是参数 $3，`rate_str`）：

1. **问题根源**：
   - `tokio_postgres` 可能无法将 `String` 类型直接序列化为 `NUMERIC` 类型
   - 参数类型推断在运行时进行，容易出错
   - `Option<String>` 到 `NUMERIC` 的转换存在问题

2. **当前代码**：
   ```rust
   let rate_str = rate.rate.to_string();  // Decimal -> String
   client.execute(
       "INSERT INTO funding_rates (rate) VALUES ($1)",
       &[&rate_str]  // String -> NUMERIC 转换失败
   )
   ```

3. **限制**：
   - `tokio_postgres` 不直接支持 `rust_decimal::Decimal` 的 `ToSql` 实现
   - 需要手动转换为 `String`，但 `String` -> `NUMERIC` 转换可能失败
   - 错误索引报告存在偏移问题（issue #742）

### sqlx 的优势

1. **更好的类型支持**：
   - 原生支持 `rust_decimal::Decimal` 类型
   - 可以直接使用 `Decimal` 类型，无需转换为 `String`
   - 编译时类型检查

2. **代码示例**：
   ```rust
   use sqlx::{PgPool, Postgres, Executor};
   use rust_decimal::Decimal;
   
   sqlx::query!(
       "INSERT INTO funding_rates (rate) VALUES ($1)",
       rate.rate  // 直接使用 Decimal，无需转换
   )
   .execute(&pool)
   .await?;
   ```

3. **内置连接池**：
   ```rust
   use sqlx::postgres::PgPoolOptions;
   
   let pool = PgPoolOptions::new()
       .max_connections(20)
       .connect(&database_url)
       .await?;
   ```

## 详细对比

### 1. 类型安全

**tokio_postgres**：
- 运行时类型检查
- 错误在运行时发现
- 需要手动处理类型转换

**sqlx**：
- 编译时类型检查（使用 `query!` macro）
- 错误在编译时发现
- 更好的类型推导

### 2. 连接池

**tokio_postgres**：
- 需要额外的 `deadpool-postgres` 依赖
- 手动配置连接池

**sqlx**：
- 内置 `PgPool` 和 `PgPoolOptions`
- 更简单的配置

### 3. Decimal/NUMERIC 支持

**tokio_postgres**：
- 不直接支持 `rust_decimal::Decimal`
- 需要转换为 `String`，但可能失败
- 当前遇到的序列化问题

**sqlx**：
- 原生支持 `rust_decimal::Decimal`
- 可以直接使用 `Decimal` 类型
- 更好的类型映射

### 4. 错误处理

**tokio_postgres**：
- 错误索引报告存在偏移（issue #742）
- 运行时错误信息可能不准确

**sqlx**：
- 更清晰的错误信息
- 编译时错误检查

### 5. 性能

**tokio_postgres**：
- ✅ 底层库，性能优秀
- ✅ 支持查询流水线（Query Pipeline），可以提高性能约 20%
- ✅ 更细粒度的控制
- ✅ 较小的运行时开销
- ⚠️ 需要手动管理连接池

**sqlx**：
- ✅ 性能优化良好
- ❌ 目前不支持查询流水线（在高并发场景下可能影响性能）
- ✅ 内置连接池，有优化
- ⚠️ 编译时宏展开可能增加编译时间（但不影响运行时性能）
- ⚠️ 依赖大小较大

## 迁移建议

### 如果迁移到 sqlx

**优势**：
1. ✅ 解决当前的 `NUMERIC` 序列化问题
2. ✅ 更好的类型安全（编译时检查）
3. ✅ 更简单的代码（无需手动类型转换）
4. ✅ 内置连接池（移除 `deadpool-postgres` 依赖）

**劣势**：
1. ⚠️ 需要重构现有代码
2. ⚠️ 编译时间可能增加（宏展开）
3. ⚠️ 依赖大小增加

**迁移步骤**：
1. 移除 `deadpool-postgres` 依赖
2. 使用 `sqlx::PgPool` 替代 `deadpool_postgres::Pool`
3. 使用 `sqlx::query!` 或 `sqlx::query` 替代 `client.execute`
4. 直接使用 `Decimal` 类型，无需转换为 `String`
5. 更新所有数据库操作代码

### 如果继续使用 tokio_postgres

**优势**：
1. ✅ 无需重构
2. ✅ 更小的依赖
3. ✅ 更细粒度的控制

**劣势**：
1. ❌ 当前的序列化问题可能难以解决
2. ❌ 需要手动处理类型转换
3. ❌ 运行时错误检查

**可能的解决方案**：
1. 尝试使用 `rust_decimal` 的 `ToSql` trait（如果支持）
2. 将 `NUMERIC` 字段改为 `TEXT` 类型（不推荐）
3. 使用自定义的类型转换函数

## 针对当前项目的建议

考虑到：
1. 当前遇到 `NUMERIC` 序列化问题
2. `Cargo.toml` 中已经包含了 `sqlx` 依赖（未使用）
3. `save_price_data` 和 `save_funding_rate` 都使用相同的模式（可能有相同问题）

**建议**：
1. **短期**：继续调试 `tokio_postgres` 的序列化问题
2. **长期**：考虑迁移到 `sqlx`，以获得更好的类型支持和更简单的代码

## 代码示例对比

### tokio_postgres（当前）

```rust
use deadpool_postgres::Pool;
use rust_decimal::Decimal;

pub async fn save_funding_rate(&self, rate: &FundingRate) -> Result<()> {
    let client = self.pool.get().await?;
    let rate_str = rate.rate.to_string();  // Decimal -> String
    
    client.execute(
        "INSERT INTO funding_rates (rate) VALUES ($1)",
        &[&rate_str]  // String -> NUMERIC 可能失败
    ).await?;
    
    Ok(())
}
```

### sqlx（建议）

```rust
use sqlx::PgPool;
use rust_decimal::Decimal;

pub async fn save_funding_rate(&self, rate: &FundingRate) -> Result<()> {
    sqlx::query!(
        "INSERT INTO funding_rates (rate) VALUES ($1)",
        rate.rate  // 直接使用 Decimal
    )
    .execute(&self.pool)
    .await?;
    
    Ok(())
}
```

## 性能详细对比

### 1. 查询流水线（Query Pipeline）

**tokio_postgres**：
- ✅ **支持查询流水线**
- 查询流水线可以在等待上一个查询的响应时，发送下一个查询
- 在高并发场景下可以提高性能约 **20%**
- 适用于需要处理大量并发查询的场景

**sqlx**：
- ❌ **不支持查询流水线**
- 必须等待上一个查询完成才能发送下一个查询
- 在高并发场景下可能性能略低于 `tokio_postgres`

### 2. 运行时性能

**tokio_postgres**：
- ✅ 底层库，运行时开销小
- ✅ 直接与 PostgreSQL 协议交互，性能优秀
- ✅ 细粒度控制，可以优化特定场景

**sqlx**：
- ✅ 性能优化良好，运行时开销较小
- ✅ 内置连接池有优化
- ⚠️ 宏展开在编译时完成，不影响运行时性能

### 3. 编译时性能

**tokio_postgres**：
- ✅ 编译时间短
- ✅ 依赖较小

**sqlx**：
- ⚠️ 编译时间较长（特别是使用 `query!` macro 时）
- ⚠️ 需要连接到数据库进行编译时检查（或使用离线模式）
- ⚠️ 依赖大小较大

### 4. 连接池性能

**tokio_postgres**：
- ⚠️ 需要 `deadpool-postgres` 等第三方连接池
- ✅ `deadpool-postgres` 性能良好
- ⚠️ 需要手动配置和优化

**sqlx**：
- ✅ 内置连接池 `PgPool`，性能优化良好
- ✅ 更简单的配置
- ✅ 内置连接池管理优化

### 5. 性能基准测试

**注意**：具体的性能表现会因以下因素而异：
- 查询复杂度
- 并发量
- 网络延迟
- 数据库服务器性能
- 应用程序的具体实现

**一般场景**：
- 两者性能都很优秀，差异通常不明显
- `tokio_postgres` 在需要查询流水线的高并发场景下可能有优势
- `sqlx` 在开发体验和类型安全方面有优势

**高并发场景**：
- `tokio_postgres` 支持查询流水线，可能性能更好（约 20% 提升）
- `sqlx` 内置连接池优化，性能也很好

**开发效率**：
- `sqlx` 的编译时检查可以提高开发效率，减少运行时错误
- `tokio_postgres` 需要更多的运行时调试

## 性能总结

| 性能指标 | tokio_postgres | sqlx | 胜者 |
|---------|---------------|------|------|
| **运行时性能** | ✅ 优秀 | ✅ 优秀 | 🟰 相当 |
| **查询流水线** | ✅ 支持（+20%）| ❌ 不支持 | 🏆 tokio_postgres |
| **连接池性能** | ✅ 良好（deadpool）| ✅ 良好（内置）| 🟰 相当 |
| **编译时间** | ✅ 短 | ⚠️ 长 | 🏆 tokio_postgres |
| **依赖大小** | ✅ 小 | ⚠️ 大 | 🏆 tokio_postgres |
| **开发效率** | ⚠️ 一般 | ✅ 高 | 🏆 sqlx |
| **类型安全** | ⚠️ 运行时 | ✅ 编译时 | 🏆 sqlx |

## 结论

### 性能角度

- **tokio_postgres**：
  - ✅ 在高并发场景下（支持查询流水线）可能性能更好
  - ✅ 编译时间短，依赖小
  - ✅ 更细粒度的性能控制
  - 适合：**高性能、高并发、需要查询流水线的场景**

- **sqlx**：
  - ✅ 运行时性能也很优秀
  - ✅ 内置连接池优化
  - ⚠️ 不支持查询流水线（在高并发场景下可能略慢）
  - 适合：**开发效率优先、类型安全优先的场景**

### 选择建议

**选择 tokio_postgres 如果**：
1. 需要查询流水线的高并发场景
2. 对编译时间敏感
3. 需要细粒度的性能控制
4. 项目对性能有极高要求

**选择 sqlx 如果**：
1. 更注重开发效率和类型安全
2. 需要解决当前的序列化问题（如 `NUMERIC` 类型）
3. 希望减少运行时错误
4. 项目性能要求不是极致（大多数场景下性能差异不明显）

### 针对当前项目

考虑到：
1. 当前遇到 `NUMERIC` 序列化问题（`sqlx` 可以解决）
2. 项目性能要求（需要评估是否需要查询流水线）
3. 开发效率和维护成本

**建议**：
- 如果不需要查询流水线的高并发性能：**推荐迁移到 `sqlx`**
- 如果需要查询流水线的高并发性能：**继续使用 `tokio_postgres`，但需要解决序列化问题**
