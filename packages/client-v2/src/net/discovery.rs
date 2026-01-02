use crate::net::protocol::ControlPacket;
use anyhow::Result;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

pub const DISCOVERY_PORT: u16 = 53530;
const DISCOVERY_MAGIC: &[u8] = b"XIAO_DISCOVERY_V2";

pub struct Discovery;

impl Discovery {
    pub async fn broadcast(tcp_port: u16, udp_port: u16) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;
        let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse()?;

        let mut msg = DISCOVERY_MAGIC.to_vec();
        msg.extend(postcard::to_allocvec(&ControlPacket::ServerHello {
            tcp_port,
            udp_port,
        })?);

        tokio::spawn(async move {
            loop {
                let _ = socket.send_to(&msg, target).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        Ok(())
    }

    pub async fn listen() -> Result<(IpAddr, u16, u16)> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
        let mut buf = [0u8; 1024];
        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;
            let data = &buf[..len];

            if data.starts_with(DISCOVERY_MAGIC) {
                let packet_data = &data[DISCOVERY_MAGIC.len()..];
                if let Ok(ControlPacket::ServerHello { tcp_port, udp_port }) =
                    postcard::from_bytes(packet_data)
                {
                    return Ok((addr.ip(), tcp_port, udp_port));
                }
            }
        }
    }
}
