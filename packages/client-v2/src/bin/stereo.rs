use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(feature = "app")]
    {
        xiao::app::stereo::entry::run_stereo().await?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("Only support Linux");
    }
    Ok(())
}
