#![cfg(target_os = "linux")]

use crate::app::client::handlers;
use crate::app::client::session::ClientSession;
use crate::audio::config::AudioConfig;
use crate::net::discovery::Discovery;
use crate::net::network::{AudioSocket, ClientNetwork};
use crate::net::protocol::{ControlPacket, DeviceInfo, RpcResult};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct Client {
    info: DeviceInfo,
    session: Arc<tokio::sync::Mutex<Option<Arc<ClientSession>>>>,
}

impl Client {
    pub fn new(model: &str, mac: &str, version: u32) -> Self {
        Self {
            info: DeviceInfo {
                model: model.to_string(),
                mac: mac.to_string(),
                version,
            },
            session: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        println!("正在寻找服务端...");
        let (server_ip, server_port) = Discovery::discover_server().await?;
        let server_addr = SocketAddr::new(server_ip, server_port);
        println!("发现服务端: {}", server_addr);

        let network = ClientNetwork::connect(server_addr).await?;
        let mut control = network.into_control();

        // 握手
        control
            .send_packet(&ControlPacket::ClientIdentify {
                info: self.info.clone(),
            })
            .await?;

        match control.recv_packet().await? {
            ControlPacket::IdentifyOk => println!("认证成功"),
            p => return Err(anyhow::anyhow!("认证失败: {:?}", p)),
        }

        let (reader, writer) = control.split();
        let session = Arc::new(ClientSession::new(self.info.clone(), writer));
        *self.session.lock().await = Some(session.clone());

        let (stop_tx, _) = broadcast::channel::<()>(1);
        let audio_socket = Arc::new(AudioSocket::bind().await?);

        // 启动测试 RPC 调用的任务
        let client_clone = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            println!("测试向服务端发起 RPC: status");
            match client_clone.call_server("status", vec![]).await {
                Ok(res) => println!("收到服务端响应: {:?}", res),
                Err(e) => eprintln!("向服务端发起 RPC 失败: {}", e),
            }
        });

        let mut reader = reader;
        loop {
            let packet = reader.recv_packet().await?;
            let self_clone = self.clone();
            let session_clone = session.clone();
            let audio_socket = audio_socket.clone();
            let stop_tx = stop_tx.clone();

            tokio::spawn(async move {
                if let Err(e) = handlers::handle_packet(
                    self_clone,
                    session_clone,
                    packet,
                    audio_socket,
                    stop_tx,
                    server_addr,
                )
                .await
                {
                    eprintln!("处理控制包出错: {}", e);
                }
            });
        }
    }

    pub async fn call_server(&self, method: &str, args: Vec<String>) -> Result<RpcResult> {
        let session = self
            .session
            .lock()
            .await
            .clone()
            .context("Session not established")?;
        let (id, rx) = session.rpc.alloc_id();
        session
            .writer
            .lock()
            .await
            .send_packet(&ControlPacket::RpcRequest {
                id,
                method: method.to_string(),
                args,
            })
            .await?;
        Ok(rx.await?)
    }
}
