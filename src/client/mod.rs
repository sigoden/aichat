#[macro_use]
mod common;
mod message;
mod model;
mod prompt_format;
mod sse_handler;

pub use crate::utils::PromptKind;
pub use common::*;
pub use message::*;
pub use model::*;
pub use prompt_format::*;
pub use sse_handler::*;

register_client!(
    (openai, "openai", OpenAIConfig, OpenAIClient),
    (gemini, "gemini", GeminiConfig, GeminiClient),
    (claude, "claude", ClaudeConfig, ClaudeClient),
    (mistral, "mistral", MistralConfig, MistralClient),
    (cohere, "cohere", CohereConfig, CohereClient),
    (perplexity, "perplexity", PerplexityConfig, PerplexityClient),
    (groq, "groq", GroqConfig, GroqClient),
    (ollama, "ollama", OllamaConfig, OllamaClient),
    (
        azure_openai,
        "azure-openai",
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
    (vertexai, "vertexai", VertexAIConfig, VertexAIClient),
    (bedrock, "bedrock", BedrockConfig, BedrockClient),
    (cloudflare, "cloudflare", CloudflareConfig, CloudflareClient),
    (replicate, "replicate", ReplicateConfig, ReplicateClient),
    (ernie, "ernie", ErnieConfig, ErnieClient),
    (qianwen, "qianwen", QianwenConfig, QianwenClient),
    (moonshot, "moonshot", MoonshotConfig, MoonshotClient),
    (
        openai_compatible,
        "openai-compatible",
        OpenAICompatibleConfig,
        OpenAICompatibleClient
    ),
);

pub const KNOWN_OPENAI_COMPATIBLE_PLATFORMS: [(&str, &str); 5] = [
    ("anyscale", "https://api.endpoints.anyscale.com/v1"),
    ("deepinfra", "https://api.deepinfra.com/v1/openai"),
    ("fireworks", "https://api.fireworks.ai/inference/v1"),
    ("octoai", "https://text.octoai.run/v1"),
    ("together", "https://api.together.xyz/v1"),
];

pub const KNOWN_OPENAI_COMPATIBLE_PROMPTS: [PromptType<'static>; 3] = [
    ("api_key", "API Key:", false, PromptKind::String),
    ("models[].name", "Model Name:", true, PromptKind::String),
    (
        "models[].max_input_tokens",
        "Max Input Tokens:",
        false,
        PromptKind::Integer,
    ),
];
