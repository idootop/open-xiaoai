use anyhow::Result;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

pub const MIXER_SOCKET_PATH: &str = "/tmp/audio_mixer.sock";

/// 混音器服务，负责接收来自多个 Injector 的音频流并进行混音
pub struct Mixer {
    clients: Arc<Mutex<Vec<UnixStream>>>,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 启动 Unix Socket 监听服务
    pub async fn start(&self) -> Result<()> {
        if Path::new(MIXER_SOCKET_PATH).exists() {
            let _ = fs::remove_file(MIXER_SOCKET_PATH);
        }

        let listener = UnixListener::bind(MIXER_SOCKET_PATH)?;
        // 允许任何用户写入，确保 ALSA 进程有权连接
        let _ = std::process::Command::new("chmod")
            .arg("666")
            .arg(MIXER_SOCKET_PATH)
            .status();

        let clients = self.clients.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let mut c = clients.lock().await;
                        c.push(stream);
                    }
                    Err(e) => {
                        eprintln!("❌ Mixer accept error: {:?}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// 获取当前连接的客户端数量
    pub async fn client_count(&self) -> usize {
        self.clients.lock().await.len()
    }

    /// 读取并混合一帧音频数据
    /// frame_size: 每声道的采样点数
    /// channels: 声道数
    pub async fn read_mixed_frame(&self, frame_size: usize, channels: usize) -> Vec<i16> {
        let total_samples = frame_size * channels;
        let bytes_per_frame = total_samples * 2;
        let mut raw_buf = vec![0u8; bytes_per_frame];

        loop {
            let mut clients = self.clients.lock().await;
            if clients.is_empty() {
                drop(clients);
                // 没有客户端时，稍微等待，避免空转
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                continue;
            }

            let mut mixed_frame = vec![0i32; total_samples];
            let mut active_clients = Vec::new();
            let mut read_any = false;

            // 遍历所有客户端，各读取一帧
            for mut client in clients.drain(..) {
                // 注意：这里 read_exact 是异步的，会按顺序等待每个客户端的数据
                // 在主从同步场景下，所有客户端通常来自本地 ALSA，速率是同步的
                match client.read_exact(&mut raw_buf).await {
                    Ok(_) => {
                        for i in 0..total_samples {
                            let sample =
                                i16::from_le_bytes([raw_buf[i * 2], raw_buf[i * 2 + 1]]) as i32;
                            mixed_frame[i] += sample;
                        }
                        active_clients.push(client);
                        read_any = true;
                    }
                    Err(_) => {
                        // 客户端断开连接
                    }
                }
            }

            *clients = active_clients;

            if read_any {
                // 饱和截断 (Clipping/Saturating) 将 i32 转回 i16
                return mixed_frame
                    .into_iter()
                    .map(|x| {
                        if x > 32767 {
                            32767
                        } else if x < -32768 {
                            -32768
                        } else {
                            x as i16
                        }
                    })
                    .collect();
            } else {
                // 如果所有客户端都断开了，继续等待新连接
                drop(clients);
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
        }
    }
}
