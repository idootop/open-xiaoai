//! # Server 模块
//!
//! 实时音频流服务器，支持：
//! - 多客户端连接管理
//! - 音频流发布-订阅
//! - 录音和播放
//! - RPC 远程调用
//! - 实时事件推送
//!
//! ## 架构
//!
//! ```text
//!                          ┌─────────────────────────────────────────────┐
//!                          │                  Server                     │
//!                          │                                             │
//!    Client 1 ──TCP────────┼──▶ SessionManager                           │
//!    Client 2 ──TCP────────┼──▶   ├─ Session 1                           │
//!    Client N ──TCP────────┼──▶   ├─ Session 2                           │
//!                          │      └─ Session N                           │
//!                          │            │                                │
//!         ┌────────────────┼────────────┴────────────────────────┐       │
//!         │                │                                     │       │
//!         ▼                │                                     ▼       │
//!    ┌─────────┐           │           ┌─────────────────────────────┐   │
//!    │Command  │           │           │         AudioBus            │   │
//!    │Handler  │           │           │                             │   │
//!    └─────────┘           │           │  ┌─────────┐  ┌──────────┐  │   │
//!         │                │           │  │Receiver │  │Broadcaster│  │   │
//!         ▼                │           │  │  Loop   │─▶│   Loop   │──┼───┼──▶ All Clients
//!    ┌─────────┐           │           │  └─────────┘  └──────────┘  │   │
//!    │ Event   │           │           └─────────────────────────────┘   │
//!    │  Bus    │───────────┼─────────────────────────────────────────────┼──▶ Events
//!    └─────────┘           │                                             │
//!                          └─────────────────────────────────────────────┘
//! ```

mod audio_bus;
mod session;
mod stream;

pub use audio_bus::{AudioBus, AudioFrame};
pub use session::{Session, SessionManager};
pub use stream::{FilePlaybackStream, RecorderStream, StreamHandle};

use crate::audio::config::AudioConfig;
use crate::audio::wav::WavReader;
use crate::net::command::{
    AudioState, Command, CommandError, CommandResult, DeviceInfo, ShellResponse,
};
use crate::net::discovery::Discovery;
use crate::net::event::{ClientEvent, ServerEvent, ServerEventBus};
use crate::net::network::Connection;
use crate::net::protocol::ControlPacket;
use crate::net::sync::now_us;
use anyhow::{Context, Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// 服务端配置
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 版本
    pub version: String,
    /// 客户端认证
    pub client_auth: String,
    /// 服务端认证
    pub server_auth: String,
    /// 连接超时（秒）
    pub timeout: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            server_auth: std::env::var("XIAO_SERVER_AUTH")
                .unwrap_or_else(|_| "xiao-server".to_string()),
            client_auth: std::env::var("XIAO_CLIENT_AUTH")
                .unwrap_or_else(|_| "xiao-client".to_string()),
            timeout: 60,
        }
    }
}

/// 实时音频服务器
pub struct Server {
    /// 配置
    config: ServerConfig,
    /// 会话管理器
    sessions: Arc<SessionManager>,
    /// 音频总线
    audio_bus: Arc<AudioBus>,
    /// 服务端事件总线
    event_bus: Arc<ServerEventBus>,
    /// 服务器取消令牌
    cancel: CancellationToken,
    /// 服务器启动时间
    started_at: std::time::Instant,
}

impl Server {
    /// 创建新服务器
    pub async fn new(config: ServerConfig) -> Result<Self> {
        let audio_bus = Arc::new(AudioBus::new().await?);

        Ok(Self {
            config,
            sessions: Arc::new(SessionManager::new()),
            audio_bus,
            event_bus: Arc::new(ServerEventBus::default()),
            cancel: CancellationToken::new(),
            started_at: std::time::Instant::now(),
        })
    }

    /// 获取事件总线（用于外部订阅）
    pub fn event_bus(&self) -> Arc<ServerEventBus> {
        self.event_bus.clone()
    }

    /// 启动服务器
    pub async fn run(self: Arc<Self>, port: u16) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        let addr = listener.local_addr()?;
        println!("[Server] Listening on TCP: {}", addr);
        println!("[Server] Audio UDP port: {}", self.audio_bus.port());

        // 广播服务发现
        Discovery::broadcast(port).await?;

        // 启动音频总线接收循环
        let bus = self.audio_bus.clone();
        tokio::spawn(async move {
            bus.run_receiver().await;
        });

        // 启动音频总线广播循环
        let bus = self.audio_bus.clone();
        tokio::spawn(async move {
            bus.run_broadcaster().await;
        });

