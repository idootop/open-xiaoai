#![cfg(target_os = "linux")]

use crate::audio::{AudioPlayer, OpusCodec};
use crate::config::AudioConfig;
use crate::stereo_core::alsa::AlsaRedirector;
use crate::stereo_core::discovery::Discovery;
use crate::stereo_core::mixer::Mixer;
use crate::stereo_core::network::{ControlConnection, MasterNetwork};
use crate::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::stereo_core::sync::now_us;
use anyhow::{Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{SignalKind, signal};
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

    // 0.1 启动混音器服务
    let mixer = Arc::new(Mixer::new());
    mixer.start().await?;

    // 1. 设置网络 (UDP + TCP)
    let network = MasterNetwork::setup(SERVER_TCP_PORT).await?;
    let audio_socket = network.audio_socket().clone_inner();

    // 2. 启动服务发现广播
    Discovery::start_broadcast(SERVER_TCP_PORT).await?;

    println!("✅ 服务已启动，等待连接...");

    let shutdown_flag = Arc::new(AtomicBool::new(false));
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
    let config = AudioConfig::music();
    let encode_config = AudioConfig {
        channels: 1,
        vbr: true,
        ..AudioConfig::music()
    };
    let mut current_player_channels = 0;
    let mut player: Option<AudioPlayer> = None;

    let mut left_pcm = vec![0i16; config.frame_size];
    let mut right_pcm = vec![0i16; config.frame_size];
    let mut opus_out = vec![0u8; 1500];
    let mut seq = 0u32;

    let delay_us = 200_000;
    let frame_duration_us =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

    let mut stream_start_ts = 0;
    let mut stream_start_seq = 0;

    let shutdown_flag_clone = shutdown_flag.clone();
    let audio_loop = async move {
        // 每个新流开始时，重置编码器状态以避免残留音频导致爆音
        let mut left_encoder = OpusCodec::new(&encode_config)?;
        let mut right_encoder = OpusCodec::new(&encode_config)?;

        loop {
            if shutdown_flag_clone.load(Ordering::Relaxed) {
                break;
            }

            // 如果当前没有客户端，重置流计时，以便新客户端连接时重新同步
            if mixer.client_count().await == 0 {
                stream_start_ts = 0;
            }

            // 从混音器读取一帧 (48kHz, 2ch, 20ms)
            let mixed_pcm = mixer
                .read_mixed_frame(config.frame_size, config.channels as usize)
                .await;

            let active_slaves = {
                let s = slaves.lock().await;
                if s.is_empty() { None } else { Some(s.clone()) }
            };

            // 检查是否需要切换播放器模式
            let target_channels = if active_slaves.is_none() { 2 } else { 1 };
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

            // 计算该帧应当播放的基准时间（相对于流开始）
            let target_ts =
                stream_start_ts + ((seq - stream_start_seq) as u128 * frame_duration_us) + delay_us;

            if let Some(slaves_list) = active_slaves {
                // 情况 1: 有从节点，主从同步
                for i in 0..config.frame_size {
                    left_pcm[i] = mixed_pcm[i * 2];
                    right_pcm[i] = mixed_pcm[i * 2 + 1];
                }

                // 1. 检查各声道是否有从节点需要
                let needs_left = slaves_list.iter().any(|s| s.role == ChannelRole::Left);
                let needs_right = slaves_list.iter().any(|s| s.role == ChannelRole::Right);

                // 2. 编码需要的声道
                let mut left_bytes = None;
                let mut right_bytes = None;

                if needs_left {
                    let len = left_encoder.encode(&left_pcm, &mut opus_out)?;
                    let packet = AudioPacket {
                        seq,
                        timestamp: target_ts,
                        data: opus_out[..len].to_vec(),
                    };
                    left_bytes = Some(postcard::to_allocvec(&packet)?);
                }

                if needs_right {
                    let len = right_encoder.encode(&right_pcm, &mut opus_out)?;
                    let packet = AudioPacket {
                        seq,
                        timestamp: target_ts,
                        data: opus_out[..len].to_vec(),
                    };
                    right_bytes = Some(postcard::to_allocvec(&packet)?);
                }

                // 3. 发送给对应的从节点
                for slave in &slaves_list {
                    let bytes = match slave.role {
                        ChannelRole::Left => left_bytes.as_ref(),
                        ChannelRole::Right => right_bytes.as_ref(),
                    };
                    if let Some(b) = bytes {
                        let _ = audio_socket.send_to(b, slave.udp_addr).await;
                    }
                }

                // 4. 本地回放
                let master_pcm = match master_role {
                    ChannelRole::Left => &left_pcm,
                    ChannelRole::Right => &right_pcm,
                };

                let now = now_us();
                if now < target_ts {
                    let wait = target_ts - now;
                    if wait > 1000 {
                        tokio::time::sleep(Duration::from_micros(wait as u64)).await;
                    }
                }
                if let Some(p) = &player {
                    if let Err(_) = p.write(master_pcm) {
                        // 如果写入失败且正在退出，直接跳出循环
                        if shutdown_flag_clone.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            } else {
                // 情况 2: 没有从节点，本地立体声播放
                if let Some(p) = &player {
                    if let Err(_) = p.write(&mixed_pcm) {
                        // 如果写入失败且正在退出，直接跳出循环
                        if shutdown_flag_clone.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            }

            seq += 1;
        }

        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        res = audio_loop => {
            if let Err(e) = res {
                eprintln!("❌ 音频循环错误: {:?}", e);
            }
        },
        _ = shutdown_signal() => {
            // 设置退出标志，通知音频循环停止
            shutdown_flag.store(true, Ordering::Relaxed);
            // 等待一小段时间让音频循环有机会退出
            tokio::time::sleep(Duration::from_millis(100)).await;
        },
    }

    // 显式清理
    AlsaRedirector::cleanup();

    // 强制退出
    std::process::exit(0);
}

/// 监听系统退出信号 (SIGINT, SIGTERM, SIGQUIT)
async fn shutdown_signal() {
    let mut sigint = signal(SignalKind::interrupt()).expect("无法注册 SIGINT 处理器");
    let mut sigterm = signal(SignalKind::terminate()).expect("无法注册 SIGTERM 处理器");
    let mut sigquit = signal(SignalKind::quit()).expect("无法注册 SIGQUIT 处理器");

    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
        _ = sigquit.recv() => {},
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
