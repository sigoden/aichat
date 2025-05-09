use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
static CLIPBOARD: std::sync::LazyLock<std::sync::Mutex<Option<arboard::Clipboard>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(arboard::Clipboard::new().ok()));

/// Attempts to set text to clipboard with OSC52 escape sequence
/// Works in many modern terminals, including over SSH.
fn set_text_osc52(text: &str) -> anyhow::Result<()> {
    let encoded = BASE64.encode(text);
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    // Write to stdout:
    if let Err(e) = std::io::Write::write_all(&mut std::io::stdout(), seq.as_bytes()) {
        return Err(anyhow::anyhow!("Failed to send OSC52 sequence").context(e));
    }
    // Flush stdout:
    if let Err(e) = std::io::Write::flush(&mut std::io::stdout()) {
        return Err(anyhow::anyhow!("Failed to flush OSC52 sequence").context(e));
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
pub fn set_text(text: &str) -> anyhow::Result<()> {
    // First try arboard:
    let mut clipboard = CLIPBOARD.lock().unwrap();
    if let Some(clipboard) = clipboard.as_mut() {
        if let Ok(()) = clipboard.set_text(text) {
            #[cfg(target_os = "linux")]
            std::thread::sleep(std::time::Duration::from_millis(50));
            return Ok(());
        }
    }
    // If arboard failed, try OSC52:
    match set_text_osc52(text) {
        Ok(()) => Ok(()),
        Err(osc_err) => Err(anyhow::anyhow!("No clipboard available")
            .context(format!("Failed to copy (OSC52 error: {})", osc_err))),
    }
}

#[cfg(any(target_os = "android", target_os = "emscripten"))]
pub fn set_text(_text: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("No clipboard available").context("Failed to copy"))
}
