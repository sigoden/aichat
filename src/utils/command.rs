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

pub fn detect_shell() -> (String, String, &'static str) {
    let os = env::consts::OS;
    if os == "windows" {
        if env::var("NU_VERSION").is_ok() {
            ("nushell".into(), "nu.exe".into(), "-c")
        } else if let Some(ret) = env::var("PSModulePath").ok().and_then(|v| {
            let v = v.to_lowercase();
            if v.split(';').count() >= 3 {
                if v.contains("powershell\\7\\") {
                    Some(("pwsh".into(), "pwsh.exe".into(), "-c"))
                } else {
                    Some(("powershell".into(), "powershell.exe".into(), "-Command"))
                }
            } else {
                None
            }
        }) {
            ret
        } else {
            ("cmd".into(), "cmd.exe".into(), "/C")
        }
    } else if env::var("NU_VERSION").is_ok() {
        ("nushell".into(), "nu".into(), "-c")
    } else {
        let shell_cmd = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let shell_name = match shell_cmd.rsplit_once('/') {
            Some((_, name)) => name.to_string(),
            None => shell_cmd.clone(),
        };
        let shell_name = if shell_name == "nu" {
            "nushell".into()
        } else {
            shell_name
        };
        (shell_name, shell_cmd, "-c")
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
