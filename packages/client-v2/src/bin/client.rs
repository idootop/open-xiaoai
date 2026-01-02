use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        xiao::app::entry::run_xiao().await.unwrap();
    }
    println!("Only support Linux");
    Ok(())
}
