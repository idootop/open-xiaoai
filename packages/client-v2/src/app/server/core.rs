use crate::app::server::handlers;
use crate::app::server::session::ServerSession;
use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::wav::{WavReader, WavWriter};
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, ControlConnection, ServerNetwork};
use crate::net::protocol::{AudioPacket, ControlPacket, RpcResult};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct Server {
    sessions: Arc<Mutex<HashMap<SocketAddr, Arc<ServerSession>>>>,
    audio_socket: Arc<AudioSocket>,
}

impl Server {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            audio_socket: Arc::new(AudioSocket::bind().await?),
        })
    }

    pub async fn run(self: Arc<Self>, port: u16) -> Result<()> {
        let network = ServerNetwork::setup(port).await?;
        println!("服务端启动在: {}", network.local_addr()?);

        // 启动广播
        Discovery::start_broadcast(port).await?;

        loop {
            let (control, addr) = network.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(control, addr).await {
                    eprintln!("连接 {} 出错: {}", addr, e);
                }
                server.sessions.lock().remove(&addr);
                println!("客户端 {} 断开连接", addr);
            });
        }
    }

    async fn handle_connection(
        &self,
        mut control: ControlConnection,
        addr: SocketAddr,
    ) -> Result<()> {
        println!("新客户端连接: {}", addr);

        // 握手认证
        let info = match control.recv_packet().await? {
            ControlPacket::ClientIdentify { info } => info,
            p => return Err(anyhow::anyhow!("预期的握手包，收到: {:?}", p)),
        };

        println!(
            "客户端识别: {} ({}) v{}",
            info.model, info.mac, info.version
        );
        control.send_packet(&ControlPacket::IdentifyOk).await?;

        let (mut reader, writer) = control.split();
        let session = Arc::new(ServerSession::new(info, writer, addr));

        self.sessions.lock().insert(addr, session.clone());

        // 处理控制消息循环
        loop {
            let packet = reader.recv_packet().await?;
            let session = session.clone();
            tokio::spawn(async move {
                if let Err(e) = handlers::handle_packet(session, packet).await {
                    eprintln!("处理来自 {} 的包时出错: {}", addr, e);
                }
            });
        }
    }

    // 暴露给外部调用的方法
    pub async fn call_shell(&self, addr: SocketAddr, cmd: &str) -> Result<RpcResult> {
        let session = self
            .sessions
            .lock()
            .get(&addr)
            .cloned()
            .context("未找到 Session")?;
        let (id, rx) = session.rpc.alloc_id();
        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::RpcRequest {
                id,
                method: "shell".to_string(),
                args: vec![cmd.to_string()],
            })
            .await?;
        Ok(rx.await?)
    }

    pub async fn start_recording(&self, addr: SocketAddr, config: AudioConfig) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .get(&addr)
            .cloned()
            .context("未找到 Session")?;

        // 发送开始录音指令
        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::StartRecording {
                config: config.clone(),
            })
            .await?;

        let audio_socket = self.audio_socket.clone();
        tokio::spawn(async move {
            if let Err(e) = save_audio_to_wav(audio_socket, config, "temp/recorded.wav").await {
                eprintln!("保存录音失败: {}", e);
            }
        });

        Ok(())
    }

    pub async fn stop_recording(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .get(&addr)
            .cloned()
            .context("未找到 Session")?;
        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::StopRecording)
            .await?;
        Ok(())
    }

    pub async fn start_playback(&self, addr: SocketAddr, config: AudioConfig) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .get(&addr)
            .cloned()
            .context("未找到 Session")?;

        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::StartPlayback {
                config: config.clone(),
            })
            .await?;

        let audio_socket = self.audio_socket.clone();
        tokio::spawn(async move {
            if let Err(e) = stream_wav_to_client(audio_socket, addr, config, "temp/test.wav").await
            {
                eprintln!("推流失败: {}", e);
            }
        });

        Ok(())
    }

    pub async fn stop_playback(&self, addr: SocketAddr) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .get(&addr)
            .cloned()
            .context("未找到 Session")?;
        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::StopPlayback)
            .await?;
        Ok(())
    }

    pub fn get_sessions(&self) -> Vec<SocketAddr> {
        self.sessions.lock().keys().cloned().collect()
    }
}

async fn save_audio_to_wav(
    socket: Arc<AudioSocket>,
    config: AudioConfig,
    path: &str,
) -> Result<()> {
    std::fs::create_dir_all("temp")?;
    let mut writer = WavWriter::create(path, config.sample_rate, config.channels)?;
    let mut codec = OpusCodec::new(&config)?;
    let mut pcm_buf = vec![0i16; config.frame_size];
    let mut udp_buf = vec![0u8; 4096];

    println!("正在录制到 {}...", path);

    for _ in 0..100 {
        let (packet, _) = socket.recv_packet(&mut udp_buf).await?;
        let pcm_len = codec.decode(&packet.data, &mut pcm_buf)?;
        writer.write_samples(&pcm_buf[..pcm_len])?;
    }

    writer.finalize()?;
    println!("录制完成: {}", path);
    Ok(())
}

async fn stream_wav_to_client(
    socket: Arc<AudioSocket>,
    target: SocketAddr,
    config: AudioConfig,
    path: &str,
) -> Result<()> {
    let mut reader = WavReader::open(path)?;
    let mut codec = OpusCodec::new(&config)?;
    let mut pcm_buf = vec![0i16; config.frame_size];
    let mut opus_buf = vec![0u8; 4096];

    println!("正在从 {} 推流...", path);

    loop {
        let n = reader.read_samples(&mut pcm_buf)?;
        if n == 0 {
            break;
        }
        let opus_len = codec.encode(&pcm_buf[..n], &mut opus_buf)?;
        let packet = AudioPacket {
            data: opus_buf[..opus_len].to_vec(),
        };
        socket.send_packet(&packet, target).await?;

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    println!("推流结束");
    Ok(())
}
