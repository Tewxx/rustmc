//! LunarForge FPS guest.
#[cfg(target_arch = "wasm32")]
mod guest {
    wit_bindgen::generate!({ path: "wit", world: "client-mod" });
    struct FpsMod;
    impl exports::rustmc::plugin::lifecycle::Guest for FpsMod {
        fn on_load(_host_api: String, _features: Vec<String>) -> Result<(), String> { Ok(()) }
        fn on_unload() {}
        fn on_tick() {}
        fn on_frame(_ctx: rustmc::plugin::types::FrameCtx) {}
    }
    impl exports::rustmc::plugin::render::Guest for FpsMod {
        fn render_hud(ctx: rustmc::plugin::types::FrameCtx) {
            rustmc::plugin::ui_draw::submit(&lunarforge_core::fps_draw_stream(ctx.fps));
        }
    }
    export!(FpsMod);
}
