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

// ==================== 命令请求类型 ====================

/// Shell 命令请求
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShellRequest {
    /// 要执行的命令
    pub command: String,
    /// 工作目录（可选）
    pub cwd: Option<String>,
    /// 环境变量（可选）
    pub env: Option<Vec<(String, String)>>,
    /// 超时秒数（可选）
    pub timeout_secs: Option<u32>,
}

/// Shell 命令响应
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ShellResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 获取设备信息请求
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GetInfoRequest;

/// 设备信息响应
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeviceInfo {
    pub model: String,
    pub serial_number: String,
    pub version: String,
    pub uptime_secs: u64,
    pub audio_state: AudioState,
}

/// 音频状态
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AudioState {
    pub is_recording: bool,
    pub is_playing: bool,
    pub volume: u8,
}

/// 设置音量请求
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetVolumeRequest {
    pub volume: u8, // 0-100
}

/// 设置音量响应
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetVolumeResponse {
    pub previous: u8,
    pub current: u8,
}

/// 文件操作请求
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FileRequest {
    /// 读取文件
    Read { path: String },
    /// 写入文件
    Write { path: String, data: Vec<u8> },
    /// 删除文件
    Delete { path: String },
    /// 列出目录
    List { path: String },
    /// 获取文件信息
    Stat { path: String },
}

/// 文件操作响应
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FileResponse {
    /// 读取结果
    Data(Vec<u8>),
    /// 写入成功
    Written { bytes: usize },
    /// 删除成功
    Deleted,
    /// 目录列表
    Entries(Vec<FileEntry>),
    /// 文件信息
    Stat(FileStat),
}

/// 文件条目
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// 文件状态
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub is_dir: bool,
    pub modified: u64,
}

/// 系统控制请求
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SystemRequest {
    /// 重启
    Reboot,
    /// 关机
    Shutdown,
    /// 获取系统负载
    GetLoad,
    /// 获取内存使用
    GetMemory,
}

/// 系统控制响应
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SystemResponse {
    /// 操作已接受
    Accepted,
    /// 系统负载
    Load { one: f32, five: f32, fifteen: f32 },
    /// 内存使用
    Memory { total: u64, used: u64, free: u64 },
}

/// 自定义命令请求（用于扩展）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomRequest {
    pub name: String,
    pub payload: Vec<u8>,
}

/// 自定义命令响应
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomResponse {
    pub payload: Vec<u8>,
}

// ==================== 统一命令枚举 ====================

/// RPC 命令 - 统一的请求类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    /// 执行 Shell 命令
    Shell(ShellRequest),
    /// 获取设备信息
    GetInfo,
    /// 设置音量
    SetVolume(SetVolumeRequest),
    /// 文件操作
    File(FileRequest),
    /// 系统控制
    System(SystemRequest),
    /// 自定义命令
    Custom(CustomRequest),
    /// Ping（用于测量延迟）
    Ping { timestamp: u64 },
}

/// RPC 结果 - 统一的响应类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CommandResult {
    /// Shell 命令结果
    Shell(ShellResponse),
    /// 设备信息
    Info(DeviceInfo),
    /// 音量设置结果
    Volume(SetVolumeResponse),
    /// 文件操作结果
    File(FileResponse),
    /// 系统控制结果
    System(SystemResponse),
    /// 自定义命令结果
    Custom(CustomResponse),
    /// Pong 响应
    Pong { timestamp: u64, server_time: u64 },
    /// 错误
    Error(CommandError),
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

impl Command {
    /// 创建 Shell 命令
    pub fn shell(cmd: impl Into<String>) -> Self {
        Self::Shell(ShellRequest {
            command: cmd.into(),
            cwd: None,
            env: None,
            timeout_secs: None,
        })
    }

    /// 创建带超时的 Shell 命令
    pub fn shell_with_timeout(cmd: impl Into<String>, timeout: u32) -> Self {
        Self::Shell(ShellRequest {
            command: cmd.into(),
            cwd: None,
            env: None,
            timeout_secs: Some(timeout),
        })
    }

    /// 创建 Ping 命令
    pub fn ping() -> Self {
        Self::Ping {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }
}

impl CommandResult {
    /// 创建成功的 Shell 响应
    pub fn shell_ok(stdout: String) -> Self {
        Self::Shell(ShellResponse {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        })
    }

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
