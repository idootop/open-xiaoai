#![cfg(target_os = "linux")]
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::process::Command;

const REAL_ASOUND_CONF: &str = "/etc/asound.conf";
const TEMP_ASOUND_CONF: &str = "/tmp/asound.stereo.conf";

/// ALSA 音频重定向器，通过 Pipe 模式将音频流注入到主进程的混音器中
pub struct AlsaRedirector;

impl AlsaRedirector {
    pub fn new() -> Result<Self> {
        Self::cleanup(); // 确保环境干净

        let original_conf = fs::read_to_string(REAL_ASOUND_CONF).unwrap_or_default();

        if !original_conf.contains("pcm.original_default") {
            let current_exe = env::current_exe()
                .context("无法获取当前可执行文件路径")?
                .to_string_lossy()
                .to_string();

            // 重命名原有的 default 逻辑，插入拦截器
            let mut new_conf = original_conf.replace("pcm.!default", "pcm.original_default");
            new_conf.push_str(&format!(
                r#"
pcm.!default {{
    type plug 
    slave {{ 
        pcm "stereo_interceptor" 
        format S16_LE 
        rate 48000 
        channels 2 
    }}
}}
pcm.stereo_interceptor {{
    type file 
    slave.pcm "null" 
    file "| {} --inject" 
    format "raw"
}}
"#,
                current_exe
            ));

            fs::write(TEMP_ASOUND_CONF, new_conf)?;

            // 挂载覆盖 /etc/asound.conf
            let status = Command::new("mount")
                .arg("--bind")
                .arg(TEMP_ASOUND_CONF)
                .arg(REAL_ASOUND_CONF)
                .status()
                .context("执行 mount 命令失败")?;

            if !status.success() {
                return Err(anyhow::anyhow!("挂载 asound.conf 失败"));
            }
        }

        Ok(Self)
    }

    pub fn cleanup() {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("umount -l {} >/dev/null 2>&1", REAL_ASOUND_CONF))
            .status();
        let _ = fs::remove_file(TEMP_ASOUND_CONF);
    }
}

impl Drop for AlsaRedirector {
    fn drop(&mut self) {
        Self::cleanup();
    }
}
