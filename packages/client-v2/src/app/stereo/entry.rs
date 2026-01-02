#![cfg(target_os = "linux")]

use crate::app::master::run_master;
use crate::app::slave::run_slave;
use crate::net::protocol::ChannelRole;
use anyhow::Result;
use std::env;

pub async fn run_stereo() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: {} [master|slave] [left|right]", args[0]);
        return Ok(());
    }

    let mode = if args[1].to_lowercase() == "master" {
        "主节点"
    } else {
        "从节点"
    };

    let role = if args[2].to_lowercase() == "left" {
        ChannelRole::Left
    } else {
        ChannelRole::Right
    };

    println!("🚗 当前为: {} {}", mode, role.to_string());

    if mode == "主节点" {
        run_master(role).await
    } else {
        run_slave(role).await
    }
}
