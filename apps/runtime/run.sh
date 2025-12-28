#!/bin/bash

set -e

# 定义 sysroot 目录
SYSROOT_DIR="/opt/sysroot"

# 如果 /opt/sysroot 已经有内容（通过 -v 挂载了 squashfs-root），则跳过解压
if [ -d "$SYSROOT_DIR/bin" ] || [ -d "$SYSROOT_DIR/lib" ]; then
    echo "Using existing sysroot at $SYSROOT_DIR"
else
    mkdir -p "$SYSROOT_DIR"
    # 优先使用环境变量指定的 squashfs 文件，否则使用默认的 root.squashfs
    ROOTFS_FILE="${SYSROOT_FILE:-root.squashfs}"
    
    if [ -f "$ROOTFS_FILE" ]; then
        echo -e "\n🔥 Unsquashing $ROOTFS_FILE to $SYSROOT_DIR...\n"
        unsquashfs -d "$SYSROOT_DIR" -f "$ROOTFS_FILE"
        echo -e "\n✅ Unsquashed rootfs\n"
    else
        echo "Error: Sysroot file $ROOTFS_FILE not found and $SYSROOT_DIR is empty."
        exit 1
    fi
fi

# 检查第一个参数。如果在 sysroot 下存在该文件，则自动补全路径
# 比如 `run /bin/sh` 会被转换为 `run /opt/sysroot/bin/sh`
if [ -n "$1" ] && [ -f "$SYSROOT_DIR$1" ]; then
    CMD="$SYSROOT_DIR$1"
    shift
    set -- "$CMD" "$@"
fi


if [ $# -eq 0 ]; then
    # 如果没有提供参数，则默认在 sysroot 中启动 shell
    exec runtime
else
    exec qemu-arm-static -L "$SYSROOT_DIR" "$@"
fi
