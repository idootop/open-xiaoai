use crate::net::protocol::{AudioPacket, ControlPacket};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;

pub struct NetConfig {
    pub tcp_port: u16,
    pub udp_port: u16,
}

/// A unified control connection over TCP
pub struct Connection {
    reader: Mutex<tokio::net::tcp::OwnedReadHalf>,
    writer: Mutex<tokio::net::tcp::OwnedWriteHalf>,
    peer_addr: SocketAddr,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Result<Self> {
        let peer_addr = stream.peer_addr()?;
        let (r, w) = stream.into_split();
        Ok(Self {
            reader: Mutex::new(r),
            writer: Mutex::new(w),
            peer_addr,
        })
    }

    pub async fn send(&self, packet: &ControlPacket) -> Result<()> {
        let bytes = postcard::to_allocvec(packet)?;
        let mut writer = self.writer.lock().await;
        writer.write_u32(bytes.len() as u32).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn recv(&self) -> Result<ControlPacket> {
        let mut reader = self.reader.lock().await;
        let len = reader.read_u32().await? as usize;
        if len > 1024 * 1024 {
            return Err(anyhow::anyhow!("Packet too large: {}", len));
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).await?;
        let packet = postcard::from_bytes(&buf)?;
        Ok(packet)
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

/// UDP Socket for audio transmission
pub struct AudioSocket {
    socket: Arc<UdpSocket>,
}

impl AudioSocket {
    pub async fn bind() -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket: Arc::new(socket),
        })
    }

    pub fn port(&self) -> u16 {
        self.socket.local_addr().unwrap().port()
    }

    pub async fn send(&self, packet: &AudioPacket, target: SocketAddr) -> Result<()> {
        let bytes = postcard::to_allocvec(packet)?;
        self.socket.send_to(&bytes, target).await?;
        Ok(())
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<(AudioPacket, SocketAddr)> {
        let (len, addr) = self.socket.recv_from(buf).await?;
        let packet = postcard::from_bytes(&buf[..len])?;
        Ok((packet, addr))
    }
}
