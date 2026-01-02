use xiao::app::client::Client;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Arc::new(Client::new());
    let c = client.clone();
    tokio::spawn(async move {
        if let Err(e) = c.run().await {
            eprintln!("Client error: {}", e);
        }
    });

    // Wait for connection
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    println!("Testing RPC call to server...");
    match client.call("hello", vec!["world".to_string()]).await {
        Ok(res) => println!("Server RPC response: {}", res.stdout),
        Err(e) => eprintln!("Server RPC call failed: {}", e),
    }

    tokio::signal::ctrl_c().await?;
    Ok(())
}
