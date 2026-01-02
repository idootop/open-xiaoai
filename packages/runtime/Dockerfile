# syntax=docker/dockerfile:1.4
FROM --platform=linux/amd64 ubuntu:20.04

# 1. 环境变量
ENV DEBIAN_FRONTEND=noninteractive \
    PATH="/opt/toolchain/bin:/root/.cargo/bin:${PATH}" \
    PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_SYSROOT_DIR=/opt/sysroot \
    PKG_CONFIG_LIBDIR=/opt/sysroot/usr/lib/pkgconfig:/opt/sysroot/lib/pkgconfig \
    CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
    CC_armv7_unknown_linux_gnueabihf="arm-linux-gnueabihf-gcc --sysroot=/opt/sysroot"

# 2. 基础依赖与多架构源
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    sed -i 's/archive.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    sed -i 's/security.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    sed -i 's/deb http/deb [arch=amd64] http/g' /etc/apt/sources.list && \
    echo "deb [arch=armhf] http://mirrors.aliyun.com/ubuntu-ports/ focal main restricted universe multiverse" > /etc/apt/sources.list.d/armhf.list && \
    echo "deb [arch=armhf] http://mirrors.aliyun.com/ubuntu-ports/ focal-updates main restricted universe multiverse" >> /etc/apt/sources.list.d/armhf.list && \
    dpkg --add-architecture armhf && \
    apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential qemu-user-static pkg-config libssl-dev xz-utils squashfs-tools \
    crossbuild-essential-armhf libasound2-dev:armhf libopus-dev:armhf libssl-dev:armhf && \
    # 删除 lists 文件以减小体积
    rm -rf /var/lib/apt/lists/*

# 3. 工具链
RUN mkdir -p /opt/toolchain && \
    curl -fSL https://releases.linaro.org/components/toolchain/binaries/latest-7/arm-linux-gnueabihf/gcc-linaro-7.5.0-2019.12-x86_64_arm-linux-gnueabihf.tar.xz | tar -xJ -C /opt/toolchain --strip-components=1 && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable && \
    rustup target add armv7-unknown-linux-gnueabihf

WORKDIR /app

# 4. 脚本与 Rootfs
COPY run.sh runtime.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/run.sh /usr/local/bin/runtime.sh

# --------------------------------------------------------------------------------
# 5. 构建混合 Sysroot
# 目标：结合 OpenWrt 的运行时库 (root.squashfs) 和 Toolchain/Ubuntu 的开发文件
# --------------------------------------------------------------------------------
RUN --mount=type=bind,source=root.squashfs,target=/app/root.squashfs \
    set -ex; \
    # 1. 解压缩并创建基础目录
    mkdir -p /opt/sysroot && unsquashfs -d /opt/sysroot -f /app/root.squashfs; \
    mkdir -p /opt/sysroot/usr/lib/pkgconfig /opt/sysroot/usr/include /opt/sysroot/lib; \
    \
    # 2. 补全启动文件和 nonshared 库 
    TC_LIBC_DIR="/opt/toolchain/arm-linux-gnueabihf/libc"; \
    if [ -d "$TC_LIBC_DIR" ]; then \
        find "$TC_LIBC_DIR/usr/lib" -name "*crt*.o" -exec cp -t /opt/sysroot/usr/lib/ {} +; \
        find "$TC_LIBC_DIR/usr/lib" \( -name "libc_nonshared.a" -o -name "libpthread_nonshared.a" \) \
            -exec cp -t /opt/sysroot/usr/lib/ {} +; \
    fi; \
    \
    # 3. 自动修复所有 .so 软链接
    find /opt/sysroot/lib /opt/sysroot/usr/lib -name "lib*.so.*" | while read src; do \
        link_name=$(echo "$src" | sed 's/\.so\..*$/.so/'); \
        if [ ! -e "$link_name" ]; then \
            ln -s "$(basename "$src")" "$link_name"; \
        fi \
    done; \
    \
    # 4. 生成 libc.so Linker Script
    rm -f /opt/sysroot/usr/lib/libc.so; \
    printf "OUTPUT_FORMAT(elf32-littlearm)\nGROUP ( /lib/libc.so.6 /usr/lib/libc_nonshared.a AS_NEEDED ( /lib/ld-linux-armhf.so.3 ) )\n" > /opt/sysroot/usr/lib/libc.so; \
    \
    # 5. 注入 Ubuntu 的开发头文件和 PC 文件
    if [ -d /usr/lib/arm-linux-gnueabihf/pkgconfig ]; then \
        find /usr/lib/arm-linux-gnueabihf/pkgconfig/ -name "*.pc" -exec cp -t /opt/sysroot/usr/lib/pkgconfig/ {} +; \
    fi; \
    cp -r /usr/include/alsa /opt/sysroot/usr/include/ 2>/dev/null || true; \
    cp -r /usr/include/opus /opt/sysroot/usr/include/ 2>/dev/null || true; \
    [ -d /usr/include/arm-linux-gnueabihf ] && cp -r /usr/include/arm-linux-gnueabihf/* /opt/sysroot/usr/include/ 2>/dev/null || true

# 6. Rust 编译参数
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS="\
    -C link-arg=--sysroot=/opt/sysroot \
    -C link-arg=-Wl,-dynamic-linker,/lib/ld-linux-armhf.so.3 \
    -C link-arg=-L/opt/sysroot/usr/lib \
    -C link-arg=-L/opt/sysroot/lib \
    -C link-arg=-Wl,-rpath-link,/opt/sysroot/usr/lib \
    -C link-arg=-Wl,-rpath-link,/opt/sysroot/lib"

CMD ["/bin/bash"]