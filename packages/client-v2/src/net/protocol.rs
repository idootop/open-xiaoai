//! # Protocol - 通信协议定义
//!
//! 定义 Client 和 Server 之间的所有通信协议。

use crate::audio::config::AudioConfig;
use crate::net::command::{Command, CommandResult};
use crate::net::event::{ClientEvent, ServerEvent};
use serde::{Deserialize, Serialize};

// ==================== 基础类型 ====================

/// 客户端信息
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientInfo {
    pub model: String,
    pub serial_number: String,
}

// ==================== 控制包 ====================

/// 控制包 - TCP 通道传输的所有消息类型
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // ========== 服务发现 ==========
    /// 服务发现广播
    Discovery { protocol: String, port: u16 },

    // ========== 握手 ==========
    /// 服务端握手
    ServerHello {
        auth: String,
        version: String,
        udp_port: u16,
    },
    /// 客户端握手
    ClientHello {
        auth: String,
        version: String,
        udp_port: u16,
        info: ClientInfo,
    },

    // ========== 心跳 ==========
    Ping,
    Pong,

    // ========== RPC ==========
    /// RPC 请求（新版，使用 Command 类型）
    RpcRequest { id: u32, command: Command },
    /// RPC 响应（新版，使用 CommandResult 类型）
    RpcResponse { id: u32, result: CommandResult },

    // ========== 事件 ==========
    /// 服务端事件推送
    ServerEvent(ServerEvent),
    /// 客户端事件推送
    ClientEvent(ClientEvent),

    // ========== 音频控制 ==========
    /// 开始录音
    StartRecording { config: AudioConfig },
    /// 停止录音
    StopRecording,
    /// 开始播放
    StartPlayback { config: AudioConfig },
    /// 停止播放
    StopPlayback,

    // ========== 订阅管理 ==========
    /// 订阅事件类型
    Subscribe { event_types: Vec<String> },
    /// 取消订阅
    Unsubscribe { event_types: Vec<String> },
}

// ==================== 音频包 ====================

/// 音频数据包 - UDP 通道传输
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    /// Opus 编码的音频数据
    pub data: Vec<u8>,
}

// ==================== 兼容性：旧版 RPC 结果 ====================

/// 旧版 RPC 结果（保持向后兼容）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RpcResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl From<crate::net::command::ShellResponse> for RpcResult {
    fn from(resp: crate::net::command::ShellResponse) -> Self {
        Self {
            stdout: resp.stdout,
            stderr: resp.stderr,
            code: resp.exit_code,
        }
    }
}
