use super::protocol::AudioPacket;
use std::collections::BinaryHeap;
use std::cmp::Ordering;

#[derive(Debug)]
struct OrderedPacket(AudioPacket);

impl PartialEq for OrderedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.0.seq == other.0.seq
    }
}
impl Eq for OrderedPacket {}
impl PartialOrd for OrderedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(other.0.seq.cmp(&self.0.seq))
    }
}
impl Ord for OrderedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.seq.cmp(&self.0.seq)
    }
}

/// 抖动缓冲区，用于处理网络延迟和乱序
pub struct JitterBuffer {
    buffer: BinaryHeap<OrderedPacket>,
    last_played_seq: Option<u32>,
    pub target_delay_us: u128,
}

impl JitterBuffer {
    pub fn new(target_delay_us: u128) -> Self {
        Self {
            buffer: BinaryHeap::new(),
            last_played_seq: None,
            target_delay_us,
        }
    }

    pub fn push(&mut self, packet: AudioPacket) {
        // 丢弃已经播放过的数据包
        if let Some(last) = self.last_played_seq {
            if packet.seq <= last {
                return;
            }
        }
        self.buffer.push(OrderedPacket(packet));
    }

    /// 根据当前时间弹出待播放的帧
    pub fn pop_frame(&mut self, current_time: u128) -> Option<(u32, Vec<u8>)> {
        if let Some(OrderedPacket(pkt)) = self.buffer.peek() {
             if current_time >= pkt.timestamp {
                 let pkt = self.buffer.pop().unwrap().0;
                 self.last_played_seq = Some(pkt.seq);
                 return Some((pkt.seq, pkt.data));
             }
        }
        None
    }
    
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last_played_seq = None;
    }
}
