use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// 获取当前微秒级时间戳
pub fn now_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("时间倒流")
        .as_micros()
}

/// 时钟同步管理器，用于计算主从节点间的时钟偏移
pub struct ClockSync {
    offsets: VecDeque<i128>,
    pub current_offset: i128,
    window_size: usize,
}

impl ClockSync {
    pub fn new(window_size: usize) -> Self {
        Self {
            offsets: VecDeque::with_capacity(window_size),
            current_offset: 0,
            window_size,
        }
    }

    /// 更新时钟偏移估计
    pub fn update(&mut self, client_send_ts: u128, server_ts: u128, client_recv_ts: u128) {
        let rtt = (client_recv_ts - client_send_ts) as i128;
        // 基础过滤：如果 RTT 过大则忽略 (例如局域网内 > 100ms)
        if rtt > 100_000 {
            return;
        }

        // 时钟偏移 = 主节点时间 - 从节点时间
        // 假设主节点收到 Ping 的时间点在 (发送时间 + 接收时间) / 2
        let estimated_server_time = server_ts as i128 + rtt / 2;
        let offset = estimated_server_time - client_recv_ts as i128;

        self.offsets.push_back(offset);
        if self.offsets.len() > self.window_size {
            self.offsets.pop_front();
        }

        // 计算中位数偏移，以增强抗干扰能力
        let mut sorted: Vec<i128> = self.offsets.iter().cloned().collect();
        sorted.sort_unstable();
        if !sorted.is_empty() {
            self.current_offset = sorted[sorted.len() / 2];
        }
    }

    /// 将本地时间转换为服务器(主节点)时间
    pub fn to_server_time(&self, client_time: u128) -> u128 {
        (client_time as i128 + self.current_offset) as u128
    }

    /// 将服务器(主节点)时间转换为本地时间
    pub fn to_client_time(&self, server_time: u128) -> u128 {
        (server_time as i128 - self.current_offset) as u128
    }
}
