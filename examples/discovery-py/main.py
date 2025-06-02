import signal
import sys
import argparse
import time
from discovery_protocol import DiscoveryService


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