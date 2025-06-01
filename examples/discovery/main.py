import socket
import struct
import time
import hmac
import hashlib
import threading
import signal
import sys
import argparse
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
        验证发现请求包
        
        Args:
            data: 接收到的数据包
            
        Returns:
            bool: 是否是有效的请求包
        """
        # 包结构: [设备ID:16字节][随机数:4字节][时间戳:8字节][HMAC:32字节]
        if len(data) != 60:
            return False
            
        device_id = data[:16]
        nonce = data[16:20]
        timestamp = struct.unpack('>Q', data[20:28])[0]
        received_hmac = data[28:60]
        
        # 时间窗口验证 (±30秒)
        current_time = time.time()
        if abs(current_time - timestamp) > 30:
            return False
            
        # 计算HMAC
        h = hmac.new(self.secret, digestmod=hashlib.sha256)
        h.update(device_id)
        h.update(nonce)
        h.update(struct.pack('>Q', timestamp))
        calculated_hmac = h.digest()
        
        # 比较HMAC
        return hmac.compare_digest(calculated_hmac, received_hmac)
    
    def _create_response(self, request: bytes) -> bytes:
        """
        创建发现响应包
        
        Args:
            request: 原始请求包
            
        Returns:
            bytes: 响应数据包
        """
        # 响应结构: [原始请求前32字节][IP地址:4字节][WebSocket端口:2字节]
        response = request[:32]
        
        # 添加当前服务器IP地址 (4字节)
        host_ip = socket.gethostbyname(socket.gethostname())
        ip_parts = [int(part) for part in host_ip.split('.')]
        print(ip_parts)
        response += struct.pack('>BBBB', *ip_parts)
        
        # 添加WebSocket端口 (2字节, big-endian)
        response += struct.pack('>H', self.ws_port)
        
        return response


# 全局发现服务实例
discovery_service = None


def parse_arguments():
    """
    解析命令行参数
    """
    parser = argparse.ArgumentParser(description="服务发现协议服务端")
    parser.add_argument("--port", type=int, default=5354, help="UDP监听端口 (默认: 5354)")
    parser.add_argument("--ws-port", type=int, default=8080, help="WebSocket服务端口 (默认: 8080)")
    parser.add_argument("--secret", type=str, default="your-secret-key", help="用于HMAC验证的密钥")
    return parser.parse_args()


def main():
    global discovery_service
    
    # 解析命令行参数
    args = parse_arguments()
    
    try:
        # 创建并启动发现服务
        discovery_service = DiscoveryService(secret=args.secret, port=args.port, ws_port=args.ws_port)
        discovery_service.start()
        
        print("✅ 服务发现服务已启动")
        print("按 Ctrl+C 停止服务")
        
        # 保持主线程运行
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            pass
        
        return 0
    except Exception as e:
        print(f"❌ 启动服务失败: {e}")
        return 1


def setup_graceful_shutdown():
    def signal_handler(_sig, _frame):
        print("\n🛑 正在停止服务...")
        if discovery_service:
            discovery_service.stop()
        sys.exit(0)
    
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)


if __name__ == "__main__":
    setup_graceful_shutdown()
    sys.exit(main())