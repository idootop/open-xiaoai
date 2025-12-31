#![cfg(target_os = "linux")]

use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::player::AudioPlayer;
use crate::net::discovery::Discovery;
use crate::net::network::SlaveNetwork;
use crate::net::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::utils::jitter_buffer::JitterBuffer;
use crate::utils::sync::{ClockSync, now_us};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, mpsc};

/// 运行从节点模式
pub async fn run_slave(role: ChannelRole) -> Result<()> {
    println!("--- 从节点模式 ({}) ---", role.to_string());

    loop {
        match handle_connection(role.clone()).await {
            Err(e) => {
                eprintln!("❌ {:?}", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            Ok(_) => {}
        }
    }
}

async fn handle_connection(role: ChannelRole) -> Result<()> {
    // 1. 发现主节点
    println!("🔍 正在扫描主节点...");
    let (master_ip, master_tcp_port) = Discovery::discover_master().await?;
    let master_tcp_addr = format!("{}:{}", master_ip, master_tcp_port);

    // 2. 建立 TCP 连接
    println!("🔥 发现主节点: {}", master_tcp_addr);
    let network = SlaveNetwork::connect(master_tcp_addr.parse()?).await?;
    let (mut control, audio) = network.split();

    // 3. 身份认证
    control
        .send_packet(&ControlPacket::ClientIdentify { role: role.clone() })
        .await?;

    let mut buf = [0u8; 1024];
    let pkt = control.recv_packet(&mut buf).await?;
    let server_udp_port = match pkt {
        ControlPacket::ServerHello { udp_port } => udp_port,
        _ => return Err(anyhow!("身份认证应答异常")),
    };

    // 4. UDP 打洞
    audio
        .punch(format!("{}:{}", master_ip, server_udp_port).parse()?)
        .await?;

    // 5. 初始化音频与同步组件
    let config = AudioConfig {
        channels: 1,
        ..AudioConfig::music()
    };
    let player = AudioPlayer::new(&config)?;
    let mut codec = OpusCodec::new(&config)?;
    let mut jitter = JitterBuffer::new(50_000, 3);
    let clock = Arc::new(Mutex::new(ClockSync::new(100)));

    // 用于通知主循环 TCP 已断开的消息通道
    let (disconnect_tx, mut disconnect_rx) = mpsc::channel::<()>(1);

    // 6. 分离 TCP 读写
    let (mut tcp_rx, mut tcp_tx) = control.split();
    let clock_updater = clock.clone();
    let d_tx_ping = disconnect_tx.clone();
    let d_tx_pong = disconnect_tx.clone();

    // 定时发送 Ping (心跳 & 时间同步)
    let _sync_handle = tokio::spawn(async move {
        let mut seq = 0;
        loop {
            let t1 = now_us();
            let msg = ControlPacket::Ping { client_ts: t1, seq };
            let data = postcard::to_allocvec(&msg).unwrap();
            if tcp_tx.write_all(&data).await.is_err() {
                let _ = d_tx_ping.send(()).await; // 通知主线程 TCP 失败
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            seq += 1;
        }
    });

    // 接收 Pong
    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    if let Ok(ControlPacket::Pong {
                        client_ts,
                        server_ts,
                        ..
                    }) = postcard::from_bytes(&buf[..n])
                    {
                        let t4 = now_us();
                        clock_updater.lock().await.update(client_ts, server_ts, t4);
                    }
                }
                _ => {
                    let _ = d_tx_pong.send(()).await; // TCP 断开
                    break;
                }
            }
        }
    });

    // 7. 接收音频数据包 (UDP)
    let (audio_tx, mut audio_rx) = mpsc::channel(100);
    let audio_socket = audio.clone_inner();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            if let Ok((len, _)) = audio_socket.recv_from(&mut buf).await {
                if let Ok(packet) = postcard::from_bytes::<AudioPacket>(&buf[..len]) {
                    if audio_tx.send(packet).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // 8. 播放主循环
    println!("✅ 主节点已连接，音频串流中...");
    let mut pcm_buf = vec![0i16; config.frame_size];
    let mut last_seq: Option<u32> = None;
    let mut last_packet_time = now_us();

    loop {
        // 检查 TCP 是否已断开
        if let Ok(_) = disconnect_rx.try_recv() {
            return Err(anyhow!("主节点已断开: {}", master_tcp_addr));
        }

        // 填充 Jitter Buffer
        while let Ok(pkt) = audio_rx.try_recv() {
            let now = now_us();
            // 如果超过 500ms 没有收到包，认为是新流开始，重置状态
            if now - last_packet_time > 500_000 {
                jitter.clear();
                last_seq = None;
                codec = OpusCodec::new(&config)?;
                let _ = player.prepare();
            }
            last_packet_time = now;
            jitter.push(pkt);
        }

        let now = now_us();
        let current_server_time = clock.lock().await.to_server_time(now);

        if let Some((seq, data)) = jitter.pop_frame(current_server_time) {
            if let Some(last) = last_seq {
                let loss_count = seq.wrapping_sub(last) as i32 - 1;
                if loss_count > 0 {
                    // 1. 优先尝试 FEC 恢复最近丢失的那一帧
                    // Opus 的 FEC 数据存储在当前包(data)中，用于恢复“前一帧”
                    if let Ok(len) = codec.decode_fec(&data, &mut pcm_buf) {
                        let _ = player.write(&pcm_buf[..len]);
                    }

                    // 2. 如果丢包超过 1 帧，剩下的帧只能靠丢包补偿(PLC)
                    for _ in 0..(loss_count - 1) {
                        if let Ok(len) = codec.decode_loss(&mut pcm_buf) {
                            let _ = player.write(&pcm_buf[..len]);
                        }
                    }
                }
            }
            last_seq = Some(seq);

            // 3. 正常解码当前帧
            let len = codec.decode(&data, &mut pcm_buf)?;
            player.write(&pcm_buf[..len])?;
        } else {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
