use anyhow::Context;

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
mod internal {
    use arboard::Clipboard;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::sync::{LazyLock, Mutex};

    static CLIPBOARD: LazyLock<Mutex<Option<Clipboard>>> =
        LazyLock::new(|| Mutex::new(Clipboard::new().ok()));

    pub fn set_text(text: &str) -> anyhow::Result<()> {
        let mut clipboard = CLIPBOARD.lock().unwrap();
        match clipboard.as_mut() {
            Some(clipboard) => {
                clipboard.set_text(text)?;
                #[cfg(target_os = "linux")]
                std::thread::sleep(std::time::Duration::from_millis(50));
                Ok(())
            }
            None => set_text_osc52(text),
        }
    }

    /// Attempts to set text to clipboard with OSC52 escape sequence
    /// Works in many modern terminals, including over SSH.
    fn set_text_osc52(text: &str) -> anyhow::Result<()> {
        let encoded = STANDARD.encode(text);
        let seq = format!("\x1b]52;c;{encoded}\x07");
        if let Err(e) = std::io::Write::write_all(&mut std::io::stdout(), seq.as_bytes()) {
            return Err(anyhow::anyhow!("Failed to send OSC52 sequence").context(e));
        }
        if let Err(e) = std::io::Write::flush(&mut std::io::stdout()) {
            return Err(anyhow::anyhow!("Failed to flush OSC52 sequence").context(e));
        }
        Ok(())
    }
}

#[cfg(any(target_os = "android", target_os = "emscripten"))]
mod internal {
    pub fn set_text(_text: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("No clipboard available"))
    }
}

pub fn set_text(text: &str) -> anyhow::Result<()> {
    internal::set_text(text).context("Failed to copy")
}
