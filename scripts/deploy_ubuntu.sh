#!/usr/bin/env bash
#
# crypto-hunter Ubuntu 部署脚本（适用于完全干净的 Ubuntu 环境）
#
# 假定：全新 Ubuntu，无 Rust、无编译工具。脚本会依次安装：
#   - 系统依赖：ca-certificates, curl, build-essential, pkg-config, libssl-dev
#   - PostgreSQL + TimescaleDB（原生 apt 安装）
#   - Rust（rustup + stable）
#   - 编译 release 并部署到指定目录
#
# 要求：root 或 sudo、网络可用、当前目录为项目根。
#
# 用法：
#   ./scripts/deploy_ubuntu.sh
#   DEPLOY_DIR=$HOME/crypto-hunter ./scripts/deploy_ubuntu.sh
#   SKIP_INSTALL=1 ./scripts/deploy_ubuntu.sh
#   SKIP_DB=1 ./scripts/deploy_ubuntu.sh      # 跳过数据库安装（使用外部 DB）
#   INSTALL_SYSTEMD=1 ./scripts/deploy_ubuntu.sh
#
set -euo pipefail

# 默认部署目录
DEPLOY_DIR="${DEPLOY_DIR:-/opt/crypto-hunter}"
INSTALL_SYSTEMD="${INSTALL_SYSTEMD:-0}"
SKIP_INSTALL="${SKIP_INSTALL:-0}"
# 是否跳过数据库安装（若使用外部 PostgreSQL/TimescaleDB 则设为 1）
SKIP_DB="${SKIP_DB:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"
if [ ! -f "Cargo.toml" ]; then
  echo "错误: 未在项目根目录找到 Cargo.toml"
  exit 1
fi

# 加载 os-release 供后续使用
if [ -f /etc/os-release ]; then . /etc/os-release; fi

echo "=== crypto-hunter Ubuntu 部署（完全干净环境） ==="
echo "  项目目录: ${ROOT_DIR}"
echo "  部署目录: ${DEPLOY_DIR}"
echo "  安装数据库: $([ "${SKIP_DB}" = "1" ] && echo "跳过" || echo "是")"
echo "  安装 systemd: ${INSTALL_SYSTEMD}"
echo ""

run_apt() {
  if [ "$(id -u)" = "0" ]; then apt-get "$@"; else sudo apt-get "$@"; fi
}

# --- 1. 检测系统与权限 ---
echo "[1/8] 检测系统..."
echo "  系统: ${ID:-unknown} ${VERSION_ID:-}"
if [ "$(id -u)" != "0" ] && ! command -v sudo &>/dev/null; then
  echo "  错误: 需要 root 或 sudo"
  exit 1
fi
if ! command -v apt-get &>/dev/null; then
  echo "  错误: 本脚本仅支持 Debian/Ubuntu"
  exit 1
fi

# --- 2. 安装系统依赖 ---
echo ""
echo "[2/8] 安装系统依赖..."
export DEBIAN_FRONTEND=noninteractive
run_apt update -qq
run_apt install -y -qq ca-certificates curl
run_apt install -y -qq build-essential pkg-config libssl-dev
echo "  已安装: ca-certificates, curl, build-essential, pkg-config, libssl-dev"

# --- 3. 安装 PostgreSQL + TimescaleDB（原生 apt） ---
echo ""
echo "[3/8] 安装 PostgreSQL 与 TimescaleDB..."
if [ "${SKIP_DB}" = "1" ]; then
  echo "  SKIP_DB=1，跳过数据库安装"
