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
        "command_run" => {
            let command = args["command"].as_str().ok_or_else(|| anyhow!("Missing command"))?;
            let (cmd, args) = if cfg!(target_os = "windows") {
                ("cmd", vec!["/C", command])
            } else {
                ("sh", vec!["-c", command])
            };
            let output = std::process::Command::new(cmd)
                .args(args)
                .output()?;

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
}
