#![cfg(target_os = "linux")]

use crate::audio::{AudioPlayer, OpusCodec};
use crate::config::AudioConfig;
use crate::stereo_core::alsa::AlsaRedirector;
use crate::stereo_core::discovery::Discovery;
use crate::stereo_core::network::{ControlConnection, MasterNetwork};
use crate::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::stereo_core::sync::now_us;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const SERVER_TCP_PORT: u16 = 53531;

/// 运行主节点模式
pub async fn run_master(role: ChannelRole) -> Result<()> {
    println!("--- 主节点模式 ({}) ---", role.to_string());

    // 0. 设置 ALSA 重定向
    let _alsa_guard = AlsaRedirector::new()?;

    // 1. 设置网络 (UDP + TCP)
    let network = MasterNetwork::setup(SERVER_TCP_PORT).await?;
    let audio_socket = network.audio_socket().clone_inner();

    // 2. 启动服务发现广播
    Discovery::start_broadcast(SERVER_TCP_PORT).await?;

    println!("正在端口 {} 等待从节点连接...", SERVER_TCP_PORT);

    loop {
        let (control_conn, addr) = network.accept().await?;
        println!("从节点已连接: {}", addr);

        let audio_socket = audio_socket.clone();
        let role = role.clone();

        // 启动会话处理句柄
        tokio::spawn(async move {
            if let Err(e) = handle_master_session(control_conn, audio_socket, role).await {
                eprintln!("会话结束: {:?}", e);
            }
        });
    }
}

/// 处理主节点与从节点的会话
async fn handle_master_session(
    mut control: ControlConnection,
    audio_socket: Arc<tokio::net::UdpSocket>,
    role: ChannelRole,
) -> Result<()> {
    let mut buf = [0u8; 1024];

    // 握手
    let pkt = control.recv_packet(&mut buf).await?;
    match pkt {
        ControlPacket::ClientIdentify { role: _r } => {}
        _ => return Err(anyhow::anyhow!("无效的握手协议")),
    };

    let hello = ControlPacket::ServerHello {
        udp_port: audio_socket.local_addr()?.port(),
    };
    control.send_packet(&hello).await?;

    // 等待 UDP 打洞/确认
    let mut buf = [0u8; 128];
    let (_, client_udp_addr) = audio_socket.recv_from(&mut buf).await?;
    println!("从节点 UDP 地址已确认: {}", client_udp_addr);

    // 配置音频
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 2,
        frame_size: 960,
        bitrate: 64000,
        ..AudioConfig::default()
    };

    let mono_config = AudioConfig {
        channels: 1,
        ..config.clone()
    };
    let mut codec = OpusCodec::new(&mono_config)?;
    let playback_config = AudioConfig {
        channels: 1,
        playback_device: "plug:original_default".into(),
        ..config.clone()
    };
    let player = AudioPlayer::new(&playback_config)?;

    let mut raw_buf = vec![0u8; config.frame_size * 2 * 2];
    let mut opus_out = vec![0u8; 1500];
    let mut seq = 0u32;

    let frame_duration_us =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;
    let delay_us = 200_000;

    println!("开始会话循环...");

    // 分离 TCP 读写，以便在不同任务中使用
    let (mut tcp_rx, mut tcp_tx) = control.split();
    let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

    // 处理来自从节点的控制消息
    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = stop_tx.send(()).await;
                    break;
                }
                Ok(n) => {
                    if let Ok(ControlPacket::Ping { client_ts, seq }) =
                        postcard::from_bytes(&buf[..n])
                    {
                        let pong = ControlPacket::Pong {
                            client_ts,
                            server_ts: now_us(),
                            seq,
                        };
                        if tcp_tx
                            .write_all(&postcard::to_allocvec(&pong).unwrap())
                            .await
                            .is_err()
                        {
                            let _ = stop_tx.send(()).await;
                            break;
                        }
                    }
                }
            }
        }
    });

    loop {
        // 打开 FIFO
        let mut fifo = tokio::select! {
            _ = stop_rx.recv() => {
                println!("等待 FIFO 时从节点断开连接。");
                return Ok(());
            }
            f = tokio::fs::File::open(AlsaRedirector::fifo_path()) => f?,
        };

        println!("音频流已启动...");
        let mut stream_start_ts = 0;
        let stream_start_seq = seq;

        loop {
            // 从 FIFO 读取
            let read_res = tokio::select! {
                _ = stop_rx.recv() => {
                    println!("串流过程中从节点断开连接。");
                    return Ok(());
                }
                res = fifo.read_exact(&mut raw_buf) => res,
            };

            if let Err(_) = read_res {
                println!("音频流结束 (FIFO 读取完毕)。");
                break;
            }

            let now = now_us();
            if stream_start_ts == 0 {
                stream_start_ts = now;
            }

            let mut local_pcm = Vec::with_capacity(config.frame_size);
            let mut remote_pcm = Vec::with_capacity(config.frame_size);

            // 提取左右声道
            for i in 0..config.frame_size {
                let l = i16::from_le_bytes([raw_buf[i * 4], raw_buf[i * 4 + 1]]);
                let r = i16::from_le_bytes([raw_buf[i * 4 + 2], raw_buf[i * 4 + 3]]);
                if role == ChannelRole::Left {
                    local_pcm.push(l);
                    remote_pcm.push(r);
                } else {
                    local_pcm.push(r);
                    remote_pcm.push(l);
                }
            }

            // 编码并发送给从节点
            let len = codec.encode(&remote_pcm, &mut opus_out)?;
            let target_ts =
                stream_start_ts + ((seq - stream_start_seq) as u128 * frame_duration_us) + delay_us;

            let packet = AudioPacket {
                seq,
                timestamp: target_ts,
                data: opus_out[..len].to_vec(),
            };

            let bytes = postcard::to_allocvec(&packet)?;
            if let Err(e) = audio_socket.send_to(&bytes, client_udp_addr).await {
                eprintln!("UDP 发送错误: {:?}", e);
                return Err(e.into());
            }

            // 本地回放同步
            let now = now_us();
            if now < target_ts {
                let wait = target_ts - now;
                if wait > 1000 {
                    tokio::time::sleep(Duration::from_micros(wait as u64)).await;
                }
            }
            player.write(&local_pcm)?;

            seq += 1;
        }
    }
}
