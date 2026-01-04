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
use std::net::SocketAddr;
use std::sync::Arc;
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
        config: AudioConfig,
        reader: WavReader,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        parent_cancel: CancellationToken,
    ) -> StreamHandle {
        let cancel = parent_cancel.child_token();
        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, reader, socket, target, token).await {
                eprintln!("[FilePlayback] Error: {}", e);
            }
        });

        StreamHandle::new(cancel)
    }

    async fn run(
        config: AudioConfig,
        mut reader: WavReader,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut codec = OpusCodec::new(&config)?;
        let mut pcm = vec![0i16; config.frame_size];
        let mut opus_buf = vec![0u8; 4096];
        let frame_duration = std::time::Duration::from_millis(20);
        let mut interval = tokio::time::interval(frame_duration);

        println!("[FilePlayback] Started -> {}", target);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    match reader.read_samples(&mut pcm) {
                        Ok(0) => {
                            println!("[FilePlayback] EOF reached");
                            break;
                        }
                        Ok(n) => {
                            if let Ok(len) = codec.encode(&pcm[..n], &mut opus_buf) {
                                let packet = AudioPacket {
                                    data: opus_buf[..len].to_vec(),
                                };
                                let _ = socket.send(&packet, target).await;
                            }
                        }
                        Err(e) => {
                            eprintln!("[FilePlayback] Read error: {}", e);
                            break;
                        }
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

/// 音频转发流 - 从总线订阅并转发到指定客户端
/// 用于实现"监听"功能或者服务端音频源推送
pub struct ForwardStream;

impl ForwardStream {
    /// 启动转发流
    ///
    /// # Arguments
    /// * `socket` - UDP socket
    /// * `target` - 目标地址
    /// * `bus_rx` - 音频总线接收器
    /// * `source_filter` - 源过滤（可选）
    /// * `parent_cancel` - 父级取消令牌
    pub fn spawn(
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        bus_rx: broadcast::Receiver<AudioFrame>,
        source_filter: Option<SocketAddr>,
        parent_cancel: CancellationToken,
    ) -> StreamHandle {
        let cancel = parent_cancel.child_token();
        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(socket, target, bus_rx, source_filter, token).await {
                eprintln!("[Forward] Error: {}", e);
            }
        });

        StreamHandle::new(cancel)
    }

    async fn run(
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        mut bus_rx: broadcast::Receiver<AudioFrame>,
        source_filter: Option<SocketAddr>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        println!(
            "[Forward] Started -> {} (filter: {:?})",
            target, source_filter
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

                            // 不转发给自己
                            if frame.source.as_ref() == Some(&target) {
                                continue;
                            }

                            let _ = socket.send(&frame.packet, target).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("[Forward] Lagged {} frames", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            }
        }

        println!("[Forward] Stopped");
        Ok(())
    }
}

