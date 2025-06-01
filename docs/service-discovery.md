# 服务发现协议

## 概述
本协议用于在局域网内自动发现服务端设备，支持Python、TypeScript和Rust实现。协议基于UDP广播，使用HMAC-SHA256进行身份验证。

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

## 实现参考

### Rust客户端
```rust
use crate::services::discovery::{DiscoveryService, DiscoveryConfig};

let discovery = DiscoveryService::new(DiscoveryConfig {
    secret: "your-secret-key".to_string(),
    broadcast_port: 5353,
    timeout_sec: 5,
});

let server_info = discovery.discover().await?;
```

### Python服务端
```python
from xiaozhi.services.discovery import DiscoveryService

discovery = DiscoveryService(secret="your-secret-key", ws_port=8080)
discovery.start()
```

### TypeScript服务端
```typescript
import { DiscoveryService } from './services/discovery';

const discovery = new DiscoveryService('your-secret-key', 8080);
discovery.start();
```

## 安全配置
1. 使用强密钥（至少32字符）
2. 定期轮换密钥
3. 在客户端和服务端使用相同的密钥
4. 确保网络环境受信任

## 故障排查
1. 检查防火墙是否允许UDP/5353
2. 验证客户端和服务端时间同步
3. 确认客户端和服务端使用相同的密钥
4. 检查网络是否支持广播