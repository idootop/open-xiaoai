use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(feature = "app")]
    {
        xiao::app::client::entry::run_client().await?;
    }
    Ok(())
}
