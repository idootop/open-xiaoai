use crate::net::protocol::{AudioPacket, ControlPacket};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// UDP 音频传输
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

    pub fn local_port(&self) -> Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    pub async fn send_packet(&self, packet: &AudioPacket, target: SocketAddr) -> Result<()> {
        let bytes = postcard::to_allocvec(packet)?;
        self.socket.send_to(&bytes, target).await?;
        Ok(())
    }

    pub async fn recv_packet(&self, buf: &mut [u8]) -> Result<(AudioPacket, SocketAddr)> {
        let (len, addr) = self.socket.recv_from(buf).await?;
        let packet = postcard::from_bytes(&buf[..len])?;
        Ok((packet, addr))
    }

    pub fn clone_inner(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }
}

/// TCP 控制连接
pub struct ControlConnection {
    stream: TcpStream,
}

impl ControlConnection {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub async fn send_packet(&mut self, packet: &ControlPacket) -> Result<()> {
        let bytes = postcard::to_allocvec(packet)?;
        let len = bytes.len() as u32;
        self.stream.write_u32(len).await?;
        self.stream.write_all(&bytes).await?;
        Ok(())
    }

    pub async fn recv_packet(&mut self) -> Result<ControlPacket> {
        let len = self.stream.read_u32().await? as usize;
        if len > 10 * 1024 * 1024 {
            return Err(anyhow::anyhow!("Packet too large: {}", len));
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;
        let packet = postcard::from_bytes(&buf)?;
        Ok(packet)
    }

    pub fn split(
        self,
    ) -> (
        tokio::net::tcp::OwnedReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        self.stream.into_split()
    }

    pub fn peer_addr(&self) -> Result<SocketAddr> {
        self.stream.peer_addr().context("Failed to get peer addr")
    }
}

/// 服务端网络管理器
pub struct ServerNetwork {
    listener: TcpListener,
}

impl ServerNetwork {
    pub async fn setup(port: u16) -> Result<Self> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        Ok(Self { listener })
    }

    pub async fn accept(&self) -> Result<(ControlConnection, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        Ok((ControlConnection::new(stream), addr))
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .context("Failed to get local addr")
    }
}

/// 客户端网络管理器
pub struct ClientNetwork {
    control: ControlConnection,
}

impl ClientNetwork {
    pub async fn connect(server_addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(server_addr)
            .await
            .context(format!("无法连接到服务端 TCP 地址: {}", server_addr))?;
        Ok(Self {
            control: ControlConnection::new(stream),
        })
    }

    pub fn into_control(self) -> ControlConnection {
        self.control
    }
}
