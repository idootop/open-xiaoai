# Open-XiaoAI Runtime

小爱音箱 ARMv7 交叉编译 + 模拟运行环境（基于 OH2P v1.58.1 固件）

## 概述

小爱音箱使用的 Linux 系统较旧，glibc 版本为 2.25，需要使用 `gcc-linaro-7.5.0-2019.12` 工具链才能编译出兼容的程序。

```bash
root@OH2P:~# uname -a
Linux OH2P 4.9.61 #1 SMP PREEMPT Fri Jun 27 06:11:47 2025 aarch64 GNU/Linux

root@OH2P:~# file /lib/libc-2.25.so
ELF 32-bit LSB shared object, ARM, EABI5 version 1 (GNU/Linux),
dynamically linked, interpreter /lib/ld-linux-armhf.so.3, for GNU/Linux 3.2.0, stripped
```

## Rust 交叉编译示例

编译产物可直接在小爱音箱上运行。

```bash
cd /path/to/your-rust-project

# 使用预先构建好的 Docker 环境进行交叉编译
docker run --rm idootop/open-xiaoai-runtime:oh2p \
    -v $(shell pwd):/app \
	cargo build --target armv7-unknown-linux-gnueabihf --release
```

## 环境说明

**交叉编译环境**（Ubuntu 20.04 x86_64）

- Linaro GCC 7.5.0 (arm-linux-gnueabihf)
- Rust 稳定版 + armv7-unknown-linux-gnueabihf target
- 包含 ALSA、Opus、OpenSSL 等系统库（ARMv7 版本）
- 从小爱音箱固件提取的完整 Sysroot

**模拟运行环境**（QEMU 用户态）

- ARMv7 架构（32-bit ARM）
- OpenWrt 定制系统 + glibc 2.25
- 使用小爱音箱真实 rootfs
- 不支持音频录制/播放等操作，需在小爱音箱真机运行

## 自定义配置

### 更换固件

默认使用 OH2P v1.58.1 固件。如需使用其他型号和版本：

```bash
# 克隆代码
git clone https://github.com/idootop/open-xiaoai.git

# 进入当前项目根目录
cd packages/runtime

# 【重要】将新的 `root.squashfs` 放到当前目录

# 构建镜像
make build
```

PS：你可以修改 `Makefile` 中的代理地址加速镜像构建（代理软件需开启局域网访问）。

```makefile
PROXY := http://host.docker.internal:7890 # 对应 0.0.0.0:7890
```

### 其他 Makefile 命令

```bash
# 构建镜像
make build

# 进入交叉编译环境（Ubuntu x86_64）
make run-x86

# 进入小爱音箱模拟环境（OpenWrt ARMv7）
make run-armv7
```
