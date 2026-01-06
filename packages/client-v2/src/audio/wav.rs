use crate::audio::config::AudioConfig;
use anyhow::Result;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

pub struct WavWriter {
    writer: BufWriter<File>,
    data_size: u32,
    sample_rate: u32,
    channels: u16,
}

impl WavWriter {
    pub fn create(path: &str, sample_rate: u32, channels: u16) -> Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Header placeholder
        writer.write_all(&[0u8; 44])?;

        Ok(Self {
            writer,
            data_size: 0,
            sample_rate,
            channels,
        })
    }

    pub fn write_samples(&mut self, samples: &[i16]) -> Result<()> {
        for &sample in samples {
            self.writer.write_all(&sample.to_le_bytes())?;
            self.data_size += 2;
        }
        Ok(())
    }

    pub fn finalize(mut self) -> Result<()> {
        self.writer.flush()?;
        let mut file = self.writer.into_inner()?;

        file.seek(SeekFrom::Start(0))?;

        let file_size = 36 + self.data_size;
        let byte_rate = self.sample_rate * self.channels as u32 * 2;
        let block_align = self.channels * 2;

        let mut header = [0u8; 44];
        header[0..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&file_size.to_le_bytes());
        header[8..12].copy_from_slice(b"WAVE");
        header[12..16].copy_from_slice(b"fmt ");
        header[16..20].copy_from_slice(&16u32.to_le_bytes());
        header[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
        header[22..24].copy_from_slice(&self.channels.to_le_bytes());
        header[24..28].copy_from_slice(&self.sample_rate.to_le_bytes());
        header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
        header[32..34].copy_from_slice(&block_align.to_le_bytes());
        header[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&self.data_size.to_le_bytes());

        file.write_all(&header)?;
        Ok(())
    }
}

pub struct WavReader {
    #[cfg(not(target_os = "linux"))]
    reader: crate::audio::reader::AudioReader,
    pcm_buffer: Vec<i16>,
    pub config: AudioConfig,
}

impl WavReader {
    pub fn open(path: &str) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let reader = crate::audio::reader::AudioReader::new(path)?;
            let channels = reader.channels as u16;

            let config = AudioConfig {
                channels,
                sample_rate: reader.sample_rate,
                frame_size: reader.sample_rate as usize / 50, // 20ms
                ..AudioConfig::music_48k()
            };

            let input_len = config.frame_size * (channels as usize);

            Ok(Self {
                reader,
                config,
                pcm_buffer: vec![0i16; input_len],
            })
        }
        #[cfg(target_os = "linux")]
        {
            Err(anyhow::anyhow!(
                "WavReader is only supported on non-Linux platforms"
            ))
        }
    }

    pub fn read_one_frame(&mut self) -> Result<Option<&[i16]>> {
        #[cfg(not(target_os = "linux"))]
        {
            match self.reader.read_chunk(self.config.frame_size)? {
                Some((left_pcm, right_pcm)) => {
                    let actual_len = left_pcm.len();
                    if self.config.channels == 2 {
                        for i in 0..actual_len {
                            self.pcm_buffer[i * 2] = left_pcm[i];
                            self.pcm_buffer[i * 2 + 1] = right_pcm[i];
                        }
                    } else {
                        self.pcm_buffer[..actual_len].copy_from_slice(&left_pcm);
                    }
                    let total_len = actual_len * (self.config.channels as usize);
                    Ok(Some(&self.pcm_buffer[..total_len]))
                }
                None => Ok(None),
            }
        }
        #[cfg(target_os = "linux")]
        {
            Err(anyhow::anyhow!(
                "WavReader is only supported on non-Linux platforms"
            ))
        }
    }
}
