import signal
import sys

from xiaozhi.xiaoai import XiaoAI
from xiaozhi.xiaozhi import XiaoZhi
from discovery_protocol import DiscoveryService


def main():
    XiaoZhi.instance().run()
    return 0


def setup_graceful_shutdown():
    def signal_handler(_sig, _frame):
        XiaoZhi.instance().shutdown()
        sys.exit(0)

    signal.signal(signal.SIGINT, signal_handler)


if __name__ == "__main__":
    DiscoveryService(secret="your-secret-key", port=5354,ws_port=4399).start()
    XiaoAI.setup_mode()
    setup_graceful_shutdown()
    sys.exit(main())
