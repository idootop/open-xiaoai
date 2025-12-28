use alsa::pcm::{PCM, HwParams, Format, Access};
use alsa::Direction;
use anyhow::{Result, Context};
use crate::config::AudioConfig;

pub struct AudioRecorder {
    pcm: PCM,
}

impl AudioRecorder {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let pcm = PCM::new(&config.capture_device, Direction::Capture, false)
            .context("Failed to open capture PCM device")?;
        
        setup_pcm(&pcm, config.sample_rate, config.channels)?;
        Ok(Self { pcm })
    }

    pub fn read(&self, buffer: &mut [i16]) -> Result<usize> {
        self.pcm.io_i16()?.readi(buffer).context("Failed to read from capture device")
    }
}

fn setup_pcm(pcm: &PCM, sample_rate: u32, channels: u16) -> Result<()> {
    let hwp = HwParams::any(pcm).context("Failed to get HwParams")?;
    hwp.set_access(Access::RWInterleaved)?;
    hwp.set_format(Format::s16())?;
    hwp.set_rate(sample_rate, alsa::ValueOr::Nearest)?;
    hwp.set_channels(channels as u32)?;
    pcm.hw_params(&hwp).context("Failed to set HwParams")?;
    
    let swp = pcm.sw_params_current()?;
    pcm.sw_params(&swp)?;
    pcm.prepare()?;
    Ok(())
}

