//! Block model baking, chunk meshing and the wgpu world renderer.

pub mod ao;
pub mod avatar;
pub mod entity_model;
pub mod fluid;
pub mod gpu;
pub mod gui_gpu;
pub mod items;
pub mod lightmap;
pub mod mesh;
pub mod mobs;
pub mod models;
pub mod objects;
pub mod post;
pub mod sky;
mod sky_gpu;
pub mod tile;
pub mod visibility;

pub use gpu::{Camera, Renderer};

#[cfg(test)]
fn headless_adapter(instance: &wgpu::Instance) -> Option<wgpu::Adapter> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    if adapter.is_none() {
        assert!(std::env::var_os("RUSTMC_REQUIRE_GPU").is_none(), "no headless GPU adapter available");
        eprintln!("no headless GPU adapter available; GPU test skipped");
    }
    adapter
}

#[cfg(test)]
fn gpu_test_jar() -> Option<std::path::PathBuf> {
    let jar = mc_testutil::jar();
    if jar.is_none() {
        assert!(std::env::var_os("RUSTMC_REQUIRE_GPU").is_none(), "GPU test needs the vanilla jar");
        eprintln!("vanilla jar unavailable; GPU test skipped");
    }
    jar
}
