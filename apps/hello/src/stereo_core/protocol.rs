use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Copy)]
pub enum ChannelRole {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlPacket {
    // 发现协议
    ServerHello {
        udp_port: u16, // UDP 音频流端口
    },
    // 握手协议
    ClientIdentify {
        role: ChannelRole,
    },
    // 时间同步 (持续进行)
    Ping {
        client_ts: u128,
        seq: u32,
    },
    Pong {
        client_ts: u128,
        server_ts: u128,
        seq: u32,
    },
    // 控制协议
    Volume(u8), // 音量 0-100
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioPacket {
    pub seq: u32,        // 序列号，用于丢包检测
    pub timestamp: u128, // 目标播放时间 (主节点时间)
    pub data: Vec<u8>,   // Opus 编码数据
}
