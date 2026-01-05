use crate::audio::config::AudioConfig;
use anyhow::{Context, Result, anyhow};

pub struct AudioRecorder {
    #[cfg(target_os = "linux")]
    pcm: alsa::pcm::PCM,
}

impl AudioRecorder {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use alsa::Direction;
            use alsa::pcm::{Access, Format, HwParams, PCM};

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
        #[cfg(not(target_os = "linux"))]
        {
            Err(anyhow!("Linux Only"))
        }
    }

    pub fn read(&self, buf: &mut [i16]) -> Result<usize> {
        #[cfg(target_os = "linux")]
        {
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
        #[cfg(not(target_os = "linux"))]
        {
            Err(anyhow!("Linux Only"))
        }
    }
}
