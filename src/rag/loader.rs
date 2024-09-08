use super::*;

use anyhow::{Context, Result};
use std::collections::HashMap;

pub const EXTENSION_METADATA: &str = "__extension__";
pub const PATH_METADATA: &str = "__path__";

pub async fn load_document(
    loaders: &HashMap<String, String>,
    path: &str,
    has_error: &mut bool,
) -> (String, Vec<(String, RagMetadata)>) {
    let mut maybe_error = None;
    let mut files = vec![];
    if is_url(path) {
        if let Some(path) = path.strip_suffix("**") {
            match load_recursive_url(loaders, path).await {
                Ok(v) => files.extend(v),
                Err(err) => maybe_error = Some(err),
            }
        } else {
            match load_url(loaders, path).await {
                Ok(v) => files.push(v),
                Err(err) => maybe_error = Some(err),
            }
        }
    } else {
        match load_path(loaders, path, has_error).await {
            Ok(v) => files.extend(v),
            Err(err) => maybe_error = Some(err),
        }
    }
    if let Some(err) = maybe_error {
        *has_error = true;
        println!("{}", warning_text(&format!("⚠️ {err:?}")));
    }
    (path.to_string(), files)
}

pub async fn load_recursive_url(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<(String, RagMetadata)>> {
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
            let mut metadata: RagMetadata = Default::default();
            metadata.insert(PATH_METADATA.into(), path);
            metadata.insert(EXTENSION_METADATA.into(), "md".into());
            (text, metadata)
        })
        .collect();
    Ok(output)
}

pub async fn load_path(
    loaders: &HashMap<String, String>,
    path: &str,
    has_error: &mut bool,
) -> Result<Vec<(String, RagMetadata)>> {
    let path = Path::new(path).absolutize()?.display().to_string();
    let file_paths = expand_glob_paths(&[path]).await?;
    let mut output = vec![];
    let file_paths_len = file_paths.len();
    match file_paths_len {
        0 => {}
        1 => output.push(load_file(loaders, &file_paths[0]).await?),
        _ => {
            for path in file_paths {
                println!("Load {path}");
                match load_file(loaders, &path).await {
                    Ok(v) => output.push(v),
                    Err(err) => {
                        *has_error = true;
                        println!("{}", warning_text(&format!("Error: {err:?}")));
                    }
                }
            }
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
