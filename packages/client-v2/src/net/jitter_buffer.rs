//! # Jitter Buffer - 抖动缓冲区
//!
//! 用于音频流的抖动缓冲和包重排序。
//!
//! ## 功能
//! - 自适应缓冲区大小
//! - 乱序包重排
//! - 丢包检测和统计
//! - 延迟统计

use crate::net::protocol::AudioPacket;
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

impl PartialEq for AudioPacket {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}
impl Eq for AudioPacket {}
impl Ord for AudioPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        other.timestamp.cmp(&self.timestamp)
    }
}
impl PartialOrd for AudioPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Jitter Buffer 统计信息
#[derive(Debug, Clone, Default)]
pub struct JitterStats {
    /// 总接收包数
    pub received: u64,
    /// 总丢包数
    pub lost: u64,
    /// 总播放包数
    pub played: u64,
    /// 迟到的包数（到达时已过播放时间）
    pub late: u64,
    /// 重复包数
    pub duplicate: u64,
    /// 当前缓冲区大小（包数）
    pub buffer_size: usize,
    /// 最小延迟（微秒）
    pub min_delay: u128,
    /// 最大延迟（微秒）
    pub max_delay: u128,
    /// 平均延迟（微秒）
    pub avg_delay: u128,
}

impl JitterStats {
    /// 计算丢包率（百分比）
    pub fn loss_rate(&self) -> f64 {
        if self.received + self.lost == 0 {
            return 0.0;
        }
        (self.lost as f64 / (self.received + self.lost) as f64) * 100.0
    }
}

/// Jitter Buffer 配置
#[derive(Debug, Clone)]
pub struct JitterConfig {
    /// 最小缓冲区大小（包数）
    pub min_buffer_size: usize,
    /// 最大缓冲区大小（包数）
    pub max_buffer_size: usize,
    /// 目标缓冲区大小（包数）
    pub target_buffer_size: usize,
    /// 自适应调整间隔（包数）
    pub adapt_interval: usize,
    /// 最大容忍延迟（微秒）
    pub max_tolerable_delay: u128,
}

impl Default for JitterConfig {
    fn default() -> Self {
        Self {
            min_buffer_size: 2,
            max_buffer_size: 20,
            target_buffer_size: 5,
            adapt_interval: 50,
            max_tolerable_delay: 100_000, // 100ms
        }
    }
}

/// Jitter Buffer - 用于音频流的抖动缓冲
pub struct JitterBuffer {
    /// 配置
    config: JitterConfig,
    /// 缓冲区（按时间戳排序）
    buffer: BTreeMap<u128, AudioPacket>,
    /// 统计信息
    stats: JitterStats,
    /// 期望的下一个序列号
    expected_seq: u32,
    /// 是否已接收第一个包
    first_packet_received: bool,
    /// 延迟样本窗口（用于自适应）
    delay_samples: VecDeque<u128>,
    /// 自适应计数器
    adapt_counter: usize,
    /// 上次播放的时间戳
    last_played_timestamp: u128,
}

impl JitterBuffer {
    /// 创建新的 Jitter Buffer
    pub fn new(config: JitterConfig) -> Self {
        Self {
            config,
            buffer: BTreeMap::new(),
            stats: JitterStats::default(),
            expected_seq: 0,
            first_packet_received: false,
            delay_samples: VecDeque::with_capacity(100),
            adapt_counter: 0,
            last_played_timestamp: 0,
        }
    }

    /// 使用默认配置创建
    pub fn default() -> Self {
        Self::new(JitterConfig::default())
    }

    /// 插入音频包
    pub fn push(&mut self, packet: AudioPacket, arrival_time: u128) {
        // 检测重复包
        if self.buffer.contains_key(&packet.timestamp) {
            self.stats.duplicate += 1;
            return;
        }

        // 初始化序列号
        if !self.first_packet_received {
            self.expected_seq = packet.seq.wrapping_add(1);
            self.first_packet_received = true;
        } else {
            // 检测丢包
            let seq_diff = packet.seq.wrapping_sub(self.expected_seq);
            if seq_diff > 0 && seq_diff < 1000 {
                // 允许一定的序列号跳跃（处理回环）
                self.stats.lost += seq_diff as u64;
            }
            self.expected_seq = packet.seq.wrapping_add(1);
        }

        // 检查是否迟到
        if self.last_played_timestamp > 0 && packet.timestamp < self.last_played_timestamp {
            self.stats.late += 1;
            return;
        }

        // 超过最大容忍延迟，直接丢弃
        if packet.timestamp < arrival_time
            && arrival_time - packet.timestamp > self.config.max_tolerable_delay
        {
            self.stats.late += 1;
            return;
        }

        let delay = if packet.timestamp >= arrival_time {
            packet.timestamp - arrival_time
        } else {
            0 // 包到达时已经过了播放时间
        };

        self.update_delay_stats(delay);

        // 插入缓冲区
        self.buffer.insert(packet.timestamp, packet);
        self.stats.received += 1;
        self.stats.buffer_size = self.buffer.len();

        // 自适应调整
        self.adapt_counter += 1;
        if self.adapt_counter >= self.config.adapt_interval {
            self.adapt_buffer_size();
            self.adapt_counter = 0;
        }
    }

