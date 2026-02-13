#!/usr/bin/env bash
set -euo pipefail

# 启动：实时数据采集（crypto-hunter 主进程）
#
# 依赖：
# - export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
# 可选：
# - export RUST_LOG="info"  # 或 "crypto_hunter=info"
#
# 用法：
#   ./scripts/run_realtime_collector.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"

# 加载 .env 文件（如果存在）
if [[ -f "${ROOT_DIR}/.env" ]]; then
  set -a
  source "${ROOT_DIR}/.env"
  set +a
fi

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres@localhost:5432/crypto_hunter'
fi

export RUST_LOG="${RUST_LOG:-info}"

TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="${LOG_DIR}/crypto-hunter_${TS}.log"

echo "DATABASE_URL=${DATABASE_URL}"
echo "RUST_LOG=${RUST_LOG}"
echo "Log: ${LOG_FILE}"
echo "Running: cargo run --bin crypto-hunter"

cd "${ROOT_DIR}"
exec cargo run --bin crypto-hunter 2>&1 | tee -a "${LOG_FILE}"

