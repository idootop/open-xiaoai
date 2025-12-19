# Open-XiaoAI x Gemini Live API

小爱音箱接入[Gemini Live API](https://ai.google.dev/gemini-api/docs/live) 的演示程序，支持自动 VAD + 连续对话。

> [!IMPORTANT]
> 你需要先到 [Google AI Studio](https://aistudio.google.com) 注册账号并[创建 API 密钥](https://aistudio.google.com/apikey)。然后更新 `GEMINI_API_KEY` 环境变量，或 `gemini/gemini.py` 文件中的密钥。

## 环境准备

为了能够正常编译运行该项目，你需要安装以下依赖环境/工具：

- uv：https://github.com/astral-sh/uv
- Rust: https://www.rust-lang.org/learn/get-started

## 编译运行

> [!NOTE]
> 请先确认你已经将小爱音箱刷机成功，并安装运行了 Rust 补丁程序 [👉 教程](../../packages/client-rust/README.md)，否则该项目启动后收不到音频输入，将无法正常工作。

```bash
# 克隆代码
git clone https://github.com/idootop/open-xiaoai.git

# 进入当前项目根目录
cd examples/gemini

# 安装 Python 依赖
uv sync --locked

# 编译运行
uv run main.py
```

## 注意事项

该演示暂不支持中断 AI 的回复，需要等待 AI 回答完毕后才能重新响应用户的语音输入。

## Token 验证机制

### 启用 Token 验证

```shell
# 设置 OPEN_XIAOAI_TOKEN 环境变量
set OPEN_XIAOAI_TOKEN=my-token-value

# 运行程序
uv run main.py
```

### 兼容模式

当 `OPEN_XIAOAI_TOKEN` 环境变量为空时，系统将自动启用兼容模式，保持与现有版本的通信方式。

> [!NOTE]
> 本项目只是一个简单的演示程序，抛砖引玉。如果你想要更多的功能，可以借助 Python 丰富的 AI 生态自行实现。
