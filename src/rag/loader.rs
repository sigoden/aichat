use super::*;

use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use serde_json::Value;
use std::{collections::HashMap, path::Path};

pub const EXTENSION_METADATA: &str = "__extension__";

pub async fn load(
    loaders: &HashMap<String, String>,
    path: &str,
    extension: &str,
) -> Result<Vec<RagDocument>> {
    if extension == RECURSIVE_URL_LOADER {
        let loader_command = loaders
            .get(extension)
            .with_context(|| format!("RAG document loader '{extension}' not configured"))?;
        let contents = run_loader_command(path, extension, loader_command)?;
        let output = match parse_json_documents(&contents) {
            Some(v) => v,
            None => vec![RagDocument::new(contents)],
        };
        Ok(output)
    } else if extension == URL_LOADER {
        let (contents, extension) = fetch(loaders, path).await?;
        let mut metadata: RagMetadata = Default::default();
        metadata.insert("path".into(), path.into());
        metadata.insert(EXTENSION_METADATA.into(), extension);
        Ok(vec![RagDocument::new(contents).with_metadata(metadata)])
    } else {
        match loaders.get(extension) {
            Some(loader_command) => load_with_command(path, extension, loader_command),
            None => load_plain(path, extension).await,
        }
    }
}

async fn load_plain(path: &str, extension: &str) -> Result<Vec<RagDocument>> {
    let contents = tokio::fs::read_to_string(path).await?;
    if extension == "json" {
        if let Some(documents) = parse_json_documents(&contents) {
            return Ok(documents);
        }
    }
    let mut document = RagDocument::new(contents);
    document.metadata.insert("path".into(), path.to_string());
    Ok(vec![document])
}

fn load_with_command(
    path: &str,
    extension: &str,
    loader_command: &str,
) -> Result<Vec<RagDocument>> {
    let contents = run_loader_command(path, extension, loader_command)?;
    let mut document = RagDocument::new(contents);
    document.metadata.insert("path".into(), path.to_string());
    document
        .metadata
        .insert(EXTENSION_METADATA.into(), DEFAULT_EXTENSION.to_string());
    Ok(vec![document])
}

fn parse_json_documents(data: &str) -> Option<Vec<RagDocument>> {
    let value: Value = serde_json::from_str(data).ok()?;
    let items = match value {
        Value::Array(v) => v,
        _ => return None,
    };
    if items.is_empty() {
        return None;
    }
    match &items[0] {
        Value::String(_) => {
            let documents: Vec<_> = items
                .into_iter()
                .flat_map(|item| {
                    if let Value::String(content) = item {
                        Some(RagDocument::new(content))
                    } else {
                        None
                    }
                })
                .collect();
            Some(documents)
        }
        Value::Object(obj) => {
            let key = [
                "page_content",
                "pageContent",
                "content",
                "html",
                "markdown",
                "text",
            ]
            .into_iter()
            .map(|v| v.to_string())
            .find(|key| obj.get(key).and_then(|v| v.as_str()).is_some())?;
            let documents: Vec<_> = items
                .into_iter()
                .flat_map(|item| {
                    if let Value::Object(mut obj) = item {
                        if let Some(page_content) = obj.get(&key).and_then(|v| v.as_str()) {
                            let page_content = page_content.to_string();
                            obj.remove(&key);
                            let mut metadata: IndexMap<_, _> = obj
                                .into_iter()
                                .map(|(k, v)| {
                                    if let Value::String(v) = v {
                                        (k, v)
                                    } else {
                                        (k, v.to_string())
                                    }
                                })
                                .collect();
                            if key == "markdown" {
                                metadata.insert(EXTENSION_METADATA.into(), "md".into());
                            } else if key == "html" {
                                metadata.insert(EXTENSION_METADATA.into(), "html".into());
                            }
                            return Some(RagDocument {
                                page_content,
                                metadata,
                            });
                        }
                    }
                    None
                })
                .collect();
            if documents.is_empty() {
                None
            } else {
                Some(documents)
            }
        }
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_glob() {
        assert_eq!(parse_glob("dir").unwrap(), ("dir".into(), vec![]));
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

    #[test]
    fn test_parse_json_documents() {
        let data = r#"["foo", "bar"]"#;
        assert_eq!(
            parse_json_documents(data).unwrap(),
            vec![RagDocument::new("foo"), RagDocument::new("bar")]
        );

        let data = r#"[{"content": "foo"}, {"content": "bar"}]"#;
        assert_eq!(
            parse_json_documents(data).unwrap(),
            vec![RagDocument::new("foo"), RagDocument::new("bar")]
        );

        let mut metadata = IndexMap::new();
        metadata.insert("k1".into(), "1".into());
        let data = r#"[{"k1": 1, "text": "foo" }]"#;
        assert_eq!(
            parse_json_documents(data).unwrap(),
            vec![RagDocument::new("foo").with_metadata(metadata.clone())]
        );

        let data = r#""hello""#;
        assert!(parse_json_documents(data).is_none());

        let data = r#"{"key":"value"}"#;
        assert!(parse_json_documents(data).is_none());

        let data = r#"[{"key":"value"}]"#;
        assert!(parse_json_documents(data).is_none());
    }
}
