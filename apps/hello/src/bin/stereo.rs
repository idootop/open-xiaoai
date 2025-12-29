use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use hello::audio::{AudioPlayer, OpusCodec};
use hello::config::AudioConfig;
use hello::net::{AudioPacket, ChannelRole, ControlPacket, DISCOVERY_PORT, SERVER_PORT};
use hello::sync::{ClockSync, now_us};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::process::Command;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::signal;

#[cfg(target_os = "linux")]
const FIFO_PATH: &str = "/tmp/stereo_out.fifo";
#[cfg(target_os = "linux")]
const TEMP_ASOUND_CONF: &str = "/tmp/asound.stereo.conf";
#[cfg(target_os = "linux")]
const REAL_ASOUND_CONF: &str = "/etc/asound.conf";

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq)]
enum DeviceModel {
    Lx06,
    Oh2p,
    Unknown,
}

#[cfg(target_os = "linux")]
fn detect_model() -> DeviceModel {
    let model = Command::new("sh")
        .args(["-c", "micocfg_model"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
        .to_uppercase();
    if model.contains("LX06") {
        return DeviceModel::Lx06;
    } else if model.contains("OH2P") {
        return DeviceModel::Oh2p;
    }
    DeviceModel::Unknown
}

#[cfg(target_os = "linux")]
fn setup_alsa_config(model: DeviceModel) -> Result<()> {
    cleanup_alsa_config();

    let original_conf = match model {
        DeviceModel::Lx06 => include_str!("../config/asound.lx06.conf"),
        DeviceModel::Oh2p => include_str!("../config/asound.oh2p.conf"),
        DeviceModel::Unknown => return Err(anyhow::anyhow!("Unsupported device model")),
    };

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

    // 创建 FIFO
    let _ = Command::new("mkfifo").arg(FIFO_PATH).status();
    let _ = Command::new("chmod").arg("666").arg(FIFO_PATH).status();

    // 挂载覆盖 /etc/asound.conf
    let status = Command::new("mount")
        .arg("--bind")
        .arg(TEMP_ASOUND_CONF)
        .arg(REAL_ASOUND_CONF)
        .status()
        .context("Failed to execute mount command")?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to mount asound.conf, need root?"));
    }

    println!("Successfully redirected ALSA output to {}", FIFO_PATH);
    Ok(())
}

#[cfg(target_os = "linux")]
fn cleanup_alsa_config() {
    println!("Cleaning up ALSA configurations...");
    let _ = Command::new("umount")
        .arg("-l")
        .arg(REAL_ASOUND_CONF)
        .status();
    let _ = fs::remove_file(TEMP_ASOUND_CONF);
    let _ = fs::remove_file(FIFO_PATH);
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        println!("Stereo 模式仅支持 Linux (需要 ALSA)");
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let args: Vec<String> = env::args().collect();
        if args.len() < 3 {
            eprintln!("用法: {} [master|slave] [left|right]", args[0]);
            eprintln!("示例:");
            eprintln!("  主设备: {} master left", args[0]);
            eprintln!("  从设备: {} slave right", args[0]);
            return Ok(());
        }

        let mode = &args[1];
        let role = if args[2].to_lowercase() == "left" {
            ChannelRole::Left
        } else {
            ChannelRole::Right
        };

        let result = tokio::select! {
            res = async {
                if mode == "master" {
                    run_master(role).await
                } else {
                    run_slave(role).await
                }
            } => res,
            _ = signal::ctrl_c() => {
                println!("\n收到退出信号");
                Ok(())
            }
        };

        cleanup_alsa_config();
        return result;
    }
}

