#!/bin/bash

set -e

SSH_PASSWORD=open-xiaoai

# # 1. 编译 client
# echo "Compiling client..."
# docker run --rm -v $(pwd):/app/hello open-xiaoai-runtime \
#     cargo build --manifest-path hello/Cargo.toml --target armv7-unknown-linux-gnueabihf --bin client --release

# 2. 上传 client 到小爱音箱
function upload_to_xiaoai() {
    local binary_name=$1
    local ip=$2
    dd if=target/armv7-unknown-linux-gnueabihf/release/$binary_name \
    | sshpass -p $SSH_PASSWORD ssh -o HostKeyAlgorithms=+ssh-rsa root@$ip "dd of=/data/$binary_name" \
    && echo "✅ Uploaded $binary_name to $ip"
}

upload_to_xiaoai stereo 192.168.31.153 # left
upload_to_xiaoai stereo 192.168.31.235 # right

# # 3. 编译 server
# cargo build --bin server --release
# ./target/release/server

# # dd if=test.wav | sshpass -p open-xiaoai ssh -o HostKeyAlgorithms=+ssh-rsa root@192.168.31.235 "dd of=/tmp/test.wav"


# /data/stereo slave left

# /data/stereo master right


# ubus call mediaplayer player_play_url '{"url":"/data/test.wav","type":1}'

# /etc/init.d/mediaplayer restart >/dev/null 2>&1
# /etc/init.d/bluetooth restart >/dev/null 2>&1
# /etc/init.d/miio  restart >/dev/null 2>&1
# /etc/init.d/mico_aivs_lab  restart >/dev/null 2>&1
# /etc/init.d/miplay  restart >/dev/null 2>&1