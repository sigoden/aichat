use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};
use indexmap::IndexSet;
use path_absolutize::Absolutize;

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

pub async fn expand_glob_paths<T: AsRef<str>>(
    paths: &[T],
    bail_non_exist: bool,
) -> Result<IndexSet<String>> {
    let mut new_paths = IndexSet::new();
    for path in paths {
        let (path_str, suffixes) = parse_glob(path.as_ref())?;
        let suffixes = if suffixes.is_empty() {
            None
        } else {
            Some(&suffixes)
        };
        list_files(
            &mut new_paths,
            Path::new(&path_str),
            suffixes,
            bail_non_exist,
        )
        .await?;
    }
    Ok(new_paths)
}

pub fn list_file_names<T: AsRef<Path>>(dir: T, ext: &str) -> Vec<String> {
    match std::fs::read_dir(dir.as_ref()) {
        Ok(rd) => {
            let mut names = vec![];
            for entry in rd.flatten() {
                let name = entry.file_name();
                if let Some(name) = name.to_string_lossy().strip_suffix(ext) {
                    names.push(name.to_string());
                }
            }
            names.sort_unstable();
            names
        }
        Err(_) => vec![],
    }
}

pub fn get_patch_extension(path: &str) -> Option<String> {
    Path::new(&path)
        .extension()
        .map(|v| v.to_string_lossy().to_lowercase())
}

pub fn to_absolute_path(path: &str) -> Result<String> {
    Ok(Path::new(&path).absolutize()?.display().to_string())
}

pub fn resolve_home_dir(path: &str) -> String {
    let mut path = path.to_string();
    if path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home_dir) = dirs::home_dir() {
            path.replace_range(..1, &home_dir.display().to_string());
        }
    }
    path
}

fn parse_glob(path_str: &str) -> Result<(String, Vec<String>)> {
    // Handle recursive glob patterns with extensions
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
    }
    // Handle recursive directory search without extension filtering
    else if path_str.ends_with("/**") || path_str.ends_with(r"\**") {
        Ok((path_str[0..path_str.len() - 3].to_string(), vec![]))
    }
    // Handle basic glob patterns like "*.md" or "dir/*.md"
    else if let Some(start) = path_str.rfind("/*.").or_else(|| path_str.rfind(r"\*.")) {
        let base_path = path_str[..start].to_string();
        let extensions_str = &path_str[start + 3..];
        let extensions = vec![extensions_str.to_string()];
        Ok((base_path, extensions))
    }
    // Handle root-level glob patterns like "*.md"
    else if path_str.starts_with("*.") {
        Ok(("".to_string(), vec![path_str[2..].to_string()]))
    }
    // No glob pattern, treat as normal path
    else {
        Ok((path_str.to_string(), vec![]))
    }
}

#[async_recursion::async_recursion]
async fn list_files(
    files: &mut IndexSet<String>,
    entry_path: &Path,
    suffixes: Option<&Vec<String>>,
    bail_non_exist: bool,
) -> Result<()> {
    if !entry_path.exists() {
        if bail_non_exist {
            bail!("Not found '{}'", entry_path.display());
        } else {
            return Ok(());
        }
    }
    if entry_path.is_dir() {
        let mut reader = tokio::fs::read_dir(entry_path).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                list_files(files, &path, suffixes, bail_non_exist).await?;
            } else {
                add_file(files, suffixes, &path);
            }
        }
    } else {
        add_file(files, suffixes, entry_path);
    }
    Ok(())
}

fn add_file(files: &mut IndexSet<String>, suffixes: Option<&Vec<String>>, path: &Path) {
    if is_valid_extension(suffixes, path) {
        let path = path.display().to_string();
        if !files.contains(&path) {
            files.insert(path);
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
        // Test recursive glob with multiple extensions
        let (path, exts) = parse_glob("dir/**/*.{md,txt}").unwrap();
        assert_eq!(path, "dir");
        assert_eq!(exts, vec!["md", "txt"]);

        // Test recursive glob with single extension
        let (path, exts) = parse_glob("dir/**/*.md").unwrap();
        assert_eq!(path, "dir");
        assert_eq!(exts, vec!["md"]);

        // Test recursive directory without extension
        let (path, exts) = parse_glob("dir/**").unwrap();
        assert_eq!(path, "dir");
        assert!(exts.is_empty());

        // Test basic glob with extension in root
        let (path, exts) = parse_glob("*.md").unwrap();
        assert_eq!(path, "");
        assert_eq!(exts, vec!["md"]);

        // Test basic glob with extension in directory
        let (path, exts) = parse_glob("dir/*.md").unwrap();
        assert_eq!(path, "dir");
        assert_eq!(exts, vec!["md"]);

        // Test Windows-style paths
        let (path, exts) = parse_glob(r"dir\*.md").unwrap();
        assert_eq!(path, "dir");
        assert_eq!(exts, vec!["md"]);

        // Test no glob pattern
        let (path, exts) = parse_glob("dir/file.md").unwrap();
        assert_eq!(path, "dir/file.md");
        assert!(exts.is_empty());
    }
}