else
  echo "  添加 TimescaleDB 仓库..."
  curl -s https://packagecloud.io/install/repositories/timescale/timescaledb/script.deb.sh | sudo bash

  echo "  安装 PostgreSQL 与 TimescaleDB..."
  run_apt install -y -qq postgresql postgresql-contrib
  PG_VER=$(psql --version 2>/dev/null | grep -oE '[0-9]+' | head -1)
  PG_VER=${PG_VER:-15}
  echo "  检测到 PostgreSQL 版本: ${PG_VER}"
  if ! run_apt install -y -qq "timescaledb-2-postgresql-${PG_VER}" 2>/dev/null; then
    run_apt install -y -qq "timescaledb-postgresql-${PG_VER}" 2>/dev/null || \
    run_apt install -y timescaledb-postgresql
  fi

  echo "  配置 TimescaleDB..."
  sudo timescaledb-tune --quiet --yes 2>/dev/null || true
  PG_CONF=$(sudo -u postgres psql -t -c "SHOW config_file;" 2>/dev/null | tr -d ' ')
  if [ -n "${PG_CONF}" ] && [ -f "${PG_CONF}" ]; then
    if ! sudo grep -q "shared_preload_libraries.*timescaledb" "${PG_CONF}" 2>/dev/null; then
      echo "shared_preload_libraries = 'timescaledb'" | sudo tee -a "${PG_CONF}" >/dev/null
    fi
  fi
  sudo systemctl restart postgresql 2>/dev/null || sudo service postgresql restart 2>/dev/null || true

  echo "  等待 PostgreSQL 就绪..."
  for i in $(seq 1 20); do
    if sudo -u postgres psql -c "SELECT 1;" >/dev/null 2>&1; then
      echo "  PostgreSQL 已就绪"
      break
    fi
    [ $i -eq 20 ] && { echo "  错误: PostgreSQL 启动超时"; exit 1; }
    sleep 1
  done

  echo "  创建数据库和用户..."
  sudo -u postgres psql -c "CREATE DATABASE crypto_hunter;" 2>/dev/null || true
  sudo -u postgres psql -c "ALTER USER postgres PASSWORD 'postgres';" 2>/dev/null || true
  sudo -u postgres psql -d crypto_hunter -c "CREATE EXTENSION IF NOT EXISTS timescaledb;" 2>/dev/null || true

  echo "  执行迁移..."
  sudo -u postgres psql -d crypto_hunter -f "${ROOT_DIR}/migrations/08_create_market_snapshots.sql" 2>/dev/null || true
  sudo -u postgres psql -d crypto_hunter -f "${ROOT_DIR}/migrations/09_drop_old_tables.sql" 2>/dev/null || true
  sudo -u postgres psql -d crypto_hunter -f "${ROOT_DIR}/migrations/10_add_retention_policy.sql" 2>/dev/null || true
  sudo -u postgres psql -d crypto_hunter -f "${ROOT_DIR}/migrations/11_convert_to_hypertable.sql" 2>/dev/null || true
  echo "  数据库迁移完成（含 TimescaleDB hypertable 配置）"
fi

# --- 4. 安装 Rust ---
echo ""
echo "[4/8] 检查/安装 Rust..."
if ! command -v cargo &>/dev/null; then
  echo "  未检测到 Rust，正在安装 rustup（需网络）..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  . "${HOME}/.cargo/env" 2>/dev/null || true
  export PATH="${HOME}/.cargo/bin:${PATH}"
fi
if ! command -v cargo &>/dev/null; then
  echo "  请执行: source ~/.cargo/env  后重试，或检查网络"
  exit 1
fi
cargo --version
rustc --version

# --- 5. 构建 release ---
echo ""
echo "[5/8] 构建 release 二进制..."
cargo build --release --bin crypto-hunter --bin arbitrage-monitor --bin pair-collector
echo "  构建完成:"
echo "    - target/release/crypto-hunter"
echo "    - target/release/arbitrage-monitor"
echo "    - target/release/pair-collector"

if [ "${SKIP_INSTALL}" = "1" ]; then
  echo ""
  echo "SKIP_INSTALL=1，跳过安装步骤。"
  echo "二进制位置: ${ROOT_DIR}/target/release/crypto-hunter"
  exit 0
fi

# --- 6. 安装到部署目录 ---
echo ""
echo "[6/8] 安装到 ${DEPLOY_DIR}..."

# 若部署目录当前用户可写则不用 sudo
run_inst() {
  if [ -n "${USE_SUDO}" ]; then sudo "$@"; else "$@"; fi
}
USE_SUDO="sudo"
if mkdir -p "${DEPLOY_DIR}" 2>/dev/null && [ -w "${DEPLOY_DIR}" ]; then
  USE_SUDO=""
  mkdir -p "${DEPLOY_DIR}/bin" "${DEPLOY_DIR}/scripts" "${DEPLOY_DIR}/logs"
else
  run_inst mkdir -p "${DEPLOY_DIR}/bin" "${DEPLOY_DIR}/scripts" "${DEPLOY_DIR}/logs"
fi

run_inst cp -f "${ROOT_DIR}/target/release/crypto-hunter" "${DEPLOY_DIR}/bin/"
run_inst cp -f "${ROOT_DIR}/target/release/arbitrage-monitor" "${DEPLOY_DIR}/bin/"
run_inst cp -f "${ROOT_DIR}/target/release/pair-collector" "${DEPLOY_DIR}/bin/"
run_inst cp -f "${SCRIPT_DIR}/run_realtime_collector.sh" "${SCRIPT_DIR}/stop_all_collectors.sh" "${SCRIPT_DIR}/start.sh" "${DEPLOY_DIR}/scripts/"
run_inst cp -f "${ROOT_DIR}/.env.example" "${DEPLOY_DIR}/.env.example"
if [ ! -f "${DEPLOY_DIR}/.env" ]; then
  run_inst cp -f "${DEPLOY_DIR}/.env.example" "${DEPLOY_DIR}/.env"
  echo "  已创建 ${DEPLOY_DIR}/.env，请编辑并填写 DATABASE_URL 等"
