use hello::audio::{OpusCodec};
use hello::config::AudioConfig;
use anyhow::Result;

// todo 向 client 分发左右声道音频数据
fn main() -> Result<()> {
    let config = AudioConfig::default();
    let mut codec = OpusCodec::new(&config)?;

    
}
