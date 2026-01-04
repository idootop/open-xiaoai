//! # AudioBus - 音频总线
//!
//! 核心的音频路由模块，采用发布-订阅模式处理多客户端音频流。
//!
//! ## 设计理念
//!
//! ```text
//!                    ┌──────────────────────────────────────────┐
//!                    │              AudioBus                    │
//!                    │                                          │
//!   Client A ──UDP──▶│   ┌─────────┐      ┌──────────────────┐  │
//!                    │   │ Ingress │──────▶  BroadcastChannel │  │──▶ Client B, C...
//!   Client B ──UDP──▶│   │ Router  │      └──────────────────┘  │
//!                    │   └─────────┘                            │
//!                    │        │                                 │
//!                    │        ▼                                 │
//!                    │   ┌─────────────┐                        │
//!                    │   │  Recorder   │──▶ WAV File            │
//!                    │   └─────────────┘                        │
//!                    │                                          │
//!                    │   ┌─────────────┐                        │
//!                    │   │  Playback   │◀── WAV/MP3 File        │──▶ Client X
//!                    │   └─────────────┘                        │
//!                    └──────────────────────────────────────────┘
//! ```
//!
//! ## 核心概念
//!
//! - **Subscriber**: 订阅者，可以是客户端或录音器
//! - **Publisher**: 发布者，可以是客户端麦克风或文件播放器
//! - **Channel**: 频道，用于隔离不同的音频流组

use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// 订阅者 ID
pub type SubscriberId = u64;

/// 音频包及其来源
#[derive(Clone, Debug)]
pub struct AudioFrame {
    /// 音频数据包
    pub packet: AudioPacket,
    /// 发送者地址（如果来自网络）
    pub source: Option<SocketAddr>,
    /// 时间戳（单调递增）
    pub timestamp: u64,
}

/// 订阅者信息
#[derive(Clone)]
pub struct Subscriber {
    pub id: SubscriberId,
    /// UDP 目标地址
    pub addr: SocketAddr,
    /// 是否过滤自己的音频（防止回声）
    pub filter_self: bool,
}

/// 音频总线 - 负责音频流的路由和分发
pub struct AudioBus {
    /// UDP socket 用于收发音频
    socket: Arc<AudioSocket>,

    /// 广播通道 - 发布订阅模式的核心
    /// 所有接收到的音频都会广播到这个 channel
    broadcast_tx: broadcast::Sender<AudioFrame>,

    /// 订阅者注册表
    /// key: SocketAddr (UDP 地址)
    /// value: Subscriber
    subscribers: DashMap<SocketAddr, Subscriber>,

    /// 地址到订阅者 ID 的反向映射
    addr_to_id: DashMap<SocketAddr, SubscriberId>,

    /// ID 生成器
    next_id: AtomicU64,

    /// 全局时间戳
    timestamp: AtomicU64,
}

impl AudioBus {
    /// 创建新的音频总线
    pub async fn new() -> anyhow::Result<Self> {
        let socket = Arc::new(AudioSocket::bind().await?);
        // 广播 channel 容量，设置较大以容纳多个订阅者
        let (broadcast_tx, _) = broadcast::channel(256);

        Ok(Self {
            socket,
            broadcast_tx,
            subscribers: DashMap::new(),
            addr_to_id: DashMap::new(),
            next_id: AtomicU64::new(1),
            timestamp: AtomicU64::new(0),
        })
    }

    /// 获取 UDP 端口
    pub fn port(&self) -> u16 {
        self.socket.port()
    }

    /// 获取 socket 引用（用于外部发送）
    pub fn socket(&self) -> Arc<AudioSocket> {
        self.socket.clone()
    }

    /// 注册一个订阅者
    pub fn register(&self, addr: SocketAddr, filter_self: bool) -> SubscriberId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let subscriber = Subscriber {
            id,
            addr,
            filter_self,
        };
        self.subscribers.insert(addr, subscriber);
        self.addr_to_id.insert(addr, id);
        println!("[AudioBus] Subscriber {id} registered at {addr}");
        id
    }

    /// 注销订阅者
    pub fn unregister(&self, addr: &SocketAddr) {
        if let Some((_, sub)) = self.subscribers.remove(addr) {
            self.addr_to_id.remove(addr);
            println!("[AudioBus] Subscriber {} unregistered", sub.id);
        }
    }

    /// 订阅广播频道，返回一个 Receiver
    pub fn subscribe(&self) -> broadcast::Receiver<AudioFrame> {
        self.broadcast_tx.subscribe()
    }

    /// 发布音频帧到总线
    pub fn publish(&self, packet: AudioPacket, source: Option<SocketAddr>) {
        let ts = self.timestamp.fetch_add(1, Ordering::SeqCst);
        let frame = AudioFrame {
            packet,
            source,
            timestamp: ts,
        };
        // 忽略没有订阅者的情况
        let _ = self.broadcast_tx.send(frame);
    }

    /// 广播音频帧到所有订阅者（除了发送者自己）
    pub async fn broadcast(&self, frame: &AudioFrame) {
        for entry in self.subscribers.iter() {
            let sub = entry.value();

            // 如果启用了自过滤，跳过发送者自己
            if sub.filter_self {
                if let Some(src) = &frame.source {
                    if src == &sub.addr {
                        continue;
                    }
                }
            }

            // 发送音频包
            if let Err(e) = self.socket.send(&frame.packet, sub.addr).await {
                eprintln!("[AudioBus] Failed to send to {}: {}", sub.addr, e);
            }
        }
    }

    /// 发送音频包到指定地址
    pub async fn send_to(&self, packet: &AudioPacket, addr: SocketAddr) -> anyhow::Result<()> {
        self.socket.send(packet, addr).await
    }

    /// 启动 UDP 接收循环
    /// 这是一个独立的任务，负责：
    /// 1. 接收 UDP 音频包
    /// 2. 发布到广播频道
    pub async fn run_receiver(&self) {
        let mut buf = vec![0u8; 4096];
        loop {
            match self.socket.recv(&mut buf).await {
                Ok((packet, src_addr)) => {
                    // 只处理已注册的发送者
                    if self.subscribers.contains_key(&src_addr) {
                        self.publish(packet, Some(src_addr));
                    }
                }
                Err(e) => {
                    eprintln!("[AudioBus] Recv error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        }
    }

    /// 启动广播分发循环
    /// 订阅广播频道并将音频转发给所有订阅者
    pub async fn run_broadcaster(self: Arc<Self>) {
        let mut rx = self.subscribe();
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    self.broadcast(&frame).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[AudioBus] Broadcaster lagged {} frames", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    }

    /// 获取当前订阅者数量
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// 检查地址是否已注册
    pub fn is_registered(&self, addr: &SocketAddr) -> bool {
        self.subscribers.contains_key(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audio_bus_basic() {
        let bus = AudioBus::new().await.unwrap();
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        let id = bus.register(addr, true);
        assert!(bus.is_registered(&addr));
        assert_eq!(bus.subscriber_count(), 1);

        bus.unregister(&addr);
        assert!(!bus.is_registered(&addr));
        assert_eq!(bus.subscriber_count(), 0);
    }
}
