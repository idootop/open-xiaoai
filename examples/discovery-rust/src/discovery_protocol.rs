use tokio::net::UdpSocket;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use byteorder::{ByteOrder, BigEndian};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{SystemTime, UNIX_EPOCH};
use local_ip_address::local_ip;

const PACKET_SIZE: usize = 28;
const RESPONSE_SIZE: usize = 66; // This constant is not used, but kept for consistency with the original Python code.
const TIME_WINDOW_SECONDS: u64 = 30;

pub struct DiscoveryService {
    secret: Vec<u8>,
    port: u16,
    ws_port: u16,
    socket: Arc<UdpSocket>,
    running: Arc<AtomicBool>,
}

impl DiscoveryService {
    pub async fn new(secret: String, port: u16, ws_port: u16) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).await?;
        socket.set_broadcast(true)?;
        Ok(Self {
            secret: secret.into_bytes(),
            port,
            ws_port,
            socket: Arc::new(socket),
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn start(&self) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        self.running.store(true, Ordering::SeqCst);
        println!("🔍 发现服务监听在 UDP/{}, WS端口: {}", self.port, self.ws_port);

        let socket = self.socket.clone();
        let running = self.running.clone();
        let secret = self.secret.clone();
        let ws_port = self.ws_port;

        tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];
            while running.load(Ordering::SeqCst) {
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        let data = &buf[..len];
                        if Self::validate_packet(data) {
                            match Self::create_response(&secret, ws_port, data) {
                                Ok(response) => {
                                    if let Err(e) = socket.send_to(&response, addr).await {
                                        eprintln!("❌ 发送响应失败: {}", e);
                                    } else {
                                        println!("✅ 响应发现请求: {}:{}", addr.ip(), addr.port());
                                    }
                                }
                                Err(e) => eprintln!("❌ 创建响应失败: {}", e),
                            }
                        } else {
                            println!("❌ 无效的发现请求: {}:{}", addr.ip(), addr.port());
                        }
                    }
                    Err(e) => {
                        if running.load(Ordering::SeqCst) {
                            eprintln!("❌ 发现服务错误: {}", e);
                        }
                    }
                }
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        println!("🛑 正在停止服务...");
    }

    fn validate_packet(data: &[u8]) -> bool {
        if data.len() != PACKET_SIZE {
            return false;
        }

        let timestamp = BigEndian::read_u64(&data[20..28]);
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("时间回溯")
            .as_secs();

        current_time.abs_diff(timestamp) <= TIME_WINDOW_SECONDS
    }

    fn create_response(secret: &[u8], ws_port: u16, request: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut response = request[..PACKET_SIZE].to_vec();

        let my_local_ip = local_ip()?;
        if let std::net::IpAddr::V4(ipv4) = my_local_ip {
            response.extend_from_slice(&ipv4.octets());
        } else {
            return Err("无法获取IPv4地址".into());
        }

        let mut ws_port_bytes = [0u8; 2];
        BigEndian::write_u16(&mut ws_port_bytes, ws_port);
        response.extend_from_slice(&ws_port_bytes);

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret)
            .map_err(|_| "HMAC密钥错误")?;
        mac.update(&request[..PACKET_SIZE]);
        mac.update(&response[PACKET_SIZE..PACKET_SIZE + 6]); // IP + 端口
        let result = mac.finalize();
        let signature = result.into_bytes();
        response.extend_from_slice(&signature);

        Ok(response)
    }
}