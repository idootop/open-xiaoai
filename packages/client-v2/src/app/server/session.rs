//! # Session - 客户端会话管理
//!
//! 轻量级的会话结构，专注于：
//! - TCP 控制连接
//! - RPC 管理
//! - 会话生命周期
//!
//! 音频流的实际处理由 AudioBus 和 Stream 模块负责。

use crate::audio::config::AudioConfig;
use crate::net::command::CommandResult;
use crate::net::network::Connection;
use crate::net::protocol::{ClientInfo, ControlPacket};
use crate::net::rpc::{RpcBuilder, RpcManager};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::stream::StreamHandle;

/// 活动流追踪
/// 用于追踪当前 session 的活动音频流
pub struct ActiveStreams {
    /// 当前录音流句柄
    pub recorder: Option<StreamHandle>,
    /// 当前播放流句柄
    pub playback: Option<StreamHandle>,
}

impl Default for ActiveStreams {
    fn default() -> Self {
        Self {
            recorder: None,
            playback: None,
        }
    }
}

impl ActiveStreams {
    /// 停止所有活动流
    pub fn stop_all(&mut self) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
        if let Some(h) = self.playback.take() {
            h.stop();
        }
    }

    /// 开始录音（停止之前的录音）
    pub fn start_recording(&mut self, handle: StreamHandle) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
        self.recorder = Some(handle);
    }

    /// 停止录音
    pub fn stop_recording(&mut self) {
        if let Some(h) = self.recorder.take() {
            h.stop();
        }
    }

    /// 开始播放（停止之前的播放）
    pub fn start_playback(&mut self, handle: StreamHandle) {
        if let Some(h) = self.playback.take() {
            h.stop();
        }
        self.playback = Some(handle);
    }

    /// 停止播放
    pub fn stop_playback(&mut self) {
        if let Some(h) = self.playback.take() {
            h.stop();
        }
    }

    /// 检查是否正在录音
    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// 检查是否正在播放
    pub fn is_playing(&self) -> bool {
        self.playback.is_some()
    }
}

/// 客户端会话
pub struct Session {
    /// 客户端信息
    pub info: ClientInfo,

    /// TCP 控制连接
    pub conn: Arc<Connection>,

    /// RPC 管理器
    pub rpc_manager: Arc<RpcManager>,

    /// TCP 地址（用作会话 ID）
    pub tcp_addr: SocketAddr,

    /// UDP 音频地址
    pub audio_addr: SocketAddr,

    /// 会话取消令牌
    pub cancel: CancellationToken,

    /// 活动流
    streams: parking_lot::Mutex<ActiveStreams>,

    /// 当前录音配置（如果正在录音）
    recording_config: parking_lot::Mutex<Option<AudioConfig>>,

    /// 会话创建时间
    created_at: std::time::Instant,
}

