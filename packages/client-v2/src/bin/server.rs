use std::sync::Arc;
use xiao::app::server::Server;
use xiao::audio::config::AudioConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Arc::new(Server::new().await?);
    let s = server.clone();

    tokio::spawn(async move {
        s.run(8080).await.unwrap();
    });

    println!("Server is running. Waiting for a client to connect...");

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let clients = server.get_clients().await;
        if !clients.is_empty() {
            let addr = clients[0];
            println!("Client connected: {}. Starting tests...", addr);

            println!("1. Testing Shell RPC...");
            let res = server
                .call(addr, "shell", vec!["echo 'Hello from Mars!'".to_string()])
                .await?;
            println!(
                "RPC Result: stdout={}, code={}",
                res.stdout.trim(),
                res.code
            );

            println!("2. Testing Audio Recording (10s)...");
            server.start_record(addr, AudioConfig::voice_16k()).await?;
            tokio::time::sleep(std::time::Duration::from_secs(12)).await;

            println!("3. Testing Audio Playback (from temp/test.wav)...");
            server.start_play(addr, "temp/test.wav").await?;

            break;
        }
    }

    println!("Tests completed. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
