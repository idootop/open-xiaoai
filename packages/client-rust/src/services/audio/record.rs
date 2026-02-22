use std::future::Future;
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::base::AppError;

use super::config::{AudioConfig, AUDIO_CONFIG};

#[derive(PartialEq)]
enum State {
    Idle,
    Recording,
}

const A113_CAPTURE_BITS_PER_SAMPLE: u16 = 32;

pub struct AudioRecorder {
    state: Arc<Mutex<State>>,
    arecord_thread: Arc<Mutex<Option<Child>>>,
    read_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

static INSTANCE: LazyLock<AudioRecorder> = LazyLock::new(AudioRecorder::new);

impl AudioRecorder {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Idle)),
            arecord_thread: Arc::new(Mutex::new(None)),
            read_thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn instance() -> &'static Self {
        &INSTANCE
    }

    pub async fn stop_recording(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        if *state == State::Idle {
            return Ok(());
        }

        if let Some(read_thread) = self.read_thread.lock().await.take() {
            read_thread.abort();
        }

        if let Some(mut arecord_thread) = self.arecord_thread.lock().await.take() {
            let _ = arecord_thread.kill().await;
        }

        *state = State::Idle;
        Ok(())
    }

    pub async fn start_recording<F, Fut>(
        &self,
        on_stream: F,
        config: Option<AudioConfig>,
    ) -> Result<(), AppError>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    {
        let mut state = self.state.lock().await;
        if *state == State::Recording {
            return Ok(());
        }

        let requested_config = config.unwrap_or_else(|| (*AUDIO_CONFIG).clone());
        let capture_config = capture_config_for_recording(&requested_config);
        let mut arecord_thread = spawn_arecord(&capture_config)?;

        let mut stdout = arecord_thread.stdout.take().unwrap();
        let read_thread = tokio::spawn(async move {
            let bytes_per_sample = (capture_config.bits_per_sample.max(8) / 8) as usize;
            let bytes_per_frame = bytes_per_sample * capture_config.channels.max(1) as usize;
            let target_frames = capture_config.buffer_size.max(1) as usize;
            let read_frames = capture_config.period_size.max(1) as usize;
            let target_size = target_frames * bytes_per_frame;
            let read_size = read_frames * bytes_per_frame;

            let mut accumulated_data = Vec::new();
            let mut buffer = vec![0u8; read_size];

            loop {
                match stdout.read(&mut buffer).await {
                    Ok(size) if size > 0 => {
                        accumulated_data.extend_from_slice(&buffer[..size]);
                        while accumulated_data.len() >= target_size {
                            let data_to_send =
                                accumulated_data.drain(..target_size).collect::<Vec<u8>>();
                            let data_to_send =
                                transform_stream_chunk(data_to_send, &requested_config, &capture_config);
                            if !data_to_send.is_empty() {
                                let _ = on_stream(data_to_send).await;
                            }
                        }
                    }
                    _ => break,
                }
            }

            let _ = AudioRecorder::instance().stop_recording().await;
        });

        self.arecord_thread.lock().await.replace(arecord_thread);
        self.read_thread.lock().await.replace(read_thread);

        *state = State::Recording;
        Ok(())
    }
}

fn capture_config_for_recording(requested: &AudioConfig) -> AudioConfig {
    let mut capture = requested.clone();
    if requested.bits_per_sample == 16 {
        capture.bits_per_sample = A113_CAPTURE_BITS_PER_SAMPLE;
    }
    capture
}

fn transform_stream_chunk(
    chunk: Vec<u8>,
    requested: &AudioConfig,
    capture: &AudioConfig,
) -> Vec<u8> {
    if requested.bits_per_sample != 16 || capture.bits_per_sample != A113_CAPTURE_BITS_PER_SAMPLE {
        return chunk;
    }
    convert_a113_s32_to_s16(&chunk)
}

fn convert_a113_s32_to_s16(chunk: &[u8]) -> Vec<u8> {
    if chunk.len() % 4 != 0 {
        return Vec::new();
    }

    let frame_count = chunk.len() / 4;
    let mut out = vec![0u8; frame_count * 2];

    for frame in 0..frame_count {
        let base = frame * 4;
        let sample = i32::from_le_bytes([
            chunk[base],
            chunk[base + 1],
            chunk[base + 2],
            chunk[base + 3],
        ]);
        // A113 PDM data lives in lower 24 bits of S32_LE: shift by 8 (not 16).
        let mapped = (sample >> 8).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let out_base = frame * 2;
        out[out_base..out_base + 2].copy_from_slice(&mapped.to_le_bytes());
    }

    out
}

fn spawn_arecord(config: &AudioConfig) -> Result<Child, AppError> {
    let child = Command::new("arecord")
        .args([
            "--quiet",
            "-t",
            "raw",
            "-D",
            &config.pcm,
            "-f",
            &format!("S{}_LE", config.bits_per_sample),
            "-r",
            &config.sample_rate.to_string(),
            "-c",
            &config.channels.to_string(),
            "--buffer-size",
            &config.buffer_size.to_string(),
            "--period-size",
            &config.period_size.to_string(),
        ])
        .stdout(Stdio::piped())
        .spawn()?;
    Ok(child)
}
