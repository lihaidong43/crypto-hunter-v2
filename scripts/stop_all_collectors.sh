#!/bin/bash

# 停止所有 crypto-hunter 相关进程

echo "=== 查找所有 crypto-hunter 相关进程 ==="
echo ""

# 查找所有相关进程
PIDS=$(ps aux | grep -E "crypto-hunter|historical-collector|pair-collector|arbitrage-monitor" | grep -v grep | awk '{print $2}')

if [ -z "$PIDS" ]; then
    echo "没有找到运行中的进程"
    exit 0
fi

echo "找到以下进程："
ps -p $PIDS -o pid,ppid,command,etime,start
echo ""

# 询问是否停止
read -p "是否停止这些进程？(y/n) " -n 1 -r
echo ""

if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "已取消"
    exit 0
fi

# 停止进程
for PID in $PIDS; do
    echo "正在停止进程 $PID..."
    kill $PID
    sleep 1
    
    # 检查进程是否还在运行
    if ps -p $PID > /dev/null 2>&1; then
        echo "  进程 $PID 仍在运行，强制停止..."
        kill -9 $PID
        sleep 1
    else
        echo "  进程 $PID 已停止"
    fi
done

echo ""
echo "=== 验证进程是否已停止 ==="
REMAINING=$(ps aux | grep -E "crypto-hunter|historical-collector|pair-collector|arbitrage-monitor" | grep -v grep)
if [ -z "$REMAINING" ]; then
    echo "✓ 所有进程已停止"
else
    echo "⚠️  仍有进程在运行："
    echo "$REMAINING"
fi
