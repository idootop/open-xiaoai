#![cfg(not(target_os = "linux"))]
use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_buf: Option<SampleBuffer<i16>>,
    channels: usize,
    left_buffer: Vec<i16>,
    right_buffer: Vec<i16>,
}

impl AudioReader {
    pub fn new(path: &str) -> Result<Self> {
        let src =
            File::open(Path::new(path)).context(format!("Failed to open audio file: {}", path))?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        let mut hint = Hint::new();
        if path.ends_with(".wav") {
            hint.with_extension("wav");
        } else if path.ends_with(".mp3") {
            hint.with_extension("mp3");
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .context("Failed to probe audio format")?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .context("No supported audio track found")?;

        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .context("Failed to create decoder")?;

        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_buf: None,
            channels,
            left_buffer: Vec::new(),
            right_buffer: Vec::new(),
        })
    }

    pub fn read_chunk(&mut self, chunk_size: usize) -> Result<Option<(Vec<i16>, Vec<i16>)>> {
        while self.left_buffer.len() < chunk_size {
            if let Some((l, r)) = self.read_frame_internal()? {
                self.left_buffer.extend(l);
                self.right_buffer.extend(r);
            } else {
                break;
            }
        }

        if self.left_buffer.is_empty() {
            return Ok(None);
        }

        let actual_size = std::cmp::min(chunk_size, self.left_buffer.len());
        let left = self.left_buffer.drain(0..actual_size).collect();
        let right = self.right_buffer.drain(0..actual_size).collect();

        Ok(Some((left, right)))
    }

    fn read_frame_internal(&mut self) -> Result<Option<(Vec<i16>, Vec<i16>)>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = self
                .decoder
                .decode(&packet)
                .context("Failed to decode packet")?;

            if self.sample_buf.is_none() {
                self.sample_buf = Some(SampleBuffer::<i16>::new(
                    decoded.capacity() as u64,
                    *decoded.spec(),
                ));
            }

            if let Some(buf) = self.sample_buf.as_mut() {
                buf.copy_interleaved_ref(decoded);
                let samples = buf.samples();

                let mut left = Vec::with_capacity(samples.len() / self.channels);
                let mut right = Vec::with_capacity(samples.len() / self.channels);

                if self.channels == 2 {
                    for i in (0..samples.len()).step_by(2) {
                        left.push(samples[i]);
                        right.push(samples[i + 1]);
                    }
                } else {
                    for &s in samples {
                        left.push(s);
                        right.push(s);
                    }
                }
                return Ok(Some((left, right)));
            }
        }
    }
}
