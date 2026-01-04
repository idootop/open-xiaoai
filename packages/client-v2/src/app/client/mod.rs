//! # Client 模块
//!
//! 音频客户端，支持：
//! - 服务发现和自动连接
//! - 音频录制和播放
//! - RPC 远程调用
//! - 实时事件处理
//!
//! ## 架构
//!
//! ```text
//!                     ┌─────────────────────────────────────┐
//!                     │              Client                 │
//!                     │                                     │
//!   Server ◀──TCP─────┼──▶ Session                          │
//!                     │      ├─ Connection                  │
//!                     │      ├─ RPC Manager                 │
//!                     │      └─ Active Pipelines            │
//!                     │                                     │
//!   Server ◀──UDP─────┼──▶ Audio Pipelines                  │
//!                     │      ├─ RecordPipeline              │
//!                     │      └─ PlaybackPipeline            │
//!                     │                                     │
//!   Events ◀──────────┼──▶ Event Handlers                   │
//!                     └─────────────────────────────────────┘
//! ```

#![cfg(target_os = "linux")]

mod pipeline;
mod session;

pub use pipeline::{PipelineHandle, PlaybackPipeline, RecordPipeline};
pub use session::Session;

use crate::net::command::{
    AudioState, Command, CommandError, CommandResult, DeviceInfo, SetVolumeResponse, ShellRequest,
    ShellResponse,
};
use crate::net::discovery::Discovery;
use crate::net::event::{ClientEvent, NotificationLevel, ServerEvent};
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{ClientInfo, ControlPacket};
use anyhow::{Result, anyhow};
use session::handshake;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;

/// 客户端配置
pub struct ClientConfig {
    /// 客户端型号
    pub model: String,
    /// 序列号
    pub serial_number: String,
    /// 心跳间隔（秒）
    pub heartbeat_interval: u64,
    /// 连接超时（秒）
    pub timeout: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            model: "Open-XiaoAi-V2".to_string(),
            serial_number: "00:00:00:00:00:00".to_string(),
            heartbeat_interval: 10,
            timeout: 60,
        }
    }
}

/// 音频客户端
pub struct Client {
    /// 配置
    config: ClientConfig,
    /// 当前活动会话
    session: RwLock<Option<Arc<Session>>>,
    /// 全局取消令牌
    cancel: CancellationToken,
    /// 服务端事件广播
    server_events: broadcast::Sender<ServerEvent>,
    /// 客户端启动时间
    started_at: std::time::Instant,
}

impl Client {
    /// 创建新客户端
    pub fn new(config: ClientConfig) -> Self {
        let (server_events, _) = broadcast::channel(64);
        Self {
            config,
            session: RwLock::new(None),
            cancel: CancellationToken::new(),
            server_events,
            started_at: std::time::Instant::now(),
        }
    }

    /// 使用默认配置创建客户端
    pub fn with_defaults() -> Self {
        Self::new(ClientConfig::default())
    }

    /// 订阅服务端事件
    pub fn subscribe_events(&self) -> broadcast::Receiver<ServerEvent> {
        self.server_events.subscribe()
    }

    /// 运行客户端（自动发现服务器并连接）
    pub async fn run(self: Arc<Self>) -> Result<()> {
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    println!("[Client] Shutting down...");
                    break;
                }
                result = self.discover_and_connect() => {
                    if let Err(e) = result {
                        eprintln!("[Client] Connection error: {}", e);
                    }
                    // 连接断开后清理并重试
                    self.cleanup().await;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
        Ok(())
    }

    /// 发现服务器并连接
    async fn discover_and_connect(&self) -> Result<()> {
        println!("[Client] Searching for server...");

        let (ip, tcp_port) = Discovery::listen().await?;
        let server_addr = SocketAddr::new(ip, tcp_port);
        println!("[Client] Found server at {}", server_addr);

        let stream = tokio::net::TcpStream::connect(server_addr).await?;
        println!("[Client] Connected to {}", server_addr);

        self.handle_session(stream, server_addr).await
    }

    /// 处理会话
    async fn handle_session(
        &self,
        stream: tokio::net::TcpStream,
        server_addr: SocketAddr,
    ) -> Result<()> {
        let conn = Arc::new(Connection::new(stream)?);
        let audio_socket = Arc::new(AudioSocket::bind().await?);

        // 执行握手
        let client_info = ClientInfo {
            model: self.config.model.clone(),
            serial_number: self.config.serial_number.clone(),
        };

        let handshake_result =
            handshake(&conn, audio_socket.port(), server_addr, client_info).await?;

        println!(
            "[Client] Handshake OK, server audio at {}",
            handshake_result.server_audio_addr
        );

        // 创建会话
        let session_cancel = self.cancel.child_token();
        let session = Arc::new(Session::new(
            conn.clone(),
            audio_socket,
            handshake_result.server_audio_addr,
            session_cancel.clone(),
        ));

        // 存储会话
        *self.session.write().await = Some(session.clone());

        // 启动心跳
        self.spawn_heartbeat(session.clone());

        // 运行消息循环
        self.message_loop(session).await
    }

    /// 启动心跳任务
    fn spawn_heartbeat(&self, session: Arc<Session>) {
        let interval = std::time::Duration::from_secs(self.config.heartbeat_interval);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = session.cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        if session.send(&ControlPacket::Ping).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }

