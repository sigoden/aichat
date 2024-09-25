use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};

pub fn safe_join_path<T1: AsRef<Path>, T2: AsRef<Path>>(
    base_path: T1,
    sub_path: T2,
) -> Option<PathBuf> {
    let base_path = base_path.as_ref();
    let sub_path = sub_path.as_ref();
    if sub_path.is_absolute() {
        return None;
    }

    let mut joined_path = PathBuf::from(base_path);

    for component in sub_path.components() {
        if Component::ParentDir == component {
            return None;
        }
        joined_path.push(component);
    }

    if joined_path.starts_with(base_path) {
        Some(joined_path)
    } else {
        None
    }
}

pub async fn expand_glob_paths<T: AsRef<str>>(paths: &[T]) -> Result<Vec<String>> {
    let mut new_paths = vec![];
    for path in paths {
        let (path_str, suffixes) = parse_glob(path.as_ref())?;
        let suffixes = if suffixes.is_empty() {
            None
        } else {
            Some(&suffixes)
        };
        list_files(&mut new_paths, Path::new(&path_str), suffixes).await?;
    }
    Ok(new_paths)
}

pub fn get_patch_extension(path: &str) -> Option<String> {
    Path::new(&path)
        .extension()
        .map(|v| v.to_string_lossy().to_lowercase())
}

fn parse_glob(path_str: &str) -> Result<(String, Vec<String>)> {
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

#[async_recursion::async_recursion]
async fn list_files(
    files: &mut Vec<String>,
    entry_path: &Path,
    suffixes: Option<&Vec<String>>,
) -> Result<()> {
    if !entry_path.exists() {
        bail!("Not found: {}", entry_path.display());
    }
    if entry_path.is_dir() {
        let mut reader = tokio::fs::read_dir(entry_path).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                list_files(files, &path, suffixes).await?;
            } else {
                add_file(files, suffixes, &path);
            }
        }
    } else {
        add_file(files, suffixes, entry_path);
    }
    Ok(())
}

fn add_file(files: &mut Vec<String>, suffixes: Option<&Vec<String>>, path: &Path) {
    if is_valid_extension(suffixes, path) {
        let path = path.display().to_string();
        if !files.contains(&path) {
            files.push(path);
        }
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
