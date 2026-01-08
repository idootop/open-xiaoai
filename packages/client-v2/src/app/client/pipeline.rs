//! # Audio Pipeline - 音频管道抽象
//!
//! 提供录音和播放的管道化处理。
//!
//! ## 设计
//!
//! ```text
//! RecordPipeline:
//!   ┌──────────┐     ┌────────────┐     ┌────────────┐
//!   │ ALSA Mic │────▶│ Opus Encode│────▶│ UDP Send   │
//!   │ (thread) │     │ (async)    │     │ (async)    │
//!   └──────────┘     └────────────┘     └────────────┘
//!
//! PlaybackPipeline:
//!   ┌────────────┐     ┌────────────┐     ┌────────────┐
//!   │ UDP Recv   │────▶│ Opus Decode│────▶│ ALSA Play  │
//!   │ (async)    │     │ (async)    │     │ (thread)   │
//!   └────────────┘     └────────────┘     └────────────┘
//! ```

use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::jitter_buffer::{JitterBuffer, JitterConfig};
use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use crate::net::sync::now_us;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// 管道句柄 - 用于控制正在运行的音频管道
pub struct PipelineHandle {
    cancel: CancellationToken,
}

impl PipelineHandle {
    fn new(cancel: CancellationToken) -> Self {
        Self { cancel }
    }

    /// 停止管道
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// 检查是否已停止
    pub fn is_stopped(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 录音管道
/// 从麦克风捕获 -> Opus 编码 -> UDP 发送到服务器
pub struct RecordPipeline;

impl RecordPipeline {
    /// 启动录音管道
    ///
    /// # Arguments
    /// * `config` - 音频配置
    /// * `socket` - UDP socket
    /// * `target` - 目标服务器地址
    /// * `parent_cancel` - 父级取消令牌
    pub fn spawn(
        config: AudioConfig,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        parent_cancel: CancellationToken,
    ) -> PipelineHandle {
        let cancel = parent_cancel.child_token();
        let handle = PipelineHandle::new(cancel.clone());

        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, socket, target, token).await {
                eprintln!("[RecordPipeline] Error: {}", e);
            }
        });

        handle
    }

    async fn run(
        config: AudioConfig,
        socket: Arc<AudioSocket>,
        target: SocketAddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        // 创建 PCM 数据通道
        let (pcm_tx, mut pcm_rx) = mpsc::channel::<Vec<i16>>(32);

        // 启动 ALSA 录音线程（阻塞 I/O）
        let recorder_config = config.clone();
        let channels = recorder_config.channels as usize;
        std::thread::spawn(move || {
            let recorder = match AudioRecorder::new(&recorder_config) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[RecordPipeline] Failed to create recorder: {}", e);
                    return;
                }
            };

            let mut buf =
                vec![0i16; recorder_config.frame_size * recorder_config.channels as usize];

            loop {
                match recorder.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let actual_samples = n * channels;
                        if pcm_tx
                            .blocking_send(buf[..actual_samples].to_vec())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {
                        // 空读取，继续
                    }
                    Err(e) => {
                        eprintln!("[RecordPipeline] Read error: {}", e);
                        break;
                    }
                }
            }
        });

        // 创建 Opus 编码器
        let mut codec = OpusCodec::new(&config)?;
        let mut opus_buf = vec![0u8; 4096];

        println!("[RecordPipeline] Started -> {}", target);

        // 主循环：从录音线程接收 PCM，编码后发送
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                pcm = pcm_rx.recv() => {
                    match pcm {
                        Some(samples) => {
                            if let Ok(encoded_len) = codec.encode(&samples, &mut opus_buf) {
                                let packet = AudioPacket {
                                    seq: 0,
                                    timestamp: 0,
                                    data: opus_buf[..encoded_len].to_vec(),
                                };
                                let _ = socket.send(&packet, target).await;
                            }
                        }
                        None => {
                            // 录音线程已退出
                            break;
                        }
                    }
                }
            }
        }

        println!("[RecordPipeline] Stopped");
        Ok(())
    }
}

/// 播放管道
/// 从 UDP 接收 -> Opus 解码 -> 扬声器播放
pub struct PlaybackPipeline;

impl PlaybackPipeline {
    /// 启动播放管道
    ///
    /// # Arguments
    /// * `config` - 音频配置
    /// * `socket` - UDP socket
    /// * `parent_cancel` - 父级取消令牌
    pub fn spawn(
        config: AudioConfig,
        clock: Arc<parking_lot::Mutex<crate::net::sync::ClockSync>>,
        socket: Arc<AudioSocket>,
        parent_cancel: CancellationToken,
    ) -> PipelineHandle {
        let cancel = parent_cancel.child_token();
        let handle = PipelineHandle::new(cancel.clone());

        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, clock, socket, token).await {
                eprintln!("[PlaybackPipeline] Error: {}", e);
            }
        });

        handle
    }

    async fn run(
        config: AudioConfig,
        clock: Arc<parking_lot::Mutex<crate::net::sync::ClockSync>>,
        socket: Arc<AudioSocket>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        // 1. 创建 PCM 通道
        let (pcm_tx, mut pcm_rx) = mpsc::channel::<Vec<i16>>(64);

        // 2. 专用播放线程
        let player_config = config.clone();
        std::thread::spawn(move || {
            let player = match AudioPlayer::new(&player_config) {
                Ok(p) => p,
                Err(e) => return eprintln!("[PlaybackPipeline] Player init error: {}", e),
            };

            // 当 pcm_tx 在异步任务中被 drop，这里会自动退出
            while let Some(samples) = pcm_rx.blocking_recv() {
                if let Err(e) = player.write(&samples) {
                    eprintln!("[PlaybackPipeline] Write error: {}", e);
                    break;
                }
            }
            println!("[PlaybackPipeline] Player thread exited naturally");
        });

        // 3. Opus 解码器与 Jitter Buffer
        let mut codec = OpusCodec::new(&config)?;
        let mut jitter_buffer = JitterBuffer::new(JitterConfig::default());
        let mut pcm_frame = vec![0i16; config.frame_size * config.channels as usize];

        // 提高定时精度
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(5));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        println!("[PlaybackPipeline] Running");

        // 4. 主逻辑循环
        // 使用 loop + select，当 cancel 触发时直接 break
        let mut udp_buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                // 优先级 1: 外部取消
                _ = cancel.cancelled() => {
                    break;
                }

                // 优先级 2: 网络接收
                result = socket.recv(&mut udp_buf) => {
                    if let Ok((packet,_)) = result {
                        let arrival_time = clock.lock().to_server_time(now_us());
                        jitter_buffer.push(packet, arrival_time);
                    }
                }

                // 优先级 3: 播放调度
                _ = ticker.tick() => {
                    let current_time = clock.lock().to_server_time(now_us());
                    while let Some(packet) = jitter_buffer.pop(current_time) {
                        if let Ok(samples) = codec.decode(&packet.data, &mut pcm_frame) {
                            let samples = pcm_frame[..samples * config.channels as usize].to_vec();
                            if pcm_tx.try_send(samples).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }

        println!("[PlaybackPipeline] Stopped");
        Ok(())
    }
}
