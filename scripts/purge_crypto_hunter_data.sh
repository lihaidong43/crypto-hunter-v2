#!/usr/bin/env bash
set -euo pipefail

# 用法：
# 1) export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
# 2) ./purge_crypto_hunter_data.sh
#
# 可选：直接写死
# DATABASE_URL="postgresql://postgres@localhost:5432/crypto_hunter"

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres@localhost:5432/crypto_hunter'
fi

echo "Target DB: ${DATABASE_URL}"
read -r -p "确认要清空 trading_pairs / market_snapshots / sync_times 吗？输入 YES 继续: " ans
if [[ "${ans}" != "YES" ]]; then
  echo "Aborted."
  exit 0
fi

# 优先用 TRUNCATE（更快），并重置自增（如果有），级联（如果有外键依赖）
psql "${DATABASE_URL}" -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;

-- 如果有外键依赖，用 CASCADE 更稳
TRUNCATE TABLE
  market_snapshots,
  trading_pairs,
  sync_times
RESTART IDENTITY CASCADE;

COMMIT;
SQL

echo "Done. 已清空 market_snapshots / trading_pairs / sync_times"