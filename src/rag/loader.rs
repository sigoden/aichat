use super::*;

use anyhow::{Context, Result};
use std::collections::HashMap;

pub const EXTENSION_METADATA: &str = "__extension__";
pub const PATH_METADATA: &str = "__path__";

pub async fn load_recursive_url(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<(String, RagMetadata)>> {
    let extension = RECURSIVE_URL_LOADER;
    let loader_command = loaders
        .get(extension)
        .with_context(|| format!("Document loader '{extension}' not configured"))?;
    let contents = run_loader_command(path, extension, loader_command)?;
    let pages: Vec<WebPage> = serde_json::from_str(&contents).context(r#"The crawler response is invalid. It should follow the JSON format: `[{"path":"...", "text":"..."}]`."#)?;
    let output = pages
        .into_iter()
        .map(|v| {
            let WebPage { path, text } = v;
            let mut metadata: RagMetadata = Default::default();
            metadata.insert(PATH_METADATA.into(), path);
            metadata.insert(EXTENSION_METADATA.into(), "md".into());
            (text, metadata)
        })
        .collect();
    Ok(output)
}

#[derive(Debug, Deserialize)]
struct WebPage {
    path: String,
    text: String,
}

pub async fn load_path(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<(String, RagMetadata)>> {
    let file_paths = expand_glob_paths(&[path]).await?;
    let mut output = vec![];
    let file_paths_len = file_paths.len();
    match file_paths_len {
        0 => {}
        1 => output.push(load_file(loaders, &file_paths[0]).await?),
        _ => {
            for path in file_paths {
                println!("🚀 Loading file {path}");
                output.push(load_file(loaders, &path).await?)
            }
            println!("✨ Load directory completed");
        }
    }
    Ok(output)
}

pub async fn load_file(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<(String, RagMetadata)> {
    let extension = get_patch_extension(path).unwrap_or_else(|| DEFAULT_EXTENSION.into());
    match loaders.get(&extension) {
        Some(loader_command) => load_with_command(path, &extension, loader_command),
        None => load_plain(path, &extension).await,
    }
}

pub async fn load_url(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<(String, RagMetadata)> {
    let (contents, extension) = fetch(loaders, path, false).await?;
    let mut metadata: RagMetadata = Default::default();
    metadata.insert(PATH_METADATA.into(), path.into());
    metadata.insert(EXTENSION_METADATA.into(), extension);
    Ok((contents, metadata))
}

async fn load_plain(path: &str, extension: &str) -> Result<(String, RagMetadata)> {
    let contents = tokio::fs::read_to_string(path).await?;
    let mut metadata: RagMetadata = Default::default();
    metadata.insert(PATH_METADATA.into(), path.to_string());
    metadata.insert(EXTENSION_METADATA.into(), extension.to_string());
    Ok((contents, metadata))
}

fn load_with_command(
    path: &str,
    extension: &str,
    loader_command: &str,
) -> Result<(String, RagMetadata)> {
    let contents = run_loader_command(path, extension, loader_command)?;
    let mut metadata: RagMetadata = Default::default();
    metadata.insert(PATH_METADATA.into(), path.to_string());
    metadata.insert(EXTENSION_METADATA.into(), DEFAULT_EXTENSION.to_string());
    Ok((contents, metadata))
}
