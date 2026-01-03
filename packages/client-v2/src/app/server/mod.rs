mod audio_manager;

use crate::audio::config::AudioConfig;
use crate::audio::wav::WavReader;
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{ClientInfo, ControlPacket, RpcResult};
use crate::net::rpc::RpcManager;
use audio_manager::ServerAudioManager;
use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub struct Session {
    pub info: ClientInfo,
    pub conn: Arc<Connection>,
    pub rpc: Arc<RpcManager>,
    pub tcp_addr: SocketAddr,
    pub audio_addr: SocketAddr,
    pub session_cancel: CancellationToken,
    pub audio_manager: ServerAudioManager,
    pub tracker: TaskTracker,
}

pub struct Server {
    sessions: DashMap<SocketAddr, Arc<Session>>,
    udp_to_tcp: DashMap<SocketAddr, SocketAddr>,
    audio: Arc<AudioSocket>,
}

impl Server {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            sessions: DashMap::new(),
            udp_to_tcp: DashMap::new(),
            audio: Arc::new(AudioSocket::bind().await?),
        })
    }

    pub async fn run(self: Arc<Self>, port: u16) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        let addr = listener.local_addr()?;
        println!("Server listening on TCP: {}", addr);
        Discovery::broadcast(port).await?;

        // Audio Dispatcher
        let this = self.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match this.audio.recv(&mut buf).await {
                    Ok((packet, src_addr)) => {
                        if let Some(tcp_addr) = this.udp_to_tcp.get(&src_addr) {
                            if let Some(session) = this.sessions.get(tcp_addr.value()) {
                                // 使用 try_send 避免某一个客户端阻塞导致全局音频延迟
                                let _ = session.audio_manager.audio_tx().try_send(packet);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Audio socket recv error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        loop {
            let (stream, addr) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.clone().handle_connection(stream, addr).await {
                    eprintln!("Session {} error: {}", addr, e);
                }
                server.remove_session(&addr).await;
            });
        }
    }

    async fn remove_session(&self, tcp_addr: &SocketAddr) {
        if let Some((_, session)) = self.sessions.remove(tcp_addr) {
            session.session_cancel.cancel();
            session.tracker.close();
            self.udp_to_tcp.remove(&session.audio_addr);
            println!("Session {} ({}) closed", tcp_addr, session.info.model);
        }
    }

    async fn handle_connection(
        self: Arc<Self>,
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
    ) -> Result<()> {
        println!("New TCP connection from {}", addr);
        let conn = Arc::new(Connection::new(stream)?);

        // --- 握手 (Handshake) ---
        let version = env!("CARGO_PKG_VERSION").to_string();
        let server_auth =
            std::env::var("XIAO_SERVER_AUTH").unwrap_or_else(|_| "open-xiaoai".to_string());
        let client_auth =
            std::env::var("XIAO_CLIENT_AUTH").unwrap_or_else(|_| "open-xiaoai".to_string());

        let (info, client_audio_port) = match conn.recv().await? {
            ControlPacket::ClientHello {
                auth,
                version: v,
                udp_port,
                info,
            } => {
                if v != version {
                    return Err(anyhow!("Client version mismatch: {} != {}", v, version));
                }
                if auth != server_auth {
                    return Err(anyhow!("Invalid client auth"));
                }
                (info, udp_port)
            }
            p => {
                return Err(anyhow!("Handshake failed: unexpected packet {:?}", p));
            }
        };

        conn.send(&ControlPacket::ServerHello {
            auth: client_auth,
            version: env!("CARGO_PKG_VERSION").to_string(),
            udp_port: self.audio.port(),
        })
        .await?;

        let audio_addr = SocketAddr::new(addr.ip(), client_audio_port);
        println!(
            "Client identified: {} ({}), audio at {}",
            info.model, info.serial_number, audio_addr
        );

        let tracker = TaskTracker::new();
        let session_cancel = CancellationToken::new();
        let (audio_manager, audio_rx, recorder_rx) =
            ServerAudioManager::new(session_cancel.clone(), tracker.clone());

        let session = Arc::new(Session {
            info,
            conn: conn.clone(),
            rpc: Arc::new(RpcManager::new()),
            tcp_addr: addr,
            audio_addr,
            session_cancel,
            audio_manager,
            tracker: tracker.clone(),
        });

        session.audio_manager.spawn_audio_processor(audio_rx, recorder_rx);

        self.sessions.insert(addr, session.clone());
        self.udp_to_tcp.insert(audio_addr, addr);

        // --- Main Connection Loop ---
        // 统一心跳与超时处理，节省资源
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = session.session_cancel.cancelled() => break,
                _ = heartbeat.tick() => {
                    if conn.send(&ControlPacket::Ping).await.is_err() { break; }
                }
                res = tokio::time::timeout(std::time::Duration::from_secs(60), conn.recv()) => {
                    match res {
                        Ok(Ok(packet)) => {
                            if let Err(e) = self.process_packet(session.clone(), packet).await {
                                eprintln!("Process packet error: {}", e);
                            }
                        }
                        Ok(Err(e)) => {
                            eprintln!("Session {} connection error: {}", addr, e);
                            break;
                        }
                        Err(_) => {
                            eprintln!("Session {} timeout", addr);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_packet(&self, session: Arc<Session>, packet: ControlPacket) -> Result<()> {
        match packet {
            ControlPacket::Ping => {
                session.conn.send(&ControlPacket::Pong).await?;
            }
            ControlPacket::Pong => {}
            ControlPacket::RpcResponse { id, result } => {
                session.rpc.resolve(id, result);
            }
            ControlPacket::RpcRequest { id, method, args } => {
                let result = self.handle_rpc(&session, &method, args).await;
                session
                    .conn
                    .send(&ControlPacket::RpcResponse { id, result })
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn get_clients(&self) -> Vec<SocketAddr> {
        self.sessions.iter().map(|r| *r.key()).collect()
    }

    pub async fn call(
        &self,
        addr: SocketAddr,
        method: &str,
        args: Vec<String>,
    ) -> Result<RpcResult> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;
        let (id, rx) = session.rpc.register();
        session
            .conn
            .send(&ControlPacket::RpcRequest {
                id,
                method: method.to_string(),
                args,
            })
            .await?;
        Ok(rx.await?)
    }

    async fn handle_rpc(
        &self,
        _session: &Arc<Session>,
        method: &str,
        args: Vec<String>,
    ) -> RpcResult {
        match method {
            "hello" => RpcResult {
                stdout: format!("Hello from server! Args: {:?}", args),
                ..Default::default()
            },
            _ => RpcResult {
                stderr: format!("Unknown method: {}", method),
                code: -1,
                ..Default::default()
            },
        }
    }

    pub async fn start_record(&self, addr: SocketAddr, config: AudioConfig) -> Result<()> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;

        let filename = format!(
            "temp/recorded_{}.wav",
            session.info.serial_number.replace(":", "")
        );

        // 通知 Audio Manager 开始录音
        session
            .audio_manager
            .start_recording(config.clone(), filename)
            .await?;

        session
            .conn
            .send(&ControlPacket::StartRecording { config })
            .await?;

        Ok(())
    }

    pub async fn stop_record(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;

        // 停止录音任务
        session.audio_manager.stop_recording().await?;
        session.conn.send(&ControlPacket::StopRecording).await?;
        Ok(())
    }

    pub async fn start_play(&self, addr: SocketAddr, file_path: &str) -> Result<()> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;

        let reader = WavReader::open(file_path)?;
        let opus_rate = if reader.sample_rate > 24000 {
            48000
        } else {
            16000
        };

        let config = AudioConfig {
            sample_rate: opus_rate,
            channels: reader.channels,
            frame_size: (opus_rate / 50) as usize, // 20ms
            ..AudioConfig::music_48k()
        };

        session
            .conn
            .send(&ControlPacket::StartPlayback {
                config: config.clone(),
            })
            .await?;

        session.audio_manager.start_playback(
            config,
            reader,
            self.audio.clone(),
            session.audio_addr,
        ).await?;

        Ok(())
    }

    pub async fn stop_play(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;

        // 停止播放任务
        session.audio_manager.stop_playback();
        session.conn.send(&ControlPacket::StopPlayback).await?;
        Ok(())
    }
}
