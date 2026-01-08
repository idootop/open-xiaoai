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

mod pipeline;
mod session;

pub use pipeline::{PipelineHandle, PlaybackPipeline, RecordPipeline};
pub use session::Session;

use crate::net::command::CommandResult;
use crate::net::discovery::Discovery;
use crate::net::event::{EventBus, EventBusSubscription, EventData};
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::ControlPacket;
use crate::net::rpc::RpcBuilder;
use crate::net::sync::now_us;
use anyhow::{Result, anyhow};
use session::handshake;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// 客户端配置
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// 版本
    pub version: String,
    /// 客户端认证
    pub client_auth: String,
    /// 服务端认证
    pub server_auth: String,
    /// 心跳间隔（毫秒）
    pub heartbeat_ms: u64,
    /// 连接超时（秒）
    pub timeout: u64,
    /// 客户端型号
    pub model: String,
    /// 序列号
    pub serial_number: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            server_auth: std::env::var("XIAO_SERVER_AUTH")
                .unwrap_or_else(|_| "xiao-server".to_string()),
            client_auth: std::env::var("XIAO_CLIENT_AUTH")
                .unwrap_or_else(|_| "xiao-client".to_string()),
            heartbeat_ms: 200,
            timeout: 60,
            // todo 获取设备信息
            model: "Open-XiaoAi-V2".to_string(),
            serial_number: "00:00:00:00:00:00".to_string(),
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
    /// 事件总线
    event_bus: Arc<EventBus>,
}

impl Client {
    /// 创建新客户端
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            session: RwLock::new(None),
            cancel: CancellationToken::new(),
            event_bus: Arc::new(EventBus::default()),
        }
    }

    /// 使用默认配置创建客户端
    pub fn with_defaults() -> Self {
        Self::new(ClientConfig::default())
    }

    /// 订阅事件
    pub fn subscribe_events(&self) -> EventBusSubscription {
        self.event_bus.subscribe()
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
        let handshake_result =
            handshake(&conn, audio_socket.port(), server_addr, &self.config).await?;

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
        let interval = std::time::Duration::from_millis(self.config.heartbeat_ms);

        tokio::spawn(async move {
            let mut seq = 0;
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = session.cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        let t1 = now_us();
                        let msg = ControlPacket::Ping { client_ts: t1, seq };
                        if session.send(&msg).await.is_err() {
                            break;
                        }
                        seq += 1;
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
            ControlPacket::Pong {
                client_ts,
                server_ts,
                ..
            } => {
                let t4 = now_us();
                session.update_clock(client_ts, server_ts, t4);
            }
            ControlPacket::Event { timestamp, data } => {
                self.event_bus.receive(data, timestamp, session.id());
            }
            ControlPacket::RpcResponse { id, result } => {
                session.resolve_rpc(id, result);
            }
            // 处理 RPC 请求（支持超时和异步控制）
            ControlPacket::RpcRequest {
                id,
                run_async,
                timeout,
                command,
            } => {
                let session_clone = session.clone();
                session
                    .rpc_manager
                    .handle_rpc_request(id, run_async, timeout, command, move |pck| {
                        let s = session_clone.clone();
                        async move {
                            return s.send(&pck).await;
                        }
                    })
                    .await?;
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
                    session.clock.clone(),
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
    pub async fn rpc(&self, builder: &RpcBuilder) -> Result<CommandResult> {
        let session_guard = self.session.read().await;
        if let Some(session) = session_guard.as_ref() {
            session.rpc(builder).await
        } else {
            Err(anyhow!("Not connected"))
        }
    }

    /// 发送事件
    pub async fn send_event(&self, event: EventData) -> Result<()> {
        let session_guard = self.session.read().await;
        if let Some(session) = session_guard.as_ref() {
            session
                .send(&ControlPacket::Event {
                    timestamp: now_us(),
                    data: event,
                })
                .await
        } else {
            Err(anyhow!("Not connected"))
        }
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
