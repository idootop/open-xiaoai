use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Copy)]
pub enum ChannelRole {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // Discovery
    ServerHello {
        udp_port: u16, // Port for UDP audio stream
    },
    // Handshake
    ClientIdentify {
        role: ChannelRole,
    },
    // Time Sync (Continuous)
    Ping {
        client_ts: u128,
        seq: u32,
    },
    Pong {
        client_ts: u128,
        server_ts: u128,
        seq: u32,
    },
    // Control
    Volume(u8), // 0-100
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    pub seq: u32,        // Sequence number for packet loss detection
    pub timestamp: u128, // Target playback time (server time)
    pub data: Vec<u8>,   // Opus encoded data
}

