use anyhow::{anyhow, Result};
use chrono::prelude::*;
use copypasta::{ClipboardContext, ClipboardProvider};
use std::io::{stdout, Write};

pub fn dump<T: ToString>(text: T, newlines: usize) {
    print!("{}{}", text.to_string(), "\n".repeat(newlines));
    let _ = stdout().flush();
}

pub fn copy(src: &str) -> Result<()> {
    ClipboardContext::new()
        .and_then(|mut ctx| ctx.set_contents(src.to_string()))
        .map_err(|err| anyhow!("Failed to copy, {err}"))
}

pub fn now() -> String {
    let now = Local::now();
    now.to_rfc3339_opts(SecondsFormat::Secs, false)
}
