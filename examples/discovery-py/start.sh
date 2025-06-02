#!/bin/bash

# 服务发现协议服务端启动脚本

# 检查Python是否安装
if ! command -v python3 &> /dev/null; then
    echo "❌ 错误: 未找到Python3，请先安装Python3"
    exit 1
fi

# 启动服务发现服务
echo "🚀 启动服务发现服务..."
python3 main.py "$@"