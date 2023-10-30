mod tiktoken;

use self::tiktoken::cl100k_base;
pub use self::tiktoken::{cl100k_base_singleton, count_tokens, text_to_tokens, tokens_to_text};

use chrono::prelude::*;
use crossterm::style::{Color, Stylize};
use std::io::{stdout, Write};

#[macro_export]
macro_rules! print_now {
    ($($arg:tt)*) => {
        $crate::utils::print_now(&format!($($arg)*))
    };
}

pub fn print_now<T: ToString>(text: &T) {
    print!("{}", text.to_string());
    let _ = stdout().flush();
}

pub fn now() -> String {
    let now = Local::now();
    now.to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn get_env_name(key: &str) -> String {
    format!(
        "{}_{}",
        env!("CARGO_CRATE_NAME").to_ascii_uppercase(),
        key.to_ascii_uppercase(),
    )
}

#[allow(unused)]
pub fn emphasis(text: &str) -> String {
    text.stylize().with(Color::White).to_string()
}

pub fn split_text(text: &str) -> Result<Vec<String>, anyhow::Error> {
    let bpe = cl100k_base()?;
    let tokens = bpe.encode_with_special_tokens(text);
    let data: Result<Vec<String>, _> = tokens.into_iter().map(|v| bpe.decode(&[v])).collect();
    data
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
