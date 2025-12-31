use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::Result;
use crate::stereo_core::mixer::MIXER_SOCKET_PATH;

/// Injector 模式的逻辑：将 stdin 数据转发到 Unix Socket
pub async fn run_injector() -> Result<()> {
    // 1. 连接到主进程的 Mixer Socket
    let mut socket = match UnixStream::connect(MIXER_SOCKET_PATH).await {
        Ok(s) => s,
        Err(e) => {
            // 如果连接失败，可能是主进程还没启动或已经退出
            // 在 Injector 模式下，静默退出即可
            return Err(e.into());
        }
    };

    let mut stdin = tokio::io::stdin();
    let mut buf = [0u8; 4096];

    // 2. 数据搬运：stdin -> socket
    loop {
        let n = stdin.read(&mut buf).await?;
        if n == 0 {
            break; // stdin 关闭
        }
        if let Err(_) = socket.write_all(&buf[..n]).await {
            break; // Socket 断开
        }
    }

    Ok(())
}

