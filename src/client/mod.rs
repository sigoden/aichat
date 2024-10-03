mod access_token;
mod common;
mod message;
#[macro_use]
mod macros;
mod model;
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
    (gemini, "gemini", GeminiConfig, GeminiClient),
    (claude, "claude", ClaudeConfig, ClaudeClient),
    (cohere, "cohere", CohereConfig, CohereClient),
    (
        azure_openai,
        "azure-openai",
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
    (vertexai, "vertexai", VertexAIConfig, VertexAIClient),
    (bedrock, "bedrock", BedrockConfig, BedrockClient),
    (ernie, "ernie", ErnieConfig, ErnieClient),
);

pub const OPENAI_COMPATIBLE_PLATFORMS: [(&str, &str); 20] = [
    ("ai21", "https://api.ai21.com/studio/v1"),
    ("cloudflare", ""),
    ("deepinfra", "https://api.deepinfra.com/v1/openai"),
    ("deepseek", "https://api.deepseek.com"),
    ("fireworks", "https://api.fireworks.ai/inference/v1"),
    ("github", "https://models.inference.ai.azure.com"),
    ("groq", "https://api.groq.com/openai/v1"),
    ("huggingface", "https://api-inference.huggingface.co/v1"),
    ("lingyiwanwu", "https://api.lingyiwanwu.com/v1"),
    ("mistral", "https://api.mistral.ai/v1"),
    ("moonshot", "https://api.moonshot.cn/v1"),
    ("openrouter", "https://openrouter.ai/api/v1"),
    ("ollama", ""),
    ("perplexity", "https://api.perplexity.ai"),
    (
        "qianwen",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    ),
    ("siliconflow", "https://api.siliconflow.cn/v1"),
    ("together", "https://api.together.xyz/v1"),
    ("zhipuai", "https://open.bigmodel.cn/api/paas/v4"),
    // RAG-dedicated
    ("jina", "https://api.jina.ai/v1"),
    ("voyageai", "https://api.voyageai.com/v1"),
];
