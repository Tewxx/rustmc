wit_bindgen::generate!({ path: "../../wit", world: "client-mod" });

struct Guest;
impl exports::rustmc::plugin::lifecycle::Guest for Guest {
    fn on_load(_: String, _: Vec<String>) -> Result<(), String> { Ok(()) }
    fn on_unload() {}
    fn on_tick() {
        let p = rustmc::plugin::game::player();
        let ticks = rustmc::plugin::game::game_time();
        if ticks == 43 { loop {} }
        if p.food == 19 && ticks == 42 { rustmc::plugin::log::log(rustmc::plugin::types::LogLevel::Info, "guest tick"); }
    }
    fn on_frame(_: exports::rustmc::plugin::lifecycle::FrameCtx) {}
}
impl exports::rustmc::plugin::render::Guest for Guest {
    fn render_hud(_: exports::rustmc::plugin::render::FrameCtx) {
        rustmc::plugin::ui_draw::submit(b"RMD1");
    }
}
export!(Guest);
