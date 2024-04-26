openai_compatible_client!(
    MistralConfig,
    MistralClient,
    "https://api.mistral.ai/v1",
    [
        // https://docs.mistral.ai/platform/endpoints/
        ("open-mistral-7b", "text", 32000),
        ("open-mixtral-8x7b", "text", 32000),
        ("open-mixtral-8x22b", "text", 64000),
        ("mistral-small-latest", "text", 32000),
        ("mistral-medium-latest", "text", 32000),
        ("mistral-large-latest", "text", 32000),
    ]
);
