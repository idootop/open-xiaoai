# Open-XiaoAI Client V2

> 开发中，敬请期待...

实时音频流推送服务，支持多客户端连接、音频录制/播放、RPC 远程调用和实时事件推送。

## 架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              Server                                     │
│                                                                         │
│  ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐     │
│  │ SessionManager  │    │    AudioBus     │    │   EventBus      │     │
│  │                 │    │  (Pub/Sub)      │    │  (Broadcast)    │     │
│  │  ├─ Session 1   │    │                 │    │                 │     │
│  │  ├─ Session 2   │◄──►│  ├─ Receiver    │    │  ├─ Server →    │     │
│  │  └─ Session N   │    │  └─ Broadcaster │    │  └─ Client →    │     │
│  └─────────────────┘    └─────────────────┘    └─────────────────┘     │
│           │                      │                      │               │
│           ▼                      ▼                      ▼               │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                     Command Handler                              │   │
│  │  ├─ Shell      (执行远程命令)                                    │   │
│  │  ├─ GetInfo    (获取设备信息)                                    │   │
│  │  ├─ SetVolume  (设置音量)                                        │   │
│  │  ├─ File       (文件操作)                                        │   │
│  │  └─ System     (系统控制)                                        │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
         │ TCP                           │ UDP
         ▼                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                              Client                                     │
│                                                                         │
│  ┌─────────────────┐    ┌─────────────────────────────────────────┐    │
│  │    Session      │    │           Audio Pipelines               │    │
│  │                 │    │                                         │    │
│  │  ├─ Connection  │    │  ┌───────────┐      ┌───────────────┐   │    │
│  │  ├─ RPC Manager │    │  │ Record    │      │   Playback    │   │    │
│  │  └─ Pipelines   │◄──►│  │ Pipeline  │      │   Pipeline    │   │    │
│  └─────────────────┘    │  │           │      │               │   │    │
│                         │  │ Mic→Opus  │      │ Opus→Speaker  │   │    │
│                         │  │   →UDP    │      │   ←UDP        │   │    │
│                         │  └───────────┘      └───────────────┘   │    │
│                         └─────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

## 项目结构

```
src/
├── lib.rs
├── app/
│   ├── server/                    # 服务端
│   │   ├── mod.rs                 # Server 主体
│   │   ├── audio_bus.rs           # 音频总线 (发布-订阅)
│   │   ├── session.rs             # 会话管理
│   │   └── stream.rs              # 音频流 (录音/播放/转发)
│   │
│   └── client/                    # 客户端
│       ├── mod.rs                 # Client 主体
│       ├── session.rs             # 会话管理
│       └── pipeline.rs            # 音频管道
│
├── audio/                         # 音频处理
│   ├── codec.rs                   # Opus 编解码
│   ├── config.rs                  # 音频配置
│   ├── player.rs                  # ALSA 播放
│   ├── recorder.rs                # ALSA 录音
│   └── wav.rs                     # WAV 文件读写
│
├── net/                           # 网络层
│   ├── command.rs                 # RPC 命令类型
│   ├── discovery.rs               # 服务发现
│   ├── event.rs                   # 实时事件系统
│   ├── network.rs                 # TCP/UDP 连接
│   ├── protocol.rs                # 通信协议
│   └── rpc.rs                     # RPC 管理
│
└── bin/
    ├── client.rs                  # 客户端 demo
    └── server.rs                  # 服务端 demo
```

## 功能特性

### RPC 命令系统

支持多种类型的远程调用：

| 命令        | 描述            | 请求                              | 响应                                        |
| ----------- | --------------- | --------------------------------- | ------------------------------------------- |
| `Shell`     | 执行 Shell 命令 | command, cwd, env, timeout        | stdout, stderr, exit_code                   |
| `GetInfo`   | 获取设备信息    | -                                 | model, serial, version, uptime, audio_state |
| `SetVolume` | 设置音量        | volume (0-100)                    | previous, current                           |
| `File`      | 文件操作        | Read/Write/Delete/List/Stat       | data/entries/stat                           |
| `System`    | 系统控制        | Reboot/Shutdown/GetLoad/GetMemory | Accepted/Load/Memory                        |
| `Ping`      | 延迟测量        | timestamp                         | timestamp, server_time                      |

### 实时事件系统

**服务端事件 (Server → Client):**

- `AudioStatusChanged` - 音频状态变化
- `ClientJoined/Left` - 客户端加入/离开
- `Notification` - 通知消息
- `RecordingComplete` - 录音完成
- `PlaybackComplete` - 播放完成

**客户端事件 (Client → Server):**

- `StatusUpdate` - 状态更新 (CPU/内存/温度)
- `AudioLevel` - 音频电平
- `KeyPress` - 按键事件
- `Alert` - 警告/错误

### 音频功能

- **录音**: 从客户端麦克风录制，服务端保存为 WAV
- **播放**: 服务端推送音频文件到客户端播放
- **编码**: Opus 编解码，支持 16kHz/48kHz
- **传输**: UDP 低延迟传输

## 使用方法

### 启动服务端

```bash
cargo run --bin server --release
```

### 启动客户端 (Linux)

```bash
cargo run --bin client --release
```

### 交叉编译 (ARM)

```bash
make build-arm
```

## 配置

通过环境变量配置认证：

```bash
export XIAO_SERVER_AUTH="your-server-secret"
export XIAO_CLIENT_AUTH="your-client-secret"
```

## 示例代码

### 服务端

```rust
use xiao::app::server::Server;
use xiao::net::command::Command;

let server = Arc::new(Server::new().await?);

// 启动服务器
tokio::spawn(async move {
    server.run(8080).await.unwrap();
});

// 执行远程命令
let result = server.shell(client_addr, "uname -a").await?;
println!("Output: {}", result.stdout);

// 开始录音
server.start_record(client_addr, AudioConfig::voice_16k()).await?;

// 广播事件
server.broadcast_event(ServerEvent::Notification {
    level: NotificationLevel::Info,
    title: "Notice".to_string(),
    message: "Hello everyone!".to_string(),
}).await;
```

### 客户端

```rust
use xiao::app::client::{Client, ClientConfig};

let client = Arc::new(Client::new(ClientConfig::default()));

// 订阅服务端事件
let mut events = client.subscribe_events();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        println!("Event: {:?}", event);
    }
});

// 运行客户端
client.run().await?;
```

## License

MIT License © 2026-PRESENT [Del Wang](https://del.wang)
