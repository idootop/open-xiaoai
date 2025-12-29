#[cfg(not(target_os = "linux"))]
use hello::audio::OpusCodec;
#[cfg(target_os = "linux")]
use hello::audio::{AudioPlayer, AudioRecorder, OpusCodec};

use anyhow::Result;
use hello::config::AudioConfig;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        println!("Test utility is only supported on Linux (ALSA required)");
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let config = AudioConfig::default();
        println!("Starting Audio Modular Demo with 1s delay");
        println!(
            "Config: capture={}, playback={}, rate={}, channels={}, frame_size={}",
            config.capture_device,
            config.playback_device,
            config.sample_rate,
            config.channels,
            config.frame_size
        );

        let recorder = AudioRecorder::new(&config)?;
        let player = AudioPlayer::new(&config)?;
        let mut codec = OpusCodec::new(&config)?;

        // 使用队列存储编码后的 Opus 数据块及录制时间
        let mut delay_queue: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();
        let delay_duration = Duration::from_secs(1);

        let mut pcm_in = vec![0i16; config.frame_size * config.channels as usize];
        let mut opus_buf = vec![0u8; 1024];

        println!("Recording... Delaying playback by 1s. Press Ctrl+C to stop.");

        loop {
            // 1. 录制原始 PCM
            let read = recorder.read(&mut pcm_in)?;
            if read == config.frame_size {
                // 2. 编码并存入队列
                let opus_len = codec.encode(&pcm_in, &mut opus_buf)?;
                let opus_data = opus_buf[..opus_len].to_vec();
                delay_queue.push_back((Instant::now(), opus_data));
            }

            // 3. 处理延迟播放：检查队列头部数据是否已等待超过 1s
            while let Some((timestamp, _)) = delay_queue.front() {
                if timestamp.elapsed() >= delay_duration {
                    let (_, opus_data) = delay_queue.pop_front().unwrap();

                    let mut pcm_out = vec![0i16; config.frame_size * config.channels as usize];
                    let decoded_len = codec.decode(&opus_data, &mut pcm_out)?;

                    if decoded_len == config.frame_size {
                        // 4. 播放还原后的 PCM
                        player.write(&pcm_out)?;
                    }
                } else {
                    // 头部数据还未到 1s，由于队列是按时间排序的，后面的肯定也没到
                    break;
                }
            }
        }
    }
}
