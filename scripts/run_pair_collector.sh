#!/usr/bin/env bash
set -euo pipefail

# 启动：交易对收集（pair-collector）
#
# 依赖：
# - export DATABASE_URL="postgresql://user:pass@host:5432/dbname"
# 可选：
# - export RUST_LOG="info"  # 或 "crypto_hunter=info"
#
# 用法：
#   ./scripts/run_pair_collector.sh
#   PAIR_INTERVAL_SECONDS=1800 ./scripts/run_pair_collector.sh
#   FORCE_SYNC=true ./scripts/run_pair_collector.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"

# 默认 DATABASE_URL，如果外部没有显式设置，则使用本地 postgres
if [[ -z "${DATABASE_URL:-}" ]]; then
  export DATABASE_URL='postgresql://postgres@localhost:5432/crypto_hunter'
fi

export RUST_LOG="${RUST_LOG:-info}"

PAIR_INTERVAL_SECONDS="${PAIR_INTERVAL_SECONDS:-}"
FORCE_SYNC="${FORCE_SYNC:-false}"

ARGS=()
if [[ -n "${PAIR_INTERVAL_SECONDS}" ]]; then
  ARGS+=(--interval "${PAIR_INTERVAL_SECONDS}")
fi
if [[ "${FORCE_SYNC}" == "true" ]]; then
  ARGS+=(--force)
fi

TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="${LOG_DIR}/pair-collector_${TS}.log"

echo "DATABASE_URL=${DATABASE_URL}"
echo "RUST_LOG=${RUST_LOG}"
echo "Log: ${LOG_FILE}"
if ((${#ARGS[@]})); then
  echo "Running: cargo run --bin pair-collector -- ${ARGS[*]}"
else
  echo "Running: cargo run --bin pair-collector"
fi

cd "${ROOT_DIR}"
if ((${#ARGS[@]})); then
  exec cargo run --bin pair-collector -- "${ARGS[@]}" 2>&1 | tee -a "${LOG_FILE}"
else
  exec cargo run --bin pair-collector 2>&1 | tee -a "${LOG_FILE}"
fi

