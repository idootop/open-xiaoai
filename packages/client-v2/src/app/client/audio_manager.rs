use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

pub struct ClientAudioManager {
    audio_socket: Arc<AudioSocket>,
    audio_addr: SocketAddr,
    session_cancel: CancellationToken,
    record_cancel: RwLock<Option<CancellationToken>>,
    play_cancel: RwLock<Option<CancellationToken>>,
}

impl ClientAudioManager {
    pub fn new(
        audio_socket: Arc<AudioSocket>,
        audio_addr: SocketAddr,
        session_cancel: CancellationToken,
    ) -> Self {
        Self {
            audio_socket,
            audio_addr,
            session_cancel,
            record_cancel: RwLock::new(None),
            play_cancel: RwLock::new(None),
        }
    }

    pub async fn stop_recorder(&self) {
        let mut cancel_guard = self.record_cancel.write().await;
        if let Some(token) = cancel_guard.take() {
            token.cancel();
        }
    }

    pub async fn stop_player(&self) {
        let mut cancel_guard = self.play_cancel.write().await;
        if let Some(token) = cancel_guard.take() {
            token.cancel();
        }
    }

    pub async fn start_recording(&self, config: AudioConfig) {
        self.stop_recorder().await;
        let token = self.session_cancel.child_token();
        *self.record_cancel.write().await = Some(token.clone());

        let audio_socket = self.audio_socket.clone();
        let audio_addr = self.audio_addr;

        tokio::spawn(async move {
            let (pcm_tx, mut pcm_rx) = mpsc::channel::<Vec<i16>>(32);
            let conf = config.clone();

            // 录音线程 (ALSA 阻塞)
            std::thread::spawn(move || {
                let recorder = match AudioRecorder::new(&conf) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Failed to start recorder: {}", e);
                        return;
                    }
                };
                let mut buf = vec![0i16; conf.frame_size];
                while let Ok(n) = recorder.read(&mut buf) {
                    if pcm_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });

            let mut codec = match OpusCodec::new(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init opus codec: {}", e);
                    return;
                }
            };
            println!("Recording started...");
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    Some(pcm) = pcm_rx.recv() => {
                        let mut out = vec![0u8; 4096];
                        if let Ok(len) = codec.encode(&pcm, &mut out) {
                            let _ = audio_socket.send(&AudioPacket { data: out[..len].to_vec() }, audio_addr).await;
                        }
                    }
                }
            }
            println!("Recording stopped.");
        });
    }

    pub async fn start_playback(&self, config: AudioConfig) {
        self.stop_player().await;
        let token = self.session_cancel.child_token();
        *self.play_cancel.write().await = Some(token.clone());

        let audio_socket = self.audio_socket.clone();

        tokio::spawn(async move {
            let (pcm_tx, mut pcm_rx) = mpsc::channel::<Vec<i16>>(32);
            let conf = config.clone();

            // 播放线程 (ALSA 阻塞)
            std::thread::spawn(move || {
                let player = match AudioPlayer::new(&conf) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Failed to start player: {}", e);
                        return;
                    }
                };
                while let Some(pcm) = pcm_rx.blocking_recv() {
                    let _ = player.write(&pcm);
                }
            });

            let mut codec = match OpusCodec::new(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init opus codec: {}", e);
                    return;
                }
            };
            let mut udp_buf = vec![0u8; 4096];
            println!("Playback started...");
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    res = audio_socket.recv(&mut udp_buf) => {
                        if let Ok((packet, _)) = res {
                            let mut pcm = vec![0i16; config.frame_size];
                            if let Ok(n) = codec.decode(&packet.data, &mut pcm) {
                                let _ = pcm_tx.send(pcm[..n].to_vec()).await;
                            }
                        }
                    }
                }
            }
            println!("Playback stopped.");
        });
    }
}
