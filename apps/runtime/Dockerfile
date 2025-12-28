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

# 配置 Cargo
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS="-C link-arg=-Wl,-dynamic-linker,/lib/ld-linux-armhf.so.3"

# 设置 CC
ENV CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc

WORKDIR /app

COPY run.sh /usr/local/bin/run
COPY runtime.sh /usr/local/bin/runtime
RUN chmod +x /usr/local/bin/run /usr/local/bin/runtime

CMD ["/bin/bash"]
