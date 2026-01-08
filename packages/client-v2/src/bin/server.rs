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
use xiao::net::event::EventData;
use xiao::net::rpc::RpcBuilder;

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
        let mut rx = event_server.subscribe_events();
        while let Some((event, ts, addr)) = rx.recv().await {
            match event {
                EventData::Hello { message, .. } => {
                    println!("[Event] Hello: {} ts:{} addr:{}", message, ts, addr);
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

        // 1. 测试 Shell 命令（同步）
        println!("1️⃣  Testing Shell RPC (sync)...");
        match server
            .rpc(
                addr,
                &RpcBuilder::default(Command::Shell("echo 'Hello from server'".to_string())),
            )
            .await
        {
            Ok(resp) => println!("   ✅ Command executed: {:?}", resp),
            Err(e) => println!("   ❌ Command failed: {}", e),
        }

        // 2. 测试 Shell 命令（带超时）
        println!("\n2️⃣  Testing Shell RPC with timeout (1s)...");
        match server
            .rpc(
                addr,
                &RpcBuilder::default(Command::Shell("sleep 2".to_string())).set_timeout(1000),
            )
            .await
        {
            Ok(resp) => println!("   ✅ Command executed: {:?}", resp),
            Err(e) => println!("   ❌ Command failed: {}", e),
        }

        // 3. 测试事件
        println!("\n3️⃣  Broadcasting notification event...");
        match server
            .send_event(
                addr,
                EventData::Hello {
                    message: "from server!".to_string(),
                },
            )
            .await
        {
            Ok(_) => println!("   ✅ Event sent"),
            Err(e) => println!("   ❌ Failed to send event: {}", e),
        }

        // 5. 测试音频录制
        println!("\n5️⃣  Testing Audio Recording (10 seconds)...");
        match server.start_record(addr, AudioConfig::voice_16k()).await {
            Ok(_) => {
                println!("   ⏺️  Recording started...");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                server.stop_record(addr).await?;
                println!("   ⏹️  Recording stopped");
            }
            Err(e) => println!("   ❌ Recording failed: {}", e),
        }

        // 6. 测试音频播放（如果有测试文件）
        println!("\n6️⃣  Testing Audio Playback...");
        if std::path::Path::new("temp/test.wav").exists() {
            match server.start_play(addr, "temp/test.wav").await {
                Ok(_) => {
                    println!("   ▶️  Playback started...");
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    server.stop_play(addr).await?;
                    println!("   ⏹️  Playback stopped");
                }
                Err(e) => println!("   ❌ Playback failed: {}", e),
            }
        } else {
            println!("   ⚠️  No test file found at temp/test.wav, skipping...");
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
