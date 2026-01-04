//! # Server Demo
//!
//! 演示服务端的主要功能：
//! - 多客户端管理
//! - RPC 调用
//! - 音频录制和播放
//! - 事件广播

use std::sync::Arc;
use xiao::app::server::{Server, ServerConfig};
use xiao::audio::config::AudioConfig;
use xiao::net::command::Command;
use xiao::net::event::{NotificationLevel, ServerEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("╔═══════════════════════════════════════════════════════╗");
    println!(
        "║          XiaoAi Audio Server v{}              ║",
        env!("CARGO_PKG_VERSION")
    );
    println!("╚═══════════════════════════════════════════════════════╝");
    println!();

    // 创建服务端
    let config = ServerConfig::default();
    let server = Arc::new(Server::new(config).await?);
    let s = server.clone();

    // 启动服务器
    tokio::spawn(async move {
        if let Err(e) = s.run(8080).await {
            eprintln!("Server error: {}", e);
        }
    });

    // 启动事件监听器
    let event_server = server.clone();
    tokio::spawn(async move {
        let mut rx = event_server.event_bus().subscribe();
        while let Some((addr, event)) = rx.recv().await {
            match event {
                ServerEvent::ClientJoined { model, .. } => {
                    println!("📱 [Event] Client joined: {} ({:?})", model, addr);
                }
                ServerEvent::ClientLeft { model, .. } => {
                    println!("📴 [Event] Client left: {} ({:?})", model, addr);
                }
                _ => {}
            }
        }
    });

    println!("Server is running on port 8080");
    println!("Waiting for clients to connect...\n");

    // 主循环：等待客户端并执行测试
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let clients = server.get_clients().await;
        if clients.is_empty() {
            continue;
        }

        let addr = clients[0];
        println!("\n═══════════════════════════════════════════════════════");
        println!("Client connected: {}", addr);
        println!("Running demo tests...\n");

        // 1. 测试 Ping
        println!("1️⃣  Testing Ping...");
        match server.execute(addr, Command::ping()).await {
            Ok(result) => println!("   ✅ Ping result: {:?}", result),
            Err(e) => println!("   ❌ Ping failed: {}", e),
        }

        // 2. 测试获取设备信息
        println!("\n2️⃣  Testing GetInfo...");
        match server.execute(addr, Command::GetInfo).await {
            Ok(result) => println!("   ✅ Device info: {:?}", result),
            Err(e) => println!("   ❌ GetInfo failed: {}", e),
        }

        // 3. 测试 Shell 命令
        println!("\n3️⃣  Testing Shell RPC...");
        match server.shell(addr, "echo 'Hello from XiaoAi!'").await {
            Ok(resp) => {
                println!("   ✅ stdout: {}", resp.stdout.trim());
                println!("   ✅ exit_code: {}", resp.exit_code);
            }
            Err(e) => println!("   ❌ Shell failed: {}", e),
        }

        // 4. 测试事件广播
        println!("\n4️⃣  Broadcasting notification event...");
        server
            .broadcast_event(ServerEvent::Notification {
                level: NotificationLevel::Info,
                title: "Test".to_string(),
                message: "This is a test notification from server".to_string(),
            })
            .await;
        println!("   ✅ Event broadcasted");

        // 6. 测试音频播放（如果有测试文件）
        println!("\n6️⃣  Testing Audio Playback...");
        if std::path::Path::new("temp/test.wav").exists() {
            match server.start_play(addr, "temp/test.wav").await {
                Ok(_) => {
                    println!("   ▶️  Playback started...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    server.stop_play(addr).await?;
                    println!("   ⏹️  Playback stopped");
                }
                Err(e) => println!("   ❌ Playback failed: {}", e),
            }
        } else {
            println!("   ⚠️  No test file found at temp/test.wav, skipping...");
        }

        // 5. 测试音频录制
        println!("\n5️⃣  Testing Audio Recording (5 seconds)...");
        match server.start_record(addr, AudioConfig::voice_16k()).await {
            Ok(_) => {
                println!("   ⏺️  Recording started...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                server.stop_record(addr).await?;
                println!("   ⏹️  Recording stopped");
            }
            Err(e) => println!("   ❌ Recording failed: {}", e),
        }

        println!("\n═══════════════════════════════════════════════════════");
        println!("✅ All tests completed!");
        println!("\nServer status:");
        println!("  • Connected clients: {}", server.client_count());
        println!("  • Uptime: {} seconds", server.uptime_secs());
        println!("\nPress Ctrl+C to exit.");

        break;
    }

    // 等待退出信号
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down server...");
    server.shutdown();
    Ok(())
}
