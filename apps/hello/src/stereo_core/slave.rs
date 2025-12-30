#![cfg(target_os = "linux")]

use crate::audio::{AudioPlayer, OpusCodec};
use crate::config::AudioConfig;
use crate::stereo_core::discovery::Discovery;
use crate::stereo_core::jitter_buffer::JitterBuffer;
use crate::stereo_core::network::SlaveNetwork;
use crate::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::stereo_core::sync::{ClockSync, now_us};
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
            Ok(_) => println!("连接正常结束，正在尝试重新连接..."),
            Err(e) => {
                eprintln!("连接异常断开 3 秒后重试... \nError: {:?}.", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

/// 处理单次完整的连接生命周期
async fn handle_connection(role: ChannelRole) -> Result<()> {
    println!("正在扫描主节点...");
    // 1. 发现主节点
    let (master_ip, master_tcp_port) = Discovery::discover_master().await?;
    let master_tcp_addr = format!("{}:{}", master_ip, master_tcp_port);
    println!("在 {} 发现主节点，正在连接...", master_tcp_addr);

    // 2. 建立 TCP 连接
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
        _ => return Err(anyhow!("应答应为 ServerHello")),
    };

    // 4. UDP 打洞
    audio
        .punch(format!("{}:{}", master_ip, server_udp_port).parse()?)
        .await?;

    // 5. 初始化音频与同步组件
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 1,
        frame_size: 960,
        bitrate: 64000,
        ..AudioConfig::default()
    };
    let player = AudioPlayer::new(&config)?;
    let mut codec = OpusCodec::new(&config)?;
    let mut jitter = JitterBuffer::new(50_000);
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
    println!("连接成功，正在播放音频...");
    let mut pcm_buf = vec![0i16; config.frame_size];
    let mut last_seq: Option<u32> = None;

    loop {
        // 检查 TCP 是否已断开
        if let Ok(_) = disconnect_rx.try_recv() {
            return Err(anyhow!("主节点连接已断开 (TCP Disconnected)"));
        }

        // 填充 Jitter Buffer
        while let Ok(pkt) = audio_rx.try_recv() {
            jitter.push(pkt);
        }

        let now = now_us();
        let current_server_time = clock.lock().await.to_server_time(now);

        if let Some((seq, data)) = jitter.pop_frame(current_server_time) {
            // 丢失处理 (PLC)
            if let Some(last) = last_seq {
                if seq > last + 1 {
                    for _ in 0..(seq - last - 1) {
                        if let Ok(len) = codec.decode_loss(&mut pcm_buf) {
                            let _ = player.write(&pcm_buf[..len]);
                        }
                    }
                }
            }
            last_seq = Some(seq);

            let len = codec.decode(&data, &mut pcm_buf)?;
            player.write(&pcm_buf[..len])?;
        } else {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
