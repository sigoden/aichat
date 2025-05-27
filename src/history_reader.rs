use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;

use crate::config::GlobalConfig; // To access TerminalHistoryRagConfig

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandHistoryEntry {
    pub command: String,
    pub timestamp: Option<i64>, // Unix epoch seconds
    pub shell: String,
}

// Placeholder for the main function that will orchestrate history reading
pub fn get_terminal_history(config: &GlobalConfig) -> Result<Vec<CommandHistoryEntry>> {
    let cfg = config.read();
    if !cfg.terminal_history_rag.enabled || !cfg.terminal_history_rag.consent_given {
        // If not enabled or consent not given, return empty or an appropriate message/status
        return Ok(Vec::new()); // Or perhaps Err(anyhow!("Consent not given or feature disabled"))
    }

    let shell_name = detect_shell()?;
    let max_commands = cfg.terminal_history_rag.max_history_commands;

    match shell_name.as_str() {
        "bash" => read_bash_history(max_commands),
        "zsh" => read_zsh_history(max_commands),
        "fish" => read_fish_history(max_commands),
        _ => Err(anyhow!(format!("Unsupported shell: {}", shell_name))),
    }
}

fn detect_shell() -> Result<String> {
    env::var("SHELL")
        .map_err(|e| anyhow!("Failed to read SHELL environment variable: {}", e))
        .map(|shell_path| {
            PathBuf::from(shell_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
}

fn get_history_file_path(shell_name: &str, env_var_name: &str, default_filename: &str) -> Result<PathBuf> {
    if let Ok(histfile_path_str) = env::var(env_var_name) {
        if !histfile_path_str.is_empty() {
            let path = PathBuf::from(histfile_path_str);
            if path.is_absolute() {
                return Ok(path);
            } else if let Some(home_dir) = dirs::home_dir() {
                return Ok(home_dir.join(path));
            }
        }
    }
    dirs::home_dir()
        .map(|home| home.join(default_filename))
        .ok_or_else(|| anyhow!("Could not determine home directory for history file"))
}


fn read_bash_history(max_commands: usize) -> Result<Vec<CommandHistoryEntry>> {
    let path = get_history_file_path("bash", "HISTFILE", ".bash_history")?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).context("Failed to open Bash history file")?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    let mut entries = Vec::new();
    let mut current_timestamp: Option<i64> = None;

    // Iterate in reverse to get recent commands first, but then reverse back for chronological order of selection.
    // A simpler way is to read all, then take the last N.
    // If max_commands is large, this might be memory intensive.
    // For now, let's collect all and then truncate.

    for line in lines.iter() {
        if line.starts_with('#') {
            if let Ok(ts) = line[1..].parse::<i64>() {
                current_timestamp = Some(ts);
                continue; // Timestamp line, command is next
            }
            // If not a parsable timestamp, it might be a comment; ignore.
        }
        // If it's not a timestamp line, it's a command.
        // current_timestamp applies to this command.
        entries.push(CommandHistoryEntry {
            command: line.clone(),
            timestamp: current_timestamp,
            shell: "bash".to_string(),
        });
        current_timestamp = None; // Reset timestamp for next potential command without one
    }
    
    // Ensure we only take the last `max_commands`
    if entries.len() > max_commands {
        entries = entries.into_iter().skip(entries.len() - max_commands).collect();
    }

    Ok(entries)
}

// Public function that application code will call.
fn read_fish_history(max_commands: usize) -> Result<Vec<CommandHistoryEntry>> {
    // In non-test builds, this closure calls the actual std::process::Command
    #[cfg(not(test))]
    let executor = |cmd_str: &str| -> Result<std::process::Output> {
        Command::new("fish")
            .arg("-c")
            .arg(cmd_str)
            .output()
            .context("Failed to execute Fish history command")
    };

    // In test builds, this closure will cause an error if called,
    // because tests should use `read_fish_history_internal` with a mock executor.
    #[cfg(test)]
    let executor = |_cmd_str: &str| -> Result<std::process::Output> {
         Err(anyhow!("Direct call to read_fish_history in test mode. Use read_fish_history_internal with a mock executor for testing fish history."))
    };
    
    read_fish_history_internal(max_commands, executor)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt; // For ExitStatus::from_raw
    use std::path::Path;
    use std::process::Output;
    use tempfile::tempdir;


    // Helper to create a temporary history file with given content
    fn create_temp_hist_file(dir: &Path, filename: &str, content: &str) -> Result<PathBuf> {
        let file_path = dir.join(filename);
        let mut file = File::create(&file_path)?;
        writeln!(file, "{}", content)?;
        Ok(file_path)
    }
    
    // RAII guard for setting and unsetting environment variables
    struct EnvVarGuard {
        name: String,
        original_value: Option<String>,
    }

    impl EnvVarGuard {
        fn new(name: &str, value: &str) -> Self {
            let original_value = env::var(name).ok();
            env::set_var(name, value);
            EnvVarGuard { name: name.to_string(), original_value }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(val) = &self.original_value {
                env::set_var(&self.name, val);
            } else {
                env::remove_var(&self.name);
            }
        }
    }


    #[test]
    fn test_read_bash_history_simple() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = "ls -l\npwd\necho hello";
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".bash_history_test_simple", hist_content)?;

        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());
        
        let entries = read_bash_history(10)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "ls -l");
        assert_eq!(entries[0].shell, "bash");
        assert_eq!(entries[1].command, "pwd");
        assert_eq!(entries[2].command, "echo hello");
        Ok(())
    }

    #[test]
    fn test_read_bash_history_with_timestamps() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = "#1678886400\nls -l\n#1678886401\npwd\necho 'no timestamp for this one'";
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".bash_history_test_ts", hist_content)?;
        
        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());

        let entries = read_bash_history(10)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "ls -l");
        assert_eq!(entries[0].timestamp, Some(1678886400));
        assert_eq!(entries[1].command, "pwd");
        assert_eq!(entries[1].timestamp, Some(1678886401));
        assert_eq!(entries[2].command, "echo 'no timestamp for this one'");
        assert_eq!(entries[2].timestamp, None);
        Ok(())
    }

    #[test]
    fn test_read_bash_history_max_commands() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = "cmd1\n#100\ncmd2\ncmd3\n#200\ncmd4\ncmd5"; // 5 commands, 2 with timestamps
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".bash_history_test_max", hist_content)?;

        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());

        let entries = read_bash_history(3)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "cmd3"); // cmd3
        assert_eq!(entries[0].timestamp, None); 
        assert_eq!(entries[1].command, "cmd4"); // cmd4 with ts 200
        assert_eq!(entries[1].timestamp, Some(200));
        assert_eq!(entries[2].command, "cmd5"); // cmd5
        assert_eq!(entries[2].timestamp, None);
        Ok(())
    }

    #[test]
    fn test_read_zsh_history_simple() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = "ls -l\npwd";
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".zsh_history_test_simple", hist_content)?;

        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());
        
        let entries = read_zsh_history(10)?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "ls -l");
        assert_eq!(entries[0].shell, "zsh");
        assert_eq!(entries[1].command, "pwd");
        Ok(())
    }

    #[test]
    fn test_read_zsh_history_extended() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = ": 1678886400:0;ls -l\n: 1678886405:0;echo hello world\npwd"; // Mix
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".zsh_history_test_ext", hist_content)?;

        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());

        let entries = read_zsh_history(10)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "ls -l");
        assert_eq!(entries[0].timestamp, Some(1678886400));
        assert_eq!(entries[1].command, "echo hello world");
        assert_eq!(entries[1].timestamp, Some(1678886405));
        assert_eq!(entries[2].command, "pwd"); // This line is not extended format
        assert_eq!(entries[2].timestamp, None);
        Ok(())
    }
    
    #[test]
    fn test_read_zsh_history_max_commands() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_content = ": 100:0;cmd1\ncmd2\n: 200:0;cmd3\ncmd4\n: 300:0;cmd5"; // 5 commands
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".zsh_history_test_max", hist_content)?;

        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());
        
        let entries = read_zsh_history(3)?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "cmd3");
        assert_eq!(entries[0].timestamp, Some(200));
        assert_eq!(entries[1].command, "cmd4");
        assert_eq!(entries[1].timestamp, None);
        assert_eq!(entries[2].command, "cmd5");
        assert_eq!(entries[2].timestamp, Some(300));
        Ok(())
    }

    #[test]
    fn test_empty_history_file() -> Result<()> {
        let temp_dir = tempdir()?;
        let hist_file_path = create_temp_hist_file(temp_dir.path(), ".empty_history", "")?;
        
        let _home_guard = EnvVarGuard::new("HOME", temp_dir.path().to_str().unwrap());
        let _histfile_guard = EnvVarGuard::new("HISTFILE", hist_file_path.to_str().unwrap());

        let bash_entries = read_bash_history(10)?;
        assert!(bash_entries.is_empty());
        
        let zsh_entries = read_zsh_history(10)?;
        assert!(zsh_entries.is_empty());
        Ok(())
    }

    // Note: Testing read_fish_history requires mocking std::process::Command.
    // This is more involved and typically requires a mocking library or conditional compilation
    // to inject a mock command execution logic. For this example, we'll skip the Fish test
    // as it relies on an external command execution not easily mocked without more infrastructure.
    // If `aichat` already has a command mocking utility for tests, that could be leveraged.

    #[test]
    fn test_read_fish_history_mocked() -> Result<()> {
        let max_commands = 5;
        let mock_executor = |_cmd: &str| -> Result<Output> {
            Ok(Output {
                status: std::process::ExitStatus::from_raw(0), // Success
                stdout: b"1678886400 ls -l\01678886405 echo hello\0".to_vec(),
                stderr: Vec::new(),
            })
        };

        let entries = read_fish_history_internal(max_commands, mock_executor)?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "ls -l");
        assert_eq!(entries[0].timestamp, Some(1678886400));
        assert_eq!(entries[0].shell, "fish");
        assert_eq!(entries[1].command, "echo hello");
        assert_eq!(entries[1].timestamp, Some(1678886405));
        assert_eq!(entries[1].shell, "fish");
        Ok(())
    }

    #[test]
    fn test_read_fish_history_mocked_empty_output() -> Result<()> {
        let max_commands = 5;
        let mock_executor = |_cmd: &str| -> Result<Output> {
            Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(), // Empty output
                stderr: Vec::new(),
            })
        };
        let entries = read_fish_history_internal(max_commands, mock_executor)?;
        assert!(entries.is_empty());
        Ok(())
    }

    #[test]
    fn test_read_fish_history_mocked_command_failure() -> Result<()> {
        let max_commands = 5;
        let mock_executor = |_cmd: &str| -> Result<Output> {
            Ok(Output {
                status: std::process::ExitStatus::from_raw(1), // Failure status
                stdout: Vec::new(),
                stderr: b"some error".to_vec(),
            })
        };
        let result = read_fish_history_internal(max_commands, mock_executor);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Fish history command failed with status exit status: 1"));
            assert!(e.to_string().contains("some error"));
        }
        Ok(())
    }
}

