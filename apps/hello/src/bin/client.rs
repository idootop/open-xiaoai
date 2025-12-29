use hello::audio::{AudioPlayer, OpusCodec};
use hello::config::AudioConfig;
use anyhow::Result;

// todo 左右声道分发，播放音频
fn main() -> Result<()> {
    let config = AudioConfig::default();
    let player = AudioPlayer::new(&config)?;
    let mut codec = OpusCodec::new(&config)?;

    
}
