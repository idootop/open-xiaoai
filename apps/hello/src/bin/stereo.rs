#![cfg(target_os = "linux")]

use anyhow::{Context, Result};
use hello::audio::{AudioPlayer, OpusCodec};
use hello::config::AudioConfig;
use hello::stereo_core::jitter_buffer::JitterBuffer;
use hello::stereo_core::protocol::{AudioPacket, ChannelRole, ControlPacket};
use hello::stereo_core::sync::{ClockSync, now_us};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Mutex, mpsc};

const FIFO_PATH: &str = "/tmp/stereo_out.fifo";
const REAL_ASOUND_CONF: &str = "/etc/asound.conf";
const TEMP_ASOUND_CONF: &str = "/tmp/asound.stereo.conf";

// Default ports
const DISCOVERY_PORT: u16 = 53530;
const SERVER_TCP_PORT: u16 = 53531;
// UDP port will be dynamic or fixed

struct AlsaRedirector;

impl AlsaRedirector {
    fn new() -> Result<Self> {
        Self::cleanup(); // Ensure clean state

        let original_conf = fs::read_to_string(REAL_ASOUND_CONF).unwrap_or_default();

        if !original_conf.contains("pcm.original_default") {
            // 重命名原有的 default 逻辑，插入 interceptor
            let mut new_conf = original_conf.replace("pcm.!default", "pcm.original_default");
            new_conf.push_str(&format!(
                r#"
    pcm.!default {{
        type plug
        slave.pcm "stereo_interceptor"
    }}
    
    pcm.stereo_interceptor {{
        type file
        slave.pcm "null"
        file "{}"
        format "raw"
    }}
    "#,
                FIFO_PATH
            ));

            fs::write(TEMP_ASOUND_CONF, new_conf)?;

            // 挂载覆盖 /etc/asound.conf
            let status = Command::new("mount")
                .arg("--bind")
                .arg(TEMP_ASOUND_CONF)
                .arg(REAL_ASOUND_CONF)
                .status()
                .context("Failed to execute mount command")?;

            if !status.success() {
                return Err(anyhow::anyhow!("Failed to mount asound.conf"));
            }
        }

        // Create FIFO
        let _ = Command::new("mkfifo").arg(FIFO_PATH).status();
        let _ = Command::new("chmod").arg("666").arg(FIFO_PATH).status();

        println!("ALSA output redirected to {}", FIFO_PATH);
        Ok(Self)
    }

    fn cleanup() {
        let _ = Command::new("umount")
            .arg("-l")
            .arg(REAL_ASOUND_CONF)
            .status();
        let _ = fs::remove_file(TEMP_ASOUND_CONF);
        let _ = fs::remove_file(FIFO_PATH);
        println!("ALSA configuration restored.");
    }
}

impl Drop for AlsaRedirector {
    fn drop(&mut self) {
        Self::cleanup();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} [master|slave] [left|right]", args[0]);
        return Ok(());
    }

    let mode = &args[1];
    let role = if args[2].to_lowercase() == "left" {
        ChannelRole::Left
    } else {
        ChannelRole::Right
    };

    // Explicit cleanup at start just in case
    AlsaRedirector::cleanup();

    if mode == "master" {
        run_master(role).await
    } else {
        run_slave(role).await
    }
}

async fn run_master(role: ChannelRole) -> Result<()> {
    println!("--- Master Mode ({:?}) ---", role);

    // 0. Setup ALSA redirection (persistent for the life of the master)
    let _alsa_guard = AlsaRedirector::new()?;

    // 1. Setup Network (UDP + TCP)
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
    let udp_port = udp_socket.local_addr()?.port();
    let udp_socket = Arc::new(udp_socket);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", SERVER_TCP_PORT)).await?;

    // 2. Discovery Service
    let _discovery = tokio::spawn(async move {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        socket.set_broadcast(true).unwrap();
        let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
            .parse()
            .unwrap();
        let msg = postcard::to_allocvec(&ControlPacket::ServerHello {
            udp_port: SERVER_TCP_PORT,
        })
        .unwrap();
        loop {
            let _ = socket.send_to(&msg, target).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    println!("Waiting for slave on port {}...", SERVER_TCP_PORT);

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("Slave connected: {}", addr);

        let udp_socket = udp_socket.clone();
        let role = role.clone();

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = handle_master_session(socket, udp_socket, udp_port, role).await {
                eprintln!("Session ended: {:?}", e);
            }
        });
    }
}

