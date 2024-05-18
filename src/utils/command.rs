use std::{collections::HashMap, env, ffi::OsStr, process::Command};

use anyhow::{Context, Result};

pub fn detect_os() -> String {
    let os = env::consts::OS;
    if os == "linux" {
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            for line in contents.lines() {
                if let Some(id) = line.strip_prefix("ID=") {
                    return format!("{os}/{id}");
                }
            }
        }
    }
    os.to_string()
}

pub struct Shell {
    pub name: String,
    pub cmd: String,
    pub arg: String,
}

impl Shell {
    pub fn new(name: &str, cmd: &str, arg: &str) -> Self {
        Self {
            name: name.to_string(),
            cmd: cmd.to_string(),
            arg: arg.to_string(),
        }
    }
}

pub fn detect_shell() -> Shell {
    let os = env::consts::OS;
    if os == "windows" {
        if let Some(ret) = env::var("PSModulePath").ok().and_then(|v| {
            let v = v.to_lowercase();
            if v.split(';').count() >= 3 {
                if v.contains("powershell\\7\\") {
                    Some(Shell::new("pwsh", "pwsh.exe", "-c"))
                } else {
                    Some(Shell::new("powershell", "powershell.exe", "-Command"))
                }
            } else {
                None
            }
        }) {
            ret
        } else {
            Shell::new("cmd", "cmd.exe", "/C")
        }
    } else {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let shell = match shell.rsplit_once('/') {
            Some((_, v)) => v,
            None => &shell,
        };
        match shell {
            "bash" | "zsh" | "fish" | "pwsh" => Shell::new(shell, shell, "-c"),
            _ => Shell::new("sh", "sh", "-c"),
        }
    }
}

pub fn run_command<T: AsRef<OsStr>>(
    cmd: &str,
    args: &[T],
    envs: Option<HashMap<String, String>>,
) -> Result<i32> {
    let status = Command::new(cmd)
        .args(args.iter())
        .envs(envs.unwrap_or_default())
        .status()?;
    Ok(status.code().unwrap_or_default())
}

pub fn run_command_with_output<T: AsRef<OsStr>>(
    cmd: &str,
    args: &[T],
    envs: Option<HashMap<String, String>>,
) -> Result<(bool, String, String)> {
    let output = Command::new(cmd)
        .args(args.iter())
        .envs(envs.unwrap_or_default())
        .output()?;
    let status = output.status;
    let stdout = std::str::from_utf8(&output.stdout).context("Invalid UTF-8 in stdout")?;
    let stderr = std::str::from_utf8(&output.stderr).context("Invalid UTF-8 in stderr")?;
    Ok((status.success(), stdout.to_string(), stderr.to_string()))
}
