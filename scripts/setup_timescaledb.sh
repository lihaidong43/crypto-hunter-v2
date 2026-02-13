#!/bin/bash

# TimescaleDB 安装脚本

set -e

echo "🚀 开始设置 TimescaleDB..."

# 检查 Docker 是否运行
if ! docker info > /dev/null 2>&1; then
    echo "❌ Docker 未运行，请先启动 Docker Desktop"
    exit 1
fi

# 检查是否已有容器运行
if docker ps -a | grep -q crypto_hunter_timescaledb; then
    echo "⚠️  发现已存在的 TimescaleDB 容器"
    read -p "是否删除并重新创建？(y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "🗑️  删除旧容器..."
        docker stop crypto_hunter_timescaledb 2>/dev/null || true
        docker rm crypto_hunter_timescaledb 2>/dev/null || true
    else
        echo "✅ 使用现有容器"
        docker start crypto_hunter_timescaledb
        exit 0
    fi
fi

# 启动 TimescaleDB 容器
echo "🐳 启动 TimescaleDB 容器..."
docker-compose up -d timescaledb

# 等待数据库就绪
echo "⏳ 等待数据库就绪..."
sleep 5

max_attempts=30
attempt=0
while [ $attempt -lt $max_attempts ]; do
    if docker exec crypto_hunter_timescaledb pg_isready -U postgres > /dev/null 2>&1; then
        echo "✅ 数据库已就绪"
        break
    fi
    attempt=$((attempt + 1))
    echo "   尝试 $attempt/$max_attempts..."
    sleep 1
done

if [ $attempt -eq $max_attempts ]; then
    echo "❌ 数据库启动超时"
    exit 1
fi

# 启用 TimescaleDB 扩展
echo "📦 启用 TimescaleDB 扩展..."
docker exec crypto_hunter_timescaledb psql -U postgres -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;" || {
    echo "⚠️  扩展可能已存在，继续..."
}

# 检查扩展是否启用
echo "🔍 检查 TimescaleDB 版本..."
docker exec crypto_hunter_timescaledb psql -U postgres -d crypto_hunter -c "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';"

echo ""
echo "✅ TimescaleDB 设置完成！"
echo ""
echo "📝 更新 DATABASE_URL 环境变量："
echo "   export DATABASE_URL=\"postgresql://postgres:postgres@localhost:5432/crypto_hunter\""
echo ""
echo "📋 或者更新 .env 文件："
echo "   DATABASE_URL=postgresql://postgres:postgres@localhost:5432/crypto_hunter"
echo ""
echo "🚀 下一步：执行 TimescaleDB 配置"
echo "   psql -U postgres -h localhost -d crypto_hunter -f migrations/06_timescaledb_ml_setup.sql"
