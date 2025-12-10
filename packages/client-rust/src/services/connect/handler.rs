use futures::future::BoxFuture;
use std::future::Future;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::base::AppError;

use super::data::{AppMessage, AuthResponse, Event, Request, Response, Stream};
use super::message::MessageManager;
use super::rpc::RPC;

type Handler<T> = Arc<dyn Fn(T) -> BoxFuture<'static, Result<(), AppError>> + Send + Sync>;

pub struct MessageHandler<T> {
    handler: Arc<Mutex<Option<Handler<T>>>>,
}

static REQUEST_INSTANCE: LazyLock<MessageHandler<Request>> = LazyLock::new(MessageHandler::new);
static RESPONSE_INSTANCE: LazyLock<MessageHandler<Response>> = LazyLock::new(MessageHandler::new);
static EVENT_INSTANCE: LazyLock<MessageHandler<Event>> = LazyLock::new(MessageHandler::new);
static STREAM_INSTANCE: LazyLock<MessageHandler<Stream>> = LazyLock::new(MessageHandler::new);
static AUTH_RESPONSE_INSTANCE: LazyLock<MessageHandler<AuthResponse>> = LazyLock::new(MessageHandler::new);

impl<T> MessageHandler<T> {
    fn new() -> Self {
        Self {
            handler: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    {
        *self.handler.lock().await = Some(Arc::new(move |data| Box::pin(handler(data))));
    }

    pub async fn on(&self, data: T) -> Result<(), AppError> {
        let handler = {
            let guard = self.handler.lock().await;
            match &*guard {
                Some(h) => Arc::clone(h),
                None => return Err("handler is not initialized".into()),
            }
        };
        handler(data).await
    }
}

impl MessageHandler<Request> {
    pub fn instance() -> &'static MessageHandler<Request> {
        &REQUEST_INSTANCE
    }

    pub async fn on_request(&self, request: Request) -> Result<(), AppError> {
        println!("🚗 收到指令: {:?}", request);
        let id = request.id.clone();
        let response: Response = match RPC::instance().on_request(request).await {
            Ok(resp) => Response { id, ..resp },
            Err(e) => Response::from_error(&id, e),
        };
        if let Ok(data) = serde_json::to_string(&AppMessage::Response(response)) {
            MessageManager::instance()
                .send(Message::Text(data.into()))
                .await?;
        }
        Ok(())
    }
}

impl MessageHandler<Response> {
    pub fn instance() -> &'static MessageHandler<Response> {
        &RESPONSE_INSTANCE
    }

    pub async fn on_response(&self, response: Response) -> Result<(), AppError> {
        RPC::instance().on_response(response).await;
        Ok(())
    }
}

impl MessageHandler<Event> {
    pub fn instance() -> &'static MessageHandler<Event> {
        &EVENT_INSTANCE
    }
}

impl MessageHandler<Stream> {
    pub fn instance() -> &'static MessageHandler<Stream> {
        &STREAM_INSTANCE
    }
}

impl MessageHandler<AuthResponse> {
    pub fn instance() -> &'static MessageHandler<AuthResponse> {
        &AUTH_RESPONSE_INSTANCE
    }
}
