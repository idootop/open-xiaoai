use crate::net::network::ControlWriter;
use crate::net::protocol::DeviceInfo;
use crate::net::rpc::RpcManager;
use std::sync::Arc;

pub struct ClientSession {
    pub info: DeviceInfo,
    pub writer: Arc<tokio::sync::Mutex<ControlWriter>>,
    pub rpc: Arc<RpcManager>,
}

impl ClientSession {
    pub fn new(info: DeviceInfo, writer: ControlWriter) -> Self {
        Self {
            info,
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
            rpc: Arc::new(RpcManager::new()),
        }
    }
}
