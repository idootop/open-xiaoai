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
use open_xiaoai::services::connect::data::{AuthResponse, Event, Request, Response, Stream};
use open_xiaoai::services::connect::handler::MessageHandler;
use open_xiaoai::services::connect::message::{MessageManager, WsStream};
use open_xiaoai::services::connect::rpc::RPC;
use open_xiaoai::services::monitor::instruction::InstructionMonitor;
use open_xiaoai::services::monitor::playing::PlayingMonitor;

struct AppClient {
    kws_monitor: KwsMonitor,
    instruction_monitor: InstructionMonitor,
    playing_monitor: PlayingMonitor,
    token: Option<String>,
}

impl AppClient {
    pub fn new(token_from_cli: Option<String>) -> Self {
        // 只从命令行获取token，不再从文件读取
        let token = if let Some(token) = token_from_cli {
            println!("✅ 已从命令行获取token，将启用token验证");
            Some(token)
        } else {
            println!("ℹ️ 未提供token，将不启用token验证");
            None
        };
        
        Self {
            kws_monitor: KwsMonitor::new(),
            instruction_monitor: InstructionMonitor::new(),
            playing_monitor: PlayingMonitor::new(),
            token,
        }
    }

    pub async fn connect(&self, url: &str) -> Result<WsStream, AppError> {
        let (ws_stream, _) = connect_async(url).await?;
        Ok(WsStream::Client(ws_stream))
    }

    pub async fn run(&mut self) {
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 2 {
            eprintln!("❌ 请输入服务器地址");
            eprintln!("用法: client <server_url> [token]");
            eprintln!("示例: client ws://192.168.31.227:4399 my-token");
            std::process::exit(1);
        }
        
        let url = &args[1];
        println!("✅ 已启动");
        loop {
            let Ok(ws_stream) = self.connect(url).await else {
                sleep(Duration::from_secs(1)).await;
                continue;
            };
            println!("✅ 已连接: {:?}", url);
            self.init(ws_stream).await;
            if let Err(e) = MessageManager::instance().process_messages().await {
                eprintln!("❌ 消息处理异常: {}", e);
            }
            self.dispose().await;
            eprintln!("❌ 已断开连接");
        }
    }

    async fn init(&mut self, ws_stream: WsStream) {
        MessageManager::instance().init(ws_stream).await;
        
        // 设置认证响应处理
        MessageHandler::<AuthResponse>::instance()
            .set_handler(on_auth_response)
            .await;
        
        // 发送认证请求
        MessageManager::instance().send_auth(self.token.clone()).await.unwrap();
        
        // 设置其他事件处理
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

        self.instruction_monitor
            .start(|event| async move {
                MessageManager::instance()
                    .send_event("instruction", Some(json!(event)))
                    .await
            })
            .await;

        self.playing_monitor
            .start(|event| async move {
                MessageManager::instance()
                    .send_event("playing", Some(json!(event)))
                    .await
            })
            .await;

        self.kws_monitor
            .start(|event| async move {
                MessageManager::instance()
                    .send_event("kws", Some(json!(event)))
                    .await
            })
            .await;
    }

    async fn dispose(&mut self) {
        MessageManager::instance().dispose().await;
        let _ = AudioPlayer::instance().stop().await;
        let _ = AudioRecorder::instance().stop_recording().await;
        self.instruction_monitor.stop().await;
        self.playing_monitor.stop().await;
        self.kws_monitor.stop().await;
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
    if tag.as_str() == "play" {
        // 播放接收到的音频流
        let _ = AudioPlayer::instance().play(bytes).await;
    }
    Ok(())
}

async fn on_auth_response(auth_response: AuthResponse) -> Result<(), AppError> {
    if auth_response.success {
        println!("✅ 认证成功");
    } else {
        eprintln!("❌ 认证失败: {:?} (code: {:?})", auth_response.msg, auth_response.code);
        // 认证失败，断开连接
        MessageManager::instance().dispose().await;
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // 获取命令行中的token参数（可选）
    let token = if args.len() > 2 {
        Some(args[2].clone())
    } else {
        None
    };
    
    AppClient::new(token).run().await;
}
