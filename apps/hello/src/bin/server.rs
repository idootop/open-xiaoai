use anyhow::{Context, Result};
use hello::audio::OpusCodec;
use hello::config::AudioConfig;
use hello::net::{AudioPacket, ChannelRole, ControlPacket, Discovery, SERVER_PORT};
use hello::sync::now_us;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 1,
        frame_size: 960, // 20ms at 48kHz
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

    stream_mp3("test.mp3", &config, clients).await?;

    Ok(())
}

async fn stream_mp3(
    path: &str,
    config: &AudioConfig,
    clients: Arc<Mutex<HashMap<ChannelRole, TcpStream>>>,
) -> Result<()> {
    let src = File::open(Path::new(path)).context("Failed to open test.mp3")?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let probed = symphonia::default::get_probe().format(
        &Hint::new(),
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_MP3)
        .context("No track")?;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let track_id = track.id;

    let mut left_codec = OpusCodec::new(config)?;
    let mut right_codec = OpusCodec::new(config)?;

    let start_time = now_us() + 500_000;
    let mut current_frame_ts = start_time;
    let frame_duration_us =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

    let mut sample_buf = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;
        if sample_buf.is_none() {
            sample_buf = Some(SampleBuffer::<i16>::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }

        if let Some(buf) = sample_buf.as_mut() {
            buf.copy_interleaved_ref(decoded.clone());
            let spec = decoded.spec();
            let num_channels = spec.channels.count();

            if num_channels == 2 {
                for chunk in buf.samples().chunks(config.frame_size * 2) {
                    if chunk.len() < config.frame_size * 2 {
                        break;
                    }

                    let mut left_pcm = vec![0i16; config.frame_size];
                    let mut right_pcm = vec![0i16; config.frame_size];

                    for i in 0..config.frame_size {
                        left_pcm[i] = chunk[i * 2];
                        right_pcm[i] = chunk[i * 2 + 1];
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
            } else {
                for chunk in buf.samples().chunks(config.frame_size) {
                    if chunk.len() < config.frame_size {
                        break;
                    }

                    send_packets(
                        &clients,
                        &mut left_codec,
                        &mut right_codec,
                        chunk,
                        chunk,
                        current_frame_ts,
                    )
                    .await?;
                    current_frame_ts += frame_duration_us;
                    pace(current_frame_ts).await;
                }
            }
        }
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
