# TimescaleDB macOS 安装指南（非 Docker）

## 方法 1: 使用 Homebrew（推荐，最简单）

### 前提条件

1. 已安装 Homebrew
2. 已安装 PostgreSQL（通过 Homebrew）

### 安装步骤

#### 步骤 1: 安装 PostgreSQL（如果还没有）

```bash
# 安装 PostgreSQL 15（推荐）
brew install postgresql@15

# 或者安装 PostgreSQL 14
brew install postgresql@14

# 启动 PostgreSQL 服务
brew services start postgresql@15
```

#### 步骤 2: 添加 TimescaleDB Homebrew Tap

```bash
# 添加 TimescaleDB 的 Homebrew tap
brew tap timescale/tap
```

#### 步骤 3: 安装 TimescaleDB

```bash
# 安装 TimescaleDB（对应 PostgreSQL 15）
brew install timescaledb

# 或者指定 PostgreSQL 版本
brew install timescaledb --with-postgresql@15
```

#### 步骤 4: 配置 PostgreSQL 加载 TimescaleDB

```bash
# 找到 PostgreSQL 配置文件位置
psql --version
# 或者
brew --prefix postgresql@15

# 编辑 postgresql.conf
# 文件位置通常是：/opt/homebrew/var/postgresql@15/postgresql.conf
# 或者：/usr/local/var/postgresql@15/postgresql.conf

# 添加以下行：
shared_preload_libraries = 'timescaledb'
```

**或者使用命令行添加：**

```bash
# 找到配置文件
PG_CONFIG=$(brew --prefix postgresql@15)/bin/pg_config
PGDATA=$(brew --prefix postgresql@15)/var

# 编辑配置文件
echo "shared_preload_libraries = 'timescaledb'" >> $PGDATA/postgresql.conf
```

#### 步骤 5: 重启 PostgreSQL

```bash
# 重启 PostgreSQL 服务
brew services restart postgresql@15
```

#### 步骤 6: 在数据库中启用 TimescaleDB 扩展

```bash
# 连接到数据库
psql -U postgres -d crypto_hunter

# 或者如果使用默认用户
psql -d crypto_hunter

# 在数据库中执行
CREATE EXTENSION IF NOT EXISTS timescaledb;
```

#### 步骤 7: 验证安装

```bash
# 检查扩展版本
psql -d crypto_hunter -c "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';"
```

---

## 方法 2: 从源码编译（高级用户）

### 前提条件

```bash
# 安装编译工具
brew install cmake git
```

### 安装步骤

#### 步骤 1: 克隆 TimescaleDB 源码

```bash
git clone https://github.com/timescale/timescaledb.git
cd timescaledb
```

#### 步骤 2: 编译

```bash
# 创建构建目录
./bootstrap -DREGRESS_CHECKS=OFF

# 编译
cd build
make
```

#### 步骤 3: 安装

```bash
# 安装到系统
sudo make install
```

#### 步骤 4: 配置 PostgreSQL

```bash
# 找到 PostgreSQL 配置目录
PGDATA=$(brew --prefix postgresql@15)/var

# 编辑 postgresql.conf
echo "shared_preload_libraries = 'timescaledb'" >> $PGDATA/postgresql.conf
```

#### 步骤 5: 重启并启用

```bash
# 重启 PostgreSQL
brew services restart postgresql@15

# 启用扩展
psql -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
```

---

## 方法 3: 使用预编译二进制包

### 步骤 1: 下载二进制包

访问 TimescaleDB 官方下载页面：
https://docs.timescale.com/install/latest/self-hosted/

选择 macOS 版本和 PostgreSQL 版本。

### 步骤 2: 安装

```bash
# 解压并安装
tar -xzf timescaledb-*.tar.gz
cd timescaledb-*
./install.sh
```

### 步骤 3: 配置和启用

同方法 1 的步骤 4-7。

---

## 故障排查

### 问题 1: 找不到 timescaledb 扩展

**错误：** `ERROR: could not open extension control file`

**解决：**
```bash
# 检查 TimescaleDB 是否已安装
brew list timescaledb

# 检查 PostgreSQL 是否能找到扩展
psql -d crypto_hunter -c "SHOW shared_preload_libraries;"

# 应该显示：timescaledb
```

