use super::*;

use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use std::{collections::HashMap, fs::read_to_string, path::Path};

pub fn load_file(
    loaders: &HashMap<String, String>,
    path: &str,
    loader_name: &str,
) -> Result<Vec<RagDocument>> {
    match loaders.get(loader_name) {
        Some(loader_command) => load_with_command(path, loader_name, loader_command),
        None => load_plain(path),
    }
}

fn load_plain(path: &str) -> Result<Vec<RagDocument>> {
    let contents = read_to_string(path)?;
    let document = RagDocument::new(contents);
    Ok(vec![document])
}

fn load_with_command(
    path: &str,
    loader_name: &str,
    loader_command: &str,
) -> Result<Vec<RagDocument>> {
    let cmd_args = shell_words::split(loader_command)
        .with_context(|| anyhow!("Invalid rag loader '{loader_name}': `{loader_command}`"))?;
    let cmd_args: Vec<_> = cmd_args
        .into_iter()
        .map(|v| if v == "$1" { path.to_string() } else { v })
        .collect();
    let cmd_eval = shell_words::join(&cmd_args);
    let (cmd, args) = cmd_args.split_at(1);
    let cmd = &cmd[0];
    let (success, stdout, stderr) =
        run_command_with_output(cmd, args, None).with_context(|| {
            format!("Unable to run `{cmd_eval}`, Perhaps '{cmd}' is not installed?")
        })?;
    if !success {
        let err = if !stderr.is_empty() {
            stderr
        } else {
            format!("The command `{cmd_eval}` exited with non-zero.")
        };
        bail!("{err}")
    }
    let document = RagDocument::new(stdout);
    Ok(vec![document])
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
}
