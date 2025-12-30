#![cfg(target_os = "linux")]

use anyhow::Result;
use hello::stereo_core::alsa::AlsaRedirector;
use hello::stereo_core::master::run_master;
use hello::stereo_core::protocol::ChannelRole;
use hello::stereo_core::slave::run_slave;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: {} [master|slave] [left|right]", args[0]);
        return Ok(());
    }

    let mode = &args[1];
    let role = if args[2].to_lowercase() == "left" {
        ChannelRole::Left
    } else {
        ChannelRole::Right
    };

    // 启动前先执行清理，确保环境干净
    AlsaRedirector::cleanup();

    if mode == "master" {
        run_master(role).await
    } else {
        run_slave(role).await
    }
}
