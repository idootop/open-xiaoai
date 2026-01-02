use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioScene {
    Music,
    Voice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub capture_device: String,
    pub playback_device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_size: usize,
    pub audio_scene: AudioScene,
    pub bitrate: i32,
    pub vbr: bool,
    pub fec: bool,
}

impl AudioConfig {
    pub fn voice_16k() -> Self {
        Self {
            capture_device: "plug:Capture".to_string(),
            playback_device: "plug:default".to_string(),
            sample_rate: 16_000,
            channels: 1,
            frame_size: 320, // 20ms
            audio_scene: AudioScene::Voice,
            bitrate: 32_000,
            vbr: true,
            fec: true,
        }
    }

    pub fn music_48k() -> Self {
        Self {
            capture_device: "plug:Capture".to_string(),
            playback_device: "plug:default".to_string(),
            sample_rate: 48_000,
            channels: 2,
            frame_size: 960, // 20ms
            audio_scene: AudioScene::Music,
            bitrate: 128_000,
            vbr: true,
            fec: true,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self::voice_16k()
    }
}
