# 服务发现协议示例

这是一个简单的服务发现协议服务端实现示例，用于在局域网内自动发现服务端设备。该示例基于UDP广播，使用HMAC-SHA256进行身份验证，与小爱开放平台的Rust客户端兼容。

## 功能特点

- 基于UDP广播的服务发现
- 使用HMAC-SHA256进行身份验证
- 支持时间窗口验证，防止重放攻击
- 返回WebSocket服务端口和IP地址
- 支持命令行参数配置

## 协议规范

### 发现请求包 (60字节)
```
0-15: 设备ID (16字节)
16-19: 随机数 (4字节)
20-27: 时间戳 (8字节, big-endian)
28-59: HMAC签名 (32字节)
```

### 发现响应包 (38字节)
```
0-31: 原始请求的前32字节
32-33: WebSocket端口 (2字节, big-endian)
34-37: IP地址 (4字节)
```

## 使用方法

### 安装依赖

本示例不需要额外的依赖，只使用Python标准库。

### 运行服务

```bash
# 使用默认配置运行
python main.py

# 指定UDP端口
python main.py --port 5354

# 指定WebSocket端口
python main.py --ws-port 8080

# 指定密钥
python main.py --secret "your-secret-key"

# 同时指定多个参数
python main.py --port 5354 --ws-port 8080 --secret "your-secret-key"
```

### 命令行参数

| 参数 | 描述 | 默认值 |
|------|------|--------|
| `--port` | UDP监听端口 | 5354 |
| `--ws-port` | WebSocket服务端口 | 8080 |
| `--secret` | 用于HMAC验证的密钥 | "your-secret-key" |

### 配置

除了使用命令行参数，你也可以在`main.py`中修改以下默认参数：

```python
class DiscoveryService:
    def __init__(self, secret: str, port: int = 5354, ws_port: int = 8080):
        # ...
```

## 安全配置

1. 使用强密钥（至少32字符）
2. 定期轮换密钥
3. 在客户端和服务端使用相同的密钥
4. 确保网络环境受信任

## 故障排查

1. 如果遇到"Address already in use"错误，请尝试使用`--port`参数指定其他端口
2. 检查防火墙是否允许UDP通信
3. 验证客户端和服务端时间同步
4. 确认客户端和服务端使用相同的密钥
5. 检查网络是否支持广播

## 与Rust客户端配合使用

本服务发现协议服务端与小爱开放平台的Rust客户端兼容。确保在Rust客户端和Python服务端使用相同的密钥。

```rust
use client_rust::services::discovery;

async fn example() {
    // 创建服务发现实例
    let discovery_service = discovery::default_discovery().await;
    
    // 发现服务器
    let server_addr = discovery_service.discover().await.expect("Failed to discover server");
    
    println!("发现服务器: {}", server_addr);
}
```

**注意**：如果你修改了默认的UDP端口（5353），需要确保Rust客户端也使用相同的端口进行广播。