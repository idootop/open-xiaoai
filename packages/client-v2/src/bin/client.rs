//! # Client Demo
//!
//! 演示客户端的主要功能：
//! - 自动服务发现
//! - 响应 RPC 调用
//! - 音频录制和播放
//! - 事件处理

#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use xiao::app::client::{Client, ClientConfig};
#[cfg(target_os = "linux")]
use xiao::net::command::Command;
#[cfg(target_os = "linux")]
use xiao::net::event::NotificationLevel;

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This client only works on Linux due to ALSA dependencies.");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║          XiaoAi Audio Client v{}              ║", env!("CARGO_PKG_VERSION"));
    println!("╚═══════════════════════════════════════════════════════╝");
    println!();

    // 创建客户端（可以自定义配置）
    let config = ClientConfig {
        model: "Open-XiaoAi-Demo".to_string(),
        serial_number: get_mac_address(),
        heartbeat_interval: 10,
        timeout: 60,
    };

    let client = Arc::new(Client::new(config));

    // 启动事件监听器
    let event_client = client.clone();
    tokio::spawn(async move {
        let mut rx = event_client.subscribe_events();
        while let Ok(event) = rx.recv().await {
            println!("📨 [ServerEvent] {:?}", event);
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
    println!("\n3️⃣  Sending alert event to server...");
    match client
        .send_alert(NotificationLevel::Info, "Client started successfully!")
        .await
    {
        Ok(_) => println!("   ✅ Alert sent"),
        Err(e) => println!("   ❌ Failed to send alert: {}", e),
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

#[cfg(target_os = "linux")]
fn get_mac_address() -> String {
    // 尝试获取 MAC 地址
    if let Ok(output) = std::process::Command::new("cat")
        .arg("/sys/class/net/eth0/address")
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    "00:00:00:00:00:00".to_string()
}
