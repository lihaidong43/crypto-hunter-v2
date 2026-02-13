#!/usr/bin/env bash
set -euo pipefail

# 启动：历史数据采集（historical-collector）
#
# 依赖：
# - export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
# 可选：
# - export RUST_LOG="info"
#
# 常用参数（环境变量方式）：
# - SYMBOLS: "BTCUSDT,ETHUSDT"（可选，默认：从数据库读全部交易对）
# - START_TIME: "2026-01-01T00:00:00Z"（可选，默认：今年 1 月 1 日）
# - END_TIME: "2026-01-20T00:00:00Z"（可选，默认：现在）
# - INTERVAL: "1h"（可选，默认：1h）
#
# 用法：
#   ./scripts/run_historical_collector.sh
#   SYMBOLS="BTCUSDT,ETHUSDT" INTERVAL="5m" ./scripts/run_historical_collector.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres:postgres@localhost:5432/crypto_hunter'
fi

export RUST_LOG="${RUST_LOG:-info}"

SYMBOLS="${SYMBOLS:-}"
START_TIME="${START_TIME:-}"
END_TIME="${END_TIME:-}"
INTERVAL="${INTERVAL:-1h}"

ARGS=(--interval "${INTERVAL}")
if [[ -n "${SYMBOLS}" ]]; then
  ARGS+=(--symbols "${SYMBOLS}")
fi
if [[ -n "${START_TIME}" ]]; then
  ARGS+=(--start-time "${START_TIME}")
fi
if [[ -n "${END_TIME}" ]]; then
  ARGS+=(--end-time "${END_TIME}")
fi

TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="${LOG_DIR}/historical-collector_${TS}.log"

echo "DATABASE_URL=${DATABASE_URL}"
echo "RUST_LOG=${RUST_LOG}"
echo "Log: ${LOG_FILE}"
echo "Running: cargo run --bin historical-collector -- ${ARGS[*]}"

cd "${ROOT_DIR}"
exec cargo run --bin historical-collector -- "${ARGS[@]}" 2>&1 | tee -a "${LOG_FILE}"

