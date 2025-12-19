#!/bin/sh

cat << 'EOF'

▄▖      ▖▖▘    ▄▖▄▖
▌▌▛▌█▌▛▌▚▘▌▀▌▛▌▌▌▐ 
▙▌▙▌▙▖▌▌▌▌▌█▌▙▌▛▌▟▖
  ▌                 

v1.0.0  by: https://del.wang

EOF

# 移除set -e，避免脚本因单个命令失败而退出
# set -e

DOWNLOAD_BASE_URL="https://gitee.com/idootop/artifacts/releases/download/open-xiaoai-client"

WORK_DIR="/data/open-xiaoai"
CLIENT_BIN="$WORK_DIR/client"
SERVER_ADDRESS="ws://127.0.0.1:4399" # 默认不会连接到任何 server

# 创建工作目录，忽略错误
mkdir -p "$WORK_DIR" 2>/dev/null || true

# 下载client程序，添加重试机制
if [ ! -f "$CLIENT_BIN" ]; then
    echo "🔥 正在下载 Client 端补丁程序..."
    # 最多重试3次
    for i in 1 2 3; do
        if curl -L -# -o "$CLIENT_BIN" "$DOWNLOAD_BASE_URL/client"; then
            chmod +x "$CLIENT_BIN" 2>/dev/null || true
            echo "✅ Client 端补丁程序下载完毕"
            break
        else
            echo "❌ 下载失败，第 $i 次重试..."
            sleep 2
        fi
    done
fi

# 读取server地址，忽略错误
if [ -f "$WORK_DIR/server.txt" ]; then
    SERVER_ADDRESS=$(cat "$WORK_DIR/server.txt" 2>/dev/null || echo "ws://127.0.0.1:4399")
fi

# 读取token.txt文件（如果存在），忽略错误
TOKEN=""
if [ -f "$WORK_DIR/token.txt" ]; then
    TOKEN=$(cat "$WORK_DIR/token.txt" 2>/dev/null | tr -d '\n' || echo "")
fi

echo "🔥 正在启动 Client 端补丁程序..."

# 改进的kill命令，避免管道命令导致脚本退出
PID=$(ps | grep -E "[o]pen-xiaoai/client" | awk '{print $1}' 2>/dev/null || true)
if [ -n "$PID" ]; then
    kill -9 "$PID" 2>/dev/null || true
fi

# 启动client程序，添加自动重启机制
echo "🔄 启动Client程序，崩溃时将自动重启..."
while true; do
    if [ -z "$TOKEN" ]; then
        "$CLIENT_BIN" "$SERVER_ADDRESS" 2>&1 || true
    else
        "$CLIENT_BIN" "$SERVER_ADDRESS" "$TOKEN" 2>&1 || true
    fi
    echo "❌ Client程序已退出，5秒后自动重启..."
    sleep 5
done
