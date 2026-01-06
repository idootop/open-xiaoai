//! # Client Demo
//!
//! 演示客户端的主要功能：
//! - 自动服务发现
//! - 响应 RPC 调用
//! - 音频录制和播放
//! - 事件处理

use std::sync::Arc;
use xiao::app::client::{Client, ClientConfig};
use xiao::net::command::Command;
use xiao::net::event::EventData;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("╔═══════════════════════════════════════════════════════╗");
    println!(
        "║          XiaoAi Audio Client v{}                  ║",
        env!("CARGO_PKG_VERSION")
    );
    println!("╚═══════════════════════════════════════════════════════╝");
    println!();

    // 创建客户端
    let config = ClientConfig::default();
    let client = Arc::new(Client::new(config));

    // 启动事件监听器
    let event_client = client.clone();
    tokio::spawn(async move {
        let mut rx = event_client.subscribe_events();
        while let Some((event, ts, addr)) = rx.recv().await {
            match event {
                EventData::Hello { message, .. } => {
                    println!("[Event] Hello: {} ts:{} addr:{}", message, ts, addr);
                }
                _ => {}
            }
        }
    });

    // 启动客户端主循环
    let run_client = client.clone();
    tokio::spawn(async move {
        if let Err(e) = run_client.run().await {
            eprintln!("Client error: {}", e);
        }
    });

    println!("Client is running, searching for server...\n");

    // 等待连接
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if client.is_connected().await {
            break;
        }
    }

    println!("\n═══════════════════════════════════════════════════════");
    println!("Connected to server!");
    println!("Running client-side tests...\n");

    // 1. 测试向服务器发送 Ping
    println!("1️⃣  Testing Ping to server...");
    match client.execute(Command::ping()).await {
        Ok(result) => println!("   ✅ Pong received: {:?}", result),
        Err(e) => println!("   ❌ Ping failed: {}", e),
    }

    // 2. 测试获取服务器信息
    println!("\n2️⃣  Testing GetInfo from server...");
    match client.execute(Command::GetInfo).await {
        Ok(result) => println!("   ✅ Server info: {:?}", result),
        Err(e) => println!("   ❌ GetInfo failed: {}", e),
    }

    // 3. 发送客户端事件
    println!("\n3️⃣  Sending event to server...");
    match client
        .send_event(EventData::Hello {
            message: "from client!".to_string(),
        })
        .await
    {
        Ok(_) => println!("   ✅ Event sent"),
        Err(e) => println!("   ❌ Failed to send event: {}", e),
    }

    println!("\n═══════════════════════════════════════════════════════");
    println!("✅ Client tests completed!");
    println!("\nClient is now ready to receive commands from server.");
    println!("Press Ctrl+C to exit.\n");

    // 保持运行，等待服务器命令
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down client...");
    client.shutdown();
    Ok(())
}
