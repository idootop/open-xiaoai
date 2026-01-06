//! # AudioStream - 音频流抽象
//!
//! 提供统一的音频流处理接口，支持多种输入源和输出目标。
//!
//! ## 设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Stream Types                             │
//! │                                                             │
//! │  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐    │
//! │  │ FileSource   │   │ BusSource    │   │ NetworkSink  │    │
//! │  │ (WAV Reader) │   │ (From Bus)   │   │ (To Client)  │    │
//! │  └──────────────┘   └──────────────┘   └──────────────┘    │
//! │                                                             │
//! │  ┌──────────────┐   ┌──────────────┐                       │
//! │  │ FileSink     │   │ BusSink      │                       │
//! │  │ (WAV Writer) │   │ (To Bus)     │                       │
//! │  └──────────────┘   └──────────────┘                       │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::wav::{WavReader, WavWriter};
use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use crate::net::sync::now_us;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::audio_bus::AudioFrame;

/// 音频流任务句柄
/// 用于控制正在运行的音频流任务
pub struct StreamHandle {
    cancel: CancellationToken,
}

impl StreamHandle {
    pub fn new(cancel: CancellationToken) -> Self {
        Self { cancel }
    }

    /// 停止流
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// 检查是否已停止
    pub fn is_stopped(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// 音频流发送控制器
pub struct StreamSender {
    socket: Arc<AudioSocket>,
    target: SocketAddr,
    codec: OpusCodec,

    // 缓存区：避免重复分配内存
    opus_buffer: Vec<u8>,

    // 状态变量
    seq: u32,
    stream_start_ts: Option<u128>,

    // 配置参数
    input_len: usize,
    frame_duration_us: u128,
    max_lead_us: u128, // 允许的最大超前时间，例如 1_000_000 (1s)
}

impl StreamSender {
    pub fn new(
        config: AudioConfig,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
    ) -> anyhow::Result<Self> {
        let codec = OpusCodec::new(&config)?;
        let input_len: usize = config.frame_size * (config.channels as usize);

        // 预分配缓冲区
        let opus_buffer = vec![0u8; 4096];

        let frame_duration_us =
            (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

        Ok(Self {
            socket,
            target,
            codec,
            opus_buffer,
            seq: 0,
            stream_start_ts: None,
            input_len,
            frame_duration_us,
            max_lead_us: 1_000_000,
        })
    }

    /// 发送音频帧
    pub async fn send(&mut self, pcm_input: &[i16]) -> anyhow::Result<()> {
        // 0. 处理输入(不足时填充静音)
        let pcm_buffer = if pcm_input.len() < self.input_len {
            let mut pcm_buffer = vec![0i16; self.input_len];
            pcm_buffer[..pcm_input.len()].copy_from_slice(pcm_input);
            pcm_buffer
        } else {
            pcm_input.to_vec()
        };

        // 1. 编码
        let encoded_len = self.codec.encode(&pcm_buffer, &mut self.opus_buffer)?;

        // 2. 时间戳处理
        let now = now_us();
        let start_ts = *self.stream_start_ts.get_or_insert(now);
        let target_ts = start_ts + (self.seq as u128 * self.frame_duration_us);

        // 3. 构建并发送
        let packet = AudioPacket {
            seq: self.seq,
            timestamp: target_ts,
            data: self.opus_buffer[..encoded_len].to_vec(),
        };

        self.socket.send(&packet, self.target).await?;
        self.seq += 1;

        // 4. 平滑流控
        // 如果发送进度超过当前时间 + 允许的缓冲量，则进行睡眠
        if target_ts > now + self.max_lead_us {
            let drift = target_ts - (now + self.max_lead_us);
            let sleep_ms = (drift / 1000).min(100) as u64;
            if sleep_ms > 0 {
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            }
        }

        Ok(())
    }
}

/// 文件播放流 - 从 WAV 文件读取并发送到指定客户端
pub struct FilePlaybackStream;

impl FilePlaybackStream {
    /// 启动文件播放流
    ///
    /// # Arguments
    /// * `config` - 音频配置（用于 Opus 编码）
    /// * `reader` - WAV 文件读取器
    /// * `socket` - UDP socket
    /// * `target` - 目标客户端地址
    /// * `parent_cancel` - 父级取消令牌（用于 session 级别取消）
    pub fn spawn(
        reader: WavReader,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        parent_cancel: CancellationToken,
    ) -> StreamHandle {
        let cancel = parent_cancel.child_token();
        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(reader, socket, target, token).await {
                eprintln!("[FilePlayback] Error: {}", e);
            }
        });

        StreamHandle::new(cancel)
    }

    async fn run(
        mut reader: WavReader,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        println!("[FilePlayback] Started -> {}", target);

        let mut sender = StreamSender::new(reader.config.clone(), socket, target)?;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = async {
                    // 读取一帧 PCM 数据
                    if let Some(pcm) = reader.read_one_frame()? {
                        // 编码并发送
                        sender.send(pcm).await
                    } else {
                        Err(anyhow::anyhow!("EOF"))
                    }
                } => {
                    if let Err(e) = result {
                        if e.to_string() != "EOF" {
                            eprintln!("[FilePlayback] Error: {}", e);
                        }
                        break;
                    }
                }
            }
        }

        println!("[FilePlayback] Stopped");
        Ok(())
    }
}

/// 录音流 - 从总线订阅音频并写入 WAV 文件
pub struct RecorderStream;

impl RecorderStream {
    /// 启动录音流
    ///
    /// # Arguments
    /// * `config` - 音频配置
    /// * `filename` - 输出文件路径
    /// * `bus_rx` - 音频总线接收器
    /// * `source_filter` - 仅录制来自此地址的音频（None 表示全部录制）
    /// * `parent_cancel` - 父级取消令牌
    pub fn spawn(
        config: AudioConfig,
        filename: String,
        bus_rx: broadcast::Receiver<AudioFrame>,
        source_filter: Option<SocketAddr>,
        parent_cancel: CancellationToken,
    ) -> StreamHandle {
        let cancel = parent_cancel.child_token();
        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, filename, bus_rx, source_filter, token).await {
                eprintln!("[Recorder] Error: {}", e);
            }
        });

        StreamHandle::new(cancel)
    }

    async fn run(
        config: AudioConfig,
        filename: String,
        mut bus_rx: broadcast::Receiver<AudioFrame>,
        source_filter: Option<SocketAddr>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut writer = WavWriter::create(&filename, config.sample_rate, config.channels)?;
        let mut codec = OpusCodec::new(&config)?;
        let mut pcm = vec![0i16; config.frame_size];

        println!(
            "[Recorder] Started -> {} (filter: {:?})",
            filename, source_filter
        );

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = bus_rx.recv() => {
                    match result {
                        Ok(frame) => {
                            // 应用源过滤
                            if let Some(filter_addr) = &source_filter {
                                if frame.source.as_ref() != Some(filter_addr) {
                                    continue;
                                }
                            }

                            // 解码并写入
                            if let Ok(n) = codec.decode(&frame.packet.data, &mut pcm) {
                                let _ = writer.write_samples(&pcm[..n]);
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("[Recorder] Lagged {} frames", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            }
        }

        // 确保正确关闭文件
        writer.finalize()?;
        println!("[Recorder] Stopped, file saved: {}", filename);
        Ok(())
    }
}
