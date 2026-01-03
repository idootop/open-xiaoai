#![cfg(target_os = "linux")]

use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{AudioPacket, ClientInfo, ControlPacket, RpcResult};
use crate::net::rpc::RpcManager;
use anyhow::{Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

/// 内部连接上下文，包含了音频流所需的全部信息
struct ActiveSession {
    conn: Arc<Connection>,
    audio_socket: Arc<AudioSocket>,
    server_audio_addr: SocketAddr,
    session_cancel: CancellationToken, // 控制整个 Session 的生命周期
    record_cancel: RwLock<Option<CancellationToken>>,
    play_cancel: RwLock<Option<CancellationToken>>,
}

pub struct Client {
    session: RwLock<Option<Arc<ActiveSession>>>,
    rpc: Arc<RpcManager>,
}

impl Client {
    pub fn new() -> Self {
        Self {
            session: RwLock::new(None),
            rpc: Arc::new(RpcManager::new()),
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        loop {
            let (ip, tcp_port) = match Discovery::listen().await {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("Discovery listen error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
            };

            let addr = SocketAddr::new(ip, tcp_port);
            println!("Found server at {}", addr);

            if let Ok(stream) = tokio::net::TcpStream::connect(addr).await {
                println!("Connected to TCP server at {}", addr);
                if let Err(e) = self.clone().handle_session(stream, addr).await {
                    eprintln!("Session error: {:?}", e);
                }
            } else {
                eprintln!("Failed to connect to {}", addr);
            }

            self.cleanup().await;
            println!(
                "Connection to {} closed, searching for server again...",
                addr
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn cleanup(&self) {
        let mut session_guard = self.session.write().await;
        if let Some(s) = session_guard.take() {
            s.session_cancel.cancel();
        }
    }

    async fn handle_session(
        self: Arc<Self>,
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
    ) -> Result<()> {
        let conn = Arc::new(Connection::new(stream)?);
        let audio_socket = Arc::new(AudioSocket::bind().await?);

        // --- 握手 (Handshake) ---
        let version = env!("CARGO_PKG_VERSION").to_string();
        let server_auth =
            std::env::var("XIAO_SERVER_AUTH").unwrap_or_else(|_| "open-xiaoai".to_string());
        let client_auth =
            std::env::var("XIAO_CLIENT_AUTH").unwrap_or_else(|_| "open-xiaoai".to_string());

        conn.send(&ControlPacket::ClientHello {
            auth: server_auth,
            version: version.clone(),
            udp_port: audio_socket.port(),
            info: ClientInfo {
                model: "Open-XiaoAi-V2".to_string(),
                serial_number: "00:00:00:00:00:00".to_string(),
            },
        })
        .await?;

        let server_udp_port = match conn.recv().await? {
            ControlPacket::ServerHello {
                version: v,
                udp_port,
                auth,
            } => {
                if v != version {
                    return Err(anyhow!("Server version mismatch: {} != {}", v, version));
                }
                if auth != client_auth {
                    return Err(anyhow!("Invalid server auth"));
                }
                udp_port
            }
            _ => return Err(anyhow!("Handshake failed")),
        };

        let server_audio_addr = SocketAddr::new(addr.ip(), server_udp_port);
        println!(
            "Handshake successful with {}, audio at {}",
            addr, server_audio_addr
        );

        // --- 初始化 Session ---
        let session = Arc::new(ActiveSession {
            conn: conn.clone(),
            audio_socket,
            server_audio_addr,
            session_cancel: CancellationToken::new(),
            record_cancel: RwLock::new(None),
            play_cancel: RwLock::new(None),
        });
        *self.session.write().await = Some(session.clone());

        // 使用 mpsc 队列来缓冲指令，确保顺序执行且不阻塞接收循环
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<ControlPacket>(64);

        // 任务 1: 心跳
        let hb_session = session.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = hb_session.session_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        if hb_session.conn.send(&ControlPacket::Ping).await.is_err() { break; }
                    }
                }
            }
        });

        // 任务 2: 命令处理器 (串行处理所有指令)
        let proc_self = self.clone();
        let proc_session = session.clone();
        tokio::spawn(async move {
            while let Some(packet) = cmd_rx.recv().await {
                if let Err(e) = proc_self.process_packet(packet, &proc_session).await {
                    eprintln!("Process error: {}", e);
                }
            }
        });

        // 任务 3: 接收循环 (高优先级，只读包并分发)
        loop {
            tokio::select! {
                _ = session.session_cancel.cancelled() => break,
                res = tokio::time::timeout(std::time::Duration::from_secs(60), conn.recv()) => {
                    match res {
                        Ok(Ok(packet)) => {
                            if cmd_tx.send(packet).await.is_err() { break; }
                        }
                        Ok(Err(e)) => {
                            return Err(anyhow!("Connection receive error: {}", e));
                        }
                        Err(_) => return Err(anyhow!("Connection timeout")),
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_packet(
        &self,
        packet: ControlPacket,
        session: &Arc<ActiveSession>,
    ) -> Result<()> {
        match packet {
            ControlPacket::Ping => {
                session.conn.send(&ControlPacket::Pong).await?;
            }
            ControlPacket::Pong => {}
            ControlPacket::RpcResponse { id, result } => {
                self.rpc.resolve(id, result);
            }
            ControlPacket::RpcRequest { id, method, args } => {
                let result = self.handle_rpc(&method, args).await;
                session
                    .conn
                    .send(&ControlPacket::RpcResponse { id, result })
                    .await?;
            }
            ControlPacket::StartRecording { config } => {
                self.stop_recorder(session).await; // 开启前先停止旧的，防止资源冲突
                let token = session.session_cancel.child_token();
                *session.record_cancel.write().await = Some(token.clone());
                self.spawn_recorder(session.clone(), config, token);
            }
            ControlPacket::StartPlayback { config } => {
                self.stop_player(session).await;
                let token = session.session_cancel.child_token();
                *session.play_cancel.write().await = Some(token.clone());
                self.spawn_player(session.clone(), config, token);
            }
            ControlPacket::StopRecording => {
                self.stop_recorder(session).await;
            }
            ControlPacket::StopPlayback => {
                self.stop_player(session).await;
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

    async fn stop_recorder(&self, session: &ActiveSession) {
        let mut cancel_guard = session.record_cancel.write().await;
        if let Some(token) = cancel_guard.take() {
            token.cancel();
        }
    }

    async fn stop_player(&self, session: &ActiveSession) {
        let mut cancel_guard = session.play_cancel.write().await;
        if let Some(token) = cancel_guard.take() {
            token.cancel();
        }
    }

    fn spawn_recorder(
        &self,
        session: Arc<ActiveSession>,
        config: AudioConfig,
        token: CancellationToken,
    ) {
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
                            let _ = session.audio_socket.send(&AudioPacket { data: out[..len].to_vec() }, session.server_audio_addr).await;
                        }
                    }
                }
            }
            println!("Recording stopped.");
        });
    }

    fn spawn_player(
        &self,
        session: Arc<ActiveSession>,
        config: AudioConfig,
        token: CancellationToken,
    ) {
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
                    res = session.audio_socket.recv(&mut udp_buf) => {
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

    pub async fn call(&self, method: &str, args: Vec<String>) -> Result<RpcResult> {
        let (id, rx) = self.rpc.register();
        let session_guard = self.session.read().await;
        if let Some(session) = session_guard.as_ref() {
            session
                .conn
                .send(&ControlPacket::RpcRequest {
                    id,
                    method: method.to_string(),
                    args,
                })
                .await?;
            Ok(rx.await?)
        } else {
            Err(anyhow!("Not connected"))
        }
    }
}
