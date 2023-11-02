mod abort_signal;
mod prompt_input;
mod split_line;
mod tiktoken;

pub use self::abort_signal::{create_abort_signal, AbortSignal};
pub use self::prompt_input::*;
pub use self::split_line::*;
pub use self::tiktoken::cl100k_base_singleton;

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

/// Split text to tokens
pub fn tokenize(text: &str) -> Vec<String> {
    let tokens = cl100k_base_singleton().lock().tokenize(text);
    tokens.into_iter().map(|(_, text)| text).collect()
}

/// Count how many tokens a piece of text needs to consume
pub fn count_tokens(text: &str) -> usize {
    cl100k_base_singleton()
        .lock()
        .encode_with_special_tokens(text)
        .len()
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

pub fn init_tokio_runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    use anyhow::Context;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "Failed to init tokio")
}