#[cfg(target_os = "linux")]
async fn run_master(role: ChannelRole) -> Result<()> {
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 2, // 拦截的是立体声
        frame_size: 960,
        bitrate: 48000,
        ..AudioConfig::default()
    };

    let model = detect_model();
    if model == DeviceModel::Unknown {
        eprintln!("警告: 无法识别设备型号，尝试使用默认配置");
    }

    println!("--- 主设备模式 ---");
    println!("本地声道: {:?}", role);

    loop {
        println!("正在启动发现服务 (UDP Broadcast)...");
        // 1. 启动发现广播
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;
        let target_addr: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT).parse()?;
        let hello = ControlPacket::ServerHello { port: SERVER_PORT };
        let msg = postcard::to_allocvec(&hello)?;

        let broadcast_handle = tokio::spawn(async move {
            loop {
                let _ = socket.send_to(&msg, target_addr).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        // 2. 等待从设备连接
        let listener = TcpListener::bind(format!("0.0.0.0:{}", SERVER_PORT)).await?;
        println!("等待从设备连接于端口 {}...", SERVER_PORT);

        let (mut slave_stream, addr) = tokio::select! {
            res = listener.accept() => res?,
            _ = signal::ctrl_c() => {
                broadcast_handle.abort();
                return Ok(());
            }
        };

        println!("从设备已连接: {}", addr);
        broadcast_handle.abort();

        // 3. 握手与时间同步
        let mut buf = [0u8; 1024];
        let n = slave_stream.read(&mut buf).await?;
        if let Ok(ControlPacket::ClientIdentify { role: slave_role }) =
            postcard::from_bytes::<ControlPacket>(&buf[..n])
        {
            println!("从设备识别为: {:?}", slave_role);
        }

        let n = slave_stream.read(&mut buf).await?;
        if let Ok(ControlPacket::Ping { client_ts }) =
            postcard::from_bytes::<ControlPacket>(&buf[..n])
        {
            let pong = ControlPacket::Pong {
                client_ts,
                server_ts: now_us(),
            };
            slave_stream
                .write_all(&postcard::to_allocvec(&pong)?)
                .await?;
            println!("时钟同步完成");
        }

        // 4. 初始化音频并开始拦截
        setup_alsa_config(model)?;

        let res = run_master_audio_loop(&mut slave_stream, role.clone(), &config).await;

        cleanup_alsa_config();

        if let Err(e) = res {
            eprintln!("主设备音频循环出错: {:?}, 准备重连...", e);
        } else {
            println!("从设备正常断开，准备下一次连接...");
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(target_os = "linux")]
async fn run_master_audio_loop(
    slave_stream: &mut TcpStream,
    role: ChannelRole,
    config: &AudioConfig,
) -> Result<()> {
    // 打开 FIFO (使用 tokio::fs 以支持异步读取)
    let mut fifo = tokio::fs::File::open(FIFO_PATH)
        .await
        .context("Failed to open FIFO for reading")?;

    // 使用 plug:original_default 以支持系统主音量控制
    let playback_config = AudioConfig {
        channels: 1,
        playback_device: "plug:original_default".to_string(),
        ..config.clone()
    };
    let player = AudioPlayer::new(&playback_config).context("无法打开本地播放设备")?;
    let mono_config = AudioConfig {
        channels: 1,
        ..config.clone()
    };
    let mut codec = OpusCodec::new(&mono_config).context("无法初始化编码器")?;

    let mut stereo_raw_buf = vec![0i16; config.frame_size * 2];
    let mut opus_buf = vec![0u8; 2048];

    let delay_us = 100_000; // 100ms 缓冲
    let mut current_ts = 0;
    let frame_duration =
        (config.frame_size as f64 / config.sample_rate as f64 * 1_000_000.0) as u128;

    println!("开始拦截并分发立体声音频...");

    let mut byte_buf = vec![0u8; config.frame_size * 2 * 2];

    loop {
        // 从 FIFO 读取 (这是同步源)
        if let Err(e) = fifo.read_exact(&mut byte_buf).await {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
            return Err(e.into());
        }

        let now = now_us();
        if current_ts == 0 {
            current_ts = now + delay_us;
        }

        for i in 0..stereo_raw_buf.len() {
            stereo_raw_buf[i] = i16::from_le_bytes([byte_buf[i * 2], byte_buf[i * 2 + 1]]);
        }

        let mut local_pcm = Vec::with_capacity(config.frame_size);
        let mut remote_pcm = Vec::with_capacity(config.frame_size);

        for i in 0..config.frame_size {
            let (l, r) = (stereo_raw_buf[i * 2], stereo_raw_buf[i * 2 + 1]);
            if role == ChannelRole::Left {
                local_pcm.push(l);
                remote_pcm.push(r);
            } else {
                local_pcm.push(r);
                remote_pcm.push(l);
            }
        }

        // 先发送网络包
        let opus_len = codec.encode(&remote_pcm, &mut opus_buf)?;
        let packet = AudioPacket {
            timestamp: current_ts,
            data: opus_buf[..opus_len].to_vec(),
        };
        let packet_data = postcard::to_allocvec(&packet)?;

        if slave_stream
            .write_all(&(packet_data.len() as u32).to_be_bytes())
            .await
            .is_err()
            || slave_stream.write_all(&packet_data).await.is_err()
        {
            return Err(anyhow::anyhow!("从设备断开连接"));
        }

        // 本地播放也需要同步
        let now = now_us();
        if now < current_ts {
            let wait = current_ts - now;
            if wait > 1000 {
                tokio::time::sleep(Duration::from_micros(wait as u64)).await;
            }
        }
        player.write(&local_pcm)?;

        current_ts += frame_duration;

        // 漂移校正
        let now = now_us();
        if now > current_ts + 200_000 {
            current_ts = now + 50_000;
        }
    }
}

#[cfg(target_os = "linux")]
async fn run_slave(role: ChannelRole) -> Result<()> {
    let config = AudioConfig {
        sample_rate: 48000,
        channels: 1,
        frame_size: 960,
        bitrate: 32000,
        playback_device: "plug:original_default".to_string(),
        ..AudioConfig::default()
    };

    let model = detect_model();
    if model == DeviceModel::Unknown {
        eprintln!("警告: 无法识别设备型号，尝试使用默认配置");
    }

    println!("--- 从设备模式 ---");
    println!("本地声道: {:?}", role);

    loop {
        // 1. 发现主设备
        println!("正在搜索主设备...");
        let master_addr = match tokio::select! {
            addr = hello::net::Discovery::client_discover_server() => addr,
            _ = signal::ctrl_c() => return Ok(()),
        } {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("搜索主设备失败: {:?}, 1秒后重试...", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        println!("发现主设备: {}", master_addr);
        let mut stream = match TcpStream::connect(master_addr).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("连接主设备失败: {:?}, 1秒后重试...", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        // 2. 身份识别
        if let Err(e) = stream
            .write_all(&postcard::to_allocvec(&ControlPacket::ClientIdentify {
                role: role.clone(),
            })?)
            .await
        {
            eprintln!("发送身份识别失败: {:?}, 准备重连...", e);
            continue;
        }

        // 3. 时间同步
        let mut clock = ClockSync::new();
        let t1 = now_us();
        if let Err(e) = stream
            .write_all(&postcard::to_allocvec(&ControlPacket::Ping {
                client_ts: t1,
            })?)
            .await
        {
            eprintln!("发送 Ping 失败: {:?}, 准备重连...", e);
            continue;
        }

        let mut buf = [0u8; 1024];
        let n = match stream.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("读取 Pong 失败: {:?}, 准备重连...", e);
                continue;
            }
        };
        if let Ok(ControlPacket::Pong {
            client_ts,
            server_ts,
        }) = postcard::from_bytes::<ControlPacket>(&buf[..n])
        {
            let t4 = now_us();
            clock.update(client_ts, server_ts, t4);
            println!("时钟同步完成. 偏移: {}us, RTT: {}us", clock.offset, t4 - t1);
        } else {
            eprintln!("收到无效的 Pong 响应, 准备重连...");
            continue;
        }

        // 4. 初始化音频并开始播放
        setup_alsa_config(model)?;

        let res = run_slave_audio_loop(stream, &config, clock).await;

        cleanup_alsa_config();

        if let Err(e) = res {
            eprintln!("从设备音频循环出错: {:?}, 准备重连...", e);
        } else {
            println!("主设备已断开，准备重连...");
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(target_os = "linux")]
async fn run_slave_audio_loop(
    stream: TcpStream,
    config: &AudioConfig,
    clock: ClockSync,
) -> Result<()> {
    let player = AudioPlayer::new(config).context("无法打开播放设备")?;
    let mut codec = OpusCodec::new(config).context("无法初始化解码器")?;
    let mut jitter_buffer: VecDeque<AudioPacket> = VecDeque::new();
    let mut pcm_buf = vec![0i16; config.frame_size];

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // 网络接收线程
    let mut stream_read = stream;
    let receive_handle = tokio::spawn(async move {
        loop {
            let mut s_buf = [0u8; 4];
            if stream_read.read_exact(&mut s_buf).await.is_err() {
                break;
            }
            let size = u32::from_be_bytes(s_buf) as usize;
            let mut data = vec![0u8; size];
            if stream_read.read_exact(&mut data).await.is_err() {
                break;
            }
            if let Ok(p) = postcard::from_bytes::<AudioPacket>(&data) {
                if tx.send(p).await.is_err() {
                    break;
                }
            }
        }
    });

    println!("开始接收并播放音频...");

    let res = loop {
        while let Ok(p) = rx.try_recv() {
            jitter_buffer.push_back(p);
        }

        if let Some(p) = jitter_buffer.front() {
            let target_client_time = clock.to_client_time(p.timestamp);
            let now = now_us();

            if now >= target_client_time {
                let packet = jitter_buffer.pop_front().unwrap();

                // 如果包太旧了（延迟超过150ms），跳过以赶上进度，防止累积卡顿
                if now > target_client_time + 150_000 {
                    continue;
                }

                let len = codec.decode(&packet.data, &mut pcm_buf)?;
                player.write(&pcm_buf[..len])?;
            } else {
                let wait = (target_client_time - now) as u64;
                if wait > 1000 {
                    // 如果时间差太大，可能是时钟跳变，清空缓冲重新同步
                    if wait > 1_000_000 {
                        jitter_buffer.clear();
                    } else {
                        tokio::time::sleep(Duration::from_micros(wait)).await;
                    }
                }
            }
        } else {
            if receive_handle.is_finished() {
                break Ok(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };

    receive_handle.abort();
    res
}
