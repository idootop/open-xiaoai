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
use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use crate::net::sync::now_us;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// 管道句柄 - 用于控制正在运行的音频管道
pub struct PipelineHandle {
    cancel: CancellationToken,
    /// 用于通知阻塞线程停止
    stop_flag: Arc<AtomicBool>,
}

impl PipelineHandle {
    fn new(cancel: CancellationToken, stop_flag: Arc<AtomicBool>) -> Self {
        Self { cancel, stop_flag }
    }

    /// 停止管道
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
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
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = PipelineHandle::new(cancel.clone(), stop_flag.clone());

        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, socket, target, token, stop_flag).await {
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
        stop_flag: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        // 创建 PCM 数据通道
        let (pcm_tx, mut pcm_rx) = mpsc::channel::<Vec<i16>>(32);

        // 启动 ALSA 录音线程（阻塞 I/O）
        let recorder_config = config.clone();
        let recorder_stop = stop_flag.clone();

        std::thread::spawn(move || {
            let recorder = match AudioRecorder::new(&recorder_config) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[RecordPipeline] Failed to create recorder: {}", e);
                    return;
                }
            };

            let mut buf = vec![0i16; recorder_config.frame_size];

            // 使用 stop_flag 来优雅退出
            while !recorder_stop.load(Ordering::SeqCst) {
                match recorder.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if pcm_tx.blocking_send(buf[..n].to_vec()).is_err() {
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
                            if let Ok(len) = codec.encode(&samples, &mut opus_buf) {
                                let packet = AudioPacket {
                                    seq: 0,
                                    timestamp: 0,
                                    data: opus_buf[..len].to_vec(),
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
        socket: Arc<AudioSocket>,
        parent_cancel: CancellationToken,
    ) -> PipelineHandle {
        let cancel = parent_cancel.child_token();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = PipelineHandle::new(cancel.clone(), stop_flag.clone());

        let token = cancel.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run(config, socket, token, stop_flag).await {
                eprintln!("[PlaybackPipeline] Error: {}", e);
            }
        });

        handle
    }

    async fn run(
        config: AudioConfig,
        socket: Arc<AudioSocket>,
        cancel: CancellationToken,
        stop_flag: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        // 创建 PCM 数据通道
        let (pcm_tx, pcm_rx) = mpsc::channel::<AudioPacket>(128);

        // 启动 ALSA 播放线程（阻塞 I/O）
        let player_config = config.clone();
        let player_stop = stop_flag.clone();

        println!("[PlaybackPipeline] Started");

        // 主循环：从 UDP 接收，解码后发送给播放线程
        let mut udp_buf = vec![0u8; 4096];
        let mut last_time = now_us();
        tokio::spawn(async move {
            loop {
                match socket.recv(&mut udp_buf).await {
                    Ok((packet, _src)) => {
                        let now = now_us();
                        let diff = now - last_time;
                        println!("Received packet now:{} diff:{}ms", now, diff / 1000);
                        last_time = now;

                        if let Err(e) = pcm_tx.send(packet).await {
                            eprintln!("[PlaybackPipeline] PCM channel send error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("[PlaybackPipeline] Recv error: {}", e);
                    }
                }
            }
        });

        let player = match AudioPlayer::new(&player_config) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[PlaybackPipeline] Failed to create player: {}", e);
                return Err(e);
            }
        };

        // 使用 blocking_recv 在线程中接收
        let mut rx = pcm_rx;
        // 创建 Opus 解码器
        let mut codec = OpusCodec::new(&config).unwrap();
        let mut pcm_buf = vec![0i16; config.frame_size * config.channels as usize];

        let mut jitter_buffer: VecDeque<AudioPacket> = VecDeque::new();

        let mut start_time = 0u128;
        let frame_duration_us =
            (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

        while !player_stop.load(Ordering::SeqCst) {
            while let Ok(p) = rx.try_recv() {
                jitter_buffer.push_back(p);
            }
            if let Some(pck) = jitter_buffer.front() {
                let now = now_us();
                if start_time == 0 {
                    start_time = now;
                }

                let target_client_time = start_time + (pck.seq as u128) * frame_duration_us;

                if now >= target_client_time {
                    let packet = jitter_buffer.pop_front().unwrap();
                    let samples = codec.decode(&packet.data, &mut pcm_buf)?;
                    player.write(&pcm_buf[..samples * config.channels as usize])?;
                } else if target_client_time - now > 500_000 {
                    // Too far in the future, maybe clock jumped?
                    jitter_buffer.pop_front();
                } else {
                    // Wait until it's time
                    let wait = (target_client_time - now) as u64;
                    if wait > 1000 {
                        tokio::time::sleep(Duration::from_micros(wait)).await;
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }

        println!("[PlaybackPipeline] Stopped");
        Ok(())
    }
}
