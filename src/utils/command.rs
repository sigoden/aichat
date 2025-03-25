use super::*;

use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, bail, Context, Result};
use dirs::home_dir;
use std::sync::LazyLock;

pub static SHELL: LazyLock<Shell> = LazyLock::new(|| detect_shell());

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
    let cmd = env::var(get_env_name("shell")).ok().or_else(|| {
        if cfg!(windows) {
            if let Ok(ps_module_path) = env::var("PSModulePath") {
                let ps_module_path = ps_module_path.to_lowercase();
                if ps_module_path.starts_with(r"c:\users") {
                    if ps_module_path.contains(r"\powershell\7\") {
                        return Some("pwsh.exe".to_string());
                    } else {
                        return Some("powershell.exe".to_string());
                    }
                }
            }
            None
        } else {
            env::var("SHELL").ok()
        }
    });
    let name = cmd
        .as_ref()
        .and_then(|v| Path::new(v).file_stem().and_then(|v| v.to_str()))
        .map(|v| {
            if v == "nu" {
                "nushell".into()
            } else {
                v.to_lowercase()
            }
        });
    let (cmd, name) = match (cmd.as_deref(), name.as_deref()) {
        (Some(cmd), Some(name)) => (cmd, name),
        _ => {
            if cfg!(windows) {
                ("cmd.exe", "cmd")
            } else {
                ("/bin/sh", "sh")
            }
        }
    };
    let shell_arg = match name {
        "powershel" => "-Command",
        "cmd" => "/C",
        _ => "-c",
    };
    Shell::new(name, cmd, shell_arg)
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

pub fn run_loader_command(path: &str, extension: &str, loader_command: &str) -> Result<String> {
    let cmd_args = shell_words::split(loader_command)
        .with_context(|| anyhow!("Invalid document loader '{extension}': `{loader_command}`"))?;
    let mut use_stdout = true;
    let outpath = temp_file("-output-", "").display().to_string();
    let cmd_args: Vec<_> = cmd_args
        .into_iter()
        .map(|mut v| {
            if v.contains("$1") {
                v = v.replace("$1", path);
            }
            if v.contains("$2") {
                use_stdout = false;
                v = v.replace("$2", &outpath);
            }
            v
        })
        .collect();
    let cmd_eval = shell_words::join(&cmd_args);
    debug!("run `{cmd_eval}`");
    let (cmd, args) = cmd_args.split_at(1);
    let cmd = &cmd[0];
    if use_stdout {
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
        Ok(stdout)
    } else {
        let status = run_command(cmd, args, None).with_context(|| {
            format!("Unable to run `{cmd_eval}`, Perhaps '{cmd}' is not installed?")
        })?;
        if status != 0 {
            bail!("The command `{cmd_eval}` exited with non-zero.")
        }
        let contents = std::fs::read_to_string(&outpath)
            .context("Failed to read file generated by the loader")?;
        Ok(contents)
    }
}

pub fn edit_file(editor: &str, path: &Path) -> Result<()> {
    let mut child = Command::new(editor).arg(path).spawn()?;
    child.wait()?;
    Ok(())
}

pub fn append_to_shell_history(shell: &str, command: &str, exit_code: i32) -> io::Result<()> {
    if let Some(history_file) = get_history_file(shell) {
        let command = command.replace('\n', " ");
        let now = now_timestamp();
        let history_txt = if shell == "fish" {
            format!("- cmd: {command}\n  when: {now}")
        } else if shell == "zsh" {
            format!(": {now}:{exit_code};{command}",)
        } else {
            command
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_file)?;
        writeln!(file, "{}", history_txt)?;
    }
    Ok(())
}

fn get_history_file(shell: &str) -> Option<PathBuf> {
    match shell {
        "bash" | "sh" => env::var("HISTFILE")
            .ok()
            .map(PathBuf::from)
            .or(Some(home_dir()?.join(".bash_history"))),
        "zsh" => env::var("HISTFILE")
            .ok()
            .map(PathBuf::from)
            .or(Some(home_dir()?.join(".zsh_history"))),
        "nushell" => Some(dirs::config_dir()?.join("nushell").join("history.txt")),
        "fish" => Some(
            home_dir()?
                .join(".local")
                .join("share")
                .join("fish")
                .join("fish_history"),
        ),
        "powershell" | "pwsh" => {
            #[cfg(not(windows))]
            {
                Some(
                    home_dir()?
                        .join(".local")
                        .join("share")
                        .join("powershell")
                        .join("PSReadLine")
                        .join("ConsoleHost_history.txt"),
                )
            }
            #[cfg(windows)]
            {
                Some(
                    dirs::data_dir()?
                        .join("Microsoft")
                        .join("Windows")
                        .join("PowerShell")
                        .join("PSReadLine")
                        .join("ConsoleHost_history.txt"),
                )
            }
        }
        "ksh" => Some(home_dir()?.join(".ksh_history")),
        "tcsh" => Some(home_dir()?.join(".history")),
        _ => None,
    }
}
