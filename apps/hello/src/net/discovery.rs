use std::net::SocketAddr;
use tokio::net::UdpSocket;
use std::time::Duration;
use crate::net::protocol::ControlPacket;
use anyhow::Result;

pub const DISCOVERY_PORT: u16 = 8888;
pub const SERVER_PORT: u16 = 8889;

pub struct Discovery;

impl Discovery {
    pub async fn start_server_beacon() -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;
        
        let target_addr: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse()?;
        let hello = ControlPacket::ServerHello { port: SERVER_PORT };
        let msg = postcard::to_allocvec(&hello)?;

        tokio::spawn(async move {
            loop {
                let _ = socket.send_to(&msg, target_addr).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Ok(())
    }

    pub async fn client_discover_server() -> Result<SocketAddr> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
        let mut buf = [0u8; 1024];

        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;
            if let Ok(ControlPacket::ServerHello { port }) = postcard::from_bytes::<ControlPacket>(&buf[..len]) {
                let mut server_addr = addr;
                server_addr.set_port(port);
                return Ok(server_addr);
            }
        }
    }
}