        // TCP 连接接受循环
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    println!("[Server] Shutting down...");
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let server = self.clone();
                            tokio::spawn(async move {
                                if let Err(e) = server.handle_connection(stream, addr).await {
                                    eprintln!("[Server] Connection {} error: {}", addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("[Server] Accept error: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 处理新连接
    async fn handle_connection(
        self: Arc<Self>,
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
    ) -> Result<()> {
        println!("[Server] New connection from {}", addr);
        let conn = Arc::new(Connection::new(stream)?);

        // --- 握手 ---
        let (info, audio_addr) = self.handshake(&conn, addr).await?;
        println!(
            "[Server] Client identified: {} ({}) audio: {}",
            info.model, info.serial_number, audio_addr
        );

        // --- 创建 Session ---
        let session_cancel = self.cancel.child_token();
        let session = Arc::new(Session::new(
            info.clone(),
            conn.clone(),
            addr,
            audio_addr,
            session_cancel.clone(),
        ));

        // 注册到 SessionManager 和 AudioBus
        self.sessions.register(session.clone());
        self.audio_bus.register(audio_addr, true);

        // 发布客户端加入事件
        self.event_bus.publish(ServerEvent::ClientJoined {
            addr: addr.to_string(),
            model: info.model.clone(),
        });

        // 广播给其他客户端
        self.sessions
            .broadcast_except(
                &ControlPacket::ServerEvent(ServerEvent::ClientJoined {
                    addr: addr.to_string(),
                    model: info.model.clone(),
                }),
                &addr,
            )
            .await;

        // --- 主循环 ---
        let result = self.session_loop(session.clone()).await;

        // --- 清理 ---
        self.audio_bus.unregister(&audio_addr);
        self.sessions.unregister(&addr);

        // 发布客户端离开事件
        self.event_bus.publish(ServerEvent::ClientLeft {
            addr: addr.to_string(),
            model: info.model.clone(),
        });

        // 广播给其他客户端
        self.sessions
            .broadcast(&ControlPacket::ServerEvent(ServerEvent::ClientLeft {
                addr: addr.to_string(),
                model: info.model,
            }))
            .await;

        result
    }

    /// 握手流程
    async fn handshake(
        &self,
        conn: &Arc<Connection>,
        addr: SocketAddr,
    ) -> Result<(crate::net::protocol::ClientInfo, SocketAddr)> {
        // 等待客户端 Hello
        let (info, client_audio_port) = match conn.recv().await? {
            ControlPacket::ClientHello {
                auth,
                version: v,
                udp_port,
                info,
            } => {
                if v != self.config.version {
                    return Err(anyhow!("Client version mismatch"));
                }
                if auth != self.config.server_auth {
                    return Err(anyhow!("Invalid client auth"));
                }
                (info, udp_port)
            }
            _ => return Err(anyhow!("Expected ClientHello")),
        };

        // 发送服务器 Hello
        conn.send(&ControlPacket::ServerHello {
            auth: self.config.client_auth.clone(),
            version: self.config.version.clone(),
            udp_port: self.audio_bus.port(),
        })
        .await?;

        let audio_addr = SocketAddr::new(addr.ip(), client_audio_port);
        Ok((info, audio_addr))
    }

    /// Session 消息循环
    async fn session_loop(&self, session: Arc<Session>) -> Result<()> {
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
            ControlPacket::Ping { client_ts, seq } => {
                let pong = ControlPacket::Pong {
                    client_ts,
                    server_ts: now_us(),
                    seq,
                };
                session.send(&pong).await?;
            }
            ControlPacket::RpcResponse { id, result } => {
                session.resolve_rpc(id, result);
            }
            ControlPacket::RpcRequest { id, command } => {
                let result = self.handle_command(session, command).await;
                session
                    .send(&ControlPacket::RpcResponse { id, result })
                    .await?;
            }
            ControlPacket::ClientEvent(event) => {
                self.handle_client_event(session, event).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// 处理 RPC 命令
    async fn handle_command(&self, session: &Arc<Session>, command: Command) -> CommandResult {
        match command {
            Command::Shell(req) => {
                // 转发给客户端执行
                match session.execute(Command::Shell(req)).await {
                    Ok(result) => result,
                    Err(e) => CommandResult::Error(CommandError::internal(e.to_string())),
                }
            }
            Command::GetInfo => {
                // 返回服务器信息
                CommandResult::Info(DeviceInfo {
                    model: "XiaoAi-Server".to_string(),
                    serial_number: "SERVER-001".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    audio_state: AudioState {
                        is_recording: session.is_recording(),
                        is_playing: session.is_playing(),
                        volume: 100,
                    },
                })
            }
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
            Command::SetVolume(_) => {
                // 转发给客户端
                match session.execute(command).await {
                    Ok(result) => result,
                    Err(e) => CommandResult::Error(CommandError::internal(e.to_string())),
                }
            }
            _ => CommandResult::Error(CommandError::not_implemented()),
        }
    }

    /// 处理客户端事件
    async fn handle_client_event(&self, session: &Arc<Session>, event: ClientEvent) {
        match &event {
            ClientEvent::Alert { level, message } => {
                println!(
                    "[Event] Alert from {}: [{:?}] {}",
                    session.tcp_addr, level, message
                );
            }
            ClientEvent::AudioLevel {
                level_db,
                is_silent,
            } => {
                println!(
                    "[Event] Audio level from {}: {:.1}dB (silent: {})",
                    session.tcp_addr, level_db, is_silent
                );
            }
            _ => {}
        }

        // 可以在这里将客户端事件转发给其他订阅者
    }

    // ==================== 公开 API ====================

    /// 获取所有已连接的客户端地址
    pub async fn get_clients(&self) -> Vec<SocketAddr> {
        self.sessions.all_addrs()
    }

    /// 获取客户端数量
    pub fn client_count(&self) -> usize {
        self.sessions.count()
    }

    /// 获取服务器运行时间
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// 向客户端发起 RPC 调用（新版）
    pub async fn execute(&self, addr: SocketAddr, command: Command) -> Result<CommandResult> {
        let session = self.sessions.get(&addr).context("Session not found")?;
        session.execute(command).await
    }

    /// 执行 Shell 命令
    pub async fn shell(&self, addr: SocketAddr, cmd: &str) -> Result<ShellResponse> {
        let result = self.execute(addr, Command::shell(cmd)).await?;
        match result {
            CommandResult::Shell(resp) => Ok(resp),
            CommandResult::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Unexpected response type")),
        }
    }

    /// 推送事件给所有客户端
    pub async fn broadcast_event(&self, event: ServerEvent) {
        self.event_bus.publish(event.clone());
        self.sessions
            .broadcast(&ControlPacket::ServerEvent(event))
            .await;
    }

    /// 推送事件给指定客户端
    pub async fn send_event(&self, addr: SocketAddr, event: ServerEvent) -> Result<()> {
        let session = self.sessions.get(&addr).context("Session not found")?;
        self.event_bus.publish_to(addr, event.clone());
        session.send(&ControlPacket::ServerEvent(event)).await
    }

    /// 开始录音
    pub async fn start_record(&self, addr: SocketAddr, config: AudioConfig) -> Result<()> {
        let session = self.sessions.get(&addr).context("Session not found")?;

        let filename = format!(
            "temp/recorded_{}.wav",
            session.info.serial_number.replace(":", "")
        );

        // 创建录音流，订阅音频总线
        let handle = RecorderStream::spawn(
            config.clone(),
            filename.clone(),
            self.audio_bus.subscribe(),
            Some(session.audio_addr),
            session.cancel.clone(),
        );

        session.start_recording(handle, config.clone());

        // 通知客户端开始发送音频
        session
            .send(&ControlPacket::StartRecording { config })
            .await?;

        // 发布事件
        session
            .send(&ControlPacket::ServerEvent(
                ServerEvent::AudioStatusChanged {
                    is_recording: true,
                    is_playing: session.is_playing(),
                },
            ))
            .await?;

        println!("[Server] Recording started for {} -> {}", addr, filename);
        Ok(())
    }

    /// 停止录音
    pub async fn stop_record(&self, addr: SocketAddr) -> Result<()> {
        let session = self.sessions.get(&addr).context("Session not found")?;
        session.stop_recording();
        session.send(&ControlPacket::StopRecording).await?;

        // 发布事件
        session
            .send(&ControlPacket::ServerEvent(
                ServerEvent::AudioStatusChanged {
                    is_recording: false,
                    is_playing: session.is_playing(),
                },
            ))
            .await?;

        println!("[Server] Recording stopped for {}", addr);
        Ok(())
    }

    /// 开始播放
    pub async fn start_play(&self, addr: SocketAddr, file_path: &str) -> Result<()> {
        let session = self.sessions.get(&addr).context("Session not found")?;

        let reader = WavReader::open(file_path)?;

        // 通知客户端准备接收音频
        session
            .send(&ControlPacket::StartPlayback {
                config: reader.config.clone(),
            })
            .await?;

        // 创建播放流
        let handle = FilePlaybackStream::spawn(
            reader,
            self.audio_bus.socket(),
            session.audio_addr,
            session.cancel.clone(),
        );

        session.start_playback(handle);

        // 发布事件
        session
            .send(&ControlPacket::ServerEvent(
                ServerEvent::AudioStatusChanged {
                    is_recording: session.is_recording(),
                    is_playing: true,
                },
            ))
            .await?;

        println!("[Server] Playback started for {} from {}", addr, file_path);
        Ok(())
    }

    /// 停止播放
    pub async fn stop_play(&self, addr: SocketAddr) -> Result<()> {
        let session = self.sessions.get(&addr).context("Session not found")?;
        session.stop_playback();
        session.send(&ControlPacket::StopPlayback).await?;

        // 发布事件
        session
            .send(&ControlPacket::ServerEvent(
                ServerEvent::AudioStatusChanged {
                    is_recording: session.is_recording(),
                    is_playing: false,
                },
            ))
            .await?;

        println!("[Server] Playback stopped for {}", addr);
        Ok(())
    }

    /// 关闭服务器
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}
