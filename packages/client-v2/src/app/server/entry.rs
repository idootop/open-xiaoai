use crate::app::server::core::Server;
use crate::audio::config::AudioConfig;
use anyhow::Result;
use std::sync::Arc;

pub async fn run_server() -> Result<()> {
    let server = Arc::new(Server::new().await?);
    let s = server.clone();

    // 运行服务器
    tokio::spawn(async move {
        if let Err(e) = s.run(53531).await {
            eprintln!("Server error: {}", e);
        }
    });

    // 等待一个客户端连接并进行演示
    println!("等待客户端连接以进行功能演示...");
    loop {
        let sessions = server.get_sessions();
        if !sessions.is_empty() {
            let addr = sessions[0];
            println!("开始对 {} 进行功能测试...", addr);

            // 1. 测试 RPC
            println!("测试 RPC: echo hello");
            let res = server.call_shell(addr, "echo hello").await?;
            println!("RPC 结果: {:?}", res);

            // 2. 测试录音
            println!("测试录制 2 秒音频...");
            server.start_recording(addr, AudioConfig::voice()).await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            server.stop_recording(addr).await?;
            println!("录制结束，请检查 temp/recorded.wav");

            // 3. 测试播放 (如果 temp/test.wav 存在)
            if std::path::Path::new("temp/test.wav").exists() {
                println!("测试播放 temp/test.wav...");
                server.start_playback(addr, AudioConfig::voice()).await?;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                server.stop_playback(addr).await?;
                println!("播放结束");
            } else {
                println!("跳过播放测试 (temp/test.wav 不存在)");
            }

            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // 保持运行
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
