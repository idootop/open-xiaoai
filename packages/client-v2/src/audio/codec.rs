use crate::audio::config::{AudioConfig, AudioScene};
use anyhow::{Context, Result};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

pub struct OpusCodec {
    encoder: Encoder,
    decoder: Decoder,
}

impl OpusCodec {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let opus_rate = match config.sample_rate {
            16000 => 16000,
            48000 => 48000,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported sample rate for Opus: {}",
                    config.sample_rate
                ));
            }
        };

        let channels = match config.channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(anyhow::anyhow!("Unsupported channels: {}", config.channels)),
        };

        let mode = match config.audio_scene {
            AudioScene::Music => Application::Audio,
            AudioScene::Voice => Application::Voip,
        };

        let mut encoder =
            Encoder::new(opus_rate, channels, mode).context("Opus encoder init failed")?;

        let bitrate = if config.bitrate <= 0 {
            Bitrate::Auto
        } else {
            Bitrate::Bits(config.bitrate)
        };

        encoder.set_bitrate(bitrate)?;
        encoder.set_vbr(config.vbr)?;
        if config.fec {
            encoder.set_inband_fec(true)?;
            encoder.set_packet_loss_perc(10)?;
        }

        let decoder =
            Decoder::new(config.sample_rate, channels).context("Opus decoder init failed")?;

        Ok(Self { encoder, decoder })
    }

    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize> {
        self.encoder.encode(pcm, out).context("Opus encode failed")
    }

    pub fn decode(&mut self, opus: &[u8], out: &mut [i16]) -> Result<usize> {
        self.decoder
            .decode(opus, out, false)
            .context("Opus decode failed")
    }
}
