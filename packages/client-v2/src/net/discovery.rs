use crate::net::protocol::ControlPacket;
use anyhow::Result;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

const DISCOVERY_PROTOCOL: &str = "XIAO_V2";

pub const DISCOVERY_PORT: u16 = 53530;

pub struct Discovery;

impl Discovery {
    pub async fn broadcast(port: u16) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;
        let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse()?;

        let msg = postcard::to_allocvec(&ControlPacket::Discovery {
            protocol: DISCOVERY_PROTOCOL.to_string(),
            port,
        })?;

        tokio::spawn(async move {
            loop {
                let _ = socket.send_to(&msg, target).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        Ok(())
    }

    pub async fn listen() -> Result<(IpAddr, u16)> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
        let mut buf = [0u8; 1024];
        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;
            let data = &buf[..len];
            if let Ok(ControlPacket::Discovery { protocol, port }) = postcard::from_bytes(data) {
                if protocol == DISCOVERY_PROTOCOL {
                    return Ok((addr.ip(), port));
                }
            }
        }
    }
}
