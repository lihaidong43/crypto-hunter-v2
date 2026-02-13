#!/bin/bash

# 检查是否有其他进程在写入数据库

echo "=== 检查正在运行的进程 ==="
echo ""

echo "1. 检查 crypto-hunter 相关进程："
ps aux | grep -E "crypto-hunter|historical-collector|pair-collector|arbitrage-monitor" | grep -v grep || echo "  没有找到相关进程"

echo ""
echo "2. 检查 PostgreSQL 连接："
echo "   查询当前连接到数据库的进程..."

# 加载 .env 文件（如果存在）
if [ -f .env ]; then
    source .env
fi

# 默认 DATABASE_URL
DATABASE_URL="${DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/crypto_hunter}"

# 提取数据库连接信息
DB_HOST=$(echo $DATABASE_URL | sed -n 's/.*@\([^:]*\):.*/\1/p')
DB_PORT=$(echo $DATABASE_URL | sed -n 's/.*:\([0-9]*\)\/.*/\1/p')
DB_NAME=$(echo $DATABASE_URL | sed -n 's/.*\/\([^?]*\).*/\1/p')
DB_USER=$(echo $DATABASE_URL | sed -n 's/.*:\/\/\([^:]*\):.*/\1/p')

echo "   数据库: $DB_NAME@$DB_HOST:$DB_PORT"
echo "   用户: $DB_USER"
echo ""
echo "   当前活跃连接："
psql "$DATABASE_URL" -c "
    SELECT 
        pid,
        usename,
        application_name,
        client_addr,
        state,
        query_start,
        state_change,
        LEFT(query, 100) as query_preview
    FROM pg_stat_activity
    WHERE datname = '$DB_NAME'
      AND state != 'idle'
    ORDER BY query_start DESC;
" 2>/dev/null || echo "   无法连接到数据库（需要安装 psql 或检查 DATABASE_URL）"

echo ""
echo "3. 检查数据库中的时间戳分布（最近1小时）："
psql "$DATABASE_URL" -c "
    SELECT 
        exchange,
        market_type,
        COUNT(*) as count,
        MIN(snapshot_time) as min_time,
        MAX(snapshot_time) as max_time,
        COUNT(DISTINCT DATE_TRUNC('minute', snapshot_time)) as distinct_minutes
    FROM market_snapshots
    WHERE snapshot_time >= NOW() - INTERVAL '1 hour'
    GROUP BY exchange, market_type
    ORDER BY exchange, market_type;
" 2>/dev/null || echo "   无法查询数据库"

echo ""
echo "4. 检查是否有未来时间戳的数据："
psql "$DATABASE_URL" -c "
    SELECT 
        exchange,
        market_type,
        COUNT(*) as count,
        MIN(snapshot_time) as min_time,
        MAX(snapshot_time) as max_time
    FROM market_snapshots
    WHERE snapshot_time > NOW()
    GROUP BY exchange, market_type
    ORDER BY exchange, market_type;
" 2>/dev/null || echo "   无法查询数据库"

echo ""
echo "=== 检查完成 ==="
