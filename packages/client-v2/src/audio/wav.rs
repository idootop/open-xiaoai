use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

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
    reader: BufReader<File>,
    pub sample_rate: u32,
    pub channels: u16,
    pub data_size: u32,
}

impl WavReader {
    pub fn open(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut header = [0u8; 44];
        reader.read_exact(&mut header)?;

        if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
            return Err(anyhow::anyhow!("Not a WAV file"));
        }

        let channels = u16::from_le_bytes([header[22], header[23]]);
        let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        let data_size = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);

        Ok(Self {
            reader,
            sample_rate,
            channels,
            data_size,
        })
    }

    pub fn read_samples(&mut self, samples: &mut [i16]) -> Result<usize> {
        let mut bytes = vec![0u8; samples.len() * 2];
        let n = self.reader.read(&mut bytes)?;
        let sample_count = n / 2;
        for i in 0..sample_count {
            samples[i] = i16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        }
        Ok(sample_count)
    }
}

