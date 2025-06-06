#!/usr/bin/env bash

# 服务发现协议服务端启动脚本(TypeScript版本)

# 检查Node.js是否安装
if ! command -v node &> /dev/null; then
    echo "❌ 错误: 未找到Node.js，请先安装Node.js"
    exit 1
fi

# 检查npm是否安装
if ! command -v npm &> /dev/null; then
    echo "❌ 错误: 未找到npm，请先安装npm"
    exit 1
fi

# 安装依赖
echo "📦 安装依赖..."
npm install

# 启动服务发现服务
echo "🚀 启动服务发现服务(TypeScript版本)..."
npm run dev -- "$@"