    /// 消息主循环
    async fn message_loop(&self, session: Arc<Session>) -> Result<()> {
        let timeout = std::time::Duration::from_secs(self.config.timeout);

        loop {
            tokio::select! {
                _ = session.cancel.cancelled() => break,
                result = tokio::time::timeout(timeout, session.recv()) => {
                    match result {
                        Ok(Ok(packet)) => {
                            self.handle_packet(&session, packet).await?;
                        }
                        Ok(Err(e)) => {
                            return Err(anyhow!("Connection error: {}", e));
                        }
                        Err(_) => {
                            return Err(anyhow!("Connection timeout"));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 处理控制包
    async fn handle_packet(&self, session: &Arc<Session>, packet: ControlPacket) -> Result<()> {
        match packet {
            ControlPacket::Ping => {
                session.send(&ControlPacket::Pong).await?;
            }
            ControlPacket::Pong => {}
            ControlPacket::RpcResponse { id, result } => {
                session.resolve_rpc(id, result);
            }
            ControlPacket::RpcRequest { id, command } => {
                let result = self.handle_command(session, command).await;
                session
                    .send(&ControlPacket::RpcResponse { id, result })
                    .await?;
            }
            ControlPacket::ServerEvent(event) => {
                self.handle_server_event(session, event).await;
            }
            ControlPacket::StartRecording { config } => {
                println!("[Client] Starting recording...");
                let handle = RecordPipeline::spawn(
                    config,
                    session.audio_socket.clone(),
                    session.server_audio_addr,
                    session.cancel.clone(),
                );
                session.start_recording(handle);
            }
            ControlPacket::StopRecording => {
                println!("[Client] Stopping recording...");
                session.stop_recording();
            }
            ControlPacket::StartPlayback { config } => {
                println!("[Client] Starting playback...");
                let handle = PlaybackPipeline::spawn(
                    config,
                    session.audio_socket.clone(),
                    session.cancel.clone(),
                );
                session.start_playback(handle);
            }
            ControlPacket::StopPlayback => {
                println!("[Client] Stopping playback...");
                session.stop_playback();
            }
            _ => {}
        }
        Ok(())
    }

    /// 处理 RPC 命令
    async fn handle_command(&self, session: &Arc<Session>, command: Command) -> CommandResult {
        match command {
            Command::Shell(req) => self.handle_shell(req),
            Command::GetInfo => self.handle_get_info(session),
            Command::Ping { timestamp } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                CommandResult::Pong {
                    timestamp,
                    server_time: now,
                }
            }
            Command::SetVolume(req) => {
                let prev = session.set_volume(req.volume);
                CommandResult::Volume(SetVolumeResponse {
                    previous: prev,
                    current: session.volume(),
                })
            }
            _ => CommandResult::Error(CommandError::not_implemented()),
        }
    }

    /// 处理 Shell 命令
    fn handle_shell(&self, req: ShellRequest) -> CommandResult {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(&req.command);

        if let Some(cwd) = &req.cwd {
            cmd.current_dir(cwd);
        }

        if let Some(env) = &req.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        match cmd.output() {
            Ok(output) => CommandResult::Shell(ShellResponse {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }),
            Err(e) => CommandResult::Error(CommandError::internal(e.to_string())),
        }
    }

    /// 处理获取设备信息
    fn handle_get_info(&self, session: &Arc<Session>) -> CommandResult {
        CommandResult::Info(DeviceInfo {
            model: self.config.model.clone(),
            serial_number: self.config.serial_number.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: session.uptime_secs(),
            audio_state: AudioState {
                is_recording: session.is_recording(),
                is_playing: session.is_playing(),
                volume: session.volume(),
            },
        })
    }

    /// 处理服务端事件
    async fn handle_server_event(&self, _session: &Arc<Session>, event: ServerEvent) {
        // 广播给订阅者
        let _ = self.server_events.send(event.clone());

        // 本地处理
        match &event {
            ServerEvent::Notification {
                level,
                title,
                message,
            } => {
                println!("[Event] [{:?}] {}: {}", level, title, message);
            }
            ServerEvent::AudioStatusChanged {
                is_recording,
                is_playing,
            } => {
                println!(
                    "[Event] Audio status: recording={}, playing={}",
                    is_recording, is_playing
                );
            }
            ServerEvent::ClientJoined { addr, model } => {
                println!("[Event] Client joined: {} ({})", model, addr);
            }
            ServerEvent::ClientLeft { addr, model } => {
                println!("[Event] Client left: {} ({})", model, addr);
            }
            _ => {}
        }
    }

    /// 清理资源
    async fn cleanup(&self) {
        let mut session_guard = self.session.write().await;
        if let Some(session) = session_guard.take() {
            session.cleanup();
            println!("[Client] Session cleaned up");
        }
    }

    // ==================== 公开 API ====================

    /// 执行 RPC 命令
    pub async fn execute(&self, command: Command) -> Result<CommandResult> {
        let session_guard = self.session.read().await;
        if let Some(session) = session_guard.as_ref() {
            session.execute(command).await
        } else {
            Err(anyhow!("Not connected"))
        }
    }

    /// 执行 Shell 命令
    pub async fn shell(&self, cmd: &str) -> Result<ShellResponse> {
        let result = self.execute(Command::shell(cmd)).await?;
        match result {
            CommandResult::Shell(resp) => Ok(resp),
            CommandResult::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Unexpected response type")),
        }
    }

    /// 发送客户端事件
    pub async fn send_event(&self, event: ClientEvent) -> Result<()> {
        let session_guard = self.session.read().await;
        if let Some(session) = session_guard.as_ref() {
            session.send(&ControlPacket::ClientEvent(event)).await
        } else {
            Err(anyhow!("Not connected"))
        }
    }

    /// 发送警告事件
    pub async fn send_alert(&self, level: NotificationLevel, message: &str) -> Result<()> {
        self.send_event(ClientEvent::Alert {
            level,
            message: message.to_string(),
        })
        .await
    }

    /// 检查是否已连接
    pub async fn is_connected(&self) -> bool {
        self.session.read().await.is_some()
    }

    /// 关闭客户端
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}
