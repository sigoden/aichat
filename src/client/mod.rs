#[macro_use]
mod common;
mod message;
mod model;
mod reply_handler;

pub use common::*;
pub use message::*;
pub use model::*;
pub use reply_handler::*;

register_client!(
    (openai, "openai", OpenAIConfig, OpenAIClient),
    (
        azure_openai,
        "azure-openai",
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
    (
        openai_compatible,
        "openai-compatible",
        OpenAICompatibleConfig,
        OpenAICompatibleClient
    ),
    (gemini, "gemini", GeminiConfig, GeminiClient),
    (vertexai, "vertexai", VertexAIConfig, VertexAIClient),
    (claude, "claude", ClaudeConfig, ClaudeClient),
    (mistral, "mistral", MistralConfig, MistralClient),
    (cohere, "cohere", CohereConfig, CohereClient),
    (perplexity, "perplexity", PerplexityConfig, PerplexityClient),
    (groq, "groq", GroqConfig, GroqClient),
    (ollama, "ollama", OllamaConfig, OllamaClient),
    (ernie, "ernie", ErnieConfig, ErnieClient),
    (qianwen, "qianwen", QianwenConfig, QianwenClient),
    (moonshot, "moonshot", MoonshotConfig, MoonshotClient),
);
