use super::*;

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const EXTENSION_METADATA: &str = "__extension__";

pub type DocumentMetadata = IndexMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedDocument {
    pub path: String,
    pub contents: String,
    #[serde(default)]
    pub metadata: DocumentMetadata,
}

impl LoadedDocument {
    pub fn new(path: String, contents: String, metadata: DocumentMetadata) -> Self {
        Self {
            path,
            contents,
            metadata,
        }
    }
}

pub async fn load_recursive_url(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<LoadedDocument>> {
    let extension = RECURSIVE_URL_LOADER;
    let pages: Vec<Page> = match loaders.get(extension) {
        Some(loader_command) => {
            let contents = run_loader_command(path, extension, loader_command)?;
            serde_json::from_str(&contents).context(r#"The crawler response is invalid. It should follow the JSON format: `[{"path":"...", "text":"..."}]`."#)?
        }
        None => {
            let options = CrawlOptions::preset(path);
            crawl_website(path, options).await?
        }
    };
    let output = pages
        .into_iter()
        .map(|v| {
            let Page { path, text } = v;
            let mut metadata: DocumentMetadata = Default::default();
            metadata.insert(EXTENSION_METADATA.into(), "md".into());
            LoadedDocument::new(path, text, metadata)
        })
        .collect();
    Ok(output)
}

pub async fn load_file(loaders: &HashMap<String, String>, path: &str) -> Result<LoadedDocument> {
    let extension = get_patch_extension(path).unwrap_or_else(|| DEFAULT_EXTENSION.into());
    match loaders.get(&extension) {
        Some(loader_command) => load_with_command(path, &extension, loader_command),
        None => load_plain(path, &extension).await,
    }
}

pub async fn load_url(loaders: &HashMap<String, String>, path: &str) -> Result<LoadedDocument> {
    let (contents, extension) = fetch_with_loaders(loaders, path, false).await?;
    let mut metadata: DocumentMetadata = Default::default();
    metadata.insert(EXTENSION_METADATA.into(), extension);
    Ok(LoadedDocument::new(path.into(), contents, metadata))
}

async fn load_plain(path: &str, extension: &str) -> Result<LoadedDocument> {
    let contents = tokio::fs::read_to_string(path).await?;
    let mut metadata: DocumentMetadata = Default::default();
    metadata.insert(EXTENSION_METADATA.into(), extension.to_string());
    Ok(LoadedDocument::new(path.into(), contents, metadata))
}

fn load_with_command(path: &str, extension: &str, loader_command: &str) -> Result<LoadedDocument> {
    let contents = run_loader_command(path, extension, loader_command)?;
    let mut metadata: DocumentMetadata = Default::default();
    metadata.insert(EXTENSION_METADATA.into(), DEFAULT_EXTENSION.to_string());
    Ok(LoadedDocument::new(path.into(), contents, metadata))
}

pub fn is_loader_protocol(loaders: &HashMap<String, String>, path: &str) -> bool {
    match path.split_once(':') {
        Some((protocol, _)) => loaders.contains_key(protocol),
        None => false,
    }
}

pub fn load_protocol_path(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<LoadedDocument>> {
    let (protocol, loader_command, new_path) = path
        .split_once(':')
        .and_then(|(protocol, path)| {
            let loader_command = loaders.get(protocol)?;
            Some((protocol, loader_command, path))
        })
        .ok_or_else(|| anyhow!("No document loader for '{}'", path))?;
    let contents = run_loader_command(new_path, protocol, loader_command)?;
    let output = if let Ok(list) = serde_json::from_str::<Vec<LoadedDocument>>(&contents) {
        list.into_iter()
            .map(|mut v| {
                if v.path.starts_with(path) {
                } else if v.path.starts_with(new_path) {
                    v.path = format!("{}:{}", protocol, v.path);
                } else {
                    v.path = format!("{}/{}", path, v.path);
                }
                v
            })
            .collect()
    } else {
        vec![LoadedDocument::new(
            path.into(),
            contents,
            Default::default(),
        )]
    };
    Ok(output)
}
