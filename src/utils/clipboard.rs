use arboard::Clipboard;
use lazy_static::lazy_static;
use std::sync::Arc;
use std::sync::Mutex;

lazy_static! {
    static ref CLIPBOARD: Arc<Mutex<Option<Clipboard>>> =
        Arc::new(Mutex::new(Clipboard::new().ok()));
}

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
pub fn set_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = CLIPBOARD.lock().unwrap();
    match clipboard.as_mut() {
        Some(clipboard) => clipboard.set_text(text)?,
        None => anyhow::bail!("No available clipboard"),
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "emscripten"))]
pub fn set_text(text: &str) -> anyhow::Result<()> {
    anyhow::bail!("No available clipboard")
}