impl Session {
    /// 创建新会话
    pub fn new(
        info: ClientInfo,
        conn: Arc<Connection>,
        tcp_addr: SocketAddr,
        audio_addr: SocketAddr,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            info,
            conn,
            rpc_manager: Arc::new(RpcManager::new()),
            tcp_addr,
            audio_addr,
            cancel,
            streams: parking_lot::Mutex::new(ActiveStreams::default()),
            recording_config: parking_lot::Mutex::new(None),
            created_at: std::time::Instant::now(),
        }
    }

    /// 获取会话 ID（使用 TCP 地址）
    pub fn id(&self) -> SocketAddr {
        self.tcp_addr
    }

    /// 检查会话是否仍然有效
    pub fn is_alive(&self) -> bool {
        !self.cancel.is_cancelled()
    }

    /// 获取会话运行时间（秒）
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

    /// 发起 RPC 调用（支持超时和异步控制）
    pub async fn rpc(&self, request: &RpcBuilder) -> Result<CommandResult> {
        let result = self
            .rpc_manager
            .call(request, |pck| async move {
                return self.conn.send(&pck).await;
            })
            .await?;
        Ok(result)
    }

    /// 处理 RPC 响应
    pub fn resolve_rpc(&self, id: u32, result: CommandResult) {
        self.rpc_manager.resolve(id, result);
    }

    /// 开始录音流
    pub fn start_recording(&self, handle: StreamHandle, config: AudioConfig) {
        self.streams.lock().start_recording(handle);
        *self.recording_config.lock() = Some(config);
    }

    /// 停止录音流
    pub fn stop_recording(&self) {
        self.streams.lock().stop_recording();
        *self.recording_config.lock() = None;
    }

    /// 开始播放流
    pub fn start_playback(&self, handle: StreamHandle) {
        self.streams.lock().start_playback(handle);
    }

    /// 停止播放流
    pub fn stop_playback(&self) {
        self.streams.lock().stop_playback();
    }

    /// 检查是否正在录音
    pub fn is_recording(&self) -> bool {
        self.streams.lock().is_recording()
    }

    /// 检查是否正在播放
    pub fn is_playing(&self) -> bool {
        self.streams.lock().is_playing()
    }

    /// 清理所有资源
    pub fn cleanup(&self) {
        self.cancel.cancel();
        self.streams.lock().stop_all();
    }

    /// 获取当前录音配置
    pub fn recording_config(&self) -> Option<AudioConfig> {
        self.recording_config.lock().clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// 会话管理器
/// 负责管理所有客户端会话
pub struct SessionManager {
    sessions: dashmap::DashMap<SocketAddr, Arc<Session>>,
    /// 从 UDP 地址到 TCP 地址的映射
    udp_to_tcp: dashmap::DashMap<SocketAddr, SocketAddr>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: dashmap::DashMap::new(),
            udp_to_tcp: dashmap::DashMap::new(),
        }
    }

    /// 注册新会话
    pub fn register(&self, session: Arc<Session>) {
        let tcp_addr = session.tcp_addr;
        let audio_addr = session.audio_addr;

        self.udp_to_tcp.insert(audio_addr, tcp_addr);
        self.sessions.insert(tcp_addr, session);

        println!(
            "[SessionManager] Registered: {} (audio: {})",
            tcp_addr, audio_addr
        );
    }

    /// 注销会话
    pub fn unregister(&self, tcp_addr: &SocketAddr) -> Option<Arc<Session>> {
        if let Some((_, session)) = self.sessions.remove(tcp_addr) {
            self.udp_to_tcp.remove(&session.audio_addr);
            session.cleanup();
            println!(
                "[SessionManager] Unregistered: {} ({})",
                tcp_addr, session.info.model
            );
            Some(session)
        } else {
            None
        }
    }

    /// 通过 TCP 地址获取会话
    pub fn get(&self, tcp_addr: &SocketAddr) -> Option<Arc<Session>> {
        self.sessions.get(tcp_addr).map(|r| r.value().clone())
    }

    /// 通过 UDP 地址获取会话
    pub fn get_by_udp(&self, udp_addr: &SocketAddr) -> Option<Arc<Session>> {
        self.udp_to_tcp
            .get(udp_addr)
            .and_then(|tcp_addr| self.get(tcp_addr.value()))
    }

    /// 获取所有会话地址
    pub fn all_addrs(&self) -> Vec<SocketAddr> {
        self.sessions.iter().map(|r| *r.key()).collect()
    }

    /// 获取所有会话
    pub fn all_sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.iter().map(|r| r.value().clone()).collect()
    }

    /// 获取会话数量
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// 广播控制包到所有会话
    pub async fn broadcast(&self, packet: &ControlPacket) {
        for entry in self.sessions.iter() {
            let _ = entry.value().send(packet).await;
        }
    }

    /// 广播控制包到所有会话（除了指定的）
    pub async fn broadcast_except(&self, packet: &ControlPacket, except: &SocketAddr) {
        for entry in self.sessions.iter() {
            if entry.key() != except {
                let _ = entry.value().send(packet).await;
            }
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
