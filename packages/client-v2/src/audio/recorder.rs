#![cfg(target_os = "linux")]

use crate::audio::config::AudioConfig;
use alsa::Direction;
use alsa::pcm::{Access, Format, HwParams, PCM};
use anyhow::{Context, Result};

pub struct AudioRecorder {
    pcm: PCM,
}

impl AudioRecorder {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let pcm = PCM::new(&config.capture_device, Direction::Capture, false)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_access(Access::RWInterleaved)?;
            hwp.set_format(Format::s16())?;
            hwp.set_rate_near(config.sample_rate, alsa::ValueOr::Nearest)?;
            hwp.set_channels_near(config.channels as u32)?;
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare()?;
        Ok(Self { pcm })
    }

    pub fn read(&self, buf: &mut [i16]) -> Result<usize> {
        match self.pcm.io_i16()?.readi(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.errno() == 32 => {
                // 32 = Broken pipe (Overrun)
                println!("ALSA recording overrun, recovering...");
                self.pcm.prepare()?;
                self.pcm
                    .io_i16()?
                    .readi(buf)
                    .context("ALSA read retry failed")
            }
            Err(e) => Err(e.into()),
        }
    }
}