    /// 获取下一个应该播放的包
    ///
    /// # Arguments
    /// * `current_time` - 当前服务器时间
    ///
    /// # Returns
    /// * `Some(packet)` - 如果有包应该播放
    /// * `None` - 如果缓冲区为空或还没到播放时间
    pub fn pop(&mut self, current_time: u128) -> Option<AudioPacket> {
        // 如果缓冲区为空，直接返回
        if self.buffer.is_empty() {
            return None;
        }

        // 检查缓冲区是否达到最小大小
        if self.buffer.len() < self.config.target_buffer_size && !self.should_drain() {
            return None;
        }

        // 获取最早的包
        if let Some((&timestamp, _)) = self.buffer.iter().next() {
            // 检查是否到达播放时间
            if current_time >= timestamp {
                let packet = self.buffer.remove(&timestamp).unwrap();
                self.last_played_timestamp = timestamp;
                self.stats.played += 1;
                self.stats.buffer_size = self.buffer.len();
                return Some(packet);
            }
        }

        None
    }

    /// 强制获取下一个包（无论时间）
    pub fn pop_next(&mut self) -> Option<AudioPacket> {
        if let Some((&timestamp, _)) = self.buffer.iter().next() {
            let packet = self.buffer.remove(&timestamp).unwrap();
            self.last_played_timestamp = timestamp;
            self.stats.played += 1;
            self.stats.buffer_size = self.buffer.len();
            return Some(packet);
        }
        None
    }

    /// 查看下一个包的播放时间（不移除）
    pub fn peek_next_timestamp(&self) -> Option<u128> {
        self.buffer.keys().next().copied()
    }

    /// 获取缓冲区大小
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// 检查缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 获取统计信息
    pub fn stats(&self) -> &JitterStats {
        &self.stats
    }

    /// 重置统计信息
    pub fn reset_stats(&mut self) {
        self.stats = JitterStats::default();
        self.stats.buffer_size = self.buffer.len();
    }

    /// 清空缓冲区
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.stats.buffer_size = 0;
        self.first_packet_received = false;
        self.last_played_timestamp = 0;
    }

    /// 检查是否应该排空缓冲区（处理长时间没有新包的情况）
    fn should_drain(&self) -> bool {
        // 如果缓冲区有包且已经等了很久，就开始播放
        !self.buffer.is_empty() && self.buffer.len() >= self.config.min_buffer_size
    }

    /// 更新延迟统计
    fn update_delay_stats(&mut self, delay: u128) {
        // 更新最小/最大延迟
        if self.stats.received == 0 {
            self.stats.min_delay = delay;
            self.stats.max_delay = delay;
            self.stats.avg_delay = delay;
        } else {
            self.stats.min_delay = self.stats.min_delay.min(delay);
            self.stats.max_delay = self.stats.max_delay.max(delay);
            // 滑动平均
            self.stats.avg_delay = (self.stats.avg_delay * 9 + delay) / 10;
        }

        // 保存延迟样本用于自适应
        self.delay_samples.push_back(delay);
        if self.delay_samples.len() > 100 {
            self.delay_samples.pop_front();
        }
    }

    /// 自适应调整缓冲区大小
    fn adapt_buffer_size(&mut self) {
        if self.delay_samples.len() < 10 {
            return;
        }

        // 计算延迟方差（抖动）
        let avg = self.stats.avg_delay;
        let variance: f64 = self
            .delay_samples
            .iter()
            .map(|&d| {
                let diff = d as i128 - avg as i128;
                (diff * diff) as f64
            })
            .sum::<f64>()
            / self.delay_samples.len() as f64;

        let jitter = variance.sqrt();

        // 根据抖动调整目标缓冲区大小
        // 抖动大 -> 增加缓冲区
        // 抖动小 -> 减少缓冲区
        let target = if jitter > 50_000.0 {
            // 高抖动（>50ms 标准差）
            self.config.target_buffer_size + 2
        } else if jitter < 10_000.0 {
            // 低抖动（<10ms 标准差）
            self.config.target_buffer_size.saturating_sub(1)
        } else {
            self.config.target_buffer_size
        };

        // 限制在最小/最大范围内
        self.config.target_buffer_size = target
            .max(self.config.min_buffer_size)
            .min(self.config.max_buffer_size);
    }
}
