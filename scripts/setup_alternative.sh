#!/bin/bash

# 替代方案安装脚本（不使用 TimescaleDB）

set -e

echo "🚀 开始设置性能优化（替代方案）..."

# 检查数据库连接
if ! psql -U postgres -d crypto_hunter -c "SELECT 1;" > /dev/null 2>&1; then
    echo "❌ 无法连接到数据库，请检查 DATABASE_URL 或 PostgreSQL 服务"
    exit 1
fi

echo "✅ 数据库连接正常"

# 执行优化脚本
echo ""
echo "📊 1. 添加索引优化..."
psql -U postgres -d crypto_hunter -f migrations/01_add_indexes.sql

echo ""
echo "📦 2. 设置数据库分区..."
psql -U postgres -d crypto_hunter -f migrations/02_table_partitioning.sql

echo ""
echo "📈 3. 创建物化视图..."
psql -U postgres -d crypto_hunter -f migrations/05_materialized_views.sql

echo ""
echo "🗄️  4. 配置数据归档（保留所有数据）..."
psql -U postgres -d crypto_hunter -f migrations/03_data_retention.sql

echo ""
echo "✅ 所有优化已完成！"
echo ""
echo "📊 性能优化包括："
echo "   ✅ 索引优化 - 提升查询性能"
echo "   ✅ 数据库分区 - 按时间分区，提升查询和删除性能"
echo "   ✅ 物化视图 - 预聚合数据，加速报表查询"
echo "   ✅ 数据归档 - 保留所有历史数据"
echo ""
echo "💡 注意：这个方案不使用 TimescaleDB，但性能足够使用"
echo "   如果需要 TimescaleDB 的高级功能，可以使用 Docker 方案"
