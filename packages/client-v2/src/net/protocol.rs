use crate::audio::config::AudioConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientInfo {
    pub model: String,
    pub serial_number: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // Discovery
    Discovery {
        protocol: String,
        port: u16,
    },

    // Handshake
    ServerHello {
        auth: String,
        version: String,
        udp_port: u16, // for audio
    },
    ClientHello {
        auth: String,
        version: String,
        udp_port: u16, // for audio
        info: ClientInfo,
    },

    // Heartbeat
    Ping,
    Pong,

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

    // Audio Control
    StartRecording {
        config: AudioConfig,
    },
    StopRecording,
    StartPlayback {
        config: AudioConfig,
    },
    StopPlayback,
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
