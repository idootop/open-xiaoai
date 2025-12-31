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
