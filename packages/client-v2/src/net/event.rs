//! # Event - 实时事件系统
//!
//! 支持双向的实时事件推送，包括：
//! - 服务端事件（推送给客户端）
//! - 客户端事件（推送给服务端）
//! - 事件订阅和过滤
//!
//! ## 设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Event System                             │
//! │                                                             │
//! │  Server Events:                                             │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │ AudioStatus  │  │ ClientJoined │  │   Message    │ ...  │
//! │  └──────────────┘  └──────────────┘  └──────────────┘      │
//! │                                                             │
//! │  Client Events:                                             │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │ StatusUpdate │  │   Metrics    │  │    Alert     │ ...  │
//! │  └──────────────┘  └──────────────┘  └──────────────┘      │
//! │                                                             │
//! │                ┌─────────────────────┐                      │
//! │                │    EventBus         │                      │
//! │                │  broadcast channel  │                      │
//! │                └─────────────────────┘                      │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

// ==================== 事件类型 ====================

/// 服务端事件（Server → Client）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerEvent {
    /// 音频状态变化
    AudioStatusChanged {
        is_recording: bool,
        is_playing: bool,
    },

    /// 客户端加入（广播给其他客户端）
    ClientJoined { addr: String, model: String },

    /// 客户端离开
    ClientLeft { addr: String, model: String },

    /// 服务器消息/通知
    Notification {
        level: NotificationLevel,
        title: String,
        message: String,
    },

    /// 录音完成
    RecordingComplete {
        filename: String,
        duration_secs: f32,
        size_bytes: u64,
    },

    /// 播放完成
    PlaybackComplete { filename: String },

    /// 服务器状态更新
    ServerStatus {
        connected_clients: u32,
        uptime_secs: u64,
    },

    /// 自定义事件
    Custom { name: String, payload: Vec<u8> },
}

/// 客户端事件（Client → Server）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientEvent {
    /// 状态更新
    StatusUpdate {
        cpu_usage: f32,
        memory_usage: f32,
        temperature: Option<f32>,
    },

    /// 音频电平
    AudioLevel { level_db: f32, is_silent: bool },

    /// 按键事件
    KeyPress { key: String, action: KeyAction },

    /// 警告/错误
    Alert {
        level: NotificationLevel,
        message: String,
    },

    /// 自定义事件
    Custom { name: String, payload: Vec<u8> },
}

/// 通知级别
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// 按键动作
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Release,
    LongPress,
}

// ==================== 事件总线 ====================

/// 事件订阅者信息
pub struct EventSubscription<E> {
    pub receiver: broadcast::Receiver<E>,
}

impl<E: Clone> EventSubscription<E> {
    /// 接收下一个事件
    pub async fn recv(&mut self) -> Option<E> {
        match self.receiver.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // 跳过丢失的事件，继续接收
                Box::pin(self.recv()).await
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

/// 服务端事件总线
pub struct ServerEventBus {
    tx: broadcast::Sender<(Option<SocketAddr>, ServerEvent)>,
}

impl ServerEventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// 发布事件（广播给所有客户端）
    pub fn publish(&self, event: ServerEvent) {
        let _ = self.tx.send((None, event));
    }

    /// 发布事件给指定客户端
    pub fn publish_to(&self, addr: SocketAddr, event: ServerEvent) {
        let _ = self.tx.send((Some(addr), event));
    }

    /// 订阅事件
    pub fn subscribe(&self) -> EventSubscription<(Option<SocketAddr>, ServerEvent)> {
        EventSubscription {
            receiver: self.tx.subscribe(),
        }
    }
}

impl Default for ServerEventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

/// 客户端事件总线
pub struct ClientEventBus {
    tx: broadcast::Sender<(SocketAddr, ClientEvent)>,
}

impl ClientEventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// 发布客户端事件
    pub fn publish(&self, client_addr: SocketAddr, event: ClientEvent) {
        let _ = self.tx.send((client_addr, event));
    }

    /// 订阅事件
    pub fn subscribe(&self) -> EventSubscription<(SocketAddr, ClientEvent)> {
        EventSubscription {
            receiver: self.tx.subscribe(),
        }
    }
}

impl Default for ClientEventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

// ==================== 事件过滤器 ====================

/// 服务端事件过滤器
pub trait ServerEventFilter: Send + Sync {
    fn should_process(&self, event: &ServerEvent) -> bool;
}

/// 接受所有事件
pub struct AcceptAll;

impl ServerEventFilter for AcceptAll {
    fn should_process(&self, _: &ServerEvent) -> bool {
        true
    }
}

/// 只接受通知事件
pub struct NotificationsOnly;

impl ServerEventFilter for NotificationsOnly {
    fn should_process(&self, event: &ServerEvent) -> bool {
        matches!(event, ServerEvent::Notification { .. })
    }
}

/// 只接受音频相关事件
pub struct AudioEventsOnly;

impl ServerEventFilter for AudioEventsOnly {
    fn should_process(&self, event: &ServerEvent) -> bool {
        matches!(
            event,
            ServerEvent::AudioStatusChanged { .. }
                | ServerEvent::RecordingComplete { .. }
                | ServerEvent::PlaybackComplete { .. }
        )
    }
}

// ==================== 事件处理器 ====================

/// 事件处理器 trait
#[allow(async_fn_in_trait)]
pub trait EventHandler<E>: Send + Sync {
    async fn handle(&self, event: E);
}

/// 运行事件处理循环
pub async fn run_event_loop<E, H>(mut subscription: EventSubscription<E>, handler: Arc<H>)
where
    E: Clone + Send + 'static,
    H: EventHandler<E> + 'static,
{
    while let Some(event) = subscription.recv().await {
        handler.handle(event).await;
    }
}
