mod abort_signal;
mod clipboard;
mod crypto;
mod prompt_input;
mod render_prompt;
mod spinner;

pub use self::abort_signal::{create_abort_signal, AbortSignal};
pub use self::clipboard::set_text;
pub use self::crypto::*;
pub use self::prompt_input::*;
pub use self::render_prompt::render_prompt;
pub use self::spinner::run_spinner;

use fancy_regex::Regex;
use lazy_static::lazy_static;
use std::env;
use std::process::Command;

lazy_static! {
    pub static ref CODE_BLOCK_RE: Regex = Regex::new(r"(?ms)```\w*(.*)```").unwrap();
}

pub fn now() -> String {
    let now = chrono::Local::now();
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

pub fn get_env_name(key: &str) -> String {
    format!(
        "{}_{}",
        env!("CARGO_CRATE_NAME").to_ascii_uppercase(),
        key.to_ascii_uppercase(),
    )
}

pub fn tokenize(text: &str) -> Vec<&str> {
    if text.is_ascii() {
        text.split_inclusive(|c: char| c.is_ascii_whitespace())
            .collect()
    } else {
        unicode_segmentation::UnicodeSegmentation::graphemes(text, true).collect()
    }
}

pub fn estimate_token_length(text: &str) -> usize {
    let mut token_length: f32 = 0.0;

    for char in text.chars() {
        if char.is_ascii() {
            if char.is_ascii_alphabetic() {
                token_length += 0.25;
            } else {
                token_length += 0.5;
            }
        } else {
            token_length += 1.5;
        }
    }

    token_length.ceil() as usize
}

pub fn light_theme_from_colorfgbg(colorfgbg: &str) -> Option<bool> {
    let parts: Vec<_> = colorfgbg.split(';').collect();
    let bg = match parts.len() {
        2 => &parts[1],
        3 => &parts[2],
        _ => {
            return None;
        }
    };
    let bg = bg.parse::<u8>().ok()?;
    let (r, g, b) = ansi_colours::rgb_from_ansi256(bg);

    let v = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;

    let light = v > 128.0;
    Some(light)
}

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

pub fn run_command(eval_str: &str) -> anyhow::Result<i32> {
    let (_shell_name, shell_cmd, shell_arg) = detect_shell();
    let status = Command::new(shell_cmd)
        .arg(shell_arg)
        .arg(eval_str)
        .status()?;
    Ok(status.code().unwrap_or_default())
}

pub fn extract_block(input: &str) -> String {
    let output: String = CODE_BLOCK_RE
        .captures_iter(input)
        .filter_map(|m| {
            m.ok()
                .and_then(|cap| cap.get(1))
                .map(|m| String::from(m.as_str()))
        })
        .collect();
    if output.is_empty() {
        input.trim().to_string()
    } else {
        output.trim().to_string()
    }
}

pub fn format_option_value<T>(value: &Option<T>) -> String
where
    T: std::fmt::Display,
{
    match value {
        Some(value) => value.to_string(),
        None => "-".to_string(),
    }
}

pub fn fuzzy_match(text: &str, pattern: &str) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();

    let mut pattern_index = 0;
    let mut text_index = 0;

    while pattern_index < pattern_chars.len() && text_index < text_chars.len() {
        if pattern_chars[pattern_index] == text_chars[text_index] {
            pattern_index += 1;
        }
        text_index += 1;
    }

    pattern_index == pattern_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("openai:gpt-4-turbo", "gpt4"));
        assert!(fuzzy_match("openai:gpt-4-turbo", "oai4"));
        assert!(!fuzzy_match("openai:gpt-4-turbo", "4gpt"));
    }
}
