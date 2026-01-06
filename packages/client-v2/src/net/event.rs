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
use tokio::sync::broadcast;

// ==================== 事件类型 ====================

/// 客户端事件（Client → Server）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EventData {
    /// 测试事件
    Hello { message: String },
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

/// 事件总线（单向，只接收转发）
pub struct EventBus {
    tx: broadcast::Sender<(EventData, u128, SocketAddr)>,
}

pub type EventBusSubscription = EventSubscription<(EventData, u128, SocketAddr)>;

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// 收到事件
    pub fn receive(&self, event: EventData, timestamp: u128, addr: SocketAddr) {
        let _ = self.tx.send((event, timestamp, addr));
    }

    /// 订阅事件
    pub fn subscribe(&self) -> EventBusSubscription {
        EventSubscription {
            receiver: self.tx.subscribe(),
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(128)
    }
}
