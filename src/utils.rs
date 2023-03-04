use anyhow::{anyhow, Result};
use copypasta::{ClipboardContext, ClipboardProvider};
use std::io::{stdout, Write};

pub fn dump<T: ToString>(text: T, newlines: usize) {
    print!("{}{}", text.to_string(), "\n".repeat(newlines));
    let _ = stdout().flush();
}

pub fn copy(src: &str) -> Result<()> {
    let mut ctx = ClipboardContext::new().map_err(|err| anyhow!("{err}"))?;
    ctx.set_contents(src.to_string())
        .map_err(|err| anyhow!("{err}"))
}

pub fn paste() -> Result<String> {
    let mut ctx = ClipboardContext::new().map_err(|err| anyhow!("{err}"))?;
    ctx.get_contents().map_err(|err| anyhow!("{err}"))
}
