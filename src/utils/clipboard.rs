#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
static CLIPBOARD: std::sync::LazyLock<std::sync::Mutex<Option<arboard::Clipboard>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(arboard::Clipboard::new().ok()));

#[cfg(not(any(target_os = "android", target_os = "emscripten")))]
pub fn set_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = CLIPBOARD.lock().unwrap();
    match clipboard.as_mut() {
        Some(clipboard) => {
            clipboard.set_text(text)?;
            #[cfg(target_os = "linux")]
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None => return Err(anyhow::anyhow!("No clipboard available").context("Failed to copy")),
    }
    Ok(())
}

#[cfg(any(target_os = "android", target_os = "emscripten"))]
pub fn set_text(_text: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("No clipboard available").context("Failed to copy"))
}
