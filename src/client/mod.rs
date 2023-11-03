#[macro_use]
mod common;
mod message;
mod model;

pub use common::*;
pub use message::*;
pub use model::*;

register_client!(
    (openai, "openai", OpenAI, OpenAIConfig, OpenAIClient),
    (localai, "localai", LocalAI, LocalAIConfig, LocalAIClient),
    (azure, "azure", Azure, AzureConfig, AzureClient),
);
