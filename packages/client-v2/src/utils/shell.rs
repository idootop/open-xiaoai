use anyhow::{Context, Result, anyhow};
use core::str;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub type ShellRequest = String;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ShellResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub struct Shell;

impl Shell {
    pub async fn run(command: String, timeout_ms: Option<u64>) -> Result<ShellResponse> {
        // 1. 配置 Command
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 当 Child 结构体被 Drop 时，自动杀死子进程（SIGKILL）
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn shell command")?;

        let mut stdout_reader = child.stdout.take().context("Failed to open stdout")?;
        let mut stderr_reader = child.stderr.take().context("Failed to open stderr")?;

        // 2. 定义执行逻辑
        let run_logic = async move {
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            // 使用 join! 并发读取流并等待进程结束
            let (res_out, res_err, res_status) = tokio::join!(
                stdout_reader.read_to_end(&mut stdout_buf),
                stderr_reader.read_to_end(&mut stderr_buf),
                child.wait()
            );

            let status = res_status.context("Wait for child failed")?;
            res_out.context("Read stdout failed")?;
            res_err.context("Read stderr failed")?;

            Ok(ShellResponse {
                stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
                stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
                exit_code: status.code().unwrap_or(-1),
            })
        };

        // 3. 应用超时
        if let Some(ms) = timeout_ms {
            tokio::time::timeout(Duration::from_millis(ms), run_logic)
                .await
                .map_err(|_| anyhow!("Shell execution timed out"))?
        } else {
            run_logic.await
        }
    }
}
