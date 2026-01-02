use crate::app::server::session::ServerSession;
use crate::net::protocol::{ControlPacket, RpcResult};
use anyhow::Result;
use std::sync::Arc;

pub async fn handle_packet(session: Arc<ServerSession>, packet: ControlPacket) -> Result<()> {
    match packet {
        ControlPacket::RpcResponse { id, result } => {
            session.rpc.fulfill(id, result);
        }
        ControlPacket::RpcRequest { id, method, args } => {
            println!(
                "收到来自客户端 {} 的 RPC 请求: {} {:?}",
                session.addr, method, args
            );
            let result = handle_server_rpc(&method, args).await;
            session
                .writer
                .lock()
                .await
                .send_packet(&ControlPacket::RpcResponse { id, result })
                .await?;
        }
        ControlPacket::Ping => {
            session
                .writer
                .lock()
                .await
                .send_packet(&ControlPacket::Pong)
                .await?;
        }
        _ => {}
    }
    Ok(())
}

async fn handle_server_rpc(method: &str, _args: Vec<String>) -> RpcResult {
    match method {
        "status" => RpcResult {
            stdout: "Server is running normally".to_string(),
            stderr: "".to_string(),
            code: 0,
        },
        _ => RpcResult {
            stdout: "".to_string(),
            stderr: format!("Server does not support method: {}", method),
            code: -1,
        },
    }
}
