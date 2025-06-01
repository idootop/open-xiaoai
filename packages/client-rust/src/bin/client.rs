use clap::{Parser, ValueHint};
use open_xiaoai::services::audio::config::AudioConfig;
use open_xiaoai::services::monitor::kws::KwsMonitor;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;

use open_xiaoai::base::AppError;
use open_xiaoai::base::VERSION;
use open_xiaoai::services::audio::play::AudioPlayer;
use open_xiaoai::services::audio::record::AudioRecorder;
use open_xiaoai::services::connect::data::{Event, Request, Response, Stream};
use open_xiaoai::services::connect::handler::MessageHandler;
use open_xiaoai::services::connect::message::{MessageManager, WsStream};
use open_xiaoai::services::connect::rpc::RPC;
use open_xiaoai::services::monitor::instruction::InstructionMonitor;
use open_xiaoai::services::monitor::playing::PlayingMonitor;

use open_xiaoai::services::discovery::{default_discovery, DiscoveryService};

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// WebSocket server URL (e.g. ws://localhost:8080)
    #[arg(long, value_hint = ValueHint::Url)]
    url: Option<String>,

    /// Secret password for service discovery
    #[arg(long)]
    secret: Option<String>,
}

struct AppClient;

impl AppClient {
    pub async fn connect(url: &str) -> Result<WsStream, AppError> {
        let (ws_stream, _) = connect_async(url).await?;
        Ok(WsStream::Client(ws_stream))
    }

    pub async fn run() {
        let args = Args::parse();

        // 优先使用--url和--secret参数
        let (cli_url, secret) = if args.url.is_some() || args.secret.is_some() {
            (args.url, args.secret)
        } else {
            // 兼容老的参数传递方式
            let url = std::env::args().nth(1).expect("❌ 请使用 --url 输入服务器地址，或者使用 --secret 传入服务器密码以自动发现局域网内的服务器");
            (Some(url), None)
        };

        if let Some(url) = cli_url {
            // 命令行地址模式
            loop {
                println!("🔍 使用命令行地址: {}, 正在连接...", url);
                match AppClient::connect(&url).await {
                    Ok(ws_stream) => {
                        println!("✅ 已连接: {}", url);
                        AppClient::handle_connection(ws_stream).await;
                    }
                    Err(e) => {
                        eprintln!("❌ 连接失败: {}, 1秒后重试", e);
                    }
                }
                sleep(Duration::from_secs(1)).await;
            }
        } else {
            // 服务发现模式
            let secret = secret.expect("❌ 请输入服务器密码 --secret your-secret-key，以自动发现局域网内的服务器");
            let discovery = default_discovery(&secret).await;
            println!("🔑 使用密码: {}", secret);
            println!("✅ 已启动 - 正在发现服务端...");
            let mut retry_count = 0;
            loop {
                // 发现服务端
                let addr = match discovery.discover().await {
                    Ok(addr) => {
                        if retry_count > 0 {
                            println!("🔍 发现服务端成功 (已重试 {} 次)", retry_count);
                        }
                        retry_count = 0;
                        addr
                    },
                    Err(e) => {
                        if retry_count == 0 {
                            eprintln!("❌ 发现服务端失败: {}, 正在重试...", e);
                        }
                        retry_count += 1;
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                
                let url = format!("ws://{}", addr);
                println!("🔍 发现服务端: {}, 正在连接...", addr);
                
                match AppClient::connect(&url).await {
                    Ok(ws_stream) => {
                        println!("✅ 已连接: {}", url);
                        AppClient::handle_connection(ws_stream).await;
                    }
                    Err(e) => {
                        eprintln!("❌ 连接失败: {}, 1秒后重试", e);
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                }
            }
        }
    }
    
    async fn handle_connection(ws_stream: WsStream) {
        AppClient::init(ws_stream).await;
        if let Err(e) = MessageManager::instance().process_messages().await {
            eprintln!("❌ 消息处理异常: {}", e);
        }
        AppClient::dispose().await;
        eprintln!("❌ 已断开连接");
    }

    async fn init(ws_stream: WsStream) {
        MessageManager::instance().init(ws_stream).await;
        MessageHandler::<Event>::instance()
            .set_handler(on_event)
            .await;
        MessageHandler::<Stream>::instance()
            .set_handler(on_stream)
            .await;

        let rpc = RPC::instance();
        rpc.add_command("get_version", get_version).await;
        rpc.add_command("run_shell", run_shell).await;
        rpc.add_command("start_play", start_play).await;
        rpc.add_command("stop_play", stop_play).await;
        rpc.add_command("start_recording", start_recording).await;
        rpc.add_command("stop_recording", stop_recording).await;

        InstructionMonitor::start(|event| async move {
            MessageManager::instance()
                .send_event("instruction", Some(json!(event)))
                .await
        })
        .await;

        PlayingMonitor::start(|event| async move {
            MessageManager::instance()
                .send_event("playing", Some(json!(event)))
                .await
        })
        .await;

        KwsMonitor::start(|event| async move {
            MessageManager::instance()
                .send_event("kws", Some(json!(event)))
                .await
        })
        .await;
    }

    async fn dispose() {
        MessageManager::instance().dispose().await;
        let _ = AudioPlayer::instance().stop().await;
        let _ = AudioRecorder::instance().stop_recording().await;
        InstructionMonitor::stop().await;
        PlayingMonitor::stop().await;
        KwsMonitor::stop().await;
    }
}

async fn get_version(_: Request) -> Result<Response, AppError> {
    let data = json!(VERSION.to_string());
    Ok(Response::from_data(data))
}

async fn start_play(request: Request) -> Result<Response, AppError> {
    let config = request
        .payload
        .and_then(|payload| serde_json::from_value::<AudioConfig>(payload).ok());
    AudioPlayer::instance().start(config).await?;
    Ok(Response::success())
}

async fn stop_play(_: Request) -> Result<Response, AppError> {
    AudioPlayer::instance().stop().await?;
    Ok(Response::success())
}

async fn start_recording(request: Request) -> Result<Response, AppError> {
    let config = request
        .payload
        .and_then(|payload| serde_json::from_value::<AudioConfig>(payload).ok());
    AudioRecorder::instance()
        .start_recording(
            |bytes| async {
                MessageManager::instance()
                    .send_stream("record", bytes, None)
                    .await
            },
            config,
        )
        .await?;
    Ok(Response::success())
}

async fn stop_recording(_: Request) -> Result<Response, AppError> {
    AudioRecorder::instance().stop_recording().await?;
    Ok(Response::success())
}

async fn run_shell(request: Request) -> Result<Response, AppError> {
    let script = match request.payload {
        Some(payload) => serde_json::from_value::<String>(payload)?,
        _ => return Err("empty command".into()),
    };
    let res = open_xiaoai::utils::shell::run_shell(script.as_str()).await?;
    Ok(Response::from_data(json!(res)))
}

async fn on_event(event: Event) -> Result<(), AppError> {
    println!("🔥 收到事件: {:?}", event);
    Ok(())
}

async fn on_stream(stream: Stream) -> Result<(), AppError> {
    let Stream { tag, bytes, .. } = stream;
    match tag.as_str() {
        "play" => {
            // 播放接收到的音频流
            let _ = AudioPlayer::instance().play(bytes).await;
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    AppClient::run().await;
}
