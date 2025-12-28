#!/bin/bash
set -e

# 在 macOS 上，使用 host.docker.internal 访问宿主机的代理端口
# 如果你的代理软件没有开启 "Allow LAN"（允许局域网连接），请确保它监听的是 0.0.0.0 而不仅仅是 127.0.0.1
PROXY="http://host.docker.internal:7890"

# 1. 构建编译环境镜像
# echo "Building Docker image with proxy..."
# docker build \
#     --build-arg http_proxy=$PROXY \
#     --build-arg https_proxy=$PROXY \
#     -t open-xiaoai-runtime .

# # 2. 编译 hello world demo
echo "Compiling hello demo..."
rm -rf ../hello/target # 先清理旧的构建产物，防止缓存干扰
docker run --rm -v $(pwd)/../hello:/app/hello open-xiaoai-runtime \
    cargo build --manifest-path hello/Cargo.toml --target armv7-unknown-linux-gnueabihf --release

# # 3. 运行编译出的二进制文件 (通过 QEMU 模拟)
echo "Running compiled binary via QEMU..."
docker run --rm -v $(pwd)/../hello/target/armv7-unknown-linux-gnueabihf/release/hello:/app/hello \
    -v $(pwd)/root.squashfs:/app/root.squashfs open-xiaoai-runtime \
    run /app/hello

# 4. 交互模式运行
# docker run --rm -it --privileged \
#     -v $(pwd)/root.squashfs:/app/root.squashfs \
#     open-xiaoai-runtime \
#     run

# 5. 上传二进制文件到小爱音箱
# dd if=target/armv7-unknown-linux-gnueabihf/release/hello \
# | ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.235 "dd of=/data/hello"
# ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.235 "dd if=/data/hello" | hello