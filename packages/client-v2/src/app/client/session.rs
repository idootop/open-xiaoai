//! # Client Session - 客户端会话管理
//!
//! 轻量级的会话结构，负责：
//! - TCP 控制连接
//! - RPC 管理
//! - 会话生命周期
//! - 活动音频流追踪

use crate::net::command::{Command, CommandResult};
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{ClientInfo, ControlPacket};
use crate::net::rpc::RpcManager;
use anyhow::{Context, Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::{ClientConfig, pipeline::PipelineHandle};

/// 活动管道追踪
pub struct ActivePipelines {
    pub recorder: Option<PipelineHandle>,
    pub player: Option<PipelineHandle>,
}

impl Default for ActivePipelines {
    fn default() -> Self {
        Self {
            recorder: None,
            player: None,
        }
    }
}

impl ActivePipelines {
    pub fn stop_all(&mut self) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
        if let Some(h) = self.player.take() {
            h.stop();
        }
    }

    pub fn start_recording(&mut self, handle: PipelineHandle) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
        self.recorder = Some(handle);
    }

    pub fn stop_recording(&mut self) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
    }

    pub fn start_playback(&mut self, handle: PipelineHandle) {
        if let Some(h) = self.player.take() {
            h.stop();
        }
        self.player = Some(handle);
    }

    pub fn stop_playback(&mut self) {
        if let Some(h) = self.player.take() {
            h.stop();
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.player.is_some()
    }
}

/// 客户端会话
pub struct Session {
    /// TCP 控制连接
    pub conn: Arc<Connection>,

    /// UDP 音频 socket
    pub audio_socket: Arc<AudioSocket>,

    /// 服务器音频地址
    pub server_audio_addr: SocketAddr,

    /// RPC 管理器
    pub rpc: Arc<RpcManager>,

    /// 会话取消令牌
    pub cancel: CancellationToken,

    /// 活动管道
    pipelines: parking_lot::Mutex<ActivePipelines>,

    /// 会话创建时间
    created_at: std::time::Instant,

    /// 当前音量
    volume: parking_lot::Mutex<u8>,
}

impl Session {
    /// 创建新会话
    pub fn new(
        conn: Arc<Connection>,
        audio_socket: Arc<AudioSocket>,
        server_audio_addr: SocketAddr,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            conn,
            audio_socket,
            server_audio_addr,
            rpc: Arc::new(RpcManager::new()),
            cancel,
            pipelines: parking_lot::Mutex::new(ActivePipelines::default()),
            created_at: std::time::Instant::now(),
            volume: parking_lot::Mutex::new(100),
        }
    }

    /// 检查会话是否仍然有效
    pub fn is_alive(&self) -> bool {
        !self.cancel.is_cancelled()
    }

    /// 获取会话运行时间
    pub fn uptime_secs(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }

    /// 发送控制包
    pub async fn send(&self, packet: &ControlPacket) -> Result<()> {
        self.conn.send(packet).await
    }

    /// 接收控制包
    pub async fn recv(&self) -> Result<ControlPacket> {
        self.conn.recv().await
    }

    /// 发起 RPC 调用（新版）
    pub async fn execute(&self, command: Command) -> Result<CommandResult> {
        let (id, rx) = self.rpc.register();
        self.conn
            .send(&ControlPacket::RpcRequest { id, command })
            .await?;
        rx.await.context("RPC channel closed")
    }

    /// 处理 RPC 响应
    pub fn resolve_rpc(&self, id: u32, result: CommandResult) {
        self.rpc.resolve(id, result);
    }

    /// 开始录音管道
    pub fn start_recording(&self, handle: PipelineHandle) {
        self.pipelines.lock().start_recording(handle);
    }

    /// 停止录音管道
    pub fn stop_recording(&self) {
        self.pipelines.lock().stop_recording();
    }

    /// 开始播放管道
    pub fn start_playback(&self, handle: PipelineHandle) {
        self.pipelines.lock().start_playback(handle);
    }

    /// 停止播放管道
    pub fn stop_playback(&self) {
        self.pipelines.lock().stop_playback();
    }

    /// 检查是否正在录音
    pub fn is_recording(&self) -> bool {
        self.pipelines.lock().is_recording()
    }

    /// 检查是否正在播放
    pub fn is_playing(&self) -> bool {
        self.pipelines.lock().is_playing()
    }

    /// 获取当前音量
    pub fn volume(&self) -> u8 {
        *self.volume.lock()
    }

    /// 设置音量
    pub fn set_volume(&self, vol: u8) -> u8 {
        let mut v = self.volume.lock();
        let prev = *v;
        *v = vol.min(100);
        prev
    }

    /// 清理所有资源
    pub fn cleanup(&self) {
        self.cancel.cancel();
        self.pipelines.lock().stop_all();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// 握手结果
pub struct HandshakeResult {
    pub server_audio_addr: SocketAddr,
}

/// 执行客户端握手
pub async fn handshake(
    conn: &Connection,
    audio_port: u16,
    server_addr: SocketAddr,
    config: &ClientConfig,
) -> Result<HandshakeResult> {
    // 发送 ClientHello
    conn.send(&ControlPacket::ClientHello {
        udp_port: audio_port,
        auth: config.server_auth.clone(),
        version: config.version.clone(),
        info: ClientInfo {
            model: config.model.clone(),
            serial_number: config.serial_number.clone(),
        },
    })
    .await?;

    // 等待 ServerHello
    let server_udp_port = match conn.recv().await? {
        ControlPacket::ServerHello {
            version: v,
            udp_port,
            auth,
        } => {
            if v != config.version {
                return Err(anyhow!("Server version mismatch"));
            }
            if auth != config.client_auth {
                return Err(anyhow!("Invalid server auth"));
            }
            udp_port
        }
        _ => return Err(anyhow!("Expected ServerHello")),
    };

    let server_audio_addr = SocketAddr::new(server_addr.ip(), server_udp_port);
    Ok(HandshakeResult { server_audio_addr })
}
