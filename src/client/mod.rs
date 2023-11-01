#[macro_use]
mod common;

pub use common::*;

use crate::{
    config::{ModelInfo, TokensCountFactors},
    repl::ReplyStreamHandler,
    utils::PromptKind,
};

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
