import asyncio
import os
import threading
import time

import numpy as np

from config import APP_CONFIG
from xiaozhi.event import EventManager
from xiaozhi.ref import get_speaker, get_xiaoai, get_xiaozhi, set_kws
from xiaozhi.services.audio.kws.sherpa import SherpaOnnx
from xiaozhi.services.audio.stream import MyAudio
from xiaozhi.services.protocols.typing import AudioConfig, DeviceState
from xiaozhi.utils.base import get_env

KWS_MIN_RMS = 300.0
KWS_MIN_TRIGGER_INTERVAL_MS = 1500


class _KWS:
    def __init__(self):
        self.last_trigger_ms = 0.0
        set_kws(self)

    def start(self):
        if not get_env("CLI"):
            return

        self.audio = MyAudio.create()
        self.stream = self.audio.open(
            format=AudioConfig.FORMAT,
            channels=1,
            rate=16000,
            input=True,
            frames_per_buffer=AudioConfig.FRAME_SIZE,
            start=True,
        )

        # 启动 KWS 服务
        self.paused = False
        self.thread = threading.Thread(target=self._detection_loop, daemon=True)
        self.thread.start()

    def get_file_path(self, file_name: str):
        current_dir = os.path.dirname(os.path.abspath(__file__))
        return os.path.join(current_dir, "../../../models", file_name)

    def pause(self):
        self.paused = True

    def resume(self):
        self.paused = False

    def _rms(self, frames: bytes) -> float:
        samples = np.frombuffer(frames, dtype=np.int16).astype(np.float32)
        if samples.size == 0:
            return 0.0
        return float(np.sqrt(np.mean(samples * samples) + 1e-6))

    def _detection_loop(self):
        SherpaOnnx.start()
        self.stream.start_stream()
        while True:
            # 读取缓冲区音频数据
            frames = self.stream.read()

            # 在说话和监听状态时，暂停 KWS
            if (
                not frames
                or self.paused
                or get_xiaozhi().device_state
                in [
                    DeviceState.LISTENING,
                    DeviceState.SPEAKING,
                ]
            ):
                time.sleep(0.01)
                continue

            # 静音门控：低能量帧跳过 KWS 推理
            if self._rms(frames) < KWS_MIN_RMS:
                time.sleep(0.005)
                continue

            result = SherpaOnnx.kws(frames)
            if result:
                # 防抖：限制连续触发频率，降低重复误触发
                now_ms = time.monotonic() * 1000.0
                if now_ms - self.last_trigger_ms < KWS_MIN_TRIGGER_INTERVAL_MS:
                    continue
                self.last_trigger_ms = now_ms
                print(f"🔥 触发唤醒: {result}")
                self.on_message(result)

    def on_message(self, text: str):
        asyncio.run_coroutine_threadsafe(
            EventManager.wakeup(text, "kws"),
            get_xiaoai().async_loop,
        )


KWS = _KWS()
