#!/usr/bin/env bash
set -euo pipefail

# 启动：套利监控（独立进程，只从数据库读取 WS 写入的快照）
#
# 依赖：
# - export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
# 可选：
# - export RUST_LOG="info"
#
# 用法：
#   ./scripts/run_arbitrage_monitor.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres:postgres@localhost:5432/crypto_hunter'
fi

export RUST_LOG="${RUST_LOG:-info}"

TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="${LOG_DIR}/arbitrage-monitor_${TS}.log"

echo "DATABASE_URL=${DATABASE_URL}"
echo "RUST_LOG=${RUST_LOG}"
echo "Log: ${LOG_FILE}"
echo "Running: cargo run --bin arbitrage-monitor"

cd "${ROOT_DIR}"
exec cargo run --bin arbitrage-monitor 2>&1 | tee -a "${LOG_FILE}"

