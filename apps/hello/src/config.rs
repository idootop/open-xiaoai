#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub capture_device: String,
    pub playback_device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_size: usize,
    pub bitrate: i32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            // 根据 apps/runtime/squashfs-root/etc/asound.conf 分析：
            // 录音设备对应 pcm.Capture -> hw:0,3 (S32_LE, 48000Hz)
            // 播放设备对应 pcm.dmixer -> hw:0,2 (S16_LE, 48000Hz)
            // 使用 "default" 或 "plug" 设备可以由 ALSA 自动处理采样率和格式转换
            capture_device: "plug:Capture".to_string(),
            playback_device: "default".to_string(),
            sample_rate: 16000, // Opus 常用 16k，ALSA plug 会自动从 48k 转换
            channels: 1,
            frame_size: 320, // 20ms at 16kHz
            bitrate: 16000,
        }
    }
}
