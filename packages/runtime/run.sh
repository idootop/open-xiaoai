#!/bin/bash

set -e

# 定义 sysroot 目录
SYSROOT_DIR="/opt/sysroot"

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
