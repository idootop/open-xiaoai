use crate::audio::config::AudioConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub model: String,
    pub mac: String,
    pub version: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // 服务发现
    ServerHello {
        tcp_port: u16,
    },
    // 握手与认证
    ClientIdentify {
        info: DeviceInfo,
    },
    IdentifyOk,

    // 音频控制
    StartRecording {
        config: AudioConfig,
    },
    StopRecording,
    StartPlayback {
        config: AudioConfig,
    },
    StopPlayback,

    // RPC
    RpcRequest {
        id: u32,
        method: String,
        args: Vec<String>,
    },
    RpcResponse {
        id: u32,
        result: RpcResult,
    },

    // 心跳
    Ping,
    Pong,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RpcResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    pub data: Vec<u8>, // Opus 编码数据
}
