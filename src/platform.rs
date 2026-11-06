//! Platform differences in one place: native file dialogs on desktop, stand-ins in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult};

#[cfg(target_arch = "wasm32")]
mod web {
    use std::path::PathBuf;

    /// In the browser there is no blocking file dialog; uploads/downloads go through the page instead.
    #[derive(Default)]
    pub struct FileDialog;
    impl FileDialog {
        pub fn new() -> Self {
            FileDialog
        }
        pub fn add_filter(self, _name: &str, _exts: &[&str]) -> Self {
            self
        }
        pub fn set_file_name(self, _name: &str) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            None
        }
    }

    pub enum MessageButtons {
        YesNo,
    }
    #[derive(PartialEq, Eq)]
    pub enum MessageDialogResult {
        Yes,
        No,
    }
    #[derive(Default)]
    pub struct MessageDialog;
    impl MessageDialog {
        pub fn new() -> Self {
            MessageDialog
        }
        pub fn set_title(self, _t: &str) -> Self {
            self
        }
        pub fn set_description(self, _d: &str) -> Self {
            self
        }
        pub fn set_buttons(self, _b: MessageButtons) -> Self {
            self
        }
        pub fn show(self) -> MessageDialogResult {
            MessageDialogResult::Yes
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

/// What BurnCraft puts on the system clipboard when it copies shapes, so a later paste knows the shapes
/// are newer than any picture copied elsewhere.
pub const CLIP_MARKER: &str = "BurnCraft shapes";

/// One clipboard for the whole run: on Linux whatever BurnCraft copies is only available while this lives.
#[cfg(not(target_arch = "wasm32"))]
fn with_clipboard<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<R>) -> Option<R> {
    static CB: std::sync::OnceLock<std::sync::Mutex<Option<arboard::Clipboard>>> = std::sync::OnceLock::new();
    let m = CB.get_or_init(|| std::sync::Mutex::new(arboard::Clipboard::new().ok()));
    let mut g = m.lock().ok()?;
    f(g.as_mut()?)
}

/// PNG bytes for an RGBA picture.
pub fn png_bytes(w: u32, h: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let buf = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(buf).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// A picture on the system clipboard (copied from a browser, screenshot tool...), as PNG bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn clipboard_image() -> Option<Vec<u8>> {
    let img = with_clipboard(|cb| cb.get_image().ok())?;
    png_bytes(img.width as u32, img.height as u32, img.bytes.into_owned())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn clipboard_text() -> Option<String> {
    with_clipboard(|cb| cb.get_text().ok())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn clipboard_set_text(t: &str) {
    let _ = with_clipboard(|cb| cb.set_text(t.to_string()).ok());
}

#[cfg(target_arch = "wasm32")]
pub fn clipboard_image() -> Option<Vec<u8>> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn clipboard_text() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn clipboard_set_text(_t: &str) {}
