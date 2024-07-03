use super::*;

use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use std::{collections::HashMap, path::Path};

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
    let (path_str, suffixes) = parse_glob(path)?;
    let suffixes = if suffixes.is_empty() {
        None
    } else {
        Some(&suffixes)
    };
    let mut file_paths = vec![];
    list_files(&mut file_paths, Path::new(&path_str), suffixes).await?;
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
    let extension = get_extension(path);
    match loaders.get(&extension) {
        Some(loader_command) => load_with_command(path, &extension, loader_command),
        None => load_plain(path, &extension).await,
    }
}

pub async fn load_url(
    loaders: &HashMap<String, String>,
    path: &str,
) -> Result<(String, RagMetadata)> {
    let (contents, extension) = fetch(loaders, path).await?;
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

pub fn parse_glob(path_str: &str) -> Result<(String, Vec<String>)> {
    if let Some(start) = path_str.find("/**/*.").or_else(|| path_str.find(r"\**\*.")) {
        let base_path = path_str[..start].to_string();
        if let Some(curly_brace_end) = path_str[start..].find('}') {
            let end = start + curly_brace_end;
            let extensions_str = &path_str[start + 6..end + 1];
            let extensions = if extensions_str.starts_with('{') && extensions_str.ends_with('}') {
                extensions_str[1..extensions_str.len() - 1]
                    .split(',')
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            } else {
                bail!("Invalid path '{path_str}'");
            };
            Ok((base_path, extensions))
        } else {
            let extensions_str = &path_str[start + 6..];
            let extensions = vec![extensions_str.to_string()];
            Ok((base_path, extensions))
        }
    } else if path_str.ends_with("/**") || path_str.ends_with(r"\**") {
        Ok((path_str[0..path_str.len() - 3].to_string(), vec![]))
    } else {
        Ok((path_str.to_string(), vec![]))
    }
}

#[async_recursion]
pub async fn list_files(
    files: &mut Vec<String>,
    entry_path: &Path,
    suffixes: Option<&Vec<String>>,
) -> Result<()> {
    if !entry_path.exists() {
        bail!("Not found: {:?}", entry_path);
    }
    if entry_path.is_file() {
        add_file(files, suffixes, entry_path);
        return Ok(());
    }
    if !entry_path.is_dir() {
        bail!("Not a directory: {:?}", entry_path);
    }
    let mut reader = tokio::fs::read_dir(entry_path).await?;
    while let Some(entry) = reader.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            add_file(files, suffixes, &path);
        } else if path.is_dir() {
            list_files(files, &path, suffixes).await?;
        }
    }
    Ok(())
}

fn add_file(files: &mut Vec<String>, suffixes: Option<&Vec<String>>, path: &Path) {
    if is_valid_extension(suffixes, path) {
        files.push(path.display().to_string());
    }
}

fn is_valid_extension(suffixes: Option<&Vec<String>>, path: &Path) -> bool {
    if let Some(suffixes) = suffixes {
        if !suffixes.is_empty() {
            if let Some(extension) = path.extension().map(|v| v.to_string_lossy().to_string()) {
                return suffixes.contains(&extension);
            }
            return false;
        }
    }
    true
}

fn get_extension(path: &str) -> String {
    Path::new(&path)
        .extension()
        .map(|v| v.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| DEFAULT_EXTENSION.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_glob() {
        assert_eq!(parse_glob("dir").unwrap(), ("dir".into(), vec![]));
        assert_eq!(parse_glob("dir/**").unwrap(), ("dir".into(), vec![]));
        assert_eq!(
            parse_glob("dir/file.md").unwrap(),
            ("dir/file.md".into(), vec![])
        );
        assert_eq!(
            parse_glob("dir/**/*.md").unwrap(),
            ("dir".into(), vec!["md".into()])
        );
        assert_eq!(
            parse_glob("dir/**/*.{md,txt}").unwrap(),
            ("dir".into(), vec!["md".into(), "txt".into()])
        );
        assert_eq!(
            parse_glob("C:\\dir\\**\\*.{md,txt}").unwrap(),
            ("C:\\dir".into(), vec!["md".into(), "txt".into()])
        );
    }
}
