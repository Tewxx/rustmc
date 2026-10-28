//! `GuiScreen.getClipboardString` / `setClipboardString`.

/// The clipboard's text, or empty when it holds none (vanilla swallows the exception).
pub fn get() -> String {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default()
}

pub fn set(text: &str) {
    if text.is_empty() {
        return;
    }
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text.to_string());
    }
}
