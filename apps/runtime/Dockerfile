FROM --platform=linux/amd64 ubuntu:20.04

ENV DEBIAN_FRONTEND=noninteractive

# 换成阿里云源
RUN sed -i 's/archive.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list && \
    sed -i 's/security.ubuntu.com/mirrors.aliyun.com/g' /etc/apt/sources.list

# 安装必要的依赖
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    qemu-user-static \
    pkg-config \
    libssl-dev \
    xz-utils \
    squashfs-tools

# 配置交叉编译工具链（Linaro GCC 7.5.0）
ENV PATH="/opt/toolchain/bin:${PATH}"
RUN mkdir -p /opt/toolchain && \
    curl -fSL -o toolchain.tar.xz https://releases.linaro.org/components/toolchain/binaries/latest-7/arm-linux-gnueabihf/gcc-linaro-7.5.0-2019.12-x86_64_arm-linux-gnueabihf.tar.xz && \
    tar -xJf toolchain.tar.xz -C /opt/toolchain --strip-components=1 && \
    rm toolchain.tar.xz

# 安装 Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add armv7-unknown-linux-gnueabihf

WORKDIR /app

COPY run.sh /usr/local/bin/run
COPY runtime.sh /usr/local/bin/runtime
RUN chmod +x /usr/local/bin/run /usr/local/bin/runtime

# 解压 root.squashfs
COPY root.squashfs /app/root.squashfs
RUN mkdir -p /opt/sysroot \
    && unsquashfs -d /opt/sysroot -f /app/root.squashfs \
    && rm -rf /app/root.squashfs

# 创建专门针对 armhf 的源文件
# 注意：ubuntu:20.04 默认源里没有 armhf，必须添加 ubuntu-ports 源
RUN echo "deb [arch=armhf] http://mirrors.aliyun.com/ubuntu-ports/ focal main restricted universe multiverse" > /etc/apt/sources.list.d/armhf.list && \
    echo "deb [arch=armhf] http://mirrors.aliyun.com/ubuntu-ports/ focal-updates main restricted universe multiverse" >> /etc/apt/sources.list.d/armhf.list && \
    # 限制原有的源只针对 amd64，防止冲突
    sed -i 's/deb http/deb [arch=amd64] http/g' /etc/apt/sources.list

# 安装 armhf 架构的开发库
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    dpkg --add-architecture armhf && \
    apt-get update && apt-get install -y \
    crossbuild-essential-armhf \
    libasound2-dev:armhf \
    libopus-dev:armhf \
    libssl-dev:armhf

# --------------------------------------------------------------------------------
# 构建混合 Sysroot
# 目标：结合 OpenWrt 的运行时库 (root.squashfs) 和 Toolchain/Ubuntu 的开发文件
# --------------------------------------------------------------------------------

# 修复 OpenWrt sysroot：自动为所有共享库创建 .so 符号链接
# OpenWrt 只有 libfoo.so.1，编译链接需要 libfoo.so
RUN find /opt/sysroot/lib /opt/sysroot/usr/lib -name "lib*.so.*" | while read src; do \
        # 获取库名 (例如 libopus.so.0.8.0 -> libopus.so)
        link_name=$(echo "$src" | sed 's/\.so\..*$/.so/'); \
        if [ ! -e "$link_name" ]; then \
            ln -s "$(basename "$src")" "$link_name"; \
        fi \
    done

