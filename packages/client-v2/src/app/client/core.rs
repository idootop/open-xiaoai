#![cfg(target_os = "linux")]

use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, ClientNetwork, ControlConnection};
use crate::net::protocol::{AudioPacket, ControlPacket, DeviceInfo, RpcResult};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct Client {
    info: DeviceInfo,
}

impl Client {
    pub fn new(model: &str, mac: &str, version: u32) -> Self {
        Self {
            info: DeviceInfo {
                model: model.to_string(),
                mac: mac.to_string(),
                version,
            },
        }
    }

    pub async fn run(self) -> Result<()> {
        println!("正在寻找服务端...");
        let (server_ip, server_port) = Discovery::discover_server().await?;
        let server_addr = SocketAddr::new(server_ip, server_port);
        println!("发现服务端: {}", server_addr);

        let network = ClientNetwork::connect(server_addr).await?;
        let mut control = network.into_control();

        // 握手
        control
            .send_packet(&ControlPacket::ClientIdentify {
                info: self.info.clone(),
            })
            .await?;

        match control.recv_packet().await? {
            ControlPacket::IdentifyOk => println!("认证成功"),
            p => return Err(anyhow::anyhow!("认证失败: {:?}", p)),
        }

        let (stop_tx, _) = broadcast::channel::<()>(1);
        let audio_socket = Arc::new(AudioSocket::bind().await?);

        loop {
            tokio::select! {
                packet = control.recv_packet() => {
                    let packet = packet?;
                    match packet {
                        ControlPacket::StartRecording { config } => {
                            println!("开始录音: {:?}", config);
                            let socket = audio_socket.clone();
                            let mut stop_rx = stop_tx.subscribe();
                            let server_addr = control.peer_addr()?;
                            tokio::spawn(async move {
                                if let Err(e) = handle_recording(config, socket, server_addr, stop_rx).await {
                                    eprintln!("录音出错: {}", e);
                                }
                            });
                        }
                        ControlPacket::StopRecording => {
                            println!("停止录音");
                            let _ = stop_tx.send(());
                        }
                        ControlPacket::StartPlayback { config } => {
                            println!("开始播放: {:?}", config);
                            let socket = audio_socket.clone();
                            let mut stop_rx = stop_tx.subscribe();
                            tokio::spawn(async move {
                                if let Err(e) = handle_playback(config, socket, stop_rx).await {
                                    eprintln!("播放出错: {}", e);
                                }
                            });
                        }
                        ControlPacket::StopPlayback => {
                            println!("停止播放");
                            let _ = stop_tx.send(());
                        }
                        ControlPacket::RpcRequest { id, method, args } => {
                            println!("收到 RPC 请求: {} {:?}", method, args);
                            let result = handle_rpc(&method, args).await;
                            control.send_packet(&ControlPacket::RpcResponse { id, result }).await?;
                        }
                        ControlPacket::Ping => {
                            control.send_packet(&ControlPacket::Pong).await?;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn handle_recording(
    config: AudioConfig,
    socket: Arc<AudioSocket>,
    server_addr: SocketAddr,
    mut stop_rx: broadcast::Receiver<()>,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let recorder = AudioRecorder::new(&config)?;
        let mut codec = OpusCodec::new(&config)?;
        let mut pcm_buf = vec![0i16; config.frame_size];
        let mut opus_buf = vec![0u8; 4096];

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            let n = recorder.read(&mut pcm_buf)?;
            if n > 0 {
                let opus_len = codec.encode(&pcm_buf[..n], &mut opus_buf)?;
                let packet = AudioPacket {
                    data: opus_buf[..opus_len].to_vec(),
                };
                socket.send_packet(&packet, server_addr).await?;
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!("当前系统不支持 ALSA 录音，模拟发送音频数据...");
        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let packet = AudioPacket {
                data: vec![0u8; 10],
            };
            socket.send_packet(&packet, server_addr).await?;
        }
    }
    Ok(())
}

async fn handle_playback(
    config: AudioConfig,
    socket: Arc<AudioSocket>,
    mut stop_rx: broadcast::Receiver<()>,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let player = AudioPlayer::new(&config)?;
        let mut codec = OpusCodec::new(&config)?;
        let mut pcm_buf = vec![0i16; config.frame_size];
        let mut udp_buf = vec![0u8; 4096];

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            // 这里简单处理，UDP 接收可能阻塞。实际建议加超时或 select
            let (packet, _) = socket.recv_packet(&mut udp_buf).await?;
            let pcm_len = codec.decode(&packet.data, &mut pcm_buf)?;
            player.write(&pcm_buf[..pcm_len])?;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!("当前系统不支持 ALSA 播放，模拟接收音频数据...");
        let mut udp_buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                _ = stop_rx.recv() => break,
                res = socket.recv_packet(&mut udp_buf) => {
                    let _ = res?;
                }
            }
        }
    }
    Ok(())
}

async fn handle_rpc(method: &str, args: Vec<String>) -> RpcResult {
    if method == "shell" && !args.is_empty() {
        let cmd = &args[0];
        println!("执行 Shell: {}", cmd);

        // 模拟执行
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            let output = Command::new("sh").arg("-c").arg(cmd).output();
            match output {
                Ok(out) => RpcResult {
                    stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                    code: out.status.code().unwrap_or(0),
                },
                Err(e) => RpcResult {
                    stdout: "".to_string(),
                    stderr: e.to_string(),
                    code: -1,
                },
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            RpcResult {
                stdout: format!("模拟执行 {} 成功", cmd),
                stderr: "".to_string(),
                code: 0,
            }
        }
    } else {
        RpcResult {
            stdout: "".to_string(),
            stderr: "未知方法或参数错误".to_string(),
            code: -1,
        }
    }
}
