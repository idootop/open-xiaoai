#!/bin/bash

set -e

# SSH_PASSWORD=open-xiaoai

# # 1. 编译 demo
# echo "Compiling hello demo..."
# docker run --rm -v $(pwd):/app/hello open-xiaoai-runtime \
#     cargo build --manifest-path hello/Cargo.toml --target armv7-unknown-linux-gnueabihf --bin client --release

# # 2. 上传二进制文件到小爱音箱
# function upload_to_xiaoai() {
#     local binary_name=$1
#     local ip=$2
#     dd if=target/armv7-unknown-linux-gnueabihf/release/$binary_name \
#     | sshpass -p $SSH_PASSWORD ssh -o HostKeyAlgorithms=+ssh-rsa root@$ip "dd of=/data/$binary_name" \
#     && echo "✅ Uploaded $binary_name to $ip"
# }

# upload_to_xiaoai client 192.168.31.153 # left
# upload_to_xiaoai client 192.168.31.235 # right

cargo build --bin server --release
./target/release/server
