use crate::stereo_core::protocol::{AudioPacket, ControlPacket};
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

    pub async fn punch(&self, target: SocketAddr) -> Result<()> {
        self.socket.send_to(&[0u8; 1], target).await?;
        Ok(())
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
        self.stream.write_all(&bytes).await?;
        Ok(())
    }

    pub async fn recv_packet(&mut self, buf: &mut [u8]) -> Result<ControlPacket> {
        let len = self.stream.read(buf).await?;
        if len == 0 {
            return Err(anyhow::anyhow!("连接已关闭"));
        }
        let packet = postcard::from_bytes(&buf[..len])?;
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
}

/// 主节点网络管理器
pub struct MasterNetwork {
    listener: TcpListener,
    audio: AudioSocket,
}

impl MasterNetwork {
    pub async fn setup(port: u16) -> Result<Self> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        let audio = AudioSocket::bind().await?;
        Ok(Self { listener, audio })
    }

    pub async fn accept(&self) -> Result<(ControlConnection, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        Ok((ControlConnection::new(stream), addr))
    }

    pub fn audio_socket(&self) -> &AudioSocket {
        &self.audio
    }
}

/// 从节点网络管理器
pub struct SlaveNetwork {
    control: ControlConnection,
    audio: AudioSocket,
}

impl SlaveNetwork {
    pub async fn connect(master_addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(master_addr)
            .await
            .context(format!("无法连接到主节点 TCP 地址: {}", master_addr))?;
        let audio = AudioSocket::bind().await?;
        Ok(Self {
            control: ControlConnection::new(stream),
            audio,
        })
    }

    pub fn split(self) -> (ControlConnection, AudioSocket) {
        (self.control, self.audio)
    }
}
