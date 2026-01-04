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
        {
            let hwp = HwParams::any(&pcm).context("Failed to get HwParams")?;
            hwp.set_access(Access::RWInterleaved)?;
            hwp.set_format(Format::s16())?;
            hwp.set_rate_near(config.sample_rate, alsa::ValueOr::Nearest)?;
            hwp.set_channels(config.channels as u32)?;
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare().context("Failed to prepare PCM")?;
        Ok(Self { pcm })
    }

    pub fn write(&self, buf: &[i16]) -> Result<usize> {
        let res = self.pcm.io_i16()?.writei(buf);
        match res {
            Ok(n) => Ok(n),
            Err(e) if e.errno() == 32 => {
                println!("ALSA write underrun, preparing PCM");
                // Broken pipe (underrun)
                self.pcm.prepare()?;
                self.pcm
                    .io_i16()?
                    .writei(buf)
                    .context("ALSA write retry failed")
            }
            Err(e) => Err(e.into()),
        }
    }
}
