use crate::net::network::ControlWriter;
use crate::net::protocol::DeviceInfo;
use crate::net::rpc::RpcManager;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct ServerSession {
    pub info: DeviceInfo,
    pub writer: Arc<tokio::sync::Mutex<ControlWriter>>,
    pub addr: SocketAddr,
    pub rpc: Arc<RpcManager>,
}

impl ServerSession {
    pub fn new(info: DeviceInfo, writer: ControlWriter, addr: SocketAddr) -> Self {
        Self {
            info,
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
            addr,
            rpc: Arc::new(RpcManager::new()),
        }
    }
}
