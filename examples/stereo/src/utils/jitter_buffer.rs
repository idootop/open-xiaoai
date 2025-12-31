use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::net::protocol::AudioPacket;

#[derive(Debug)]
struct OrderedPacket {
    seq: u32,
    timestamp: u128,
    data: Vec<u8>,
}

// 仅针对序列号进行比较，处理回绕逻辑
impl PartialEq for OrderedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.seq == other.seq
    }
}

impl Eq for OrderedPacket {}

impl PartialOrd for OrderedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        // 使用 wrapping_sub 处理 u32 回绕 (Rollover)
        let diff = self.seq.wrapping_sub(other.seq) as i32;
        diff.cmp(&0)
    }
}

pub struct JitterBuffer {
    // 使用 Reverse 将 BinaryHeap 变为小顶堆，避免手动实现大量 Trait
    buffer: BinaryHeap<Reverse<OrderedPacket>>,
    last_played_seq: Option<u32>,
    pub target_delay_us: u128,
    min_packets: usize, // 最小缓冲包数，防止微小抖动
}

impl JitterBuffer {
    pub fn new(target_delay_us: u128, min_packets: usize) -> Self {
        Self {
            buffer: BinaryHeap::with_capacity(32), // 预分配初始容量
            last_played_seq: None,
            target_delay_us,
            min_packets,
        }
    }

    pub fn push(&mut self, packet: AudioPacket) {
        // 1. 处理序列号回绕的丢包逻辑
        if let Some(last) = self.last_played_seq {
            let diff = packet.seq.wrapping_sub(last) as i32;
            if diff <= 0 {
                return; // 这是一个延迟到达的旧包，直接丢弃
            }
        }

        self.buffer.push(Reverse(OrderedPacket {
            seq: packet.seq,
            timestamp: packet.timestamp,
            data: packet.data,
        }));
    }

    pub fn pop_frame(&mut self, current_time: u128) -> Option<(u32, Vec<u8>)> {
        // 2. 预缓冲逻辑：如果包量太少，先不播放，等待填充
        if self.buffer.len() < self.min_packets && self.last_played_seq.is_none() {
            return None;
        }

        // 3. 检查堆顶元素
        if let Some(Reverse(pkt)) = self.buffer.peek() {
            // 判断是否到达播放时间（考虑目标延迟）
            if current_time >= pkt.timestamp + (self.target_delay_us / 1000) {
                let Reverse(pkt) = self.buffer.pop().unwrap();
                self.last_played_seq = Some(pkt.seq);
                return Some((pkt.seq, pkt.data));
            }
        }
        None
    }

    /// 如果缓冲区堆积过大，可以主动跳帧以降低延迟
    pub fn shrink_to_fit_latency(&mut self, max_size: usize) {
        while self.buffer.len() > max_size {
            self.buffer.pop();
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last_played_seq = None;
    }
}
