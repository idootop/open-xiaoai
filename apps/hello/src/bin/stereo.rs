#![cfg(target_os = "linux")]

use anyhow::Result;
use hello::stereo_core::master::run_master;
use hello::stereo_core::protocol::ChannelRole;
use hello::stereo_core::slave::run_slave;
use hello::stereo_core::injector::run_injector;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    // 优先处理 --inject 模式
    if args.iter().any(|arg| arg == "--inject") {
        return run_injector().await;
    }

    if args.len() < 3 {
        eprintln!("用法:");
        eprintln!("  主节点: {} master [left|right]", args[0]);
        eprintln!("  从节点: {} slave [left|right]", args[0]);
        eprintln!("  注入器: {} --inject (由 ALSA 自动拉起)", args[0]);
        return Ok(());
    }

    let mode = &args[1];
    let role = if args[2].to_lowercase() == "left" {
        ChannelRole::Left
    } else {
        ChannelRole::Right
    };

    if mode == "master" {
        run_master(role).await
    } else {
        run_slave(role).await
    }
}
