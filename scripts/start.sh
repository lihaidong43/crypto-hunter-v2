#!/usr/bin/env bash
set -euo pipefail

# crypto-hunter 主程序启动脚本
#
# 用法：
#   ./scripts/start.sh              # 前台运行，日志同时输出到终端和文件
#   ./scripts/start.sh -d           # 后台运行（daemon 模式）
#   ./scripts/start.sh --daemon     # 同上
#
# 环境：
#   - 自动加载项目根目录 .env
#   - 默认 DATABASE_URL=postgresql://postgres@localhost:5432/crypto_hunter
#   - 默认 RUST_LOG=info

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
mkdir -p "${LOG_DIR}"
cd "${ROOT_DIR}"

# 加载 .env
if [ -f .env ]; then
  set -a
  . ./.env
  set +a
fi

# 默认配置
export DATABASE_URL="${DATABASE_URL:-postgresql://postgres@localhost:5432/crypto_hunter}"
export RUST_LOG="${RUST_LOG:-info}"

TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="${LOG_DIR}/crypto-hunter_${TS}.log"

# 选择可执行文件：部署环境优先用二进制
BIN="${ROOT_DIR}/bin/crypto-hunter"
if [ -x "${BIN}" ]; then
  CMD="${BIN}"
  DESC="bin/crypto-hunter"
else
  CMD="cargo run --bin crypto-hunter"
  DESC="cargo run --bin crypto-hunter"
fi

# 后台模式
DAEMON=0
for arg in "$@"; do
  case "$arg" in
    -d|--daemon) DAEMON=1 ;;
  esac
done

if [ "$DAEMON" = "1" ]; then
  echo "后台启动 crypto-hunter..."
  echo "  DATABASE_URL=${DATABASE_URL}"
  echo "  日志: ${LOG_FILE}"
  nohup $CMD >> "${LOG_FILE}" 2>&1 &
  echo "  PID: $!"
  echo "停止: kill $!  或使用 scripts/stop_all_collectors.sh"
else
  echo "前台启动 crypto-hunter..."
  echo "  DATABASE_URL=${DATABASE_URL}"
  echo "  日志: ${LOG_FILE}"
  exec $CMD 2>&1 | tee -a "${LOG_FILE}"
fi
