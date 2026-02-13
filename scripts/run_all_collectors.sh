#!/usr/bin/env bash
set -euo pipefail

# 一键启动（后台）：交易对收集 + 实时采集
# 历史采集一般是一次性任务，不建议常驻；需要时单独跑 run_historical_collector.sh
#
# 依赖：
# - export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
#
# 用法：
#   ./scripts/run_all_collectors.sh
#
# 停止：
#   kill $(cat /tmp/crypto-hunter-v2_pids.txt)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres:postgres@localhost:5432/crypto_hunter'
fi

export RUST_LOG="${RUST_LOG:-info}"

TS="$(date +%Y%m%d_%H%M%S)"
PID_FILE="/tmp/crypto-hunter-v2_pids.txt"
> "${PID_FILE}"

echo "Starting pair-collector (background)..."
"${ROOT_DIR}/scripts/run_pair_collector.sh" > "${LOG_DIR}/pair-collector_${TS}.log" 2>&1 &
echo $! >> "${PID_FILE}"

sleep 2

echo "Starting crypto-hunter (background)..."
"${ROOT_DIR}/scripts/run_realtime_collector.sh" > "${LOG_DIR}/crypto-hunter_${TS}.log" 2>&1 &
echo $! >> "${PID_FILE}"

echo "Done. PID file: ${PID_FILE}"
echo "Logs:"
echo "  ${LOG_DIR}/pair-collector_${TS}.log"
echo "  ${LOG_DIR}/crypto-hunter_${TS}.log"

