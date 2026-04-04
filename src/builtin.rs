use crate::function::FunctionDeclaration;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub fn declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration {
            name: "fs_cat".to_string(),
            description: "Read the contents of a file.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to read"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_ls".to_string(),
            description: "List files in a directory.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the directory to list (defaults to current directory)"
                    }
                }
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_mkdir".to_string(),
            description: "Create a directory.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the directory to create"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_write".to_string(),
            description: "Write content to a file.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to write"
                    },
                    "contents": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "contents"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_search".to_string(),
            description: "Search for text in files (substring search).".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the directory to search in"
                    },
                    "text": {
                        "type": "string",
                        "description": "The text to search for"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "The file pattern to filter by (substring match on filename)"
                    }
                },
                "required": ["path", "text"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_stat".to_string(),
            description: "Get metadata for a file or directory.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file or directory"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_file_exists".to_string(),
            description: "Check if a file or directory exists.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to check"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_is_dir".to_string(),
            description: "Check if a path is a directory.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to check"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_is_file".to_string(),
            description: "Check if a path is a file.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to check"
                    }
                },
                "required": ["path"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "fs_patch".to_string(),
            description: "Patch a file by replacing a search block with a replace block.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to patch"
                    },
                    "search": {
                        "type": "string",
                        "description": "The block of text to search for"
                    },
                    "replace": {
                        "type": "string",
                        "description": "The block of text to replace it with"
                    }
                },
                "required": ["path", "search", "replace"]
            }))
            .unwrap(),
            agent: false,
        },
        FunctionDeclaration {
            name: "command_run".to_string(),
            description: "Run a shell command.".to_string(),
            parameters: serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to run"
                    }
                },
                "required": ["command"]
            }))
            .unwrap(),
            agent: false,
        },
    ]
}

pub fn run(name: &str, args: &Value) -> Result<Option<Value>> {
    match name {
        "fs_cat" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let content = fs::read_to_string(path)?;
            Ok(Some(json!({ "content": content })))
        }
        "fs_ls" => {
            let path = args["path"].as_str().unwrap_or(".");
            let mut files = vec![];
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let file_name = entry.file_name().to_string_lossy().to_string();
                let file_type = if entry.file_type()?.is_dir() { "dir" } else { "file" };
                files.push(format!("{} ({})", file_name, file_type));
            }
            Ok(Some(json!({ "files": files })))
        }
        "fs_mkdir" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            fs::create_dir_all(path)?;
            Ok(Some(json!({ "success": true })))
        }
        "fs_write" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let contents = args["contents"].as_str().ok_or_else(|| anyhow!("Missing contents"))?;
            fs::write(path, contents)?;
            Ok(Some(json!({ "success": true })))
        }
        "fs_search" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let text = args["text"].as_str().ok_or_else(|| anyhow!("Missing text"))?;
            let file_pattern = args["file_pattern"].as_str();

            let mut results = vec![];
            visit_dirs(Path::new(path), text, file_pattern, &mut results)?;
            Ok(Some(json!({ "results": results })))
        }
        "fs_stat" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            if let Ok(metadata) = fs::metadata(path) {
                let is_dir = metadata.is_dir();
                let is_file = metadata.is_file();
                let size = metadata.len();
                let modified = metadata.modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or(std::time::Duration::from_secs(0))
                    .as_secs();
                Ok(Some(json!({ "exists": true, "is_dir": is_dir, "is_file": is_file, "size": size, "modified": modified })))
            } else {
                Ok(Some(json!({ "exists": false })))
            }
        }
        "fs_file_exists" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let exists = Path::new(path).exists();
            Ok(Some(json!({ "exists": exists })))
        }
        "fs_is_dir" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let is_dir = Path::new(path).is_dir();
            Ok(Some(json!({ "is_dir": is_dir })))
        }
        "fs_is_file" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let is_file = Path::new(path).is_file();
            Ok(Some(json!({ "is_file": is_file })))
        }
        "fs_patch" => {
            let path = args["path"].as_str().ok_or_else(|| anyhow!("Missing path"))?;
            let search = args["search"].as_str().ok_or_else(|| anyhow!("Missing search"))?;
            let replace = args["replace"].as_str().ok_or_else(|| anyhow!("Missing replace"))?;
            let content = fs::read_to_string(path)?;
            if !content.contains(search) {
                return Ok(Some(json!({ "error": "Search string not found in file" })));
            }
            let new_content = content.replacen(search, replace, 1);
            fs::write(path, new_content)?;
            Ok(Some(json!({ "success": true })))
        }
        "command_run" => {
            let command = args["command"].as_str().ok_or_else(|| anyhow!("Missing command"))?;
            let args = shell_words::split(command).map_err(|e| anyhow!("Invalid command: {}", e))?;
            let (cmd, args) = args
                .split_first()
                .ok_or_else(|| anyhow!("Missing command"))?;
            let output = std::process::Command::new(cmd).args(args).output()?;

            Ok(Some(json!({
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
                "exit_code": output.status.code().unwrap_or(0),
            })))
        }
        _ => Ok(None),
    }
}

fn visit_dirs(dir: &Path, text: &str, file_pattern: Option<&str>, results: &mut Vec<String>) -> Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, text, file_pattern, results)?;
            } else {
                if let Some(pattern) = file_pattern {
                     if !path.to_string_lossy().contains(pattern) {
                         continue;
                     }
                }

                if let Ok(content) = fs::read_to_string(&path) {
                     if content.contains(text) {
                         results.push(format!("{}: Found match", path.display()));
                     }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_declarations() {
        let decls = declarations();
        assert!(decls.iter().any(|d| d.name == "fs_cat"));
        assert!(decls.iter().any(|d| d.name == "fs_ls"));
    }

    #[test]
    fn test_run_ls() {
        let args = json!({ "path": "." });
        let result = run("fs_ls", &args).unwrap();
        assert!(result.is_some());
        let json = result.unwrap();
        assert!(json["files"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_command_run_injection() {
        let args = json!({ "command": "echo hello; echo world" });
        let result = run("command_run", &args).unwrap();
        assert!(result.is_some());
        let json = result.unwrap();
        let stdout = json["stdout"].as_str().unwrap();
        assert!(stdout.contains(";"));
    }
}
