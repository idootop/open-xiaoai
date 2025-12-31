mod app;
mod audio;
mod net;
mod utils;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        crate::app::entry::run_stereo().await.unwrap();
    }
    println!("Only support Linux");
    Ok(())
}
