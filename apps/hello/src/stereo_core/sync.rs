use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_micros()
}

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

    pub fn update(&mut self, client_send_ts: u128, server_ts: u128, client_recv_ts: u128) {
        let rtt = (client_recv_ts - client_send_ts) as i128;
        // Basic filter: ignore if RTT is absurdly large (e.g. > 100ms on LAN)
        if rtt > 100_000 {
            return;
        }

        // Clock offset = server_time - client_time
        // server_ts is at time (send + recv)/2
        let estimated_server_time = server_ts as i128 + rtt / 2;
        let offset = estimated_server_time - client_recv_ts as i128;

        self.offsets.push_back(offset);
        if self.offsets.len() > self.window_size {
            self.offsets.pop_front();
        }

        // Calculate median or average offset
        // Median is more robust to outliers
        let mut sorted: Vec<i128> = self.offsets.iter().cloned().collect();
        sorted.sort_unstable();
        if !sorted.is_empty() {
            self.current_offset = sorted[sorted.len() / 2];
        }
    }

    pub fn to_server_time(&self, client_time: u128) -> u128 {
        (client_time as i128 + self.current_offset) as u128
    }

    pub fn to_client_time(&self, server_time: u128) -> u128 {
        (server_time as i128 - self.current_offset) as u128
    }
}

