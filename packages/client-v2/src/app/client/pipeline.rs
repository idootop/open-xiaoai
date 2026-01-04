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
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
        let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<i16>>(32);

        // 启动 ALSA 播放线程（阻塞 I/O）
        let player_config = config.clone();
        let player_stop = stop_flag.clone();

        std::thread::spawn(move || {
            let player = match AudioPlayer::new(&player_config) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[PlaybackPipeline] Failed to create player: {}", e);
                    return;
                }
            };

            // 使用 blocking_recv 在线程中接收
            let mut rx = pcm_rx;
            while !player_stop.load(Ordering::SeqCst) {
                match rx.blocking_recv() {
                    Some(samples) => {
                        if let Err(e) = player.write(&samples) {
                            eprintln!("[PlaybackPipeline] Write error: {}", e);
                            break;
                        }
                    }
                    None => {
                        // 通道关闭
                        break;
                    }
                }
            }
        });

        // 创建 Opus 解码器
        let mut codec = OpusCodec::new(&config)?;
        let mut pcm_buf = vec![0i16; config.frame_size];
        let mut udp_buf = vec![0u8; 4096];

        println!("[PlaybackPipeline] Started");

        // 主循环：从 UDP 接收，解码后发送给播放线程
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = socket.recv(&mut udp_buf) => {
                    match result {
                        Ok((packet, _src)) => {
                            if let Ok(n) = codec.decode(&packet.data, &mut pcm_buf) {
                                // 使用 try_send 避免阻塞
                                let _ = pcm_tx.try_send(pcm_buf[..n].to_vec());
                            }
                        }
                        Err(e) => {
                            eprintln!("[PlaybackPipeline] Recv error: {}", e);
                        }
                    }
                }
            }
        }

        println!("[PlaybackPipeline] Stopped");
        Ok(())
    }
}

