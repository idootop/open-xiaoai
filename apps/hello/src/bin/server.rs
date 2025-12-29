use anyhow::Result;
use hello::audio::{AudioReader, OpusCodec};
use hello::config::AudioConfig;
use hello::net::{AudioPacket, ChannelRole, ControlPacket, Discovery, SERVER_PORT};
use hello::sync::now_us;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 1,
        frame_size: 960, // 20ms at 48kHz
        bitrate: 32000,  // 32kbps
        ..AudioConfig::default()
    };

    println!("Starting Stereo Server on port {}", SERVER_PORT);

    Discovery::start_server_beacon().await?;

    let listener = TcpListener::bind(format!("0.0.0.0:{}", SERVER_PORT)).await?;
    let clients: Arc<Mutex<HashMap<ChannelRole, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));

    let clients_clone = Arc::clone(&clients);
    tokio::spawn(async move {
        while let Ok((mut stream, addr)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf).await {
                if let Ok(packet) = postcard::from_bytes::<ControlPacket>(&buf[..n]) {
                    match packet {
                        ControlPacket::ClientIdentify { role } => {
                            println!("Client {} identified as {:?}", addr, role);
                            // Simple handshake for sync
                            let mut sync_buf = [0u8; 1024];
                            if let Ok(sn) = stream.read(&mut sync_buf).await {
                                if let Ok(ControlPacket::Ping { client_ts }) =
                                    postcard::from_bytes::<ControlPacket>(&sync_buf[..sn])
                                {
                                    let pong = ControlPacket::Pong {
                                        client_ts,
                                        server_ts: now_us(),
                                    };
                                    let pong_msg = postcard::to_allocvec(&pong).unwrap();
                                    let _ = stream.write_all(&pong_msg).await;
                                    clients_clone.lock().await.insert(role, stream);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    println!("Waiting for Left and Right clients...");
    loop {
        if clients.lock().await.len() >= 2 {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    println!("Both clients connected. Starting stream in 1s...");
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    stream_audio("test.mp3", &config, clients).await?;

    Ok(())
}

async fn stream_audio(
    path: &str,
    config: &AudioConfig,
    clients: Arc<Mutex<HashMap<ChannelRole, TcpStream>>>,
) -> Result<()> {
    let mut reader = AudioReader::new(path)?;

    let mut left_codec = OpusCodec::new(config)?;
    let mut right_codec = OpusCodec::new(config)?;

    let start_time = now_us() + 500_000;
    let mut current_frame_ts = start_time;
    let frame_duration_us =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

    while let Some((left_pcm, right_pcm)) = reader.read_chunk(config.frame_size)? {
        if left_pcm.len() < config.frame_size {
            break;
        }

        send_packets(
            &clients,
            &mut left_codec,
            &mut right_codec,
            &left_pcm,
            &right_pcm,
            current_frame_ts,
        )
        .await?;
        current_frame_ts += frame_duration_us;
        pace(current_frame_ts).await;
    }
    Ok(())
}

async fn send_packets(
    clients: &Arc<Mutex<HashMap<ChannelRole, TcpStream>>>,
    left_codec: &mut OpusCodec,
    right_codec: &mut OpusCodec,
    left_pcm: &[i16],
    right_pcm: &[i16],
    ts: u128,
) -> Result<()> {
    let mut left_opus = vec![0u8; 1275];
    let mut right_opus = vec![0u8; 1275];

    let l_len = left_codec.encode(left_pcm, &mut left_opus)?;
    let r_len = right_codec.encode(right_pcm, &mut right_opus)?;

    let l_p = postcard::to_allocvec(&AudioPacket {
        timestamp: ts,
        data: left_opus[..l_len].to_vec(),
    })?;
    let r_p = postcard::to_allocvec(&AudioPacket {
        timestamp: ts,
        data: right_opus[..r_len].to_vec(),
    })?;

    let mut lock = clients.lock().await;
    if let Some(s) = lock.get_mut(&ChannelRole::Left) {
        let _ = send_with_size(s, &l_p).await;
    }
    if let Some(s) = lock.get_mut(&ChannelRole::Right) {
        let _ = send_with_size(s, &r_p).await;
    }
    Ok(())
}

async fn pace(current_frame_ts: u128) {
    let now = now_us();
    if current_frame_ts > now + 1_000_000 {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

async fn send_with_size(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    stream.write_all(&(data.len() as u32).to_be_bytes()).await?;
    stream.write_all(data).await?;
    Ok(())
}
