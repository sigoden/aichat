openai_compatible_client!(
    GroqConfig,
    GroqClient,
    "https://api.groq.com/openai/v1",
    [
        // https://console.groq.com/docs/models
        ("llama3-8b-8192", "text", 8192),
        ("llama3-70b-8192", "text", 8192),
        ("llama2-70b-4096", "text", 4096),
        ("mixtral-8x7b-32768", "text", 32768),
        ("gemma-7b-it", "text", 8192),
    ]
);
