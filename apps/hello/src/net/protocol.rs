use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelRole {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // Discovery
    ServerHello {
        port: u16,
    },
    // Handshake
    ClientIdentify {
        role: ChannelRole,
    },
    // Time Sync
    Ping {
        client_ts: u128,
    },
    Pong {
        client_ts: u128,
        server_ts: u128,
    },
    // Stream control
    StartStreaming {
        start_time: u128, // Server time when streaming starts
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    pub timestamp: u128, // Target server time for playback
    pub data: Vec<u8>,   // Opus encoded data
}
