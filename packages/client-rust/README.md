# Rust Client

这是使用 Rust 编写的小爱音箱补丁程序，主要负责：

- 共享通信协议（Server 端和 Client 端实时双向通信）
- 转发小爱音箱的麦克风输入音频流到 Server 端进行识别处理
- 转发小爱音箱上的事件到 Server 端（语音识别结果、播放状态等）
- 响应来自 Server 端的指令调用（执行脚本、播放音频流、系统升级等）

## 快速开始

> [!NOTE]
> 以下操作需要先将小爱音箱刷机， 然后 SSH 连接到小爱音箱。👉 [教程](../../docs/flash.md)

```shell
# 创建 open-xiaoai 文件夹
mkdir /data/open-xiaoai

# 设置 server 地址（注意替换成自己的 server 地址）
echo 'ws://192.168.31.227:4399' > /data/open-xiaoai/server.txt

# 可选：设置 token（如果 Server 端启用了 token 验证）
echo 'your-token-value' > /data/open-xiaoai/token.txt

# 运行启动脚本 init.sh
curl -sSfL https://gitee.com/idootop/artifacts/releases/download/open-xiaoai-client/init.sh | sh
```

## Token 验证机制

### 启用 Token 验证

#### 方法一：通过命令行参数（推荐）
```shell
client ws://192.168.31.227:4399 my-token-value
```

#### 方法二：通过 token.txt 文件
```shell
# 在open-xiaoai目录下创建token.txt文件
echo 'my-token-value' > /data/open-xiaoai/token.txt

# 运行启动脚本，会自动读取token.txt文件
curl -sSfL https://gitee.com/idootop/artifacts/releases/download/open-xiaoai-client/init.sh | sh
```

### 禁用 Token 验证

- 不传入 token 参数
- 或删除 `token.txt` 文件，或者清空文件内容

### 验证流程

1. WebSocket 连接建立后，Client 端立即发送 token 验证请求
2. Server 端验证 token 的有效性
3. 如果验证成功，Client 端继续正常运行
4. 如果验证失败，Client 端会显示错误信息并断开连接

### 错误信息

- `Invalid token`: 提供的 token 与 Server 端配置的 token 不匹配
- `Token required`: Server 端启用了 token 验证，但 Client 端未提供 token

### 兼容性

- 未配置 token 时，系统保持与当前版本的兼容性
- 支持与不支持 token 验证的 Server 端版本兼容

> [!IMPORTANT]
> 你可能需要先在电脑上运行其他演示程序，以获取 server 地址
>
> 注意安全！不要连接来路不明的 server 🚨

如果你想要开机自启动，运行以下命令重启小爱音箱即可。

```shell
# 下载 boot.sh 文件到 /data/init.sh 开机时自启动
curl -L -o /data/init.sh https://gitee.com/idootop/artifacts/releases/download/open-xiaoai-client/boot.sh

# 重启小爱音箱
reboot
```

## 编译运行

> [!TIP]
> 如果你是一名开发者，想要修改源代码实现自己想要的功能，可以按照下面的步骤，自行编译运行该项目。

首先，你需要在电脑上安装 `Rust` 开发环境 👉 [传送门](https://www.rust-lang.org/)

为了构建能够在小爱音箱上运行的 ARMv7 应用，你还需要安装 `cross` 👉 [传送门](https://github.com/cross-rs/cross)

```shell
# 克隆代码
git clone https://github.com/idootop/open-xiaoai.git

# 进入当前项目根目录
cd packages/client-rust

# 交叉编译
cross build --release --target armv7-unknown-linux-gnueabihf
```

> [!TIP]
> 如果你是 Apple Silicon 芯片，为了能够正常使用 cross 交叉编译镜像，请先在 Docker Desktop - Settings - General - Virtual Machine Options 中打开 Apple Virtual framework 选项，然后开启 `Use Rosetta for x86_64/amd64 emulation on Apple Silicon`

编译成功后，将构建好的补丁程序 `client` 复制到小爱音箱上

```shell
# client 文件路径
./target/armv7-unknown-linux-gnueabihf/release/client
```

如果你是 macOS 系统，可以直接使用 `dd` + `ssh` 命令复制文件到小爱音箱

```shell
dd if=target/armv7-unknown-linux-gnueabihf/release/client \
| ssh -o HostKeyAlgorithms=+ssh-rsa root@你的小爱音箱IP地址 "dd of=/data/open-xiaoai/client"
```

> [!TIP]
> 注意替换你自己的小爱音箱局域网 IP 地址，比如： root@192.168.31.227
>
> 如果提示 No such file or directory 请先在小爱音箱上创建 `/data/open-xiaoai` 文件夹

你也可以先把 `client` 文件上传到一个地方，然后 SSH 连接到小爱音箱后，再用 `curl` 命令下载到本地。

```shell
# 连接到小爱音箱
ssh -o HostKeyAlgorithms=+ssh-rsa root@你的小爱音箱IP地址

# 创建 open-xiaoai 文件夹
mkdir /data/open-xiaoai

# 下载文件
curl -# -o /data/open-xiaoai/client https://你的client文件下载链接
```

最后，在小爱音箱上授予 `client` 文件运行权限，然后运行：

```shell
# 授权
chmod +x /data/open-xiaoai/client

# 运行
/data/open-xiaoai/client ws://你的 server 端地址（默认使用 4399 端口）

# 比如：/data/open-xiaoai/client ws://192.168.31.227:4399
```

## 注意事项

当前 Client 端使用 Rust 编写，只负责转发和被动响应 Server 端的调用，不实现具体的业务逻辑。

一方面，小爱音箱的内存算力和存储空间极为有限，语音识别之类的任务并不适合放在 Client 端侧运行。

另一方面，使用 Rust 编写一些业务逻辑，不如 Python 和 Node.js 灵活开发效率高，后者的应用生态也更丰富一些。

> [!CAUTION]
> 当你在公网上运行本项目时，应当提高警惕。🚨

本项目只是一个基础的演示程序，抛砖引玉。诸如多设备连接管理、身份认证、通信数据加密、音频压缩传输等均未做处理。

其默认提供了**执行任意脚本**的能力演示，虽然在启动 Client 端时需要由你本人指定可信的 Server 端连接地址，但还是要务必小心！

> [!TIP]
> 本项目 Rust 端通过 binding 与 Python 和 Node.js 端双向互调，实现了网络通信模块的共享复用。当然你也可以参考 Rust 端通信协议源码，在其他 Server 端重新实现 WebSocket 通信过程。

你也可以 Fork 本项目，在此基础上将 MiGPT 和小智 AI 等业务逻辑完全放在 Client 端运行，这样就不需要额外的服务器或 NAS 设备来部署运行了。
