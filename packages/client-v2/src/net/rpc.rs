//! # RPC Manager - RPC 调用管理
//!
//! 管理 RPC 请求的生命周期，包括：
//! - 请求 ID 生成
//! - 请求/响应匹配
//! - 超时处理
//! - 异步任务管理

use crate::net::command::{Command, CommandError, CommandResult};
use crate::net::protocol::ControlPacket;
use crate::utils::shell::Shell;
use anyhow::Result;
use dashmap::DashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("RPC timeout")]
    Timeout,
    #[error("RPC cancelled")]
    Cancelled,
    #[error("RPC internal error: {0}")]
    Internal(String),
}

pub struct RpcManager {
    handler: RpcHandler,
    next_id: AtomicU32,
    pending: Arc<DashMap<u32, oneshot::Sender<CommandResult>>>,
}

impl RpcManager {
    pub fn new() -> Self {
        Self {
            handler: RpcHandler::new(),
            next_id: AtomicU32::new(1),
            pending: Arc::new(DashMap::new()),
        }
    }

    pub fn resolve(&self, id: u32, result: CommandResult) {
        if let Some((_, tx)) = self.pending.remove(&id) {
            let _ = tx.send(result);
        }
    }

    pub fn cancel(&self, id: u32) {
        self.pending.remove(&id);
    }

    /// 发起异步 RPC 调用
    ///
    /// F: 异步闭包，输入 ID，返回 Future
    pub async fn call<F, Fut>(
        &self,
        builder: &RpcBuilder,
        send_fn: F,
    ) -> Result<CommandResult, RpcError>
    where
        F: FnOnce(ControlPacket) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let command = builder.command.clone();
        let timeout = builder.timeout;
        let run_async = builder.run_async;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        // 1. 注册请求
        self.pending.insert(id, tx);

        // 2. RAII 守卫：确保无论因超时、报错还是外部 Drop，ID 都会被清理
        let _guard = DropGuard {
            id,
            pending: Arc::clone(&self.pending),
        };

        // 3. 执行异步发送逻辑
        if let Err(e) = send_fn(ControlPacket::RpcRequest {
            id,
            command,
            timeout,
            run_async,
        })
        .await
        {
            return Err(RpcError::Internal(e.to_string()));
        }

        // 4. 等待响应或超时
        match timeout {
            Some(ms) => match tokio::time::timeout(Duration::from_millis(ms), rx).await {
                Ok(Ok(res)) => Ok(res),
                Ok(Err(_)) => Err(RpcError::Cancelled), // tx 关闭，任务被取消
                Err(_) => Err(RpcError::Timeout),
            },
            None => rx.await.map_err(|_| RpcError::Cancelled), // tx 关闭，任务被取消
        }
    }

    /// 处理 RPC 调用
    ///
    /// F: 异步闭包，输入 ID，返回 Future
    pub async fn handle_rpc_request<F, Fut>(
        &self,
        id: u32,
        run_async: bool,
        timeout_ms: Option<u64>,
        command: Command,
        send_fn: F,
    ) -> Result<()>
    where
        F: Fn(ControlPacket) -> Fut + Send + Sync + 'static, // 异步执行需要 'static
        Fut: Future<Output = Result<()>> + Send,
    {
        let sender = Arc::new(send_fn);
        let handler = self.handler.clone();

        let task = async move {
            let execute_future = handler.handle(command, timeout_ms);

            match timeout_ms {
                None => {
                    // 无超时逻辑
                    let result = execute_future.await;
                    sender(ControlPacket::RpcResponse { id, result }).await
                }
                Some(ms) => {
                    let d = Duration::from_millis(ms);
                    match tokio::time::timeout(d, execute_future).await {
                        Ok(result) => {
                            // 1. 正常完成
                            sender(ControlPacket::RpcResponse { id, result }).await
                        }
                        Err(_) => {
                            // 2. 触发超时：发送超时错误消息
                            let err_result = CommandResult::error(CommandError::timeout(format!(
                                "Task {} timed out after {}ms",
                                id, ms
                            )));
                            sender(ControlPacket::RpcResponse {
                                id,
                                result: err_result,
                            })
                            .await
                        }
                    }
                }
            }
        };

        if run_async {
            tokio::spawn(task);
            Ok(())
        } else {
            task.await
        }
    }
}

#[derive(Debug, Clone)]
pub struct RpcBuilder {
    pub command: Command,
    pub run_async: bool,
    pub timeout: Option<u64>,
}

impl RpcBuilder {
    /// 基础构造器
    pub fn new(command: Command) -> Self {
        Self {
            command,
            run_async: false,
            timeout: None,
        }
    }

    /// 便捷方法：默认异步配置
    pub fn default(command: Command) -> Self {
        Self::new(command).set_async(true).set_timeout(10_000)
    }

    /// 设置为异步运行
    pub fn set_async(mut self, is_async: bool) -> Self {
        self.run_async = is_async;
        self
    }

    /// 设置超时时长（毫秒）
    pub fn set_timeout(mut self, ms: u64) -> Self {
        self.timeout = Some(ms);
        self
    }
}

#[derive(Debug, Clone)]
pub struct RpcHandler;

impl RpcHandler {
    fn new() -> Self {
        Self {}
    }

    async fn handle(&self, command: Command, timeout_ms: Option<u64>) -> CommandResult {
        match command {
            Command::Shell(req) => match Shell::run(req, timeout_ms).await {
                Ok(resp) => CommandResult::Shell(resp),
                Err(e) => CommandResult::Error(CommandError::internal(e.to_string())),
            },
            _ => CommandResult::Error(CommandError::not_implemented()),
        }
    }
}

struct DropGuard {
    id: u32,
    pending: Arc<DashMap<u32, oneshot::Sender<CommandResult>>>,
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        self.pending.remove(&self.id);
    }
}
