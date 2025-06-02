#! /bin/sh

cat << 'EOF'

▄▖      ▖▖▘    ▄▖▄▖
▌▌▛▌█▌▛▌▚▘▌▀▌▛▌▌▌▐ 
▙▌▙▌▙▖▌▌▌▌▌█▌▙▌▛▌▟▖
  ▌                 

v1.0.0  by: https://del.wang

EOF

set -e

DOWNLOAD_BASE_URL="https://gitee.com/idootop/artifacts/releases/download/open-xiaoai-client"


WORK_DIR="/data/open-xiaoai"
CLIENT_BIN="$WORK_DIR/client"

if [ ! -d "$WORK_DIR" ]; then
    mkdir -p "$WORK_DIR"
fi

if [ ! -f "$CLIENT_BIN" ]; then
    echo "🔥 正在下载 Client 端补丁程序..."
    curl -L -# -o "$CLIENT_BIN" "$DOWNLOAD_BASE_URL/client"
    chmod +x "$CLIENT_BIN"
    echo "✅ Client 端补丁程序下载完毕"
fi

SECRET=""
if [ -f "$WORK_DIR/secret.txt" ]; then
    SECRET=$(cat "$WORK_DIR/secret.txt")
fi

echo "🔥 正在启动 Client 端补丁程序..."

kill -9 `ps|grep "open-xiaoai/client"|grep -v grep|awk '{print $1}'` > /dev/null 2>&1 || true

if [ -n "$SECRET" ]; then
    "$CLIENT_BIN" --secret "$SECRET"
fi