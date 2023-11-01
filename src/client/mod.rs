#[macro_use]
mod common;

pub mod azure_openai;
pub mod localai;
pub mod openai;

pub use common::*;

use self::azure_openai::AzureOpenAIConfig;
use self::localai::LocalAIConfig;
use self::openai::OpenAIConfig;

use crate::{
    config::{Config, ModelInfo, SharedConfig},
    utils::PromptKind,
};

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::Value;

register_role!(
    ("openai", OpenAI, OpenAIConfig, OpenAIClient),
    ("localai", LocalAI, LocalAIConfig, LocalAIClient),
    (
        "azure-openai",
        AzureOpenAI,
        AzureOpenAIConfig,
        AzureOpenAIClient
    ),
);
