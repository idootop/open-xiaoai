use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub enum AppMessage {
    Request(Request),
    Response(Response),
    Event(Event),
    Stream(Stream),
    Auth(AuthRequest),
    AuthResponse(AuthResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub token: Option<String>,
}

impl AuthRequest {
    pub fn new(token: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            token,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub msg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<i32>,
}

impl AuthResponse {
    pub fn success(id: &str) -> Self {
        Self {
            id: id.to_string(),
            success: true,
            msg: Some("Authentication successful".to_string()),
            code: Some(0),
        }
    }

    pub fn error(id: &str, code: i32, msg: &str) -> Self {
        Self {
            id: id.to_string(),
            success: false,
            msg: Some(msg.to_string()),
            code: Some(code),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    pub id: String,
    pub tag: String,
    pub bytes: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<Value>,
}

impl Stream {
    pub fn new(tag: &str, bytes: Vec<u8>, data: Option<Value>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            tag: tag.to_string(),
            bytes,
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<Value>,
}

impl Event {
    pub fn new(event: &str, data: Option<Value>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event: event.to_string(),
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub command: String,
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub msg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<Value>,
}

impl Response {
    pub fn success() -> Self {
        Self {
            id: 0.to_string(),
            code: Some(0),
            msg: Some("success".to_string()),
            data: None,
        }
    }

    pub fn from_data(data: Value) -> Self {
        Self {
            id: 0.to_string(),
            code: None,
            msg: None,
            data: Some(data),
        }
    }

    pub fn from_error(id: &str, e: impl std::fmt::Display) -> Self {
        Self {
            id: id.to_string(),
            code: Some(-1),
            msg: Some(e.to_string()),
            data: None,
        }
    }
}
