#![cfg(target_os = "linux")]

use anyhow::Result;

pub async fn run_client() -> Result<()> {
    println!("Hello, Client!");
    Ok(())
}
