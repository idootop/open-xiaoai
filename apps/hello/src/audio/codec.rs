use crate::config::{AudioConfig, AudioScene};
use anyhow::{Context, Result};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

pub struct OpusCodec {
    encoder: Encoder,
    decoder: Decoder,
}

impl OpusCodec {
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let channels = match config.channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(anyhow::anyhow!("Invalid channels: {}", config.channels)),
        };
        let mode = match config.audio_scene {
            AudioScene::Music => Application::Audio,
            AudioScene::Voice => Application::Voip,
        };
        let bitrate = match config.bitrate {
            -1 => Bitrate::Max,
            0 => Bitrate::Auto,
            _ => Bitrate::Bits(config.bitrate),
        };
        
        let mut encoder = Encoder::new(config.sample_rate, channels, mode)
            .context("Failed to create Opus encoder")?;

        encoder.set_bitrate(bitrate)?;
        if config.vbr {
            encoder.set_vbr(true)?;
        }
        if config.fec {
            encoder.set_inband_fec(true)?; // 内联前向纠错
            encoder.set_packet_loss_perc(20)?; // 预期丢包率20%
        }

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

    /// 前向纠错(FEC)
    pub fn decode_fec(&mut self, opus: &[u8], out: &mut [i16]) -> Result<usize> {
        self.decoder
            .decode(opus, out, true)
            .context("Opus FEC decoding failed")
    }

    /// 丢包补偿(PLC)
    pub fn decode_loss(&mut self, out: &mut [i16]) -> Result<usize> {
        self.decoder
            .decode(&[], out, false)
            .context("Opus PLC (decode_loss) failed")
    }
}
