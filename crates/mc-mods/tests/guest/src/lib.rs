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
        let mut stream = b"RMD1\0".to_vec();
        for v in [1u32, 2, 3, 4, 0xff010203] { stream.extend(v.to_le_bytes()); }
        rustmc::plugin::ui_draw::submit(&stream);
    }
}
export!(Guest);
