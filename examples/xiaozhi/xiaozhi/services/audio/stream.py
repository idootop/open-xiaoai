import uuid
from typing import Any, Callable, ClassVar, Optional

from xiaozhi.ref import get_xiaoai


class __GlobalStream:
    def __init__(self):
        self.readers = {}
        self.on_output_data = None

    def register_reader(self, reader):
        if reader.id not in self.readers:
            self.readers[reader.id] = reader

    def unregister_reader(self, reader) -> None:
        if reader.id in self.readers:
            del self.readers[reader.id]

    def input(self, data: bytes) -> None:
        for key in self.readers:
            self.readers[key].input(data)

    def output(self, frames: bytes) -> None:
        if self.on_output_data:
            self.on_output_data(frames)


GlobalStream = __GlobalStream()


class MyStream:
    def __init__(
        self,
        rate: int,
        channels: int,
        format: int,
        input: bool = False,
        output: bool = False,
        frames_per_buffer: int = 1024,
        start: bool = True,
    ) -> None:
        self.id = uuid.uuid4()
        self._rate = rate
        self._channels = channels
        self._format = format
        self._frames_per_buffer = frames_per_buffer
        self._is_input = input
        self._is_output = output
        self._is_active = False

        self.input_bytes: list[int] = []

        if start:
            self.start_stream()

    def close(self) -> None:
        self.stop_stream()

    def is_active(self) -> bool:
        return self._is_active

    def start_stream(self) -> None:
        if not self._is_active:
            self._is_active = True
            if self._is_input:
                GlobalStream.register_reader(self)

    def stop_stream(self) -> None:
        if self._is_active:
            self._is_active = False
            if self._is_input:
                GlobalStream.unregister_reader(self)
                self.input_bytes.clear()

    def write(self, frames: bytes) -> None:
        # 发送输出音频流到扬声器
        if not self._is_output or not self._is_active:
            return
        GlobalStream.output(frames)

    def input(self, data: bytes):
        # 收到麦克风输入音频流
        if not self._is_input or not self._is_active:
            return

        if len(data) > 0:
            self.input_bytes.extend(data)

    def read(self, num_frames=None, exception_on_overflow=False) -> bytes:
        if num_frames is None:
            data = bytes(self.input_bytes)
            self.input_bytes.clear()
            return data

        num_frames = num_frames * 2
        if (
            not self._is_input
            or not self._is_active
            # 达不到预期长度时，返回空字节，等待下一次读取
            or len(self.input_bytes) < num_frames
        ):
            return bytes([])

        data = bytes(self.input_bytes[:num_frames])
        self.input_bytes = self.input_bytes[num_frames:]

        return data


class MyAudio:
    """PyAudio 替代品，用于创建和管理音频流"""

    Stream: ClassVar[type] = MyStream

    @classmethod
    def create(cls):
        if get_xiaoai().mode != "xiaozhi":
            return MyAudio()
        else:
            from pyaudio import PyAudio

            return PyAudio()

    @classmethod
    def get_input_device_index(cls, audio):
        if get_xiaoai().mode != "xiaozhi":
            return 0
        try:
            device = audio.get_default_input_device_info()
            return device["index"]
        except Exception:
            for i in range(audio.get_device_count()):
                dev = audio.get_device_info_by_index(i)
                if dev["maxInputChannels"] > 0:
                    return i
            return 0

    @classmethod
    def get_output_device_index(cls, audio):
        if get_xiaoai().mode != "xiaozhi":
            return 0
        try:
            device = audio.get_default_output_device_info()
            return device["index"]
        except Exception:
            for i in range(audio.get_device_count()):
                dev = audio.get_device_info_by_index(i)
                if dev["maxOutputChannels"] > 0:
                    return i
            return 0

    def __init__(self) -> None:
        self._is_terminated = False

    def open(
        self,
        rate: int,
        channels: int,
        format: int,
        input: bool = False,
        output: bool = False,
        input_device_index: Optional[int] = None,
        output_device_index: Optional[int] = None,
        frames_per_buffer: int = 1024,
        start: bool = True,
        input_host_api_specific_stream_info: Optional[Any] = None,
        output_host_api_specific_stream_info: Optional[Any] = None,
        stream_callback: Optional[Callable] = None,
    ) -> MyStream:
        if self._is_terminated:
            raise RuntimeError("MyAudio instance has been terminated")

        return MyStream(
            rate=rate,
            channels=channels,
            format=format,
            input=input,
            output=output,
            frames_per_buffer=frames_per_buffer,
            start=start,
        )

    def terminate(self) -> None:
        if not self._is_terminated:
            self._is_terminated = True