### 问题 2: PostgreSQL 启动失败

**错误：** `FATAL: could not load library "timescaledb"`

**解决：**
```bash
# 检查 TimescaleDB 库文件位置
find $(brew --prefix) -name "timescaledb*.so" 2>/dev/null

# 检查 PostgreSQL 配置
psql -d crypto_hunter -c "SHOW shared_preload_libraries;"

# 确保配置正确
echo "shared_preload_libraries = 'timescaledb'" >> $(brew --prefix postgresql@15)/var/postgresql.conf
```

### 问题 3: 版本不匹配

**错误：** `extension "timescaledb" version "x.x.x" does not match`

**解决：**
```bash
# 检查 PostgreSQL 版本
psql --version

# 检查 TimescaleDB 版本
brew info timescaledb

# 确保版本兼容
# TimescaleDB 2.x 支持 PostgreSQL 12-16
```

---

## 验证安装

### 完整验证脚本

```bash
#!/bin/bash

echo "1. 检查 PostgreSQL 版本："
psql --version

echo ""
echo "2. 检查 TimescaleDB 是否安装："
brew list timescaledb 2>/dev/null || echo "TimescaleDB 未通过 Homebrew 安装"

echo ""
echo "3. 检查 PostgreSQL 配置："
psql -d crypto_hunter -c "SHOW shared_preload_libraries;" 2>/dev/null || echo "无法连接到数据库"

echo ""
echo "4. 检查 TimescaleDB 扩展："
psql -d crypto_hunter -c "SELECT extname, extversion FROM pg_extension WHERE extname = 'timescaledb';" 2>/dev/null || echo "扩展未启用"

echo ""
echo "5. 测试 TimescaleDB 功能："
psql -d crypto_hunter -c "SELECT create_hypertable('test_table', 'time');" 2>/dev/null || echo "无法创建超表（可能表不存在）"
```

---

## 推荐安装流程

### 快速安装（推荐）

```bash
# 1. 安装 PostgreSQL（如果还没有）
brew install postgresql@15
brew services start postgresql@15

# 2. 添加 TimescaleDB tap
brew tap timescale/tap

# 3. 安装 TimescaleDB
brew install timescaledb

# 4. 配置 PostgreSQL
PGDATA=$(brew --prefix postgresql@15)/var
echo "shared_preload_libraries = 'timescaledb'" >> $PGDATA/postgresql.conf

# 5. 重启 PostgreSQL
brew services restart postgresql@15

# 6. 启用扩展
psql -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"

# 7. 验证
psql -d crypto_hunter -c "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';"
```

---

## 与 Docker 版本的对比

| 特性 | Homebrew 安装 | Docker 安装 |
|------|--------------|------------|
| 安装难度 | ⭐⭐ 中等 | ⭐ 简单 |
| 性能 | ⭐⭐⭐⭐⭐ 原生性能 | ⭐⭐⭐⭐ 容器性能 |
| 管理 | ⭐⭐⭐ 需要手动管理 | ⭐⭐⭐⭐⭐ 容器管理 |
| 隔离性 | ⭐⭐ 系统级别 | ⭐⭐⭐⭐⭐ 完全隔离 |
| 推荐场景 | 生产环境 | 开发/测试环境 |

---

## 注意事项

1. **PostgreSQL 版本兼容性**
   - TimescaleDB 2.x 支持 PostgreSQL 12-16
   - 确保 PostgreSQL 版本在支持范围内

2. **配置文件位置**
   - Homebrew PostgreSQL 配置文件通常在：`$(brew --prefix postgresql@15)/var/postgresql.conf`
   - 数据目录通常在：`$(brew --prefix postgresql@15)/var`

3. **权限问题**
   - 某些操作可能需要 `sudo`
   - 确保 PostgreSQL 用户有足够权限

4. **端口冲突**
   - 如果 Docker 也在运行，确保端口不冲突
   - 默认 PostgreSQL 端口：5432

---

## 下一步

安装完成后，执行：

```bash
# 执行 TimescaleDB 初始化脚本
psql -U postgres -d crypto_hunter -f migrations/07_init_timescaledb.sql
```

然后更新 `.env` 文件：

```
DATABASE_URL=postgresql://postgres@localhost:5432/crypto_hunter
```

（注意：本地安装通常不需要密码）