fn read_zsh_history(max_commands: usize) -> Result<Vec<CommandHistoryEntry>> {
    let path = get_history_file_path("zsh", "HISTFILE", ".zsh_history")?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).context("Failed to open Zsh history file")?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.context("Failed to read line from Zsh history")?;
        let mut command = line.as_str();
        let mut timestamp: Option<i64> = None;

        // Check for Zsh extended history format: : <timestamp>:<duration>;<command>
        if command.starts_with(':') {
            if let Some(parts) = command.splitn(3, ';').nth(0) { // Get ': <timestamp>:<duration>'
                if let Some(ts_part) = parts.split(':').nth(1) { // Get ' <timestamp>'
                    timestamp = ts_part.trim().parse::<i64>().ok();
                }
            }
            if let Some(cmd_part) = command.splitn(2, ';').nth(1) { // Get command part
                command = cmd_part;
            }
            // If parsing failed at any step, command remains the original line, timestamp None.
        }
        
        // Zsh might escape newlines and other characters.
        // A full robust parser would handle Zsh's specific quoting/escaping (metafy/unmetafy).
        // For now, we take the command as is after basic split.
        // This might include leading/trailing spaces if not careful with parsing.
        entries.push(CommandHistoryEntry {
            command: command.to_string(),
            timestamp,
            shell: "zsh".to_string(),
        });
    }
    
    if entries.len() > max_commands {
        entries = entries.into_iter().skip(entries.len() - max_commands).collect();
    }

    Ok(entries)
}


