//! # Command - RPC 命令类型系统
//!
//! 支持多种类型的命令，每种命令有独立的请求和响应结构。
//!
//! ## 设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Command Types                            │
//! │                                                             │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │    Shell     │  │   GetInfo    │  │   SetVolume  │ ...  │
//! │  │  cmd → out   │  │  () → Info   │  │  vol → ()    │      │
//! │  └──────────────┘  └──────────────┘  └──────────────┘      │
//! │                                                             │
//! │                 ┌──────────────────┐                       │
//! │                 │   RpcRequest     │                       │
//! │                 │   id + Command   │                       │
//! │                 └────────┬─────────┘                       │
//! │                          │                                  │
//! │                          ▼                                  │
//! │                 ┌──────────────────┐                       │
//! │                 │   RpcResponse    │                       │
//! │                 │   id + Result    │                       │
//! │                 └──────────────────┘                       │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};

use crate::utils::shell::{ShellRequest, ShellResponse};

// ==================== 命令请求类型 ====================

// ==================== 统一命令枚举 ====================

/// RPC 命令 - 统一的请求类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    /// 执行 Shell 命令
    Shell(ShellRequest),
}

/// RPC 结果 - 统一的响应类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CommandResult {
    /// 错误
    Error(CommandError),
    /// Shell 命令结果
    Shell(ShellResponse),
}

/// 命令错误
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommandError {
    pub code: i32,
    pub message: String,
}

impl CommandError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(-1, msg)
    }

    pub fn invalid_args(msg: impl Into<String>) -> Self {
        Self::new(-2, msg)
    }

    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::new(-3, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(-500, msg)
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(-408, msg)
    }

    pub fn not_implemented() -> Self {
        Self::new(-501, "Not implemented")
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

// ==================== 便捷构造方法 ====================

impl CommandResult {
    /// 创建错误响应
    pub fn error(err: CommandError) -> Self {
        Self::Error(err)
    }

    /// 检查是否是错误
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// 获取错误（如果有）
    pub fn as_error(&self) -> Option<&CommandError> {
        match self {
            Self::Error(e) => Some(e),
            _ => None,
        }
    }
}
