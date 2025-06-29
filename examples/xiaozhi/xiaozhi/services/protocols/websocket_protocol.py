import asyncio
import json

import websockets

from xiaozhi.ref import get_xiaozhi
from xiaozhi.services.protocols.protocol import Protocol
from xiaozhi.services.protocols.typing import DeviceState
from xiaozhi.utils.config import ConfigManager


class WebsocketProtocol(Protocol):
    def __init__(self):
        super().__init__()
        # 获取配置管理器实例
        self.config = ConfigManager.instance()
        self.websocket = None
        self.server_sample_rate = 24000
        self.server_frame_duration = 60
        self.server_frame_size = int(
            self.server_sample_rate * (self.server_frame_duration / 1000)
        )
        self.connected = False
        self.hello_received = None  # 初始化时先设为 None
        self.WEBSOCKET_URL = self.config.get_config("NETWORK.WEBSOCKET_URL")
        self.WEBSOCKET_ACCESS_TOKEN = self.config.get_config(
            "NETWORK.WEBSOCKET_ACCESS_TOKEN"
        )
        self.CLIENT_ID = self.config.get_client_id()
        self.DEVICE_ID = self.config.get_device_id()

    async def _close_websocket(self):
        if self.websocket:
            try:
                await self.websocket.close()
                self.websocket = None
                self.connected = False
            except Exception:
                pass

    async def connect(self) -> bool:
        """连接到WebSocket服务器"""
        try:
            await self._close_websocket()

            # 在连接时创建 Event，确保在正确的事件循环中
            self.hello_received = asyncio.Event()

            # 配置连接
            headers = {
                "Authorization": f"Bearer {self.WEBSOCKET_ACCESS_TOKEN}",
                "Protocol-Version": "1",
                "Device-Id": self.DEVICE_ID,  # 获取设备MAC地址
                "Client-Id": self.CLIENT_ID,
            }

            # 建立WebSocket连接
            self.websocket = await websockets.connect(
                uri=self.WEBSOCKET_URL, additional_headers=headers
            )

            # 启动消息处理循环
            asyncio.create_task(self._message_handler())

            # 发送客户端hello消息
            hello_message = {
                "type": "hello",
                "version": 1,
                "transport": "websocket",
                "audio_params": {
                    "format": "opus",
                    "sample_rate": 16000,
                    "channels": 1,
                    "frame_duration": 60,
                },
            }
            await self.send_text(json.dumps(hello_message))

            # 等待服务器hello响应
            try:
                await asyncio.wait_for(self.hello_received.wait(), timeout=10.0)
                self.connected = True
                return True
            except asyncio.TimeoutError:
                if self.on_network_error:
                    self.on_network_error("等待响应超时")
                return False
        except Exception as e:
            if self.on_network_error:
                self.on_network_error(f"无法连接服务: {str(e)}")
            return False

    async def _message_handler(self):
        """处理接收到的WebSocket消息"""
        try:
            async for message in self.websocket:
                if isinstance(message, str):
                    try:
                        data = json.loads(message)
                        msg_type = data.get("type")
                        if msg_type == "hello":
                            await self._handle_server_hello(data)
                        else:
                            if self.on_incoming_json:
                                self.on_incoming_json(data)
                    except json.JSONDecodeError:
                        pass
                elif self.on_incoming_audio:
                    self.on_incoming_audio(message)
        except Exception:
            self.connected = False
            if self.on_audio_channel_closed:
                await self.on_audio_channel_closed()

    async def send_audio(self, frames: list[bytes]):
        """发送音频数据"""
        if not self.is_audio_channel_opened():  # 使用已有的 is_connected 方法
            return

        try:
            for frame in frames:
                await self.websocket.send(frame)
        except Exception:
            pass

    async def send_text(self, message: str):
        """发送文本消息"""
        if self.websocket:
            try:
                await self.websocket.send(message)
            except Exception as e:
                raise e
    def is_audio_channel_opened(self) -> bool:
        """检查音频通道是否打开"""
        return self.websocket is not None and self.connected

    _is_heartbeat_running = False

    async def open_audio_channel(self):
        if not self._is_heartbeat_running:
            self._is_heartbeat_running = True
            asyncio.create_task(self.heartbeat())
        await self.connect()

    async def _handle_server_hello(self, data: dict):
        """处理服务器的 hello 消息

        解析服务器返回的 hello 消息，设置相关参数并通知音频通道已打开

        Args:
            data: 服务器返回的 hello 消息数据
        """
        try:
            # 验证传输方式
            transport = data.get("transport")
            if not transport or transport != "websocket":
                return

            # TODO 使用默认的 24k 采样率
            # xiaozhi-esp32-server 返回的参数是 16k 采样率，但实际用的是 24k 采样率

            # 获取音频参数
            # audio_params = data.get("audio_params")
            # if audio_params:
            #     # 获取服务器的采样率
            #     sample_rate = audio_params.get("sample_rate")
            #     if sample_rate:
            #         self.server_sample_rate = sample_rate
            #     frame_duration = audio_params.get("frame_duration")
            #     if frame_duration:
            #         self.server_frame_duration = frame_duration
            #     self.server_frame_size = int(
            #         self.server_sample_rate * (self.server_frame_duration / 1000)
            #     )

            # 设置 hello 接收事件
            self.hello_received.set()

            # 通知音频通道已打开
            if self.on_audio_channel_opened:
                await self.on_audio_channel_opened()

        except Exception as e:
            if self.on_network_error:
                self.on_network_error(f"处理服务器响应失败: {str(e)}")

    async def close_audio_channel(self):
        """关闭音频通道"""
        if self.websocket:
            try:
                await self.websocket.close()
                self.websocket = None
                self.connected = False
                if self.on_audio_channel_closed:
                    await self.on_audio_channel_closed()
            except Exception:
                pass

    async def heartbeat(self):
        while True:
            if self.websocket and get_xiaozhi().device_state == DeviceState.IDLE:
                try:
                    await self.send_text(
                        json.dumps({"session_id": "", "type": "ping"})
                    )
                except Exception:
                    # 发送心跳失败，重新连接
                    await self.open_audio_channel()
            await asyncio.sleep(1)
