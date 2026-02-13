# 依赖和编译问题解决方案

## 当前已知问题

### 问题 1: Rust 版本要求

某些依赖（如 `home` crate）需要较新的 Rust 版本。如果遇到编译错误，请确保使用最新的 Rust 稳定版：

```bash
rustup update stable
rustup default stable
```

### 问题 2: 依赖版本兼容性

如果遇到依赖版本冲突，可以尝试：

1. **更新 Rust 工具链**:
   ```bash
   rustup update
   ```

2. **清理 Cargo 缓存**:
   ```bash
   cargo clean
   rm -rf ~/.cargo/registry/cache
   ```

3. **锁定依赖版本**（如果必要）:
   编辑 `Cargo.toml`，使用更保守的版本号。

### 问题 3: SQLx 离线模式

如果数据库连接有问题，SQLx 可能需要离线模式。安装 SQLx CLI：

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

然后在项目根目录运行：

```bash
# 设置数据库 URL
export DATABASE_URL=postgresql://user:password@localhost:5432/crypto_hunter

# 准备离线文件
sqlx prepare --database-url $DATABASE_URL
```

这将生成 `.sqlx/query-*.json` 文件，允许在没有数据库连接的情况下编译。

## 替代方案

如果某些依赖有问题，可以考虑：

### 1. 使用更简单的数据库客户端

如果 `deadpool-postgres` 有问题，可以直接使用 `sqlx::PgPool`：

```rust
// 替代 deadpool-postgres
use sqlx::{PgPool, postgres::PgPoolOptions};

let pool = PgPoolOptions::new()
    .max_connections(20)
    .connect(&database_url)
    .await?;
```

### 2. 简化配置管理

如果 `config` crate 有问题，可以直接使用环境变量：

```rust
use std::env;

let database_url = env::var("DATABASE_URL")
    .expect("DATABASE_URL must be set");
```

### 3. 可选功能

某些功能可以标记为可选，例如 Telegram 通知。在 `Cargo.toml` 中：

```toml
[dependencies]
teloxide = { version = "0.12", optional = true }

[features]
default = []
telegram = ["teloxide"]
```

然后使用 `cargo build --no-default-features` 跳过可选依赖。

## 最小化依赖版本

如果需要最小化依赖，可以使用以下简化版本：

```toml
[dependencies]
# HTTP客户端
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"

# 数据库（直接使用 sqlx）
sqlx = { version = "0.7", features = ["runtime-tokio-native-tls", "postgres", "chrono"] }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 错误处理
anyhow = "1.0"

# 数值计算
rust_decimal = { version = "1.33", features = ["serde-float"] }
rust_decimal_macros = "1.33"

# 工具
uuid = { version = "1.6", features = ["v4", "serde"] }
dashmap = "5.5"
```

## 推荐的 Rust 版本

- **最低要求**: Rust 1.70+
- **推荐**: Rust 1.75+（最新稳定版）

检查版本：

```bash
rustc --version
```

## 获取帮助

如果问题仍然存在：

1. 检查 Rust 版本是否符合要求
2. 查看完整的错误信息
3. 尝试最小化依赖版本
4. 提交 Issue 并提供：
   - Rust 版本 (`rustc --version`)
   - Cargo 版本 (`cargo --version`)
   - 完整的错误信息
   - 操作系统信息