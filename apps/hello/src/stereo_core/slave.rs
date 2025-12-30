#![cfg(target_os = "linux")]
use anyhow::Result;
use crate::audio::{AudioPlayer, OpusCodec};
use crate::config::AudioConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{Mutex, mpsc};
use crate::stereo_core::jitter_buffer::JitterBuffer;
use crate::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use crate::stereo_core::sync::{ClockSync, now_us};
use crate::stereo_core::master::{DISCOVERY_PORT};

/// 运行从节点模式
pub async fn run_slave(role: ChannelRole) -> Result<()> {
    println!("--- 从节点模式 ({:?}) ---", role);

    println!("正在扫描主节点...");
    let udp_disc = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
    let mut buf = [0u8; 1024];
    let (len, master_addr) = udp_disc.recv_from(&mut buf).await?;

    let master_tcp_port = match postcard::from_bytes::<ControlPacket>(&buf[..len])? {
        ControlPacket::ServerHello { udp_port: p } => p,
        _ => return Err(anyhow::anyhow!("无效的发现数据包")),
    };

    let master_ip = master_addr.ip();
    let master_tcp_addr = format!("{}:{}", master_ip, master_tcp_port);
    println!("在 {} 发现主节点", master_tcp_addr);

    let mut tcp = TcpStream::connect(master_tcp_addr).await?;

    // 身份认证
    tcp.write_all(&postcard::to_allocvec(&ControlPacket::ClientIdentify {
        role: role.clone(),
    })?)
    .await?;

    let len = tcp.read(&mut buf).await?;
    let server_udp_port = match postcard::from_bytes::<ControlPacket>(&buf[..len])? {
        ControlPacket::ServerHello { udp_port } => udp_port,
        _ => return Err(anyhow::anyhow!("应答应为 ServerHello")),
    };

    // UDP 打洞以接收音频流
    let udp = UdpSocket::bind("0.0.0.0:0").await?;
    let punch_packet = vec![0u8; 1];
    udp.send_to(&punch_packet, format!("{}:{}", master_ip, server_udp_port))
        .await?;

    // 音频配置
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
    let clock = ClockSync::new(100);

    // 分离读写
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    let udp = Arc::new(udp);
    let udp_rx = udp.clone();

    // 定时发送 Ping 进行时间同步
    let _sync_handle = tokio::spawn(async move {
        let mut seq = 0;
        loop {
            let t1 = now_us();
            let msg = ControlPacket::Ping { client_ts: t1, seq };
            if tcp_tx
                .write_all(&postcard::to_allocvec(&msg).unwrap())
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            seq += 1;
        }
    });

    let clock = Arc::new(Mutex::new(clock));
    let clock_updater = clock.clone();

    // 接收 Pong 并更新时钟偏移
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
                _ => break,
            }
        }
    });

    // 接收音频数据包
    let (audio_tx, mut audio_rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            if let Ok((len, _)) = udp_rx.recv_from(&mut buf).await {
                if let Ok(packet) = postcard::from_bytes::<AudioPacket>(&buf[..len]) {
                    let _ = audio_tx.send(packet).await;
                }
            }
        }
    });

    let mut pcm_buf = vec![0i16; config.frame_size];
    println!("正在监听音频数据...");

    let mut last_seq: Option<u32> = None;

    loop {
        // 将收到的数据包放入抖动缓冲区
        while let Ok(pkt) = audio_rx.try_recv() {
            jitter.push(pkt);
        }

        let now = now_us();
        let current_server_time = {
            let c = clock.lock().await;
            c.to_server_time(now)
        };

        // 从抖动缓冲区提取待播放帧
        if let Some((seq, data)) = jitter.pop_frame(current_server_time) {
            // 丢包处理 (PLC)
            if let Some(last) = last_seq {
                if seq > last + 1 {
                    println!("检测到丢包: {} -> {}", last, seq);
                    // 补偿丢失的帧
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
            // 稍作等待以减少 CPU 占用
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

