// 导入所需的外部依赖
use crate::base::AppError;                         // 应用错误类型
use crate::services::discovery::DiscoveryService;  // 服务发现接口特性
use hmac::{Hmac, Mac};                            // HMAC消息认证码实现
use sha2::Sha256;                                 // SHA-256哈希算法
use std::net::{Ipv4Addr, SocketAddr};             // IPv4地址和套接字地址类型
use std::time::{Duration, SystemTime, UNIX_EPOCH}; // 时间相关类型
use tokio::net::UdpSocket;                        // 异步UDP套接字
use tokio::time;                                  // 异步时间操作
use std::sync::Arc;                               // 原子引用计数智能指针

/// 定义HMAC-SHA256类型别名，用于消息认证
type HmacSha256 = Hmac<Sha256>;

/// 服务发现使用的UDP端口号
const DISCOVERY_PORT: u16 = 5354;
/// 等待服务器响应的超时时间（秒）
const RESPONSE_TIMEOUT: u64 = 3;
/// 设备唯一标识符，用于在发现包中标识客户端
const DEVICE_ID: [u8; 16] = *b"xiaoai-device-01";

/// UDP服务发现实现
/// 
/// 该结构体实现了通过UDP广播进行服务发现的功能。
/// 它使用HMAC-SHA256进行消息认证，确保只有合法的服务器才能被发现。
/// 工作流程：
/// 1. 创建一个带有设备ID、随机数和时间戳的发现包(28字节)
/// 2. 将发现包通过UDP广播发送到网络
/// 3. 等待服务器响应(66字节)
/// 4. 验证服务器响应中的HMAC签名
pub struct UdpDiscoveryService {
    /// UDP套接字，用于发送广播和接收响应
    /// 使用Arc包装以便在异步任务间共享
    socket: Arc<UdpSocket>,
    /// HMAC认证使用的共享密钥
    secret: String,
}

impl UdpDiscoveryService {
    /// 创建一个新的UDP服务发现实例
    /// 
    /// 该方法绑定一个UDP套接字到所有网络接口的随机端口，
    /// 并启用广播功能，以便发送服务发现广播包。
    /// 
    /// # 返回值
    /// 
    /// * `Result<Self, AppError>` - 成功时返回UdpDiscoveryService实例，失败时返回错误
    /// 
    /// # 错误
    /// 
    /// 如果无法绑定UDP套接字或设置广播选项，将返回错误
    pub async fn new(secret: &str) -> Result<Self, AppError> {
        // 绑定到所有网络接口的随机端口
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        // 启用广播功能
        socket.set_broadcast(true)?;
        
        Ok(Self {
            socket: Arc::new(socket),
            secret: secret.to_string(),
        })
    }

    /// 创建服务发现数据包
    /// 
    /// 该方法生成一个包含以下内容的发现数据包：
    /// - 设备ID (16字节)
    /// - 随机数 (4字节) - 防止重放攻击
    /// - 时间戳 (8字节) - 提供时效性验证
    ///
    /// 数据包格式：
    /// +----------------+-------------+---------------+
    /// |   设备ID       |   随机数    |    时间戳      |
    /// | (16 bytes)    | (4 bytes)   |  (8 bytes)    |
    /// +----------------+-------------+---------------+
    /// 
    /// # 返回值
    /// 
    /// * `Result<Vec<u8>, AppError>` - 成功时返回序列化的数据包，失败时返回错误
    /// 
    /// # 错误
    /// 
    /// 如果无法创建HMAC或获取系统时间，将返回错误
    fn create_discovery_packet(&self) -> Result<Vec<u8>, AppError> {
        // 生成随机数，用于防止重放攻击
        let nonce = rand::random::<u32>();
        // 获取当前UNIX时间戳（秒）
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        // 创建数据包，预分配足够的容量
        let mut packet = Vec::with_capacity(28); // 16+4+8=28字节
        // 按顺序添加各个字段
        packet.extend_from_slice(&DEVICE_ID);           // 16字节
        packet.extend_from_slice(&nonce.to_be_bytes()); // 4字节
        packet.extend_from_slice(&timestamp.to_be_bytes()); // 8字节

        Ok(packet)
    }

