#![cfg(target_os = "linux")]

use crate::audio::{AudioPlayer, OpusCodec};
use crate::config::AudioConfig;
use crate::stereo_core::alsa::AlsaRedirector;
use crate::stereo_core::discovery::Discovery;
use crate::stereo_core::network::{ControlConnection, MasterNetwork};
use crate::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::stereo_core::sync::now_us;
use anyhow::{Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub const SERVER_TCP_PORT: u16 = 53531;

#[derive(Clone)]
struct SlaveSession {
    udp_addr: SocketAddr,
    role: ChannelRole,
}

pub async fn run_master(master_role: ChannelRole) -> Result<()> {
    println!("--- 主节点模式 ({}) ---", master_role.to_string());

    // 0. 设置 ALSA 重定向
    let _alsa_guard = AlsaRedirector::new()?;

    // 1. 设置网络 (UDP + TCP)
    let network = MasterNetwork::setup(SERVER_TCP_PORT).await?;
    let audio_socket = network.audio_socket().clone_inner();

    // 2. 启动服务发现广播
    Discovery::start_broadcast(SERVER_TCP_PORT).await?;

    println!("✅ 服务已启动，等待连接...");

    let slaves = Arc::new(Mutex::new(Vec::<SlaveSession>::new()));

    // 3. 启动连接监听任务
    let slaves_clone = slaves.clone();
    let audio_socket_clone = audio_socket.clone();
    tokio::spawn(async move {
        loop {
            match network.accept().await {
                Ok((control_conn, client_addr)) => {
                    let slaves_for_session = slaves_clone.clone();
                    let audio_socket_for_session = audio_socket_clone.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_master_session(
                            control_conn,
                            audio_socket_for_session,
                            slaves_for_session,
                            client_addr.to_string(),
                        )
                        .await
                        {
                            eprintln!("❌ 会话错误: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("❌ Accept 错误: {:?}", e);
                }
            }
        }
    });

    // 4. 音频处理主循环
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 2,
        frame_size: 960,
        bitrate: 64000,
        ..AudioConfig::default()
    };

    let mut mono_codec = OpusCodec::new(&AudioConfig {
        channels: 1,
        ..config.clone()
    })?;

    let mut current_player_channels = 0;
    let mut player: Option<AudioPlayer> = None;

    let mut raw_buf = vec![0u8; config.frame_size * 2 * 2];
    let mut opus_out = vec![0u8; 1500];
    let mut seq = 0u32;

    let delay_us = 200_000;
    let frame_duration_us =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

    let mut stream_start_ts = 0;
    let mut stream_start_seq = 0;

    loop {
        // 打开 FIFO
        let mut fifo = match tokio::fs::File::open(AlsaRedirector::fifo_path()).await {
            Ok(f) => f,
            Err(e) => {
                eprintln!("❌ 无法打开 FIFO: {:?}, 重试...", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        loop {
            // 从 FIFO 读取
            if let Err(_) = fifo.read_exact(&mut raw_buf).await {
                break; // FIFO 关闭，重新打开
            }

            let active_slaves = slaves.lock().await.clone();

            // 检查是否需要切换播放器模式
            let target_channels = if active_slaves.is_empty() { 2 } else { 1 };
            if player.is_none() || current_player_channels != target_channels {
                println!(
                    "🔄 切换播放模式: {}",
                    if target_channels == 2 {
                        "本地立体声"
                    } else {
                        "主从同步 (单声道)"
                    }
                );
                let playback_config = AudioConfig {
                    channels: target_channels,
                    playback_device: "plug:original_default".into(),
                    ..config.clone()
                };
                player = Some(AudioPlayer::new(&playback_config)?);
                current_player_channels = target_channels;
            }

            let now = now_us();
            if stream_start_ts == 0 {
                stream_start_ts = now;
                stream_start_seq = seq;
            }

            if active_slaves.is_empty() {
                // 情况 1: 没有从节点，本地立体声播放
                let mut pcm = Vec::with_capacity(config.frame_size * 2);
                for i in 0..config.frame_size {
                    let l = i16::from_le_bytes([raw_buf[i * 4], raw_buf[i * 4 + 1]]);
                    let r = i16::from_le_bytes([raw_buf[i * 4 + 2], raw_buf[i * 4 + 3]]);
                    pcm.push(l);
                    pcm.push(r);
                }
                if let Some(p) = &player {
                    p.write(&pcm)?;
                }
            } else {
                // 情况 2: 有从节点，主从同步
                let mut local_pcm = Vec::with_capacity(config.frame_size);
                let mut remote_pcm = Vec::with_capacity(config.frame_size);

                // 提取左右声道 (假设当前逻辑只处理一个从节点的情况，或所有从节点角色一致)
                // 如果有多个从节点角色不同，这里需要更复杂的逻辑
                let slave_role = active_slaves[0].role;

                for i in 0..config.frame_size {
                    let l = i16::from_le_bytes([raw_buf[i * 4], raw_buf[i * 4 + 1]]);
                    let r = i16::from_le_bytes([raw_buf[i * 4 + 2], raw_buf[i * 4 + 3]]);
                    if master_role == ChannelRole::Left {
                        local_pcm.push(l);
                    } else {
                        local_pcm.push(r);
                    }
                    if slave_role == ChannelRole::Left {
                        remote_pcm.push(l);
                    } else {
                        remote_pcm.push(r);
                    }
                }

                // 编码并发送给所有从节点
                let len = mono_codec.encode(&remote_pcm, &mut opus_out)?;
                let target_ts = stream_start_ts
                    + ((seq - stream_start_seq) as u128 * frame_duration_us)
                    + delay_us;

                let packet = AudioPacket {
                    seq,
                    timestamp: target_ts,
                    data: opus_out[..len].to_vec(),
                };

                let bytes = postcard::to_allocvec(&packet)?;
                for slave in &active_slaves {
                    let _ = audio_socket.send_to(&bytes, slave.udp_addr).await;
                }

                // 本地回放同步
                let now = now_us();
                if now < target_ts {
                    let wait = target_ts - now;
                    if wait > 1000 {
                        tokio::time::sleep(Duration::from_micros(wait as u64)).await;
                    }
                }
                if let Some(p) = &player {
                    p.write(&local_pcm)?;
                }
            }

            seq += 1;
        }
        // 重置流计时
        stream_start_ts = 0;
    }
}

/// 处理主节点与从节点的会话
async fn handle_master_session(
    mut control: ControlConnection,
    audio_socket: Arc<tokio::net::UdpSocket>,
    slaves: Arc<Mutex<Vec<SlaveSession>>>,
    client_tcp_addr: String,
) -> Result<()> {
    let mut buf = [0u8; 1024];

    // 握手
    let pkt = control.recv_packet(&mut buf).await?;
    let slave_role = match pkt {
        ControlPacket::ClientIdentify { role } => role,
        _ => return Err(anyhow!("无效的握手协议")),
    };

    let hello = ControlPacket::ServerHello {
        udp_port: audio_socket.local_addr()?.port(),
    };
    control.send_packet(&hello).await?;

    // 等待 UDP 打洞/确认
    let mut buf = [0u8; 128];
    let (_, client_udp_addr) = audio_socket.recv_from(&mut buf).await?;

    println!(
        "✅ 从节点已连接: {} {}",
        client_tcp_addr,
        slave_role.to_string(),
    );

    // 添加到从节点列表
    let session = SlaveSession {
        udp_addr: client_udp_addr,
        role: slave_role,
    };
    {
        let mut s = slaves.lock().await;
        s.push(session.clone());
    }

    // 分离 TCP 读写，处理控制消息和心跳
    let (mut tcp_rx, mut tcp_tx) = control.split();

    let mut buf = [0u8; 1024];
    loop {
        match tcp_rx.read(&mut buf).await {
            Ok(0) | Err(_) => {
                break;
            }
            Ok(n) => {
                if let Ok(ControlPacket::Ping { client_ts, seq }) = postcard::from_bytes(&buf[..n])
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
                        break;
                    }
                }
            }
        }
    }

    println!(
        "❌ 从节点已断开: {} {}",
        client_tcp_addr,
        slave_role.to_string(),
    );

    // 从列表中移除
    {
        let mut s = slaves.lock().await;
        s.retain(|x| x.udp_addr != client_udp_addr);
    }

    Ok(())
}
