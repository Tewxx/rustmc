//! Headless smoke test for a package.
use mc_mods::{ModInstance, Package, Snapshot, FrameCtx};
use mc_mods::rustmc::plugin::types::Vec3;
fn main() -> anyhow::Result<()> {
    let path = std::env::args_os().nth(1).ok_or_else(|| anyhow::anyhow!("expected package path"))?;
    let package = Package::read(std::path::Path::new(&path))?;
    println!("{} {}", package.id, package.version);
    let mut m = ModInstance::load(&package)?;
    m.tick(Snapshot::default())?;
    let frame = FrameCtx { partial_ticks: 0., fps: 60, gui_width: 854, gui_height: 480, gui_scale: 1, mouse_x: 0, mouse_y: 0, cam: Vec3 { x: 0., y: 0., z: 0. }, cam_yaw: 0., cam_pitch: 0., fov: 70. };
    if let Some(bytes) = m.render_hud(frame)? { println!("{:?}", mc_mods::draw::decode(&bytes)?); }
    Ok(())
}
