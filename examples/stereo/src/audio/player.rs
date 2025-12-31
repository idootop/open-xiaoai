#![cfg(target_os = "linux")]

use crate::audio::config::AudioConfig;
use alsa::Direction;
use alsa::pcm::{Access, Format, HwParams, PCM};
use anyhow::{Context, Result};

pub struct AudioPlayer {
    pcm: PCM,
}

impl AudioPlayer {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let pcm = PCM::new(&config.playback_device, Direction::Playback, false)
            .context("Failed to open playback PCM device")?;

        setup_pcm(&pcm, config.sample_rate, config.channels)?;
        Ok(Self { pcm })
    }

    pub fn write(&self, buffer: &[i16]) -> Result<usize> {
        let res = self.pcm.io_i16()?.writei(buffer);

        match res {
            Ok(written) => Ok(written),
            Err(e) => {
                // Buffer Underrun，即播放缓冲区的数据被耗尽，导致音频流中断
                if e.errno() == 32 {
                    // 恢复音频流状态
                    self.pcm.prepare()?;
                    // 重新获取 IO 对象并尝试写入数据
                    self.pcm
                        .io_i16()?
                        .writei(buffer)
                        .context("Failed to write to playback device after recovery")
                } else {
                    Err(e).context("Failed to write to playback device")
                }
            }
        }
    }

    pub fn prepare(&self) -> Result<()> {
        self.pcm.prepare().context("Failed to prepare PCM")
    }
}

fn setup_pcm(pcm: &PCM, sample_rate: u32, channels: u16) -> Result<()> {
    let hwp = HwParams::any(pcm).context("Failed to get HwParams")?;
    hwp.set_access(Access::RWInterleaved)?;
    hwp.set_format(Format::s16())?;
    hwp.set_rate(sample_rate, alsa::ValueOr::Nearest)?;
    hwp.set_channels(channels as u32)?;

    // // 设置较大的缓冲区以减少由于调度抖动导致的断音
    // // 48000Hz * 0.2s = 9600 samples
    // let buffer_size = (sample_rate as f64 * 0.2) as u32;
    // let period_size = buffer_size / 4;
    // hwp.set_buffer_size_near(buffer_size as alsa::pcm::Frames)?;
    // hwp.set_period_size_near(period_size as alsa::pcm::Frames, alsa::ValueOr::Nearest)?;

    pcm.hw_params(&hwp).context("Failed to set HwParams")?;

    let swp = pcm.sw_params_current()?;
    // 设置 start_threshold 为 buffer_size 的一半，确保缓冲区有足够数据再开始播放
    // swp.set_start_threshold(buffer_size as alsa::pcm::Frames / 2)?;
    pcm.sw_params(&swp)?;
    pcm.prepare()?;
    Ok(())
}