    /// 解析服务器响应数据包
    /// 
    /// 该方法从服务器响应中提取服务器的IP地址和端口号并验证HMAC签名。
    /// 响应数据包格式：
    /// +----------------+-------------+---------------+---------------+-------------+-----------------+
    /// |   设备ID       |   随机数    |    时间戳      |    IP地址     |    端口     |      HMAC      |
    /// | (16 bytes)    | (4 bytes)   |  (8 bytes)    |   (4 bytes)   |  (2 bytes)  |   (32 bytes)   |
    /// +----------------+-------------+---------------+---------------+-------------+-----------------+
    /// 
    /// # 参数
    /// 
    /// * `response` - 服务器响应的字节数组
    /// 
    /// # 返回值
    /// 
    /// * `Result<SocketAddr, AppError>` - 成功时返回服务器的套接字地址，失败时返回错误
    /// 
    /// # 错误
    /// 
    /// 如果响应长度不足或格式不正确，将返回错误
    fn parse_response(&self, response: &[u8]) -> Result<SocketAddr, AppError> {
        // 检查响应长度是否足够包含IP地址、端口和HMAC（至少需要66字节）
        if response.len() < 66 { // 28(原始请求) + 4(IP) + 2(端口) + 32(HMAC) = 66
            return Err("Invalid response length".into());
        }

        // 验证服务端HMAC签名
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())?;
        mac.update(&response[..34]); // 原始请求(28) + IP(4) + port(2)
        let calculated_hmac = mac.finalize().into_bytes();
        
        if !calculated_hmac[..].eq(&response[34..66]) {
            return Err("Invalid HMAC in response".into());
        }

        // 从响应中提取IP地址（4字节）
        let ip = Ipv4Addr::new(response[28], response[29], response[30], response[31]);
        // 从响应中提取端口号（2字节，大端序）
        let port = u16::from_be_bytes([response[32], response[33]]);
        
        // 打印解析结果
        println!("🔍 解析服务端地址: IP={}, Port={}", ip, port);
        
        // 创建并返回套接字地址
        Ok(SocketAddr::new(ip.into(), port))
    }
}

/// 为UdpDiscoveryService实现DiscoveryService特性
impl DiscoveryService for UdpDiscoveryService {
    /// 实现服务发现方法
    /// 
    /// 该方法通过以下步骤发现服务器：
    /// 1. 创建一个认证的发现数据包
    /// 2. 将数据包广播到网络上的所有设备
    /// 3. 等待服务器响应，直到超时
    /// 4. 验证响应并提取服务器地址
    /// 
    /// # 返回值
    /// 
    /// * `Result<std::net::SocketAddr, AppError>` - 成功时返回服务器的套接字地址，失败时返回错误
    /// 
    /// # 错误
    /// 
    /// - 如果无法创建发现数据包，将返回错误
    /// - 如果无法发送广播，将返回错误
    /// - 如果在超时时间内未收到有效响应，将返回超时错误
    /// - 如果接收响应时发生错误，将返回相应错误
    fn discover(&self) -> impl std::future::Future<Output = Result<std::net::SocketAddr, AppError>> + Send {
        async move {
        // 创建发现数据包
        let packet = self.create_discovery_packet()?;
        // 创建广播地址（255.255.255.255:5353）
        let broadcast_addr = std::net::SocketAddr::new(Ipv4Addr::BROADCAST.into(), DISCOVERY_PORT);
        
        // 发送广播数据包
        self.socket.send_to(&packet, &broadcast_addr).await?;
        
        // 准备接收缓冲区
        let mut buf = [0; 1024];
        // 记录开始时间，用于计算超时
        let start_time = SystemTime::now();
        // 克隆套接字引用，以便在异步闭包中使用
        let socket_clone = Arc::clone(&self.socket);
        
        // 循环等待响应，直到超时
        loop {
            // 计算已经过去的时间
            let elapsed = start_time.elapsed()?.as_secs();
            // 如果已经超时，返回错误
            if elapsed >= RESPONSE_TIMEOUT {
                return Err("Discovery timeout".into());
            }
            // 计算剩余的超时时间
            let remaining = Duration::from_secs(RESPONSE_TIMEOUT - elapsed);
            // 创建超时Future
            let timeout = time::sleep(remaining);
            
            // 创建接收数据的Future
            let recv_fut = async {
                let (size, _) = socket_clone.recv_from(&mut buf).await?;
                Ok::<_, AppError>((size, buf))
            };
            
            // 将Future固定到栈上，以便在select中使用
            tokio::pin!(timeout);
            tokio::pin!(recv_fut);
            
            // 使用select同时等待超时和接收数据
            tokio::select! {
                // 如果超时先发生
                _ = &mut timeout => {
                    return Err("Discovery timeout".into());
                }
                // 如果接收到数据
                result = &mut recv_fut => {
                    match result {
                        Ok((size, buf)) => {
                            // 提取接收到的响应
                            let response = &buf[..size];
                            // 验证响应的前32字节是否与请求匹配（设备ID+随机数+时间戳）
                            // 这确保响应是针对我们的请求的
                            if response.starts_with(&packet) {
                                // 解析响应并返回服务器地址
                                return self.parse_response(response);
                            }
                            // 如果不匹配，继续循环等待下一个响应
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        }
    }
}