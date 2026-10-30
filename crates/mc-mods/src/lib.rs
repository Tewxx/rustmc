//! Native component-model mod runtime. No WASI or ambient host resources are linked.
pub mod draw;
pub mod worker;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};
use wasmtime::{component::{Component, Linker}, Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({ path: "wit", world: "client-mod" });
mod input_bindings {
    wasmtime::component::bindgen!({
        path: "wit", world: "input-mod",
        with: {
            "rustmc:plugin/types": crate::rustmc::plugin::types,
            "rustmc:plugin/game": crate::rustmc::plugin::game,
            "rustmc:plugin/ui-draw": crate::rustmc::plugin::ui_draw,
            "rustmc:plugin/log": crate::rustmc::plugin::log,
        },
    });
}
use rustmc::plugin::{game, log, types, ui_draw};
pub use rustmc::plugin::types::FrameCtx;
pub use input_bindings::exports::rustmc::plugin::events::{KeyEvent, MouseEvent};
use input_bindings::rustmc::plugin::input;

#[derive(Clone)]
pub struct Snapshot {
    pub player: game::PlayerState,
    pub ticks: u64,
    pub screen_open: bool,
    pub player_using_item: Option<bool>,
}

impl Default for Snapshot {
    fn default() -> Self {
        let v = types::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
        Self { player: game::PlayerState { pos: v.clone(), prev_pos: v.clone(), motion: v, yaw: 0., pitch: 0., on_ground: false, health: 20., food: 20, saturation: 5., xp_level: 0, selected_slot: 0, sprinting: false, sneaking: false, flying: false, gamemode: 0 }, ticks: 0, screen_open: false, player_using_item: None }
    }
}

struct Host {
    snapshot: Snapshot,
    hud: bool,
    draw: Option<Vec<u8>>,
    limits: StoreLimits,
    started: std::time::Instant,
    input_at: Option<std::time::Instant>,
}
impl types::Host for Host {}
impl input::Host for Host {
    fn monotonic_ms(&mut self) -> u64 { self.input_at.unwrap_or_else(std::time::Instant::now).saturating_duration_since(self.started).as_millis().min(u64::MAX as u128) as u64 }
    fn screen_open(&mut self) -> bool { self.snapshot.screen_open }
    fn player_using_item(&mut self) -> Option<bool> { self.snapshot.player_using_item }
}
impl game::Host for Host {
    fn player(&mut self) -> game::PlayerState { self.snapshot.player.clone() }
    fn game_time(&mut self) -> u64 { self.snapshot.ticks }
}
impl ui_draw::Host for Host {
    fn submit(&mut self, stream: Vec<u8>) {
        if self.hud && stream.len() <= 256 * 1024 { self.draw = Some(stream); }
    }
}
impl log::Host for Host {
    fn log(&mut self, _level: types::LogLevel, msg: String) {
        if msg.len() <= 4096 { eprintln!("[wasm mod] {msg}"); }
    }
}

#[derive(Deserialize)]
struct Manifest { #[serde(rename = "mod")] info: ModInfo, api: Api, #[serde(default)] capabilities: Capabilities }
#[derive(Deserialize)]
struct ModInfo { id: String, name: String, version: String, entry: String, side: String }
#[derive(Deserialize)]
struct Api { version: String, min: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities { #[serde(default)] hud: bool, #[serde(default)] input_read: bool, #[serde(default = "default_memory")] memory_mib: usize }
impl Default for Capabilities { fn default() -> Self { Self { hud: false, input_read: false, memory_mib: default_memory() } } }
fn default_memory() -> usize { 32 }

/// A validated package and its untrusted component bytes. No archive entry is extracted.
pub struct Package { pub id: String, pub name: String, pub version: String, hud: bool, input_read: bool, memory_mib: usize, bytes: Vec<u8> }
impl Package {
    pub fn read(path: &Path) -> Result<Self> {
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if ["optifine", "optifabric", "of_hd"].iter().any(|s| file_name.contains(s)) { bail!("OptiFine is blocked"); }
        let mut zip = zip::ZipArchive::new(File::open(path)?)?;
        if zip.len() > 1024 { bail!("too many archive entries"); }
        let mut names = std::collections::HashSet::new();
        for i in 0..zip.len() {
            let f = zip.by_index(i)?;
            let name = f.name().to_ascii_lowercase();
            if !names.insert(name.clone()) { bail!("duplicate archive entry"); }
            if f.enclosed_name().is_none() || f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) { bail!("unsafe archive entry"); }
            if name.starts_with("optifine/") || name.starts_with("net/optifine/") || name.contains("optifinetweaker") || name.contains("optifineforgetweaker") || name.contains("optifineclasstransformer") { bail!("OptiFine is blocked"); }
        }
        let manifest: Manifest = {
            let mut f = zip.by_name("mod.toml").context("missing mod.toml")?;
            if f.size() > 64 * 1024 { bail!("manifest too large"); }
            let mut s = String::new(); f.read_to_string(&mut s)?;
            if ["optifine", "optifabric", "of_hd"].iter().any(|x| s.to_ascii_lowercase().contains(x)) { bail!("OptiFine is blocked"); }
            toml::from_str(&s)?
        };
        let info = manifest.info;
        if !(2..=32).contains(&info.id.len()) || !info.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') { bail!("invalid mod id"); }
        if info.side != "client" || manifest.api.version != "1.0" || manifest.api.min != "1.0" { bail!("unsupported mod side or API version"); }
        if !(1..=128).contains(&manifest.capabilities.memory_mib) { bail!("invalid memory limit"); }
        let mut f = zip.by_name(&info.entry).context("missing component")?;
        if f.size() > 16 * 1024 * 1024 { bail!("component too large"); }
        let mut bytes = Vec::new(); f.read_to_end(&mut bytes)?;
        Ok(Self { id: info.id, name: info.name, version: info.version, hud: manifest.capabilities.hud, input_read: manifest.capabilities.input_read, memory_mib: manifest.capabilities.memory_mib, bytes })
    }
}

/// Owns one component instance. Each call is fuel-bounded; a trap disables this instance.
enum Bindings { Basic(ClientMod), Input(input_bindings::InputMod) }
pub struct ModInstance { store: Store<Host>, bindings: Bindings, disabled: bool }
impl ModInstance {
    pub fn load(package: &Package) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true).consume_fuel(true).cranelift_nan_canonicalization(true).max_wasm_stack(512 * 1024);
        let engine = Engine::new(&config)?;
        let component = Component::new(&engine, &package.bytes).context("invalid WASM component")?;
        let mut linker = Linker::new(&engine);
        if package.input_read { input_bindings::InputMod::add_to_linker(&mut linker, |s: &mut Host| s)?; }
        else { ClientMod::add_to_linker(&mut linker, |s: &mut Host| s)?; }
        let limits = StoreLimitsBuilder::new().memory_size(package.memory_mib * 1024 * 1024).table_elements(65536).instances(16).build();
        let mut store = Store::new(&engine, Host { snapshot: Snapshot::default(), hud: package.hud, draw: None, limits, started: std::time::Instant::now(), input_at: None });
        store.limiter(|s| &mut s.limits);
        store.set_fuel(20_000_000)?;
        let bindings = if package.input_read { Bindings::Input(input_bindings::InputMod::instantiate(&mut store, &component, &linker)?) }
            else { Bindings::Basic(ClientMod::instantiate(&mut store, &component, &linker)?) };
        match &bindings {
            Bindings::Basic(b) => b.rustmc_plugin_lifecycle().call_on_load(&mut store, "1.0", &[])?,
            Bindings::Input(b) => b.rustmc_plugin_lifecycle().call_on_load(&mut store, "1.0", &[])?,
        }.map_err(anyhow::Error::msg)?;
        Ok(Self { store, bindings, disabled: false })
    }
    pub fn render_hud(&mut self, frame: FrameCtx) -> Result<Option<Vec<u8>>> {
        if self.disabled { bail!("mod disabled"); }
        if !self.store.data().hud { return Ok(None); }
        self.store.data_mut().draw = None;
        self.store.set_fuel(8_000_000)?;
        let on_frame = match &self.bindings {
            Bindings::Basic(b) => b.rustmc_plugin_lifecycle().call_on_frame(&mut self.store, frame.clone()),
            Bindings::Input(b) => b.rustmc_plugin_lifecycle().call_on_frame(&mut self.store, frame.clone()),
        };
        if let Err(e) = on_frame {
            self.disabled = true; return Err(e);
        }
        let rendered = match &self.bindings {
            Bindings::Basic(b) => b.rustmc_plugin_render().call_render_hud(&mut self.store, frame),
            Bindings::Input(b) => b.rustmc_plugin_render().call_render_hud(&mut self.store, frame),
        };
        if let Err(e) = rendered {
            self.disabled = true; self.store.data_mut().draw = None; return Err(e);
        }
        Ok(self.store.data_mut().draw.take())
    }
    pub fn mouse(&mut self, event: MouseEvent, at: std::time::Instant, screen_open: bool, player_using_item: Option<bool>) -> Result<()> {
        if self.disabled { bail!("mod disabled"); }
        let Bindings::Input(b) = &self.bindings else { return Ok(()) };
        self.store.data_mut().snapshot.screen_open = screen_open;
        self.store.data_mut().snapshot.player_using_item = player_using_item;
        self.store.data_mut().input_at = Some(at);
        self.store.set_fuel(2_000_000)?;
        let result = b.rustmc_plugin_events().call_on_mouse(&mut self.store, event);
        self.store.data_mut().input_at = None;
        if let Err(e) = result { self.disabled = true; return Err(e); }
        Ok(()) // Observation only; a guest cannot consume input.
    }
    pub fn key(&mut self, event: KeyEvent, at: std::time::Instant, screen_open: bool, player_using_item: Option<bool>) -> Result<()> {
        if self.disabled { bail!("mod disabled"); }
        let Bindings::Input(b) = &self.bindings else { return Ok(()) };
        self.store.data_mut().snapshot.screen_open = screen_open;
        self.store.data_mut().snapshot.player_using_item = player_using_item;
        self.store.data_mut().input_at = Some(at);
        self.store.set_fuel(2_000_000)?;
        let result = b.rustmc_plugin_events().call_on_key(&mut self.store, event);
        self.store.data_mut().input_at = None;
        if let Err(e) = result { self.disabled = true; return Err(e); }
        Ok(())
    }
    fn reset_snapshot(&mut self) { self.store.data_mut().snapshot = Snapshot::default(); }
    pub fn tick(&mut self, snapshot: Snapshot) -> Result<()> {
        if self.disabled { bail!("mod disabled"); }
        self.store.data_mut().snapshot = snapshot;
        self.store.set_fuel(20_000_000)?;
        let tick = match &self.bindings {
            Bindings::Basic(b) => b.rustmc_plugin_lifecycle().call_on_tick(&mut self.store),
            Bindings::Input(b) => b.rustmc_plugin_lifecycle().call_on_tick(&mut self.store),
        };
        if let Err(e) = tick { self.disabled = true; return Err(e); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package() -> Package {
        Package { id: "test".into(), name: "Test".into(), version: "1.0.0".into(), hud: true, input_read: false, memory_mib: 32, bytes: include_bytes!("../tests/data/guest.wasm").to_vec() }
    }
    #[test]
    fn input_component_receives_clicks_without_wasi() {
        let mut p = package(); p.input_read = true;
        p.bytes = include_bytes!("../tests/data/cps.wasm").to_vec();
        let mut m = ModInstance::load(&p).unwrap();
        let mut snapshot = Snapshot::default(); snapshot.player_using_item = Some(false);
        m.tick(snapshot).unwrap();
        m.mouse(MouseEvent { x: 0, y: 0, button: 0, down: true, wheel: 0 }, std::time::Instant::now(), false, Some(false)).unwrap();
        let frame = FrameCtx { partial_ticks: 0., fps: 60, gui_width: 854, gui_height: 480, gui_scale: 1, mouse_x: 0, mouse_y: 0, cam: types::Vec3 { x: 0., y: 0., z: 0. }, cam_yaw: 0., cam_pitch: 0., fov: 70. };
        let output = draw::decode(&m.render_hud(frame).unwrap().unwrap()).unwrap();
        assert!(output.iter().any(|c| matches!(c, draw::Draw::Text { text, .. } if text == "1 CPS")));
        p.input_read = false;
        assert!(ModInstance::load(&p).is_err(), "undeclared input import must fail");
    }

    #[test]
    fn loads_real_rmod_and_rejects_unknown_capabilities() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("rustmc-mods-test-{}.rmod", std::process::id()));
        let write = |caps: &str| {
            let f = File::create(&path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let options = zip::write::SimpleFileOptions::default();
            z.start_file("mod.toml", options).unwrap();
            write!(z, "[mod]\nid='test'\nname='Test'\nversion='1.0.0'\nentry='mod.wasm'\nside='client'\n[api]\nversion='1.0'\nmin='1.0'\n[capabilities]\n{caps}\n").unwrap();
            z.start_file("mod.wasm", options).unwrap();
            z.write_all(include_bytes!("../tests/data/guest.wasm")).unwrap();
            z.finish().unwrap();
        };
        write("hud=true");
        let p = Package::read(&path).unwrap();
        assert_eq!(p.id, "test");
        ModInstance::load(&p).unwrap();
        write("network=['example.com']");
        assert!(Package::read(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn executes_component_and_stops_infinite_loop() {
        let mut m = ModInstance::load(&package()).unwrap();
        let mut snap = Snapshot::default();
        snap.player.food = 19; snap.ticks = 42;
        m.tick(snap.clone()).unwrap();
        let v = types::Vec3 { x: 0., y: 0., z: 0. };
        let frame = FrameCtx { partial_ticks: 0., fps: 60, gui_width: 854, gui_height: 480, gui_scale: 1, mouse_x: 0, mouse_y: 0, cam: v, cam_yaw: 0., cam_pitch: 0., fov: 70. };
        let output = m.render_hud(frame).unwrap().unwrap();
        assert_eq!(draw::decode(&output).unwrap(), vec![draw::Draw::Rect { left: 1, top: 2, right: 3, bottom: 4, argb: 0xff010203 }]);
        snap.ticks = 43;
        assert!(m.tick(snap).is_err());
        assert!(m.tick(Snapshot::default()).is_err());
    }
}