else
  echo "  保留已有 ${DEPLOY_DIR}/.env"
fi

# 创建使用部署目录的启动脚本
RUN_SH_CONTENT='#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${DIR}"
mkdir -p logs

# 先加载 .env 文件
if [ -f "${DIR}/.env" ]; then
  set -a
  . "${DIR}/.env"
  set +a
fi

# 生产环境默认使用 warn 级别，减少日志量
export DATABASE_URL="${DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/crypto_hunter}"
export RUST_LOG="${RUST_LOG:-crypto_hunter=info,warn}"

TS=$(date +%Y%m%d_%H%M%S)
LOG_FILE="${DIR}/logs/crypto-hunter_${TS}.log"
echo "启动 crypto-hunter，日志: ${LOG_FILE}"
exec "${DIR}/bin/crypto-hunter" >> "${LOG_FILE}" 2>&1
'
echo "${RUN_SH_CONTENT}" | run_inst tee "${DEPLOY_DIR}/run.sh" >/dev/null
run_inst chmod +x "${DEPLOY_DIR}/run.sh"
echo "  已创建 ${DEPLOY_DIR}/run.sh（数据采集器）"

# 创建 arbitrage-monitor 启动脚本
RUN_ARB_CONTENT='#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${DIR}"
mkdir -p logs

# 先加载 .env 文件
if [ -f "${DIR}/.env" ]; then
  set -a
  . "${DIR}/.env"
  set +a
fi

# 生产环境默认使用 warn 级别
export DATABASE_URL="${DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/crypto_hunter}"
export RUST_LOG="${RUST_LOG:-crypto_hunter=info,warn}"

TS=$(date +%Y%m%d_%H%M%S)
LOG_FILE="${DIR}/logs/arbitrage-monitor_${TS}.log"
echo "启动 arbitrage-monitor，日志: ${LOG_FILE}"
exec "${DIR}/bin/arbitrage-monitor" >> "${LOG_FILE}" 2>&1
'
echo "${RUN_ARB_CONTENT}" | run_inst tee "${DEPLOY_DIR}/run_arbitrage.sh" >/dev/null
run_inst chmod +x "${DEPLOY_DIR}/run_arbitrage.sh"
echo "  已创建 ${DEPLOY_DIR}/run_arbitrage.sh（套利监控）"

# 创建 pair-collector 启动脚本
RUN_PAIR_CONTENT='#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${DIR}"
mkdir -p logs

# 先加载 .env 文件
if [ -f "${DIR}/.env" ]; then
  set -a
  . "${DIR}/.env"
  set +a
fi

# 生产环境默认使用 warn 级别
export DATABASE_URL="${DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/crypto_hunter}"
export RUST_LOG="${RUST_LOG:-pair_collector=info,warn}"

TS=$(date +%Y%m%d_%H%M%S)
LOG_FILE="${DIR}/logs/pair-collector_${TS}.log"
echo "启动 pair-collector，日志: ${LOG_FILE}"
exec "${DIR}/bin/pair-collector" >> "${LOG_FILE}" 2>&1
'
echo "${RUN_PAIR_CONTENT}" | run_inst tee "${DEPLOY_DIR}/run_pairs.sh" >/dev/null
run_inst chmod +x "${DEPLOY_DIR}/run_pairs.sh"
echo "  已创建 ${DEPLOY_DIR}/run_pairs.sh（交易对同步）"

# 创建一键启动所有服务脚本
RUN_ALL_CONTENT='#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${DIR}"
mkdir -p logs

# 先加载 .env 文件
if [ -f "${DIR}/.env" ]; then
  set -a
  . "${DIR}/.env"
  set +a
fi

# 生产环境配置
export DATABASE_URL="${DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/crypto_hunter}"
export RUST_LOG="${RUST_LOG:-crypto_hunter=info,pair_collector=info,arbitrage_monitor=info,warn}"

TS=$(date +%Y%m%d_%H%M%S)
PAIR_LOG="${DIR}/logs/pair-collector_${TS}.log"
HUNTER_LOG="${DIR}/logs/crypto-hunter_${TS}.log"
ARB_LOG="${DIR}/logs/arbitrage-monitor_${TS}.log"

echo "启动交易对同步..."
"${DIR}/bin/pair-collector" >> "${PAIR_LOG}" 2>&1 &
PAIR_PID=$!
echo "  pair-collector PID: ${PAIR_PID}, 日志: ${PAIR_LOG}"

