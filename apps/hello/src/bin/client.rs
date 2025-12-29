#![cfg(target_os = "linux")]

use anyhow::Result;
use hello::audio::{AudioPlayer, OpusCodec};
use hello::config::AudioConfig;
use hello::net::{AudioPacket, ChannelRole, ControlPacket, Discovery};
use hello::sync::{ClockSync, now_us};
use std::collections::VecDeque;
use std::env;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} [left|right]", args[0]);
        return Ok(());
    }
    let role = if args[1] == "left" {
        ChannelRole::Left
    } else {
        ChannelRole::Right
    };

    let config = AudioConfig {
        sample_rate: 48000,
        channels: 1,
        frame_size: 960,
        bitrate: 32000,
        ..AudioConfig::default()
    };

    println!("Searching for server...");
    let server_addr = Discovery::client_discover_server().await?;
    let mut stream = TcpStream::connect(server_addr).await?;

    // 1. Identify
    stream
        .write_all(&postcard::to_allocvec(&ControlPacket::ClientIdentify {
            role: role.clone(),
        })?)
        .await?;

    // 2. Sync
    let mut clock = ClockSync::new();
    let t1 = now_us();
    stream
        .write_all(&postcard::to_allocvec(&ControlPacket::Ping {
            client_ts: t1,
        })?)
        .await?;
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    if let Ok(ControlPacket::Pong {
        client_ts,
        server_ts,
        ..
    }) = postcard::from_bytes::<ControlPacket>(&buf[..n])
    {
        let t4 = now_us();
        clock.update(client_ts, server_ts, t4);
        println!(
            "Clock synced. Offset: {}us, RTT: {}us",
            clock.offset,
            t4 - t1
        );
    }

    let player = AudioPlayer::new(&config)?;
    let mut codec = OpusCodec::new(&config)?;
    let mut jitter_buffer: VecDeque<AudioPacket> = VecDeque::new();
    let mut pcm_buf = vec![0i16; config.frame_size];

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
        loop {
            let mut s_buf = [0u8; 4];
            if stream.read_exact(&mut s_buf).await.is_err() {
                break;
            }
            let size = u32::from_be_bytes(s_buf) as usize;
            let mut data = vec![0u8; size];
            if stream.read_exact(&mut data).await.is_err() {
                break;
            }
            if let Ok(p) = postcard::from_bytes::<AudioPacket>(&data) {
                let _ = tx.send(p).await;
            }
        }
    });

    println!("Streaming started. Playing channel {:?}", role);

    loop {
        while let Ok(p) = rx.try_recv() {
            jitter_buffer.push_back(p);
        }

        if let Some(p) = jitter_buffer.front() {
            let target_client_time = clock.to_client_time(p.timestamp);
            let now = now_us();

            if now >= target_client_time {
                let packet = jitter_buffer.pop_front().unwrap();
                let len = codec.decode(&packet.data, &mut pcm_buf)?;
                player.write(&pcm_buf[..len])?;
            } else if target_client_time - now > 500_000 {
                // Too far in the future, maybe clock jumped?
                jitter_buffer.pop_front();
            } else {
                // Wait until it's time
                let wait = (target_client_time - now) as u64;
                if wait > 1000 {
                    tokio::time::sleep(Duration::from_micros(wait)).await;
                }
            }
        } else {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
