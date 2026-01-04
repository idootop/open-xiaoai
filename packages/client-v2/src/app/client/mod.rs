#![cfg(target_os = "linux")]

mod audio_manager;

use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{ClientInfo, ControlPacket, RpcResult};
use crate::net::rpc::RpcManager;
use anyhow::{Result, anyhow};
use audio_manager::ClientAudioManager;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock};
use tokio_util::sync::CancellationToken;

struct ActiveSession {
    conn: Arc<Connection>,
    audio_manager: ClientAudioManager,
    session_cancel: CancellationToken,
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
            println!("Searching for server...");
            let (ip, tcp_port) = match Discovery::listen().await {
                Ok(res) => res,
                Err(e) => {
                    eprintln!("Discovery listen error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            let addr = SocketAddr::new(ip, tcp_port);
            println!("Found server at {}", addr);

            match tokio::net::TcpStream::connect(addr).await {
                Err(e) => eprintln!("Failed to connect to {}: {}", addr, e),
                Ok(stream) => {
                    println!("Connected to TCP server at {}", addr);
                    if let Err(e) = self.clone().handle_session(stream, addr).await {
                        eprintln!("Session error: {:?}", e);
                    }
                }
            }

            self.cleanup().await;
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
            std::env::var("XIAO_SERVER_AUTH").unwrap_or_else(|_| "xiao-server".to_string());
        let client_auth =
            std::env::var("XIAO_CLIENT_AUTH").unwrap_or_else(|_| "xiao-client".to_string());

        conn.send(&ControlPacket::ClientHello {
            auth: server_auth,
            version: version.clone(),
            udp_port: audio_socket.port(),
            // todo client info
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

        let audio_addr = SocketAddr::new(addr.ip(), server_udp_port);
        println!(
            "Handshake successful with {}, audio at {}",
            addr, audio_addr
        );

        // --- 初始化 Session ---
        let session_cancel = CancellationToken::new();
        let session = Arc::new(ActiveSession {
            conn: conn.clone(),
            audio_manager: ClientAudioManager::new(
                audio_socket,
                audio_addr,
                session_cancel.clone(),
            ),
            session_cancel,
        });
        *self.session.write().await = Some(session.clone());

        // 心跳
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

        // 消息主循环
        loop {
            tokio::select! {
                _ = session.session_cancel.cancelled() => break,
                res = tokio::time::timeout(std::time::Duration::from_secs(60), conn.recv()) => {
                    match res {
                        Ok(Ok(packet)) => {
                            if let Err(e) = self.process_packet(packet, &session).await {
                                eprintln!("Process packet error: {}", e);
                            }
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

    // todo 考虑有些操作比较耗时，需要非阻塞处理
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
                session.audio_manager.start_recording(config).await;
            }
            ControlPacket::StartPlayback { config } => {
                session.audio_manager.start_playback(config).await;
            }
            ControlPacket::StopRecording => {
                session.audio_manager.stop_recorder().await;
            }
            ControlPacket::StopPlayback => {
                session.audio_manager.stop_player().await;
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
