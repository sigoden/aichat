#[macro_use]
mod common;
mod message;
mod model_info;

pub use common::*;
pub use message::*;
pub use model_info::*;

register_client!(
    (openai, "openai", OpenAI, OpenAIConfig, OpenAIClient),
    (localai, "localai", LocalAI, LocalAIConfig, LocalAIClient),
    (
        azure_openai,
        "azure-openai",
        AzureOpenAI,
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
);
