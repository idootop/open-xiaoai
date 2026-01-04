//! # RPC Manager - RPC 调用管理
//!
//! 管理 RPC 请求的生命周期，包括：
//! - 请求 ID 生成
//! - 请求/响应匹配
//! - 超时处理

use crate::net::command::CommandResult;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

/// RPC 调用错误
#[derive(Debug)]
pub enum RpcError {
    /// 超时
    Timeout,
    /// 通道关闭
    Cancelled,
    /// 未连接
    NotConnected,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Timeout => write!(f, "RPC timeout"),
            RpcError::Cancelled => write!(f, "RPC cancelled"),
            RpcError::NotConnected => write!(f, "Not connected"),
        }
    }
}

impl std::error::Error for RpcError {}

/// 等待中的 RPC 请求
struct PendingRequest {
    tx: oneshot::Sender<CommandResult>,
    created_at: std::time::Instant,
}

/// RPC 管理器
pub struct RpcManager {
    next_id: AtomicU32,
    pending: Mutex<HashMap<u32, PendingRequest>>,
    default_timeout: Duration,
}

impl RpcManager {
    /// 创建新的 RPC 管理器
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }

    /// 创建带自定义超时的 RPC 管理器
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            next_id: AtomicU32::new(1),
            pending: Mutex::new(HashMap::new()),
            default_timeout: timeout,
        }
    }

    /// 注册一个新的 RPC 请求
    ///
    /// 返回 (请求ID, 响应接收器)
    pub fn register(&self) -> (u32, oneshot::Receiver<CommandResult>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(
            id,
            PendingRequest {
                tx,
                created_at: std::time::Instant::now(),
            },
        );
        (id, rx)
    }

    /// 解析 RPC 响应
    pub fn resolve(&self, id: u32, result: CommandResult) {
        if let Some(req) = self.pending.lock().remove(&id) {
            let _ = req.tx.send(result);
        }
    }

    /// 取消指定的 RPC 请求
    pub fn cancel(&self, id: u32) {
        self.pending.lock().remove(&id);
    }

    /// 获取待处理请求数量
    pub fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    /// 清理超时的请求
    pub fn cleanup_expired(&self) {
        let mut pending = self.pending.lock();
        let now = std::time::Instant::now();

        pending.retain(|_, req| {
            if now.duration_since(req.created_at) > self.default_timeout {
                false
            } else {
                true
            }
        });
    }

    /// 获取默认超时时间
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }
}

impl Default for RpcManager {
    fn default() -> Self {
        Self::new()
    }
}

/// RPC 调用辅助函数
pub async fn call_with_timeout<F, Fut>(
    timeout: Duration,
    register_fn: F,
) -> Result<CommandResult, RpcError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<oneshot::Receiver<CommandResult>, RpcError>>,
{
    let rx = register_fn().await?;

    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(RpcError::Cancelled),
        Err(_) => Err(RpcError::Timeout),
    }
}
