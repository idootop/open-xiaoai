use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::wav::{WavReader, WavWriter};
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, Connection};
use crate::net::protocol::{AudioPacket, ControlPacket, DeviceInfo, RpcResult};
use crate::net::rpc::RpcManager;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Session {
    pub info: DeviceInfo,
    pub conn: Arc<Connection>,
    pub rpc: Arc<RpcManager>,
    pub audio_addr: SocketAddr,
}

pub struct Server {
    sessions: Arc<Mutex<HashMap<SocketAddr, Arc<Session>>>>,
    audio: Arc<AudioSocket>,
}

impl Server {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            audio: Arc::new(AudioSocket::bind().await?),
        })
    }

    pub async fn run(self: Arc<Self>, port: u16) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        let addr = listener.local_addr()?;
        println!("Server listening on TCP: {}", addr);
        Discovery::broadcast(port, self.audio.port()).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(stream, addr).await {
                    eprintln!("Session {} error: {}", addr, e);
                }
                server.sessions.lock().await.remove(&addr);
            });
        }
    }

    async fn handle_connection(
        &self,
        stream: tokio::net::TcpStream,
        addr: SocketAddr,
    ) -> Result<()> {
        println!("New TCP connection from {}", addr);
        let conn = Arc::new(Connection::new(stream)?);

        let (info, client_udp_port) = match conn.recv().await? {
            ControlPacket::ClientIdentify { info, udp_port } => (info, udp_port),
            p => {
                println!("Expected Identify from {}, got {:?}", addr, p);
                return Err(anyhow::anyhow!("Expected Identify, got {:?}", p));
            }
        };

        let audio_addr = SocketAddr::new(addr.ip(), client_udp_port);
        println!(
            "Client identified: {} ({}) version {}, audio at {}",
            info.model, addr, info.version, audio_addr
        );
        conn.send(&ControlPacket::IdentifyOk).await?;

        let session = Arc::new(Session {
            info,
            conn: conn.clone(),
            rpc: Arc::new(RpcManager::new()),
            audio_addr,
        });

        self.sessions.lock().await.insert(addr, session.clone());

        loop {
            let packet = conn.recv().await?;
            let session = session.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_packet(session, packet).await {
                    eprintln!("Handle packet error: {}", e);
                }
            });
        }
    }

    pub async fn get_clients(&self) -> Vec<SocketAddr> {
        self.sessions.lock().await.keys().cloned().collect()
    }

    pub async fn call(
        &self,
        addr: SocketAddr,
        method: &str,
        args: Vec<String>,
    ) -> Result<RpcResult> {
        let session = self
            .sessions
            .lock()
            .await
            .get(&addr)
            .cloned()
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

    pub async fn start_record(&self, addr: SocketAddr, config: AudioConfig) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .await
            .get(&addr)
            .cloned()
            .context("Session not found")?;
        session
            .conn
            .send(&ControlPacket::StartRecording {
                config: config.clone(),
            })
            .await?;
        let audio = self.audio.clone();
        let target_addr = session.audio_addr;
        let filename = format!("temp/recorded_{}.wav", session.info.mac.replace(":", ""));
        tokio::spawn(async move {
            let mut writer =
                WavWriter::create(&filename, config.sample_rate, config.channels).unwrap();
            let mut codec = OpusCodec::new(&config).unwrap();
            let mut pcm = vec![0i16; config.frame_size];
            let mut buf = vec![0u8; 4096];
            println!(
                "Recording started for {}, saving to {}",
                target_addr, filename
            );
            for _ in 0..500 {
                // Record ~10s
                if let Ok((packet, src_addr)) = audio.recv(&mut buf).await {
                    if src_addr == target_addr {
                        if let Ok(n) = codec.decode(&packet.data, &mut pcm) {
                            writer.write_samples(&pcm[..n]).unwrap();
                        }
                    }
                }
            }
            writer.finalize().unwrap();
            println!("Recording saved to {}", filename);
        });
        Ok(())
    }

    pub async fn start_play(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .await
            .get(&addr)
            .cloned()
            .context("Session not found")?;
        let reader = WavReader::open("temp/test.wav")?;

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
        let audio = self.audio.clone();
        let target_addr = session.audio_addr;
        tokio::spawn(async move {
            let mut reader = reader;
            let mut codec = OpusCodec::new(&config).unwrap();
            let mut pcm = vec![0i16; config.frame_size];
            let mut opus = vec![0u8; 4096];
            while let Ok(n) = reader.read_samples(&mut pcm) {
                if n == 0 {
                    break;
                }
                if let Ok(len) = codec.encode(&pcm[..n], &mut opus) {
                    let _ = audio
                        .send(
                            &AudioPacket {
                                data: opus[..len].to_vec(),
                            },
                            target_addr,
                        )
                        .await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        });
        Ok(())
    }
}

async fn handle_packet(session: Arc<Session>, packet: ControlPacket) -> Result<()> {
    match packet {
        ControlPacket::RpcRequest { id, method, args } => {
            let result = handle_rpc(&session, &method, args).await;
            session
                .conn
                .send(&ControlPacket::RpcResponse { id, result })
                .await?;
        }
        ControlPacket::RpcResponse { id, result } => session.rpc.resolve(id, result),
        ControlPacket::Ping => session.conn.send(&ControlPacket::Pong).await?,
        _ => {}
    }
    Ok(())
}

async fn handle_rpc(_session: &Arc<Session>, method: &str, args: Vec<String>) -> RpcResult {
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
