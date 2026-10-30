//! LunarForge CPS guest. Requires the reserved host input interface.
#[cfg(target_arch = "wasm32")]
mod guest {
    wit_bindgen::generate!({ path: "wit", world: "input-mod" });
    use rustmc::plugin::{input, types, ui_draw};
    use lunarforge_core::{ClickCounter, CpsOptions, cps_text};
    use std::cell::RefCell;
    thread_local! { static CLICKS: RefCell<ClickCounter> = RefCell::new(ClickCounter::default()); }
    struct Cps;
    impl exports::rustmc::plugin::lifecycle::Guest for Cps {
        fn on_load(_api: String, _features: Vec<String>) -> Result<(), String> { Ok(()) }
        fn on_unload() { CLICKS.with(|c| *c.borrow_mut() = ClickCounter::default()); }
        fn on_tick() {}
        fn on_frame(_ctx: types::FrameCtx) {}
    }
    impl exports::rustmc::plugin::render::Guest for Cps {
        fn render_hud(_ctx: types::FrameCtx) {
            if input::screen_open() { return; }
            let text = CLICKS.with(|clicks| cps_text(&mut clicks.borrow_mut(), input::monotonic_ms(), CpsOptions::default()));
            let mut out = b"RMD1".to_vec();
            out.push(0);
            for v in [0i32, 0, 56, 18] { out.extend(v.to_le_bytes()); }
            out.extend(0x6f000000u32.to_le_bytes());
            out.push(3); out.extend(4i32.to_le_bytes()); out.extend(5i32.to_le_bytes());
            out.extend(0xffff_ffffu32.to_le_bytes()); out.push(1);
            out.extend((text.len() as u16).to_le_bytes()); out.extend(text.as_bytes());
            ui_draw::submit(&out);
        }
    }
    impl exports::rustmc::plugin::events::Guest for Cps {
        fn on_key(_e: exports::rustmc::plugin::events::KeyEvent) -> bool { false }
        fn on_mouse(e: exports::rustmc::plugin::events::MouseEvent) -> bool {
            if e.down && (e.button == 0 || e.button == 1) {
                let now = input::monotonic_ms();
                let screen = input::screen_open();
                let using_item = input::player_using_item();
                CLICKS.with(|c| c.borrow_mut().mouse_down(e.button as u8, now, screen, using_item));
            }
            false
        }
        fn on_screen_open(_kind: exports::rustmc::plugin::events::ScreenKind, _title: String) {}
        fn on_screen_close(_kind: exports::rustmc::plugin::events::ScreenKind) {}
    }
    export!(Cps);
}
