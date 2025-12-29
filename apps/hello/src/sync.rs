use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_micros()
}

pub struct ClockSync {
    pub offset: i128, // server_time - client_time
}

impl ClockSync {
    pub fn new() -> Self {
        Self { offset: 0 }
    }

    pub fn update(&mut self, client_send_ts: u128, server_ts: u128, client_recv_ts: u128) {
        let rtt = (client_recv_ts - client_send_ts) as i128;
        // Estimated server time when client_recv_ts occurred: server_ts + rtt/2
        let estimated_server_now = server_ts as i128 + rtt / 2;
        self.offset = estimated_server_now - client_recv_ts as i128;
    }

    pub fn to_server_time(&self, client_time: u128) -> u128 {
        (client_time as i128 + self.offset) as u128
    }

    pub fn to_client_time(&self, server_time: u128) -> u128 {
        (server_time as i128 - self.offset) as u128
    }
}