// Internal function that takes an executor. This allows mocking.
fn read_fish_history_internal<F>(
    max_commands: usize,
    executor: F,
) -> Result<Vec<CommandHistoryEntry>>
where
    F: FnOnce(&str) -> Result<std::process::Output>, // Executor is a closure
{
    let command_str = format!(
        "history --show-time='%s ' --null-character --max-results={}",
        max_commands
    );

    let output = executor(&command_str)?;

    if !output.status.success() {
        return Err(anyhow!(
            "Fish history command failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let history_data = output.stdout;
    let mut entries = Vec::new();

    let raw_entries: Vec<&[u8]> = history_data.split(|&c| c == 0).collect();

    for entry_bytes in raw_entries {
        if entry_bytes.is_empty() {
            continue;
        }
        let entry_str = String::from_utf8_lossy(entry_bytes);
        let mut parts = entry_str.splitn(2, ' ');
        let timestamp_str = parts.next();
        let command_str = parts.next().unwrap_or("").trim();

        if command_str.is_empty() {
            continue;
        }

        let timestamp = timestamp_str.and_then(|ts| ts.trim().parse::<i64>().ok());

        entries.push(CommandHistoryEntry {
            command: command_str.to_string(),
            timestamp,
            shell: "fish".to_string(),
        });
    }
    
    // Fish `history` command already respects max_results, but if it returned more for some reason,
    // or if we wanted to be absolutely sure (e.g. if --max-results was not available on old fish),
    // we could truncate here. Given we pass --max-results, this should be redundant.
    // if entries.len() > max_commands {
    //     entries = entries.into_iter().skip(entries.len() - max_commands).collect();
    // }

    Ok(entries)
}
