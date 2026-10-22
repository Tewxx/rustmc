//! The window icon (`Minecraft.setWindowIcon`): vanilla's `icons/icon_16x16.png` and
//! `icons/icon_32x32.png` from the asset index, embedded so the exe needs nothing extra.

use winit::window::Icon;

const ICON_16: &[u8] = include_bytes!("../assets/icon_16x16.png");
const ICON_32: &[u8] = include_bytes!("../assets/icon_32x32.png");

fn decode(png: &[u8]) -> Option<Icon> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Window/taskbar icon. Windows scales the 32px one for the taskbar, like LWJGL's `setIcon`.
pub fn window_icon() -> Option<Icon> {
    decode(ICON_32).or_else(|| decode(ICON_16))
}

#[cfg(test)]
mod tests {
    #[test]
    fn icons_decode() {
        assert!(super::decode(super::ICON_16).is_some());
        assert!(super::decode(super::ICON_32).is_some());
    }
}
