openai_compatible_client!(
    MoonshotConfig,
    MoonshotClient,
    "https://api.moonshot.cn/v1",
    [
        // https://platform.moonshot.cn/docs/intro#%E6%A8%A1%E5%9E%8B%E5%88%97%E8%A1%A8
        ("moonshot-v1-8k", "text", 8000),
        ("moonshot-v1-32k", "text", 32000),
        ("moonshot-v1-128k", "text", 128000),
    ]
);
