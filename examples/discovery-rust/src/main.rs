use clap::Parser;
mod discovery_protocol;
use crate::discovery_protocol::DiscoveryService;

const DISCOVERY_PORT: u16 = 5354;
const DEFAULT_WS_PORT: u16 = 8080;
const DEFAULT_SECRET: &str = "your-secret-key";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// UDP监听端口
    #[arg(long, default_value_t = DISCOVERY_PORT)]
    port: u16,

    /// WebSocket服务端口
    #[arg(long, default_value_t = DEFAULT_WS_PORT)]
    ws_port: u16,

    /// 用于HMAC验证的密钥
    #[arg(long, default_value_t = DEFAULT_SECRET.to_string())]
    secret: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    let discovery_service = DiscoveryService::new(args.secret, args.port, args.ws_port).await?;
    discovery_service.start().await;

    println!("✅ 服务发现服务已启动");
    println!("按 Ctrl+C 停止服务");

    tokio::signal::ctrl_c().await?;

    discovery_service.stop();

    Ok(())
}
