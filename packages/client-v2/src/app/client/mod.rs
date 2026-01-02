#![cfg(target_os = "linux")]

use crate::audio::codec::OpusCodec;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{AudioPacket, ControlPacket, DeviceInfo, RpcResult};
use crate::net::rpc::RpcManager;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};

pub struct Client {
    info: DeviceInfo,
    conn: Mutex<Option<Arc<Connection>>>,
    rpc: Arc<RpcManager>,
    server_audio_addr: Mutex<Option<SocketAddr>>,
}

impl Client {
    pub fn new() -> Self {
        Self {
            info: DeviceInfo::current(),
            conn: Mutex::new(None),
            rpc: Arc::new(RpcManager::new()),
            server_audio_addr: Mutex::new(None),
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let (ip, tcp_port, udp_port) = Discovery::listen().await?;
        let addr = SocketAddr::new(ip, tcp_port);
        let audio_addr = SocketAddr::new(ip, udp_port);
        println!("Found server at {}, audio at {}", addr, audio_addr);
        *self.server_audio_addr.lock().await = Some(audio_addr);

        println!("Connecting to TCP server at {}...", addr);
        let stream = tokio::net::TcpStream::connect(addr).await?;
        let conn = Arc::new(Connection::new(stream)?);
        println!("TCP connected, sending identification...");

        let audio = Arc::new(AudioSocket::bind().await?);
        conn.send(&ControlPacket::ClientIdentify {
            info: self.info.clone(),
            udp_port: audio.port(),
        })
        .await?;
        match conn.recv().await? {
            ControlPacket::IdentifyOk => println!("Connected to server"),
            p => return Err(anyhow::anyhow!("Handshake failed: {:?}", p)),
        }

        *self.conn.lock().await = Some(conn.clone());
        let (stop_tx, _) = broadcast::channel(1);

        loop {
            let packet = conn.recv().await?;
            let this = self.clone();
            let audio = audio.clone();
            let stop_tx = stop_tx.clone();
            let audio_addr = self.server_audio_addr.lock().await.unwrap();
            tokio::spawn(async move {
                if let Err(e) = this.handle_packet(packet, audio, stop_tx, audio_addr).await {
                    eprintln!("Handle packet error: {}", e);
                }
            });
        }
    }

    pub async fn call(&self, method: &str, args: Vec<String>) -> Result<RpcResult> {
        let (id, rx) = self.rpc.register();
        if let Some(conn) = self.conn.lock().await.as_ref() {
            conn.send(&ControlPacket::RpcRequest {
                id,
                method: method.to_string(),
                args,
            })
            .await?;
            Ok(rx.await?)
        } else {
            Err(anyhow::anyhow!("Not connected"))
        }
    }

    async fn handle_packet(
        &self,
        packet: ControlPacket,
        audio: Arc<AudioSocket>,
        stop_tx: broadcast::Sender<()>,
        server_addr: SocketAddr,
    ) -> Result<()> {
        match packet {
            ControlPacket::RpcRequest { id, method, args } => {
                let result = self.handle_rpc(&method, args).await;
                if let Some(conn) = self.conn.lock().await.as_ref() {
                    conn.send(&ControlPacket::RpcResponse { id, result })
                        .await?;
                }
            }
            ControlPacket::RpcResponse { id, result } => self.rpc.resolve(id, result),
            ControlPacket::StartRecording { config } => {
                let mut stop_rx = stop_tx.subscribe();
                tokio::spawn(async move {
                    let (pcm_tx, mut pcm_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(20);

                    // 录音线程：使用 std::thread 处理阻塞的 ALSA 调用
                    let config_clone = config.clone();
                    std::thread::spawn(move || {
                        let recorder = match AudioRecorder::new(&config_clone) {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("Failed to start recorder: {}", e);
                                return;
                            }
                        };
                        loop {
                            let mut pcm = vec![0i16; config_clone.frame_size];
                            match recorder.read(&mut pcm) {
                                Ok(n) => {
                                    if pcm_tx.blocking_send(pcm[..n].to_vec()).is_err() {
                                        break; // Receiver dropped, stop recording
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Recorder read error: {}", e);
                                    break;
                                }
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
                            _ = stop_rx.recv() => break,
                            Some(pcm_data) = pcm_rx.recv() => {
                                let mut opus = vec![0u8; 4096];
                                if let Ok(len) = codec.encode(&pcm_data, &mut opus) {
                                    let _ = audio.send(&AudioPacket { data: opus[..len].to_vec() }, server_addr).await;
                                }
                            }
                        }
                    }
                    println!("Recording stopped.");
                });
            }
            ControlPacket::StartPlayback { config } => {
                let mut stop_rx = stop_tx.subscribe();
                tokio::spawn(async move {
                    let (pcm_tx, mut pcm_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(20);

                    // 播放线程：使用 std::thread 处理阻塞的 ALSA 调用
                    let config_clone = config.clone();
                    std::thread::spawn(move || {
                        let player = match AudioPlayer::new(&config_clone) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("Failed to start player: {}", e);
                                return;
                            }
                        };
                        while let Some(pcm_data) = pcm_rx.blocking_recv() {
                            let _ = player.write(&pcm_data);
                        }
                    });

                    let mut codec = match OpusCodec::new(&config) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Failed to init opus codec: {}", e);
                            return;
                        }
                    };

                    let mut buf = vec![0u8; 4096];
                    println!("Playback started with jitter buffer...");
                    loop {
                        tokio::select! {
                            _ = stop_rx.recv() => break,
                            res = audio.recv(&mut buf) => {
                                match res {
                                    Ok((packet, _)) => {
                                        let mut pcm = vec![0i16; config.frame_size];
                                        if let Ok(n) = codec.decode(&packet.data, &mut pcm) {
                                            let _ = pcm_tx.send(pcm[..n].to_vec()).await;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Audio recv error: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    println!("Playback stopped.");
                });
            }
            ControlPacket::StopRecording | ControlPacket::StopPlayback => {
                let _ = stop_tx.send(());
            }
            ControlPacket::Ping => {
                if let Some(conn) = self.conn.lock().await.as_ref() {
                    conn.send(&ControlPacket::Pong).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_rpc(&self, method: &str, args: Vec<String>) -> RpcResult {
        match method {
            "shell" if !args.is_empty() => {
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&args[0])
                    .output();
                match output {
                    Ok(out) => RpcResult {
                        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                        code: out.status.code().unwrap_or(0),
                    },
                    Err(e) => RpcResult {
                        stderr: e.to_string(),
                        code: -1,
                        ..Default::default()
                    },
                }
            }
            _ => RpcResult {
                stderr: "Unsupported method".to_string(),
                code: -1,
                ..Default::default()
            },
        }
    }
}
