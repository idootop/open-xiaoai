#!/bin/bash

SYSROOT="/opt/sysroot"
QEMU_BIN="/usr/bin/qemu-arm-static" 


echo "正在准备 OpenWrt 模拟环境..."

# 1. 拷贝 QEMU 静态二进制文件
cp "$QEMU_BIN" "$SYSROOT/usr/bin/"

# 2. 挂载必要文件系统
mount_fs() {
    for dir in proc sys dev dev/pts tmp; do
        mkdir -p "$SYSROOT/$dir"
        if ! mountpoint -q "$SYSROOT/$dir"; then
            if [[ "$dir" == "proc" ]]; then
                mount -t proc proc "$SYSROOT/$dir"
            elif [[ "$dir" == "sys" ]]; then
                mount -t sysfs sysfs "$SYSROOT/$dir"
            else
                mount --bind "/$dir" "$SYSROOT/$dir"
            fi
        fi
    done
}

mount_fs


# 3. 执行进入命令
# 注意：OpenWrt 默认没有 bash，通常使用 /bin/sh (busybox)
echo "------------------------------------------------"
echo "成功进入 OpenWrt 环境。输入 'exit' 退出。"
echo "------------------------------------------------"

chroot "$SYSROOT" /usr/bin/$(basename $QEMU_BIN) /bin/sh