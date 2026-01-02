#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AudioScene {
    Music,
    Voice,
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    // ALSA 设备参数，用于录音和播放
    pub capture_device: String,
    pub playback_device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_size: usize, // 帧大小，单位为采样点

    // Opus 编解码参数，用于音频传输
    pub audio_scene: AudioScene,
    pub bitrate: i32,
    pub vbr: bool, // 是否启用 VBR（动态比特率）
    pub fec: bool, // 是否启用 FEC（内联前向纠错）
}

impl AudioConfig {
    pub fn music() -> Self {
        Self {
            audio_scene: AudioScene::Music,
            sample_rate: 48_000, // 48kHz
            channels: 2,
            frame_size: 960,  // 20ms at 48kHz
            bitrate: 320_000, // 320 kbps
            ..Default::default()
        }
    }

    pub fn voice() -> Self {
        Self {
            audio_scene: AudioScene::Voice,
            sample_rate: 16_000, // 16kHz
            channels: 1,
            frame_size: 320, // 20ms at 16kHz
            bitrate: 32_000, // 32 kbps
            ..Default::default()
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            audio_scene: AudioScene::Voice,
            capture_device: "plug:Capture".to_string(),
            playback_device: "default".to_string(),
            sample_rate: 16_000,
            channels: 1,
            frame_size: 320, // 20ms at 16kHz
            bitrate: 32_000,
            vbr: false,
            fec: false,
        }
    }
}
