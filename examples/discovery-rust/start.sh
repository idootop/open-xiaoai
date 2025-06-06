#!/bin/bash

# 服务发现协议服务端启动脚本 (Rust 版本)

# 检查 Rust 是否安装
if ! command -v cargo &> /dev/null; then
    echo "❌ 错误: 未找到 Rust/Cargo，请先安装 Rust"
    exit 1
fi

# 启动服务发现服务
echo "🚀 启动服务发现服务 (Rust 版本)..."
./target/debug/discovery-rust "$@"