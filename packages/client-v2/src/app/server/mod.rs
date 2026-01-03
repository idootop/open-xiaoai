use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::wav::{WavReader, WavWriter};
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{AudioPacket, ClientInfo, ControlPacket, RpcResult};
use crate::net::rpc::RpcManager;
use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub enum RecorderCommand {
    Start {
        config: AudioConfig,
        filename: String,
    },
    Stop,
}

pub struct Session {
    pub info: ClientInfo,
    pub conn: Arc<Connection>,
    pub rpc: Arc<RpcManager>,
    pub tcp_addr: SocketAddr,
    pub audio_addr: SocketAddr,
    pub session_cancel: CancellationToken,
    pub record_cancel: Mutex<Option<CancellationToken>>,
    pub play_cancel: Mutex<Option<CancellationToken>>,
    pub audio_tx: mpsc::Sender<AudioPacket>,
    pub recorder_tx: mpsc::Sender<RecorderCommand>,
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
                                let _ = session.audio_tx.try_send(packet);
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

        let (audio_tx, audio_rx) = mpsc::channel(1024);
        let (recorder_tx, recorder_rx) = mpsc::channel(64);
        let tracker = TaskTracker::new();
        let session = Arc::new(Session {
            info,
            conn: conn.clone(),
            rpc: Arc::new(RpcManager::new()),
            tcp_addr: addr,
            audio_addr,
            session_cancel: CancellationToken::new(),
            record_cancel: Mutex::new(None),
            play_cancel: Mutex::new(None),
            audio_tx,
            recorder_tx,
            tracker: tracker.clone(),
        });

        self.sessions.insert(addr, session.clone());
        self.udp_to_tcp.insert(audio_addr, addr);

        // --- Audio Processor Task ---
        // 优化 Audio Receiver 的 OwnerShip，并使用 Tracker 跟踪
        let processor_session = session.clone();
        tracker.spawn(async move {
            let mut active_recorder: Option<(WavWriter, OpusCodec, usize)> = None;
            let mut audio_rx = audio_rx;
            let mut recorder_rx = recorder_rx;
            loop {
                tokio::select! {
                    _ = processor_session.session_cancel.cancelled() => break,
                    cmd = recorder_rx.recv() => {
                        match cmd {
                            Some(RecorderCommand::Start { config, filename }) => {
                                if let Some((writer, _, _)) = active_recorder.take() {
                                    let _ = writer.finalize();
                                }
                                match WavWriter::create(&filename, config.sample_rate, config.channels) {
                                    Ok(writer) => {
                                        match OpusCodec::new(&config) {
                                            Ok(codec) => active_recorder = Some((writer, codec, config.frame_size)),
                                            Err(e) => eprintln!("Failed to create opus codec: {}", e),
                                        }
                                    }
                                    Err(e) => eprintln!("Failed to create wav writer: {}", e),
                                }
                            }
                            Some(RecorderCommand::Stop) => {
                                if let Some((writer, _, _)) = active_recorder.take() {
                                    let _ = writer.finalize();
                                }
                            }
                            None => break,
                        }
                    }
                    packet = audio_rx.recv() => {
                        match packet {
                            Some(packet) => {
                                if let Some((writer, codec, frame_size)) = &mut active_recorder {
                                    let mut pcm = vec![0i16; *frame_size];
                                    if let Ok(n) = codec.decode(&packet.data, &mut pcm) {
                                        let _ = writer.write_samples(&pcm[..n]);
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            if let Some((writer, _, _)) = active_recorder.take() {
                let _ = writer.finalize();
            }
        });

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

        // 仅停止之前的录音任务
        {
            let mut guard = session.record_cancel.lock();
            if let Some(token) = guard.take() {
                token.cancel();
            }
            *guard = Some(session.session_cancel.child_token());
        }

        let filename = format!(
            "temp/recorded_{}.wav",
            session.info.serial_number.replace(":", "")
        );

        // 通知 Audio Processor 开始录音
        session
            .recorder_tx
            .send(RecorderCommand::Start {
                config: config.clone(),
                filename,
            })
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
        if let Some(token) = session.record_cancel.lock().take() {
            token.cancel();
        }

        let _ = session.recorder_tx.send(RecorderCommand::Stop).await;
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

        // 仅停止之前的播放任务
        let token = {
            let mut guard = session.play_cancel.lock();
            if let Some(token) = guard.take() {
                token.cancel();
            }
            let token = session.session_cancel.child_token();
            *guard = Some(token.clone());
            token
        };

        session
            .conn
            .send(&ControlPacket::StartPlayback {
                config: config.clone(),
            })
            .await?;

        let audio_socket = self.audio.clone();
        let target_addr = session.audio_addr;
        let playback_session = session.clone();

        session.tracker.spawn(async move {
            let mut reader = reader;
            let mut codec = match OpusCodec::new(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to create opus codec: {}", e);
                    return;
                }
            };
            let mut pcm = vec![0i16; config.frame_size];
            let mut opus = vec![0u8; 4096];
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(20));

            println!("Playback started for {}", playback_session.tcp_addr);

            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = playback_session.session_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        match reader.read_samples(&mut pcm) {
                            Ok(0) => break,
                            Ok(n) => {
                                if let Ok(len) = codec.encode(&pcm[..n], &mut opus) {
                                    let _ = audio_socket
                                        .send(
                                            &AudioPacket {
                                                data: opus[..len].to_vec(),
                                            },
                                            target_addr,
                                        )
                                        .await;
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to read samples: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
            println!("Playback finished for {}", playback_session.tcp_addr);
        });

        Ok(())
    }

    pub async fn stop_play(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .get(&addr)
            .map(|r| r.value().clone())
            .context("Session not found")?;

        // 停止播放任务
        if let Some(token) = session.play_cancel.lock().take() {
            token.cancel();
        }

        session.conn.send(&ControlPacket::StopPlayback).await?;
        Ok(())
    }
}
