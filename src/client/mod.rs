#[macro_use]
mod common;
mod access_token;
mod message;
mod model;
mod prompt_format;
mod stream;

pub use crate::function::{ToolCall, ToolResults};
pub use crate::utils::PromptKind;
pub use common::*;
pub use message::*;
pub use model::*;
pub use stream::*;

register_client!(
    (openai, "openai", OpenAIConfig, OpenAIClient),
    (
        openai_compatible,
        "openai-compatible",
        OpenAICompatibleConfig,
        OpenAICompatibleClient
    ),
    (
        rag_dedicated,
        "rag-dedicated",
        RagDedicatedConfig,
        RagDedicatedClient
    ),
    (gemini, "gemini", GeminiConfig, GeminiClient),
    (claude, "claude", ClaudeConfig, ClaudeClient),
    (cohere, "cohere", CohereConfig, CohereClient),
    (ollama, "ollama", OllamaConfig, OllamaClient),
    (
        azure_openai,
        "azure-openai",
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
    (vertexai, "vertexai", VertexAIConfig, VertexAIClient),
    (
        vertexai_claude,
        "vertexai-claude",
        VertexAIClaudeConfig,
        VertexAIClaudeClient
    ),
    (bedrock, "bedrock", BedrockConfig, BedrockClient),
    (cloudflare, "cloudflare", CloudflareConfig, CloudflareClient),
    (replicate, "replicate", ReplicateConfig, ReplicateClient),
    (ernie, "ernie", ErnieConfig, ErnieClient),
    (qianwen, "qianwen", QianwenConfig, QianwenClient),
);

pub const OPENAI_COMPATIBLE_PLATFORMS: [(&str, &str); 13] = [
    ("anyscale", "https://api.endpoints.anyscale.com/v1"),
    ("deepinfra", "https://api.deepinfra.com/v1/openai"),
    ("deepseek", "https://api.deepseek.com"),
    ("fireworks", "https://api.fireworks.ai/inference/v1"),
    ("groq", "https://api.groq.com/openai/v1"),
    ("mistral", "https://api.mistral.ai/v1"),
    ("moonshot", "https://api.moonshot.cn/v1"),
    ("openrouter", "https://openrouter.ai/api/v1"),
    ("octoai", "https://text.octoai.run/v1"),
    ("perplexity", "https://api.perplexity.ai"),
    ("together", "https://api.together.xyz/v1"),
    ("zhipuai", "https://open.bigmodel.cn/api/paas/v4"),
    ("lingyiwanwu", "https://api.lingyiwanwu.com/v1"),
];

pub const RAG_DEDICATED_PLATFORMS: [(&str, &str); 2] = [
    ("jina", "https://api.jina.ai/v1"),
    ("voyageai", "https://api.voyageai.com/v1"),
];
