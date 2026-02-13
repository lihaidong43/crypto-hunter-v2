# TimescaleDB 安装指南（macOS）

## 方法 1: 使用 Homebrew 安装 PostgreSQL + TimescaleDB（推荐）

### 步骤 1: 安装 PostgreSQL

```bash
# 安装 PostgreSQL（如果还没有安装）
brew install postgresql@14
# 或者
brew install postgresql@15
# 或者
brew install postgresql@16
```

### 步骤 2: 安装 TimescaleDB

TimescaleDB 在 macOS 上需要通过 PostgreSQL 扩展安装，而不是独立的 formula。

```bash
# 方法 A: 使用 TimescaleDB 官方安装脚本（推荐）
# 首先确保 PostgreSQL 已安装并运行
brew services start postgresql@14  # 或你的 PostgreSQL 版本

# 下载并运行 TimescaleDB 安装脚本
curl -sSL https://packagecloud.io/install/repositories/timescale/timescaledb/script.deb.sh | sudo bash

# 但这是 Debian/Ubuntu 的脚本，macOS 需要使用不同的方法
```

### 方法 2: 使用 Docker（最简单，推荐）

如果直接安装困难，使用 Docker 是最简单的方法：

```bash
# 1. 安装 Docker Desktop（如果还没有）
# 下载地址：https://www.docker.com/products/docker-desktop

# 2. 运行带 TimescaleDB 的 PostgreSQL 容器
docker run -d \
  --name timescaledb \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=crypto_hunter \
  -v timescaledb_data:/var/lib/postgresql/data \
  timescale/timescaledb:latest-pg14

# 3. 连接到容器中的数据库
docker exec -it timescaledb psql -U postgres -d crypto_hunter

# 4. 在数据库中启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;
```

### 方法 3: 从源码编译（高级用户）

如果必须使用本地 PostgreSQL，可以从源码编译：

```bash
# 1. 安装依赖
brew install cmake git

# 2. 克隆 TimescaleDB 源码
git clone https://github.com/timescale/timescaledb.git
cd timescaledb

# 3. 编译（需要指定 PostgreSQL 路径）
./bootstrap -DREGRESS_CHECKS=OFF
cd build && make

# 4. 安装
sudo make install

# 5. 在数据库中启用扩展
psql -U postgres -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
```

---

## 方法 4: 使用替代方案（如果安装困难）

如果 TimescaleDB 安装困难，可以使用以下替代方案：

### 方案 A: 数据库分区 + 物化视图

```bash
# 1. 索引优化（必须）
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql

# 2. 数据库分区
psql -U postgres -d crypto_hunter -f migrations/02_table_partitioning.sql

# 3. 物化视图
psql -U postgres -d crypto_hunter -f migrations/05_materialized_views.sql

# 4. 数据归档（不删除，保留所有数据）
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql
```

这个组合可以提供类似的性能，虽然不如 TimescaleDB 强大，但足够使用。

---

## 检查安装

### 检查 PostgreSQL 版本

```bash
psql --version
```

### 检查 TimescaleDB 是否已安装

```sql
-- 连接到数据库
psql -U postgres -d crypto_hunter

-- 检查扩展
SELECT * FROM pg_extension WHERE extname = 'timescaledb';

-- 如果已安装，会显示版本信息
SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';
```

---

## 推荐方案

### 对于开发环境

**推荐：使用 Docker**

```bash
# 启动 TimescaleDB 容器
docker run -d \
  --name timescaledb \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=crypto_hunter \
  -v timescaledb_data:/var/lib/postgresql/data \
  timescale/timescaledb:latest-pg14

# 更新 DATABASE_URL
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/crypto_hunter"
```

### 对于生产环境

**推荐：使用替代方案（数据库分区 + 物化视图）**

如果生产环境安装 TimescaleDB 困难，使用组合方案：
- 索引优化
- 数据库分区
- 物化视图
- 数据归档

---

## 故障排查

### 问题 1: 找不到 timescaledb 扩展

**错误：** `ERROR: could not open extension control file`

**解决：**
1. 确认 TimescaleDB 已正确安装
2. 检查 PostgreSQL 的 `sharedir` 路径
3. 确认扩展文件在正确位置

```sql
-- 查看 PostgreSQL 配置
SHOW sharedir;

-- 检查扩展文件
\dx timescaledb
```

### 问题 2: 版本不匹配

**错误：** `extension "timescaledb" version "x.x.x" does not match`

**解决：**
- 确保 TimescaleDB 版本与 PostgreSQL 版本兼容
- 参考 TimescaleDB 官方文档的版本兼容性表

---

## 下一步

安装成功后，执行：

```bash
# 执行 TimescaleDB 配置
psql -U postgres -d crypto_hunter -f migrations/06_timescaledb_ml_setup.sql
```