# 注入 Toolchain 的 C 运行时启动文件 (Scrt1.o, crti.o 等)
RUN cp -r /opt/toolchain/arm-linux-gnueabihf/libc/usr/lib/*crt*.o /opt/sysroot/usr/lib/ 2>/dev/null || \
    cp -r /opt/toolchain/arm-linux-gnueabihf/libc/lib/*crt*.o /opt/sysroot/usr/lib/ 2>/dev/null || true

# 特殊处理 glibc 的 linker script
# OpenWrt rootfs 中没有开发用的 libc.so (linker script) 和 libc_nonshared.a
# 我们需要从 toolchain 中复制 libc_nonshared.a，并手动创建 libc.so 脚本
# 这样 linker 才能正确处理 libc.so.6 和 ld-linux-armhf.so.3 的依赖关系 (解决 __tls_get_addr 错误)
RUN cp /opt/toolchain/arm-linux-gnueabihf/libc/usr/lib/libc_nonshared.a /opt/sysroot/usr/lib/ && \
    # 删除之前步骤可能自动生成的软链接
    rm -f /opt/sysroot/usr/lib/libc.so && \
    # 创建 linker script
    echo 'OUTPUT_FORMAT(elf32-littlearm)' > /opt/sysroot/usr/lib/libc.so && \
    echo 'GROUP ( /lib/libc.so.6 /usr/lib/libc_nonshared.a AS_NEEDED ( /lib/ld-linux-armhf.so.3 ) )' >> /opt/sysroot/usr/lib/libc.so && \
    # 同理处理 libpthread.so (如果存在 libpthread.so.0)
    if [ -f /opt/sysroot/lib/libpthread.so.0 ]; then \
        cp /opt/toolchain/arm-linux-gnueabihf/libc/usr/lib/libpthread_nonshared.a /opt/sysroot/usr/lib/ 2>/dev/null || true; \
        rm -f /opt/sysroot/usr/lib/libpthread.so; \
        echo 'OUTPUT_FORMAT(elf32-littlearm)' > /opt/sysroot/usr/lib/libpthread.so; \
        echo 'GROUP ( /lib/libpthread.so.0 /usr/lib/libpthread_nonshared.a )' >> /opt/sysroot/usr/lib/libpthread.so; \
    fi

# 注入 Ubuntu 的开发头文件和 pkg-config 定义 (用于 alsa, opus 等)
# 我们只取头文件和 .pc 文件，不取库文件，确保链接的是 OpenWrt 的库
RUN mkdir -p /opt/sysroot/usr/include /opt/sysroot/usr/lib/pkgconfig && \
    cp -r /usr/include/alsa /opt/sysroot/usr/include/ 2>/dev/null || true && \
    cp -r /usr/include/opus /opt/sysroot/usr/include/ 2>/dev/null || true && \
    cp -r /usr/include/arm-linux-gnueabihf/* /opt/sysroot/usr/include/ 2>/dev/null || true && \
    cp -r /usr/lib/arm-linux-gnueabihf/pkgconfig/* /opt/sysroot/usr/lib/pkgconfig/ 2>/dev/null || true

# 优化 Rust 链接参数
# 1. 优先使用 -L 指定 sysroot 路径
# 2. 使用 -rpath-link 解决库的间接依赖问题
# 3. 确保 dynamic-linker 路径正确 (OpenWrt glibc 常用路径)
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS="\
    -C link-arg=--sysroot=/opt/sysroot \
    -C link-arg=-Wl,-dynamic-linker,/lib/ld-linux-armhf.so.3 \
    -C link-arg=-L/opt/sysroot/usr/lib \
    -C link-arg=-L/opt/sysroot/lib \
    -C link-arg=-Wl,-rpath-link,/opt/sysroot/usr/lib \
    -C link-arg=-Wl,-rpath-link,/opt/sysroot/lib"

# 强制 pkg-config 只在 sysroot 中寻找，忽略宿主机容器路径
ENV PKG_CONFIG_ALLOW_CROSS=1
ENV PKG_CONFIG_SYSROOT_DIR=/opt/sysroot
ENV PKG_CONFIG_LIBDIR=/opt/sysroot/usr/lib/pkgconfig:/opt/sysroot/lib/pkgconfig

# 设置编译器，强制指定 sysroot
ENV CC_armv7_unknown_linux_gnueabihf="arm-linux-gnueabihf-gcc --sysroot=/opt/sysroot"

CMD ["/bin/bash"]
