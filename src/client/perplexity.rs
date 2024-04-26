openai_compatible_client!(
    PerplexityConfig,
    PerplexityClient,
    "https://api.perplexity.ai",
    [
        // https://docs.perplexity.ai/docs/model-cards
        ("sonar-small-chat", "text", 16384),
        ("sonar-small-online", "text", 12000),
        ("sonar-medium-chat", "text", 16384),
        ("sonar-medium-online", "text", 12000),

        ("llama-3-8b-instruct", "text", 8192),
        ("llama-3-70b-instruct", "text", 8192),
        ("codellama-70b-instruct", "text", 16384),
        ("mistral-7b-instruct", "text", 16384),
        ("mixtral-8x7b-instruct", "text", 16384),
        ("mixtral-8x22b-instruct", "text", 16384),
    ]
);
