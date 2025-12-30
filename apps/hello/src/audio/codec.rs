use crate::config::AudioConfig;
use anyhow::{Context, Result};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

pub struct OpusCodec {
    encoder: Encoder,
    decoder: Decoder,
}

impl OpusCodec {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let channels = if config.channels == 1 {
            Channels::Mono
        } else {
            Channels::Stereo
        };
        let mut encoder = Encoder::new(config.sample_rate, channels, Application::Audio)
            .context("Failed to create Opus encoder")?;
        encoder.set_bitrate(Bitrate::Bits(config.bitrate))?;

        let decoder =
            Decoder::new(config.sample_rate, channels).context("Failed to create Opus decoder")?;

        Ok(Self { encoder, decoder })
    }

    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize> {
        self.encoder
            .encode(pcm, out)
            .context("Opus encoding failed")
    }

    pub fn decode(&mut self, opus: &[u8], out: &mut [i16]) -> Result<usize> {
        self.decoder
            .decode(opus, out, false)
            .context("Opus decoding failed")
    }

    // Packet Loss Concealment
    pub fn decode_loss(&mut self, out: &mut [i16]) -> Result<usize> {
        self.decoder
            .decode(&[], out, false)
            .context("Opus PLC (decode_loss) failed")
    }
}
