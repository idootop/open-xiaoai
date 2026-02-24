# Open-XiaoAI x 小智 AI

[Open-XiaoAI](https://github.com/idootop/open-xiaoai) 的 Python 版 Server 端，用来演示小爱音箱接入[小智 AI](https://github.com/78/xiaozhi-esp32)。

> [!IMPORTANT]
> 本项目只是一个简单的演示程序，抛砖引玉。诸如一些音频压缩、加密传输、多账号管理等功能并未提供，建议只在局域网内测试运行，不推荐部署在公网服务器上（消耗流量 100kb/s），请自行评估相关风险，合理使用。

- 小爱音箱接入小智 AI
- 支持连续对话和中途打断
- 自定义唤醒词（中英文）和提示语
- 支持自定义消息处理，方便个人定制

## 快速开始

> [!NOTE]
> 继续下面的操作之前，你需要先在小爱音箱上启动运行 Rust 补丁程序 [👉 教程](../../packages/client-rust/README.md)

首先，克隆仓库代码到本地。

```shell
# 克隆代码
git clone https://github.com/idootop/open-xiaoai.git

# 进入当前项目根目录
cd examples/xiaozhi
```

然后把 `config.py` 文件里的配置修改成你自己的。

```typescript
APP_CONFIG = {
    "wakeup": {
        # 自定义唤醒词
        "keywords": [
            "豆包豆包",
            "你好小智",
            "hi siri",
        ],
    },
    "xiaozhi": {
        "OTA_URL": "https://api.tenclass.net/xiaozhi/ota/",
        "WEBSOCKET_URL": "wss://api.tenclass.net/xiaozhi/v1/",
    },
}
```

### Docker 运行

[![Docker Image Version](https://img.shields.io/docker/v/idootop/open-xiaoai-xiaozhi?color=%23086DCD&label=docker%20image)](https://hub.docker.com/r/idootop/open-xiaoai-xiaozhi)

推荐使用以下命令，直接 Docker 一键运行。

```shell
docker run -it --rm -p 4399:4399 -v $(pwd)/config.py:/app/config.py idootop/open-xiaoai-xiaozhi:latest
```

### 编译运行

为了能够正常编译运行该项目，你需要安装以下依赖环境/工具：

- uv：https://github.com/astral-sh/uv
- Rust: https://www.rust-lang.org/learn/get-started
- [Opus](https://opus-codec.org/): 自行询问 AI 如何安装动态链接库，或参考[这篇文章](https://github.com/huangjunsen0406/py-xiaozhi/blob/3bfd2887244c510a13912c1d63263ae564a941e9/documents/docs/guide/01_%E7%B3%BB%E7%BB%9F%E4%BE%9D%E8%B5%96%E5%AE%89%E8%A3%85.md#2-opus-%E9%9F%B3%E9%A2%91%E7%BC%96%E8%A7%A3%E7%A0%81%E5%99%A8)

```bash
# 安装 Python 依赖
uv sync --locked

# 编译运行（GUI 模式，不支持唤醒词唤醒）
uv run main.py

# 或者设置环境变量 CLI=true，开启 CLI 模式（支持自定义唤醒词）
CLI=true uv run main.py
```

如果你只是想体验一下小智 AI，请使用以下命令启动：

```bash
uv run main.py --mode xiaozhi
```

该模式下使用电脑的麦克风和扬声器作为音频输入输出设备，无需连接小爱音箱。

## 常见问题

### Q：回答太长了，如何打断小智 AI 的回答？

直接召唤“小爱同学”，即可打断小智 AI 的回答 ;)

### Q：第一次运行提示我输入验证码绑定设备，如何操作？

第一次启动对话时，会有语音提示使用验证码绑定设备。请打开你的小智 AI [管理后台](https://xiaozhi.me/)，然后根据提示创建 Agent 绑定设备即可。验证码消息会在终端打印，或者打开你的 `config.py` 文件查看。

```py
APP_CONFIG = {
    "xiaozhi": {
        "VERIFICATION_INFO": "首次登录时，验证码会在这里更新",
    },
    # ... 其他配置
}
```

PS：绑定设备成功后，可能需要重启应用才会生效。

### Q：怎样使用自己部署的 [xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server) 服务？

如果你想使用自己部署的 [xiaozhi-esp32-server](https://github.com/xinnan-tech/xiaozhi-esp32-server)，请更新 `config.py` 文件里的接口地址，然后重启应用。

```py
APP_CONFIG = {
    "xiaozhi": {
        "OTA_URL": "https://2662r3426b.vicp.fun/xiaozhi/ota/",
        "WEBSOCKET_URL": "wss://2662r3426b.vicp.fun/xiaozhi/v1/",
    },
    # ... 其他配置
}
```

### Q：有时候话还没说完 AI 就开始回答了，如何优化？

你可以调大 `config.py` 配置文件里的 `min_silence_duration` 参数，然后重启应用 / Docker 试试看。

```py
APP_CONFIG = {
    "vad": {
        # 最小静默时长（ms）
        "min_silence_duration": 1000,
    },
    # ... 其他配置
}
```

### Q：对话的时候，文字识别不是很准？

文字识别结果取决于你的小智 AI 服务器端的语音识别方案，与本项目无关。

### Q：唤醒词一直没有反应？

如果唤醒词还是不敏感，可以先调低 `vad.threshold`，然后重启应用 / Docker 试试看。

```py
APP_CONFIG = {
    "vad": {
        # 语音检测阈值（0-1，越小越灵敏）
        "threshold": 0.05,
    },
    # ... 其他配置
}
```

另外，应用 / Docker 刚刚启动时需要加载模型文件，比较耗时一些，可以等 30s 之后再试试看。

如果是英文唤醒词，可以尝试将最小发音用空格分开，比如：比如：'openai' 👉 'open ai'

PS：如果还是不行，建议更换其他更易识别的唤醒词。

### Q: 我想自己编译运行，模型文件在哪里下载？

由于 ASR 相关模型文件体积较大，并未直接提交在 git 仓库中，你可以在 release 中下载 [VAD + KWS 相关模型](https://github.com/idootop/open-xiaoai/releases/tag/vad-kws-models)，然后解压到 `xiaozhi/models` 路径下即可。

## 相关项目

- [oxa-server](https://github.com/pu-007/oxa-server): 提供了更强大易用的 config.py 的配置方式

## 鸣谢

该演示使用的 Python 版小智 AI 客户端基于 [py-xiaozhi](https://github.com/Huang-junsen/py-xiaozhi) 项目，特此鸣谢。
