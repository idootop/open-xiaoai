import argparse
import asyncio
import threading

import numpy as np
import open_xiaoai_server

from xiaozhi.event import EventManager
from xiaozhi.ref import get_speaker, set_xiaoai
from xiaozhi.services.audio.stream import GlobalStream
from xiaozhi.services.speaker import SpeakerManager
from xiaozhi.utils.base import json_decode

ASCII_BANNER = """
▄▖      ▖▖▘    ▄▖▄▖
▌▌▛▌█▌▛▌▚▘▌▀▌▛▌▌▌▐ 
▙▌▙▌▙▖▌▌▌▌▌█▌▙▌▛▌▟▖
  ▌                
                                                                                                                
v1.0.0  by: https://del.wang
"""


class XiaoAI:
    mode = "xiaoai"
    speaker = SpeakerManager()
    async_loop: asyncio.AbstractEventLoop = None

    @classmethod
    def setup_mode(cls):
        set_xiaoai(cls)
        parser = argparse.ArgumentParser(
            description="小爱音箱接入小智 AI | by: https://del.wang"
        )
        parser.add_argument(
            "--mode",
            type=str,
            choices=["xiaoai", "xiaozhi"],
            default="xiaoai",
            help="运行模式：【xiaoai】使用小爱音箱的输入输出音频（默认）、【xiaozhi】使用本地电脑的输入输出音频",
        )
        args = parser.parse_args()
        if args.mode == "xiaozhi":
            cls.mode = "xiaozhi"

    @classmethod
    def on_input_data(cls, data: bytes):
        audio_array = np.frombuffer(data, dtype=np.uint16)
        GlobalStream.input(audio_array.tobytes())

    @classmethod
    def on_output_data(cls, data: bytes):
        async def on_output_data_async(data: bytes):
            return await open_xiaoai_server.on_output_data(data)

        asyncio.run_coroutine_threadsafe(
            on_output_data_async(data),
            cls.async_loop,
        )

    @classmethod
    async def run_shell(cls, script: str, timeout: float = 10 * 1000):
        return await open_xiaoai_server.run_shell(script, timeout)

    @classmethod
    async def on_event(cls, event: str):
        event_json = json_decode(event) or {}
        event_data = event_json.get("data", {})
        event_type = event_json.get("event")

        if not event_json.get("event"):
            return

        if event_type == "instruction" and event_data.get("NewLine"):
            line = json_decode(event_data.get("NewLine"))
            if (
                line
                and line.get("header", {}).get("namespace") == "SpeechRecognizer"
                and line.get("header", {}).get("name") == "RecognizeResult"
            ):
                text = line.get("payload", {}).get("results")[0].get("text")
                if not text and not line.get("payload", {}).get("is_vad_begin"):
                    print("🔥 唤醒小爱")
                    EventManager.on_interrupt()
                elif text and line.get("payload", {}).get("is_final"):
                    print(f"🔥 收到指令: {text}")
                    await EventManager.wakeup(text, "xiaoai")
        elif event_type == "playing":
            get_speaker().status = event_data.lower()

    @classmethod
    def __init_background_event_loop(cls):
        def run_event_loop():
            cls.async_loop = asyncio.new_event_loop()
            asyncio.set_event_loop(cls.async_loop)
            cls.async_loop.run_forever()

        thread = threading.Thread(target=run_event_loop, daemon=True)
        thread.start()

    @classmethod
    def __on_event(cls, event: str):
        asyncio.run_coroutine_threadsafe(
            cls.on_event(event),
            cls.async_loop,
        )

    @classmethod
    async def init_xiaoai(cls):
        GlobalStream.on_output_data = cls.on_output_data
        open_xiaoai_server.register_fn("on_input_data", cls.on_input_data)
        open_xiaoai_server.register_fn("on_event", cls.__on_event)
        print(ASCII_BANNER)
        
        # 优先从环境变量读取token，然后从配置文件读取
        import os
        from config import APP_CONFIG
        token = os.getenv("OPEN_XIAOAI_TOKEN") or APP_CONFIG.get("server", {}).get("token")
        
        await open_xiaoai_server.start_server(token)
