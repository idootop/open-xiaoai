use crate::audio::config::AudioConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub model: String,
    pub mac: String,
    pub version: u32,
}

impl DeviceInfo {
    pub fn current() -> Self {
        Self {
            model: "Open-XiaoAi-V2".to_string(),
            mac: "00:00:00:00:00:00".to_string(), // TODO: Get actual MAC
            version: 1,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // Discovery
    ServerHello {
        tcp_port: u16,
        udp_port: u16,
    },
    // Handshake
    ClientIdentify {
        info: DeviceInfo,
        udp_port: u16,
    },
    IdentifyOk,

    // Audio Control
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

    // Heartbeat
    Ping,
    Pong,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RpcResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    pub data: Vec<u8>,
}