# 等待交易对同步完成
sleep 10

echo "启动数据采集器..."
"${DIR}/bin/crypto-hunter" >> "${HUNTER_LOG}" 2>&1 &
HUNTER_PID=$!
echo "  crypto-hunter PID: ${HUNTER_PID}, 日志: ${HUNTER_LOG}"

echo "启动套利监控..."
"${DIR}/bin/arbitrage-monitor" >> "${ARB_LOG}" 2>&1 &
ARB_PID=$!
echo "  arbitrage-monitor PID: ${ARB_PID}, 日志: ${ARB_LOG}"

# 保存 PID 到文件，方便停止
echo "${PAIR_PID}" > "${DIR}/logs/pair-collector.pid"
echo "${HUNTER_PID}" > "${DIR}/logs/crypto-hunter.pid"
echo "${ARB_PID}" > "${DIR}/logs/arbitrage-monitor.pid"

echo ""
echo "所有服务已启动（后台运行，日志输出到文件）"
echo "  pair-collector:     PID=${PAIR_PID}"
echo "  crypto-hunter:      PID=${HUNTER_PID}"
echo "  arbitrage-monitor:  PID=${ARB_PID}"
echo ""
echo "查看日志: tail -f ${DIR}/logs/*.log"
echo "停止服务: kill ${PAIR_PID} ${HUNTER_PID} ${ARB_PID}"
'
echo "${RUN_ALL_CONTENT}" | run_inst tee "${DEPLOY_DIR}/run_all.sh" >/dev/null
run_inst chmod +x "${DEPLOY_DIR}/run_all.sh"
echo "  已创建 ${DEPLOY_DIR}/run_all.sh（一键启动所有服务）"

# --- 7. 可选: systemd 服务 ---
if [ "${INSTALL_SYSTEMD}" = "1" ]; then
  echo ""
  echo "[7/8] 安装 systemd 服务..."
  SVC_NAME="crypto-hunter"
  SVC_FILE="/etc/systemd/system/${SVC_NAME}.service"
  sudo tee "${SVC_FILE}" >/dev/null << SVC
[Unit]
Description=Crypto Hunter Realtime Collector
After=network.target postgresql.service
Wants=network.target postgresql.service

[Service]
Type=simple
WorkingDirectory=${DEPLOY_DIR}
EnvironmentFile=${DEPLOY_DIR}/.env
Environment=RUST_LOG=info
ExecStart=${DEPLOY_DIR}/bin/crypto-hunter
Restart=on-failure
RestartSec=10
StandardOutput=append:${DEPLOY_DIR}/logs/service.log
StandardError=append:${DEPLOY_DIR}/logs/service.log

[Install]
WantedBy=multi-user.target
SVC
  sudo systemctl daemon-reload
  echo "  已创建 ${SVC_FILE}"
  echo "  启用并启动: sudo systemctl enable ${SVC_NAME} && sudo systemctl start ${SVC_NAME}"
  echo "  查看状态:   sudo systemctl status ${SVC_NAME}"
  echo "  查看日志:   journalctl -u ${SVC_NAME} -f"
fi

echo ""
echo "=== [8/8] 部署完成 ==="
echo ""
echo "  部署目录:  ${DEPLOY_DIR}"
echo "  二进制:    ${DEPLOY_DIR}/bin/{crypto-hunter,arbitrage-monitor,pair-collector}"
echo "  配置文件:  ${DEPLOY_DIR}/.env"
echo ""
echo "  启动脚本:"
echo "    ${DEPLOY_DIR}/run.sh           - 数据采集器"
echo "    ${DEPLOY_DIR}/run_arbitrage.sh - 套利监控"
echo "    ${DEPLOY_DIR}/run_pairs.sh     - 交易对同步"
echo "    ${DEPLOY_DIR}/run_all.sh       - 一键启动所有服务"
echo ""
echo "  建议启动顺序:"
echo "    1. 先同步交易对: ${DEPLOY_DIR}/run_pairs.sh"
echo "    2. 启动数据采集: ${DEPLOY_DIR}/run.sh -d"
echo "    3. 启动套利监控: ${DEPLOY_DIR}/run_arbitrage.sh -d"
echo ""
if [ "${INSTALL_SYSTEMD}" = "1" ]; then
  echo "  systemd 服务:"
  echo "    sudo systemctl start crypto-hunter"
  echo "    sudo systemctl status crypto-hunter"
  echo ""
fi
echo "  数据库: PostgreSQL + TimescaleDB 已安装"
echo "  默认连接: postgresql://postgres:postgres@localhost:5432/crypto_hunter"
echo ""
