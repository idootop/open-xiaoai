#![cfg(target_os = "linux")]

use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

const FIFO_PATH: &str = "/tmp/stereo_out.fifo";
const REAL_ASOUND_CONF: &str = "/etc/asound.conf";
const TEMP_ASOUND_CONF: &str = "/tmp/asound.stereo.conf";

/// ALSA 音频重定向器，用于拦截系统音频输出到 FIFO 管道
pub struct AlsaRedirector;

impl AlsaRedirector {
    pub fn new() -> Result<Self> {
        Self::cleanup(); // 确保环境干净

        let original_conf = fs::read_to_string(REAL_ASOUND_CONF).unwrap_or_default();

        if !original_conf.contains("pcm.original_default") {
            // 重命名原有的 default 逻辑，插入拦截器
            let mut new_conf = original_conf.replace("pcm.!default", "pcm.original_default");
            new_conf.push_str(&format!(
                "\npcm.!default {{ type plug slave {{ pcm \"stereo_interceptor\" format S16_LE rate 48000 channels 2 }} }}\n\
                pcm.stereo_interceptor {{ type file slave.pcm \"null\" file \"{}\" format \"raw\" }}\n",
                FIFO_PATH
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

            Self::restart_applications();
        }

        // 创建 FIFO 管道
        let _ = Command::new("mkfifo").arg(FIFO_PATH).status();
        let _ = Command::new("chmod").arg("666").arg(FIFO_PATH).status();

        Ok(Self)
    }

    pub fn cleanup() {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("umount -l {} >/dev/null 2>&1", REAL_ASOUND_CONF))
            .status();
        let _ = fs::remove_file(TEMP_ASOUND_CONF);
        let _ = fs::remove_file(FIFO_PATH);
        Self::restart_applications();
    }

    pub fn fifo_path() -> &'static str {
        FIFO_PATH
    }

    pub fn restart_applications() {
        // 重启媒体播放器
        let _ = Command::new("sh")
            .arg("-c")
            .arg("/etc/init.d/mediaplayer restart >/dev/null 2>&1")
            .status();
        // 重启蓝牙
        let _ = Command::new("sh")
            .arg("-c")
            .arg("/etc/init.d/bluetooth restart >/dev/null 2>&1")
            .status();
    }
}

impl Drop for AlsaRedirector {
    fn drop(&mut self) {
        Self::cleanup();
    }
}