async fn handle_master_session(
    mut tcp: TcpStream,
    udp: Arc<UdpSocket>,
    _local_udp_port: u16,
    role: ChannelRole,
) -> Result<()> {
    let mut buf = [0u8; 1024];

    let len = tcp.read(&mut buf).await?;
    match postcard::from_bytes::<ControlPacket>(&buf[..len])? {
        ControlPacket::ClientIdentify { role: _r } => {}
        _ => return Err(anyhow::anyhow!("Invalid handshake")),
    };

    let hello = ControlPacket::ServerHello {
        udp_port: udp.local_addr()?.port(),
    };
    tcp.write_all(&postcard::to_allocvec(&hello)?).await?;

    let mut buf = [0u8; 128];
    let (_, client_udp_addr) = udp.recv_from(&mut buf).await?;
    println!("Client UDP address confirmed: {}", client_udp_addr);

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

    println!("Starting session loop...");

    // Use into_split for owned ReadHalf/WriteHalf to move into tasks
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

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
        // Open FIFO. This will block until a writer opens it.
        // We use select to allow exiting if the slave disconnects while we wait.
        let mut fifo = tokio::select! {
            _ = stop_rx.recv() => {
                println!("Slave disconnected while waiting for FIFO.");
                return Ok(());
            }
            f = tokio::fs::File::open(FIFO_PATH) => f?,
        };

        println!("Audio stream started...");
        let mut stream_start_ts = 0;
        let stream_start_seq = seq;

        loop {
            // Read from FIFO with timeout/select to check for disconnection
            let read_res = tokio::select! {
                _ = stop_rx.recv() => {
                    println!("Slave disconnected during streaming.");
                    return Ok(());
                }
                res = fifo.read_exact(&mut raw_buf) => res,
            };

            if let Err(_) = read_res {
                println!("Audio stream ended (FIFO EOF).");
                break;
            }

            let now = now_us();
            if stream_start_ts == 0 {
                stream_start_ts = now;
            }

            let mut local_pcm = Vec::with_capacity(config.frame_size);
            let mut remote_pcm = Vec::with_capacity(config.frame_size);

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

            let len = codec.encode(&remote_pcm, &mut opus_out)?;
            let target_ts =
                stream_start_ts + ((seq - stream_start_seq) as u128 * frame_duration_us) + delay_us;

            let packet = AudioPacket {
                seq,
                timestamp: target_ts,
                data: opus_out[..len].to_vec(),
            };

            let bytes = postcard::to_allocvec(&packet)?;
            if let Err(e) = udp.send_to(&bytes, client_udp_addr).await {
                eprintln!("UDP send error: {:?}", e);
                return Err(e.into());
            }

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

async fn run_slave(role: ChannelRole) -> Result<()> {
    println!("--- Slave Mode ({:?}) ---", role);

    println!("Scanning for Master...");
    let udp_disc = UdpSocket::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
    let mut buf = [0u8; 1024];
    let (len, master_addr) = udp_disc.recv_from(&mut buf).await?;

    let master_tcp_port = match postcard::from_bytes::<ControlPacket>(&buf[..len])? {
        ControlPacket::ServerHello { udp_port: p } => p,
        _ => return Err(anyhow::anyhow!("Invalid discovery packet")),
    };

    let master_ip = master_addr.ip();
    let master_tcp_addr = format!("{}:{}", master_ip, master_tcp_port);
    println!("Found Master at {}", master_tcp_addr);

    let mut tcp = TcpStream::connect(master_tcp_addr).await?;

    tcp.write_all(&postcard::to_allocvec(&ControlPacket::ClientIdentify {
        role: role.clone(),
    })?)
    .await?;

    let len = tcp.read(&mut buf).await?;
    let server_udp_port = match postcard::from_bytes::<ControlPacket>(&buf[..len])? {
        ControlPacket::ServerHello { udp_port } => udp_port,
        _ => return Err(anyhow::anyhow!("Expected ServerHello")),
    };

    let udp = UdpSocket::bind("0.0.0.0:0").await?;
    let punch_packet = vec![0u8; 1];
    udp.send_to(&punch_packet, format!("{}:{}", master_ip, server_udp_port))
        .await?;

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

    // Use into_split for owned ReadHalf/WriteHalf to move into tasks
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    let udp = Arc::new(udp);
    let udp_rx = udp.clone();

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
    println!("Listening for audio...");

    let mut last_seq: Option<u32> = None;

    loop {
        while let Ok(pkt) = audio_rx.try_recv() {
            jitter.push(pkt);
        }

        let now = now_us();
        let current_server_time = {
            let c = clock.lock().await;
            c.to_server_time(now)
        };

        if let Some((seq, data)) = jitter.pop_frame(current_server_time) {
            // PLC logic
            let mut _lost = false;
            if let Some(last) = last_seq {
                if seq > last + 1 {
                    println!("Packet loss detected: {} -> {}", last, seq);
                    _lost = true;
                    // Conceal missing frames
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
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}
