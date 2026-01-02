use crate::net::protocol::ControlPacket;
use anyhow::Result;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

pub const DISCOVERY_PORT: u16 = 53530;

/// 服务发现模块，用于主从节点的自动发现
pub struct Discovery;

impl Discovery {
    /// 主节点：启动广播，告知从节点自己的 TCP 端口
    pub async fn start_broadcast(tcp_port: u16) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;

        let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse()?;
        let msg = postcard::to_allocvec(&ControlPacket::ServerHello { udp_port: tcp_port })?;

        tokio::spawn(async move {
            loop {
                let _ = socket.send_to(&msg, target).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Ok(())
    }

    /// 从节点：监听广播，发现主节点的 IP 和 TCP 端口
    pub async fn discover_master() -> Result<(IpAddr, u16)> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
        let mut buf = [0u8; 1024];

        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;
            if let Ok(ControlPacket::ServerHello { udp_port }) =
                postcard::from_bytes::<ControlPacket>(&buf[..len])
            {
                return Ok((addr.ip(), udp_port));
            }
        }
    }
}
