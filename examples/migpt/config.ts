import { sleep } from "@mi-gpt/utils";
import { OpenXiaoAIConfig } from "./migpt/xiaoai.js";

export const kOpenXiaoAIConfig: OpenXiaoAIConfig = {
  openai: {
    /**
     * 你的大模型服务提供商的接口地址
     *
     * 支持兼容 OpenAI 接口的大模型服务，比如：DeepSeek V3 等
     *
     * 注意：一般以 /v1 结尾，不包含 /chat/completions 部分
     * - ✅ https://api.openai.com/v1
     * - ❌ https://api.openai.com/v1/（最后多了一个 /
     * - ❌ https://api.openai.com/v1/chat/completions（不需要加 /chat/completions）
     */
    baseURL: "https://api.openai.com/v1",
    /**
     * API 密钥
     */
    apiKey: "sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    /**
     * 模型名称
     */
    model: "gpt-4.1-mini",
  },
  server: {
    /**
     * 服务器端token配置，如果为空则不启用token验证
     */
    token: "",
  },
  prompt: {
    /**
     * 系统提示词，如需关闭可设置为：''（空字符串）
     */
    system: "你是一个智能助手，请根据用户的问题给出回答。",
  },
  context: {
    /**
     * 每次对话携带的最大历史消息数（如需关闭可设置为：0）
     */
    historyMaxLength: 10,
  },
  /**
   * 只回答以下关键词开头的消息：
   *
   * - 请问地球为什么是圆的？
   * - 你知道世界上跑的最快的动物是什么吗？
   */
  callAIKeywords: ["请", "你"],
  /**
   * 自定义消息回复
   */
  async onMessage(engine, { text }) {
    if (text === "测试播放文字") {
      return { text: "你好，很高兴认识你！" };
    }

    if (text === "测试播放音乐") {
      return { url: "https://example.com/hello.mp3" };
    }

    if (text === "测试其他能力") {
      // 打断原来小爱的回复
      await engine.speaker.abortXiaoAI();

      // 播放文字
      await sleep(2000); // 打断小爱后需要等待 2 秒，使其恢复运行后才能继续 TTS
      await engine.speaker.play({ text: "你好，很高兴认识你！", blocking: true });

      // 播放音频链接
      await engine.speaker.play({ url: "https://example.com/hello.mp3" });

      // 告诉 MiGPT 已经处理过这条消息了，不再使用默认的 AI 回复
      return { handled: true };
    }
  },
};
