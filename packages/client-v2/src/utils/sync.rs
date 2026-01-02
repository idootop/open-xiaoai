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
/// 采用改进的 NTP 算法 + Kalman 滤波思想
pub struct ClockSync {
    /// 偏移量样本窗口
    offsets: VecDeque<OffsetSample>,
    /// 当前估计的时钟偏移 (server_time - client_time)
    pub current_offset: i128,
    /// RTT 样本窗口
    rtts: VecDeque<i128>,
    /// 当前估计的最小 RTT
    min_rtt: i128,
    /// 窗口大小
    window_size: usize,
    /// 时钟漂移率 (ppm: parts per million)
    /// 正值表示从节点时钟比主节点快
    drift_rate: f64,
    /// 上次更新时间
    last_update_time: u128,
    /// 漂移率估计窗口
    drift_samples: VecDeque<DriftSample>,
}

#[derive(Clone, Copy)]
struct OffsetSample {
    offset: i128,
    rtt: i128,
    timestamp: u128,
}

#[derive(Clone, Copy)]
struct DriftSample {
    offset: i128,
    timestamp: u128,
}

impl ClockSync {
    pub fn new(window_size: usize) -> Self {
        Self {
            offsets: VecDeque::with_capacity(window_size),
            current_offset: 0,
            rtts: VecDeque::with_capacity(window_size),
            min_rtt: i128::MAX,
            window_size,
            drift_rate: 0.0,
            last_update_time: now_us(),
            drift_samples: VecDeque::with_capacity(60), // 保留 60 秒的样本
        }
    }

    /// 更新时钟偏移估计 (NTP 算法)
    /// 
    /// NTP 时间戳标记:
    /// t1 = client_send_ts  : 客户端发送 Ping 的时间
    /// t2 = server_ts       : 服务器接收 Ping 的时间  
    /// t3 = server_ts       : 服务器发送 Pong 的时间 (假设处理时间忽略不计)
    /// t4 = client_recv_ts  : 客户端接收 Pong 的时间
    ///
    /// RTT = (t4 - t1) - (t3 - t2) = (t4 - t1) (因为 t3 = t2)
    /// Offset = ((t2 - t1) + (t3 - t4)) / 2 = ((t2 - t1) + (t2 - t4)) / 2
    ///        = t2 - (t1 + t4) / 2
    pub fn update(&mut self, client_send_ts: u128, server_ts: u128, client_recv_ts: u128) {
        let t1 = client_send_ts as i128;
        let t2 = server_ts as i128;
        let t4 = client_recv_ts as i128;

        let rtt = t4 - t1;

        // 过滤异常 RTT (局域网内 > 100ms 视为异常)
        if rtt < 0 || rtt > 100_000 {
            return;
        }

        // 计算时钟偏移: offset = server_time - client_time
        // offset = t2 - (t1 + t4) / 2
        let offset = t2 - (t1 + t4) / 2;

        // 更新 RTT 窗口
        self.rtts.push_back(rtt);
        if self.rtts.len() > self.window_size {
            self.rtts.pop_front();
        }
        self.min_rtt = *self.rtts.iter().min().unwrap_or(&rtt);

        // 更新偏移量窗口
        let sample = OffsetSample {
            offset,
            rtt,
            timestamp: client_recv_ts,
        };
        self.offsets.push_back(sample);
        if self.offsets.len() > self.window_size {
            self.offsets.pop_front();
        }

        // 偏移量估计: 使用低 RTT 样本的中位数
        // 原理: RTT 较小的样本受网络抖动影响小，时间测量更准确
        let mut low_rtt_offsets: Vec<i128> = self
            .offsets
            .iter()
            .filter(|s| s.rtt <= self.min_rtt + 5000) // 5ms 容差
            .map(|s| s.offset)
            .collect();

        if !low_rtt_offsets.is_empty() {
            low_rtt_offsets.sort_unstable();
            let new_offset = low_rtt_offsets[low_rtt_offsets.len() / 2];

            // 漂移率估计
            self.estimate_drift(new_offset, client_recv_ts);

            // 平滑更新偏移量 (避免突变)
            let alpha = 0.3; // 低通滤波系数
            self.current_offset =
                (alpha * new_offset as f64 + (1.0 - alpha) * self.current_offset as f64) as i128;
        }

        self.last_update_time = client_recv_ts;
    }

    /// 估计时钟漂移率
    /// 时钟漂移率 = d(offset) / dt
    fn estimate_drift(&mut self, offset: i128, timestamp: u128) {
        self.drift_samples.push_back(DriftSample { offset, timestamp });
        if self.drift_samples.len() > 60 {
            self.drift_samples.pop_front();
        }

        // 至少需要 10 秒的数据才能估计漂移
        if self.drift_samples.len() < 10 {
            return;
        }

        // 使用线性回归估计漂移率
        let first = self.drift_samples.front().unwrap();
        let last = self.drift_samples.back().unwrap();

        let dt = (last.timestamp - first.timestamp) as f64;
        let d_offset = (last.offset - first.offset) as f64;

        if dt > 10_000_000.0 {
            // 超过 10 秒
            // drift_rate 单位: 微秒/秒 = ppm
            let new_drift = d_offset / (dt / 1_000_000.0);

            // 平滑更新漂移率
            let beta = 0.1;
            self.drift_rate = beta * new_drift + (1.0 - beta) * self.drift_rate;
        }
    }

    /// 将本地时间转换为服务器(主节点)时间
    /// 考虑时钟漂移补偿
    pub fn to_server_time(&self, client_time: u128) -> u128 {
        let base_server_time = (client_time as i128 + self.current_offset) as u128;

        // 漂移补偿: 根据距离上次同步的时间，补偿时钟漂移
        let elapsed_since_update = client_time.saturating_sub(self.last_update_time) as f64;
        let drift_correction = (self.drift_rate * elapsed_since_update / 1_000_000.0) as i128;

        (base_server_time as i128 + drift_correction) as u128
    }

    /// 将服务器(主节点)时间转换为本地时间
    pub fn to_client_time(&self, server_time: u128) -> u128 {
        // 简化版本，不考虑漂移补偿 (播放时主要用 to_server_time)
        (server_time as i128 - self.current_offset) as u128
    }

    /// 获取当前估计的 RTT (微秒)
    pub fn get_rtt(&self) -> i128 {
        self.min_rtt
    }

    /// 获取当前时钟漂移率 (ppm)
    pub fn get_drift_rate(&self) -> f64 {
        self.drift_rate
    }

    /// 获取同步质量评估 (0-100, 越高越好)
    pub fn get_sync_quality(&self) -> u8 {
        if self.offsets.is_empty() {
            return 0;
        }

        // 基于 RTT 稳定性和偏移量方差评估
        let rtt_variance = self.calculate_variance(&self.rtts.iter().copied().collect::<Vec<_>>());
        let offset_variance = self.calculate_variance(
            &self.offsets.iter().map(|s| s.offset).collect::<Vec<_>>(),
        );

        // RTT 越稳定，方差越小，质量越高
        let rtt_score = ((100_000.0 - rtt_variance.min(100_000.0)) / 100_000.0 * 50.0) as u8;
        let offset_score = ((50_000.0 - offset_variance.min(50_000.0)) / 50_000.0 * 50.0) as u8;

        rtt_score + offset_score
    }

    fn calculate_variance(&self, samples: &[i128]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let mean = samples.iter().sum::<i128>() as f64 / samples.len() as f64;
        let variance = samples
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / samples.len() as f64;
        variance.sqrt()
    }
}
