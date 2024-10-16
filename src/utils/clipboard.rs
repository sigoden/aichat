#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
lazy_static::lazy_static! {
    static ref CLIPBOARD: std::sync::Arc<std::sync::Mutex<Option<arboard::Clipboard>>> =
        std::sync::Arc::new(std::sync::Mutex::new(arboard::Clipboard::new().ok()));
}

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
pub fn set_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = CLIPBOARD.lock().unwrap();
    match clipboard.as_mut() {
        Some(clipboard) => {
            clipboard.set_text(text)?;
            #[cfg(target_os = "linux")]
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None => anyhow::bail!("Failed to copy the text; no available clipboard"),
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "emscripten"))]
pub fn set_text(_text: &str) -> anyhow::Result<()> {
    anyhow::bail!("Failed to copy the text; no available clipboard")
}
