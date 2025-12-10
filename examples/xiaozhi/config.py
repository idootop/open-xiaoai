import asyncio


async def before_wakeup(speaker, text, source):
    """
    处理收到的用户消息，并决定是否唤醒小智 AI

    - source: 唤醒来源
        - 'kws': 关键字唤醒
        - 'xiaoai': 小爱同学收到用户指令
    """
    if source == "kws":
        # 播放唤醒提示语
        await speaker.play(text="你好主人，我是小智，请问有什么吩咐？")
        # 返回 True 唤醒小智 AI
        return True

    if source == "xiaoai" and text == "召唤小智":
        # 打断原来的小爱同学
        await speaker.abort_xiaoai()
        # 等待 2 秒，让小爱 TTS 恢复可用
        await asyncio.sleep(2)
        # 播放唤醒提示语（如果你不使用自带的小爱 TTS，可以去掉上面的延时）
        await speaker.play(text="小智来了，主人有什么吩咐？")
        # 唤醒小智 AI
        return True


async def after_wakeup(speaker):
    """
    退出唤醒状态
    """
    await speaker.play(text="主人再见，拜拜")


APP_CONFIG = {
    "server": {
        # 服务器端token配置，如果为空则不启用token验证
        "token": "",
    },
    "wakeup": {
        # 自定义唤醒词列表（英文字母要全小写）
        "keywords": [
            "天猫精灵",
            "小度小度",
            "豆包豆包",
            "你好小智",
            "你好小爱",
            "hi siri",
            "hey siri",
        ],
        # 静音多久后自动退出唤醒（秒）
        "timeout": 20,
        # 语音识别结果回调
        "before_wakeup": before_wakeup,
        # 退出唤醒时的提示语（设置为空可关闭）
        "after_wakeup": after_wakeup,
    },
    "vad": {
        # 录音音量增强倍数（小爱音箱录音音量较小，需要后期放大一下）
        "boost": 10,
        # 语音检测阈值（0-1，越小越灵敏）
        "threshold": 0.10,
        # 最小语音时长（ms）
        "min_speech_duration": 250,
        # 最小静默时长（ms）
        "min_silence_duration": 500,
    },
    "xiaozhi": {
        "OTA_URL": "https://api.tenclass.net/xiaozhi/ota/",
        "WEBSOCKET_URL": "wss://api.tenclass.net/xiaozhi/v1/",
        "WEBSOCKET_ACCESS_TOKEN": "", #（可选）一般用不到这个值
        "DEVICE_ID": "", #（可选）默认自动生成
        "VERIFICATION_CODE": "", # 首次登陆时，验证码会在这里更新
    },
}
