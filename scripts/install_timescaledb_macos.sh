#!/bin/bash

# TimescaleDB macOS 安装脚本

set -e

echo "🚀 开始安装 TimescaleDB（macOS 非 Docker 版本）..."
echo ""

# 检查 Homebrew
if ! command -v brew &> /dev/null; then
    echo "❌ 未找到 Homebrew，请先安装 Homebrew"
    echo "   安装命令：/bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
    exit 1
fi

echo "✅ Homebrew 已安装"

# 检查 PostgreSQL
if ! command -v psql &> /dev/null; then
    echo "📦 安装 PostgreSQL..."
    brew install postgresql@15
    brew services start postgresql@15
    echo "✅ PostgreSQL 已安装并启动"
else
    echo "✅ PostgreSQL 已安装"
    PSQL_VERSION=$(psql --version | awk '{print $3}' | cut -d. -f1)
    echo "   PostgreSQL 版本: $PSQL_VERSION"
fi

# 添加 TimescaleDB tap
echo ""
echo "📦 添加 TimescaleDB Homebrew tap..."
brew tap timescale/tap

# 安装 TimescaleDB
echo ""
echo "📦 安装 TimescaleDB..."
if brew list timescaledb &> /dev/null; then
    echo "⚠️  TimescaleDB 已安装，跳过安装步骤"
else
    brew install timescaledb
    echo "✅ TimescaleDB 已安装"
fi

# 配置 PostgreSQL
echo ""
echo "⚙️  配置 PostgreSQL..."

# 找到 PostgreSQL 配置目录
if [ -d "$(brew --prefix postgresql@15)/var" ]; then
    PGDATA="$(brew --prefix postgresql@15)/var"
    PG_VERSION="15"
elif [ -d "$(brew --prefix postgresql@14)/var" ]; then
    PGDATA="$(brew --prefix postgresql@14)/var"
    PG_VERSION="14"
elif [ -d "$(brew --prefix postgresql)/var" ]; then
    PGDATA="$(brew --prefix postgresql)/var"
    PG_VERSION="latest"
else
    echo "❌ 无法找到 PostgreSQL 数据目录"
    exit 1
fi

CONFIG_FILE="$PGDATA/postgresql.conf"

if [ ! -f "$CONFIG_FILE" ]; then
    echo "❌ 无法找到 PostgreSQL 配置文件: $CONFIG_FILE"
    exit 1
fi

# 检查是否已配置
if grep -q "shared_preload_libraries.*timescaledb" "$CONFIG_FILE"; then
    echo "⚠️  TimescaleDB 已在配置文件中，跳过配置"
else
    # 备份配置文件
    cp "$CONFIG_FILE" "$CONFIG_FILE.backup.$(date +%Y%m%d_%H%M%S)"
    
    # 添加配置
    if grep -q "shared_preload_libraries" "$CONFIG_FILE"; then
        # 如果已有 shared_preload_libraries，追加 timescaledb
        sed -i '' "s/^shared_preload_libraries = '\(.*\)'/shared_preload_libraries = '\1,timescaledb'/" "$CONFIG_FILE"
        sed -i '' "s/^shared_preload_libraries = \"\(.*\)\"/shared_preload_libraries = '\1,timescaledb'/" "$CONFIG_FILE"
        # 如果没有 timescaledb，添加
        if ! grep -q "timescaledb" "$CONFIG_FILE"; then
            sed -i '' "s/^shared_preload_libraries = '\(.*\)'/shared_preload_libraries = '\1,timescaledb'/" "$CONFIG_FILE"
        fi
    else
        # 如果不存在，添加新行
        echo "" >> "$CONFIG_FILE"
        echo "# TimescaleDB configuration" >> "$CONFIG_FILE"
        echo "shared_preload_libraries = 'timescaledb'" >> "$CONFIG_FILE"
    fi
    
    echo "✅ PostgreSQL 配置已更新"
fi

# 重启 PostgreSQL
echo ""
echo "🔄 重启 PostgreSQL 服务..."
brew services restart postgresql@$PG_VERSION || brew services restart postgresql
sleep 3

# 等待 PostgreSQL 启动
echo "⏳ 等待 PostgreSQL 启动..."
for i in {1..30}; do
    if pg_isready -h localhost -p 5432 &> /dev/null; then
        echo "✅ PostgreSQL 已就绪"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "❌ PostgreSQL 启动超时"
        exit 1
    fi
    sleep 1
done

# 创建数据库（如果不存在）
echo ""
echo "📊 检查数据库..."
if ! psql -lqt | cut -d \| -f 1 | grep -qw crypto_hunter; then
    echo "📦 创建数据库 crypto_hunter..."
    createdb crypto_hunter || psql -c "CREATE DATABASE crypto_hunter;"
    echo "✅ 数据库已创建"
else
    echo "✅ 数据库 crypto_hunter 已存在"
fi

# 启用 TimescaleDB 扩展
echo ""
echo "🔌 启用 TimescaleDB 扩展..."
if psql -d crypto_hunter -c "SELECT 1 FROM pg_extension WHERE extname = 'timescaledb';" | grep -q 1; then
    echo "⚠️  TimescaleDB 扩展已启用"
else
    psql -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;" || {
        echo "❌ 启用 TimescaleDB 扩展失败"
        echo "   请检查 PostgreSQL 日志以获取更多信息"
        exit 1
    }
    echo "✅ TimescaleDB 扩展已启用"
fi

# 验证安装
echo ""
echo "🔍 验证安装..."
VERSION=$(psql -d crypto_hunter -t -c "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';" | xargs)
if [ -n "$VERSION" ]; then
    echo "✅ TimescaleDB 版本: $VERSION"
else
    echo "❌ 无法获取 TimescaleDB 版本"
    exit 1
fi

echo ""
echo "🎉 TimescaleDB 安装完成！"
echo ""
echo "📝 下一步："
echo "   1. 更新 .env 文件："
echo "      DATABASE_URL=postgresql://postgres:postgres@localhost:5432/crypto_hunter"
echo ""
echo "   2. 执行初始化脚本："
echo "      psql -d crypto_hunter -f migrations/07_init_timescaledb.sql"
echo ""
echo "   3. 运行程序："
echo "      cargo run"
