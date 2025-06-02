import socket
import struct
import time
import hmac
import hashlib
import threading
from typing import Optional


class DiscoveryService:
    def __init__(self, secret: str, port: int = 5354, ws_port: int = 8080):
        """
        初始化服务发现服务
        
        Args:
            secret: 用于HMAC验证的密钥
            port: UDP监听端口
            ws_port: WebSocket服务端口
        """
        self.secret = secret.encode('utf-8')
        self.port = port
        self.ws_port = ws_port
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        try:
            self.socket.bind(('0.0.0.0', port))
        except OSError as e:
            if e.errno == 48:  # Address already in use
                raise OSError(f"端口 {port} 已被占用，请尝试使用其他端口。使用 --port 参数指定其他端口。")
            raise
        self.running = False
        self.thread: Optional[threading.Thread] = None
        
    def start(self):
        """
        启动服务发现服务
        """
        if self.running:
            return
            
        self.running = True
        self.thread = threading.Thread(target=self._listen, daemon=True)
        self.thread.start()
        print(f"🔍 发现服务监听在 UDP/{self.port}, WS端口: {self.ws_port}")
        
    def stop(self):
        """
        停止服务发现服务
        """
        self.running = False
        if self.thread and self.thread.is_alive():
            self.thread.join(timeout=1.0)
        self.socket.close()
        
    def _listen(self):
        """
        监听UDP广播请求
        """
        while self.running:
            try:
                data, addr = self.socket.recvfrom(1024)
                if self._validate_packet(data):
                    response = self._create_response(data)
                    self.socket.sendto(response, addr)
                    print(f"✅ 响应发现请求: {addr[0]}:{addr[1]}")
                else:
                    print(f"❌ 无效的发现请求: {addr[0]}:{addr[1]}")
            except Exception as e:
                if self.running:
                    print(f"❌ 发现服务错误: {e}")
    
    def _validate_packet(self, data: bytes) -> bool:
        """
        验证发现请求包(28字节)
        
        Args:
            data: 接收到的数据包
            
        Returns:
            bool: 是否是有效的请求包
        """
        # 包结构: [设备ID:16字节][随机数:4字节][时间戳:8字节]
        # 注意: HMAC签名不会在请求时发送，由客户端在验证响应时计算
        if len(data) != 28:
            return False
            
        timestamp = struct.unpack('>Q', data[20:28])[0]
        
        # 时间窗口验证 (±30秒)
        current_time = time.time()
        return abs(current_time - timestamp) <= 30
    
    def _create_response(self, request: bytes) -> bytes:
        """
        创建发现响应包(66字节)
        
        Args:
            request: 原始请求包(28字节)
            
        Returns:
            bytes: 响应数据包(66字节)
        """
        # 响应结构: [原始请求28字节][IP地址:4字节][WebSocket端口:2字节][HMAC:32字节]
        # 服务端必须证明知道密钥，客户端掌握最终验证权
        response = request[:28]
        
        # 添加当前服务器IP地址 (4字节)
        host_ip = socket.gethostbyname(socket.gethostname())
        ip_parts = [int(part) for part in host_ip.split('.')]
        response += struct.pack('>BBBB', *ip_parts)
        
        # 添加WebSocket端口 (2字节, big-endian)
        response += struct.pack('>H', self.ws_port)
        
        # 计算HMAC签名
        h = hmac.new(self.secret, digestmod=hashlib.sha256)
        h.update(request[:28])  # 设备ID + 随机数 + 时间戳
        h.update(response[28:34])  # IP + 端口
        response += h.digest()
        
        return response