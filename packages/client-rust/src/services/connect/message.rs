use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::future::Future;
use std::sync::{Arc, LazyLock};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Semaphore};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use super::rpc::RPC;
use crate::base::AppError;
use crate::utils::task::TaskManager;

use super::data::{AppMessage, AuthRequest, AuthResponse, Event, Request, Response, Stream};
use super::handler::MessageHandler;

pub enum WsStream {
    Server(WebSocketStream<TcpStream>),
    Client(WebSocketStream<MaybeTlsStream<TcpStream>>),
}

pub enum WsReader {
    Server(SplitStream<WebSocketStream<TcpStream>>),
    Client(SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>),
}

pub enum WsWriter {
    Server(SplitSink<WebSocketStream<TcpStream>, Message>),
    Client(SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>),
}

pub struct MessageManager {
    semaphore: Arc<Semaphore>,
    reader: Arc<Mutex<Option<WsReader>>>,
    writer: Arc<Mutex<Option<WsWriter>>>,
}

static INSTANCE: LazyLock<MessageManager> = LazyLock::new(MessageManager::new);

impl MessageManager {
    fn new() -> Self {
        Self {
            reader: Arc::new(Mutex::new(None)),
            writer: Arc::new(Mutex::new(None)),
            semaphore: Arc::new(Semaphore::new(32)),
        }
    }

    pub fn instance() -> &'static Self {
        &INSTANCE
    }

    pub async fn init(&self, ws_stream: WsStream) {
        match ws_stream {
            WsStream::Client(stream) => {
                let (tx, rx) = stream.split();
                self.reader.lock().await.replace(WsReader::Client(rx));
                self.writer.lock().await.replace(WsWriter::Client(tx));
            }
            WsStream::Server(stream) => {
                let (tx, rx) = stream.split();
                self.reader.lock().await.replace(WsReader::Server(rx));
                self.writer.lock().await.replace(WsWriter::Server(tx));
            }
        }
        RPC::instance()
            .init(|request| async {
                let data = serde_json::to_string(&AppMessage::Request(request)).unwrap();
                MessageManager::instance()
                    .send(Message::Text(data.into()))
                    .await
            })
            .await;
    }

    pub async fn send_auth(&self, token: Option<String>) -> Result<(), AppError> {
        let auth = AuthRequest::new(token);
        let data = serde_json::to_string(&AppMessage::Auth(auth)).unwrap();
        self.send(Message::Text(data.into())).await
    }

    pub async fn dispose(&self) {
        *self.reader.lock().await = None;
        *self.writer.lock().await = None;
        RPC::instance().dispose().await;
        TaskManager::instance().dispose("MessageManager").await;
    }

    pub async fn send(&self, msg: Message) -> Result<(), AppError> {
        let mut writer_guard = self.writer.lock().await;

        let Some(writer) = &mut *writer_guard else {
            return Err("WebSocket writer is not initialized".into());
        };

        match writer {
            WsWriter::Client(w) => w.send(msg).await?,
            WsWriter::Server(w) => w.send(msg).await?,
        }

        Ok(())
    }

    pub async fn send_event(&self, event: &str, data: Option<Value>) -> Result<(), AppError> {
        let event: Event = Event::new(event, data);
        let data = serde_json::to_string(&AppMessage::Event(event)).unwrap();
        MessageManager::instance()
            .send(Message::Text(data.into()))
            .await
    }

    pub async fn send_stream(
        &self,
        tag: &str,
        bytes: Vec<u8>,
        data: Option<Value>,
    ) -> Result<(), AppError> {
        let stream = serde_json::to_vec(&Stream::new(tag, bytes, data)).unwrap();
        MessageManager::instance()
            .send(Message::Binary(stream.into()))
            .await
    }

    pub async fn process_messages(&self) -> Result<(), AppError> {
        if self.reader.lock().await.is_none() {
            return Err("WebSocket reader is not initialized".into());
        }

        loop {
            let next_msg = {
                let mut reader = self.reader.lock().await;
                match reader.as_mut() {
                    None => break,
                    Some(WsReader::Client(reader)) => reader.next().await,
                    Some(WsReader::Server(reader)) => reader.next().await,
                }
            };
            match next_msg {
                None => break,
                Some(Ok(Message::Close(_))) => break,
                Some(Err(e)) => return Err(e.into()),
                Some(Ok(msg)) => {
                    match msg {
                        Message::Text(text) => {
                            let _ = self.on_text(text.to_string()).await;
                        }
                        Message::Binary(bytes) => {
                            let _ = self.on_bytes(bytes.into()).await;
                        }
                        _ => {}
                    };
                }
            }
        }

        Ok(())
    }

    async fn on_bytes(&self, bytes: Vec<u8>) -> Result<(), AppError> {
        let data = serde_json::from_slice::<Stream>(&bytes)?;
        MessageHandler::<Stream>::instance().on(data).await
    }

    async fn on_text(&self, text: String) -> Result<(), AppError> {
        let msg = serde_json::from_str::<AppMessage>(&text)?;

        match msg {
            AppMessage::Request(request) => {
                self.run_concurrently(move || {
                    let request = request.clone();
                    async move {
                        MessageHandler::<Request>::instance()
                            .on_request(request)
                            .await?;
                        Ok(())
                    }
                })
                .await
            }
            AppMessage::Response(response) => {
                MessageHandler::<Response>::instance()
                    .on_response(response)
                    .await
            }
            AppMessage::Event(event) => MessageHandler::<Event>::instance().on(event).await,
            AppMessage::Auth(auth_request) => {
                // Server端处理Client的认证请求
                self.handle_auth_request(auth_request).await
            }
            AppMessage::AuthResponse(auth_response) => {
                // Client端处理Server的认证响应
                MessageHandler::<AuthResponse>::instance().on(auth_response).await
            }
            _ => Ok(()),
        }
    }

    async fn handle_auth_request(&self, auth_request: AuthRequest) -> Result<(), AppError> {
        // 获取Server端配置的token
        let server_token = std::env::var("OPEN_XIAOAI_TOKEN").ok();
        
        let response = if let Some(token) = server_token {
            // Server端配置了token，需要验证
            if auth_request.token.as_deref() == Some(token.as_str()) {
                println!("✅ 认证成功: Client端提供了正确的token");
                AuthResponse::success(&auth_request.id)
            } else {
                println!("❌ 认证失败: Client端提供的token无效");
                AuthResponse::error(&auth_request.id, 401, "无效的token")
            }
        } else {
            // Server端未配置token，直接通过验证
            println!("ℹ️  Server端未配置token，跳过认证");
            AuthResponse::success(&auth_request.id)
        };
        
        let data = serde_json::to_string(&AppMessage::AuthResponse(response)).unwrap();
        self.send(Message::Text(data.into())).await
    }

    async fn run_concurrently<F, Fut>(&self, run: F) -> Result<(), AppError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    {
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => self.semaphore.clone().acquire_owned().await?,
        };

        let task = tokio::spawn(async move {
            let _ = run().await;
            drop(permit);
        });

        TaskManager::instance().add("MessageManager", task).await;
        Ok(())
    }
}
