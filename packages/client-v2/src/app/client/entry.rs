#![cfg(target_os = "linux")]

use crate::app::client::core::Client;
use anyhow::Result;

pub async fn run_client() -> Result<()> {
    // 模拟从系统获取信息
    let model = "XiaoAi-V2-Simulated";
    let mac = "00:11:22:33:44:55";
    let version = 1;

    let client = Client::new(model, mac, version);
    client.run().await
}
