use crate::audio::codec::OpusCodec;
use crate::audio::config::AudioConfig;
use crate::audio::wav::{WavReader, WavWriter};
use crate::net::network::AudioSocket;
use crate::net::protocol::AudioPacket;
use anyhow::Result;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub enum RecorderCommand {
    Start {
        config: AudioConfig,
        filename: String,
    },
    Stop,
}

pub struct ServerAudioManager {
    session_cancel: CancellationToken,
    record_cancel: Mutex<Option<CancellationToken>>,
    play_cancel: Mutex<Option<CancellationToken>>,
    audio_tx: mpsc::Sender<AudioPacket>,
    recorder_tx: mpsc::Sender<RecorderCommand>,
    tracker: TaskTracker,
}

impl ServerAudioManager {
    pub fn new(
        session_cancel: CancellationToken,
        tracker: TaskTracker,
    ) -> (
        Self,
        mpsc::Receiver<AudioPacket>,
        mpsc::Receiver<RecorderCommand>,
    ) {
        let (audio_tx, audio_rx) = mpsc::channel(1024);
        let (recorder_tx, recorder_rx) = mpsc::channel(64);

        let manager = Self {
            session_cancel,
            record_cancel: Mutex::new(None),
            play_cancel: Mutex::new(None),
            audio_tx,
            recorder_tx,
            tracker,
        };

        (manager, audio_rx, recorder_rx)
    }

    pub fn audio_tx(&self) -> mpsc::Sender<AudioPacket> {
        self.audio_tx.clone()
    }

    pub async fn start_recording(&self, config: AudioConfig, filename: String) -> Result<()> {
        // 仅停止之前的录音任务
        {
            let mut guard = self.record_cancel.lock();
            if let Some(token) = guard.take() {
                token.cancel();
            }
            *guard = Some(self.session_cancel.child_token());
        }

        self.recorder_tx
            .send(RecorderCommand::Start { config, filename })
            .await?;
        Ok(())
    }

    pub async fn stop_recording(&self) -> Result<()> {
        if let Some(token) = self.record_cancel.lock().take() {
            token.cancel();
        }
        let _ = self.recorder_tx.send(RecorderCommand::Stop).await;
        Ok(())
    }

    pub async fn start_playback(
        &self,
        config: AudioConfig,
        reader: WavReader,
        audio_socket: Arc<AudioSocket>,
        target_addr: SocketAddr,
    ) -> Result<()> {
        let token = {
            let mut guard = self.play_cancel.lock();
            if let Some(token) = guard.take() {
                token.cancel();
            }
            let token = self.session_cancel.child_token();
            *guard = Some(token.clone());
            token
        };

        let session_cancel = self.session_cancel.clone();

        self.tracker.spawn(async move {
            let mut reader = reader;
            let mut codec = match OpusCodec::new(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to create opus codec: {}", e);
                    return;
                }
            };
            let mut pcm = vec![0i16; config.frame_size];
            let mut opus = vec![0u8; 4096];
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(20));

            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = session_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        match reader.read_samples(&mut pcm) {
                            Ok(0) => break,
                            Ok(n) => {
                                if let Ok(len) = codec.encode(&pcm[..n], &mut opus) {
                                    let _ = audio_socket
                                        .send(
                                            &AudioPacket {
                                                data: opus[..len].to_vec(),
                                            },
                                            target_addr,
                                        )
                                        .await;
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to read samples: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub fn stop_playback(&self) {
        if let Some(token) = self.play_cancel.lock().take() {
            token.cancel();
        }
    }

    pub fn spawn_audio_processor(
        &self,
        mut audio_rx: mpsc::Receiver<AudioPacket>,
        mut recorder_rx: mpsc::Receiver<RecorderCommand>,
    ) {
        let session_cancel = self.session_cancel.clone();
        self.tracker.spawn(async move {
            let mut active_recorder: Option<(WavWriter, OpusCodec, usize)> = None;
            loop {
                tokio::select! {
                    _ = session_cancel.cancelled() => break,
                    cmd = recorder_rx.recv() => {
                        match cmd {
                            Some(RecorderCommand::Start { config, filename }) => {
                                if let Some((writer, _, _)) = active_recorder.take() {
                                    let _ = writer.finalize();
                                }
                                match WavWriter::create(&filename, config.sample_rate, config.channels) {
                                    Ok(writer) => {
                                        match OpusCodec::new(&config) {
                                            Ok(codec) => active_recorder = Some((writer, codec, config.frame_size)),
                                            Err(e) => eprintln!("Failed to create opus codec: {}", e),
                                        }
                                    }
                                    Err(e) => eprintln!("Failed to create wav writer: {}", e),
                                }
                            }
                            Some(RecorderCommand::Stop) => {
                                if let Some((writer, _, _)) = active_recorder.take() {
                                    let _ = writer.finalize();
                                }
                            }
                            None => break,
                        }
                    }
                    packet = audio_rx.recv() => {
                        match packet {
                            Some(packet) => {
                                if let Some((writer, codec, frame_size)) = &mut active_recorder {
                                    let mut pcm = vec![0i16; *frame_size];
                                    if let Ok(n) = codec.decode(&packet.data, &mut pcm) {
                                        let _ = writer.write_samples(&pcm[..n]);
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            if let Some((writer, _, _)) = active_recorder.take() {
                let _ = writer.finalize();
            }
        });
    }
}
