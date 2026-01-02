use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(feature = "app")]
    {
        xiao::app::server::entry::run_server().await?;
    }
    Ok(())
}
