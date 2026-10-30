//! Native ports of individual LunarForge 0.1.0 behaviors. See the coverage ledger.
use std::collections::VecDeque;

/// LunarForge's click counter keeps separate left, left-while-not-using, and right streams.
#[derive(Default)]
pub struct ClickCounter {
    left: VecDeque<u64>,
    left_not_using: VecDeque<u64>,
    right: VecDeque<u64>,
}
impl ClickCounter {
    /// Only button-down events without an open screen are counted.
    pub fn mouse_down(&mut self, button: u8, now_ms: u64, screen_open: bool, player_using_item: Option<bool>) {
        if screen_open { return; }
        match button {
            0 => {
                self.left.push_back(now_ms);
                if player_using_item == Some(false) { self.left_not_using.push_back(now_ms); }
            }
            1 => self.right.push_back(now_ms),
            _ => {}
        }
    }
    fn count(queue: &mut VecDeque<u64>, now_ms: u64) -> usize {
        let cutoff = now_ms.saturating_sub(1000);
        while queue.front().is_some_and(|&v| v < cutoff) { queue.pop_front(); }
        queue.len()
    }
    pub fn left(&mut self, now_ms: u64, ignore_while_using: bool) -> usize {
        Self::count(if ignore_while_using { &mut self.left_not_using } else { &mut self.left }, now_ms)
    }
    pub fn right(&mut self, now_ms: u64) -> usize { Self::count(&mut self.right, now_ms) }
    pub fn clear_right(&mut self) { self.right.clear(); }
}

/// English FPS label from LunarForge's en_US language asset.
pub fn fps_text(fps: u32, reverse: bool) -> String {
    if reverse { format!("FPS: {fps}") } else { format!("{fps} FPS") }
}

/// CPS display choices and English text, with LunarForge's defaults.
#[derive(Clone, Copy, Debug)]
pub struct CpsOptions {
    pub right_click: bool,
    pub show_text: bool,
    pub reverse: bool,
    pub ignore_while_using: bool,
}
impl Default for CpsOptions {
    fn default() -> Self { Self { right_click: false, show_text: true, reverse: false, ignore_while_using: false } }
}
pub fn cps_text(counter: &mut ClickCounter, now_ms: u64, opts: CpsOptions) -> String {
    let left = counter.left(now_ms, opts.ignore_while_using);
    let right = if opts.right_click { format!("  {}", counter.right(now_ms)) } else { String::new() };
    let label = if opts.show_text { "CPS" } else { "" };
    let space = if opts.show_text { " " } else { "" };
    if opts.reverse { format!("{label}{space}{left}{right}") } else { format!("{left}{right}{space}{label}") }
}

/// The eight cardinal labels used by LunarForge's coordinates HUD.
pub fn direction(yaw: f32) -> Option<&'static str> {
    if !yaw.is_finite() { return None; }
    let mut angle = yaw % 360.0;
    if angle >= 180.0 { angle -= 360.0; }
    if angle < -180.0 { angle += 360.0; }
    let index = (((angle as f64 + 180.0 + 22.5) % 360.0) / 45.0).floor() as usize;
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].get(index).copied()
}

/// The coordinates HUD floors each displayed axis, including negative positions.
pub fn coordinate_blocks(pos: [f64; 3]) -> Option<[i32; 3]> {
    let mut result = [0; 3];
    for (out, value) in result.iter_mut().zip(pos) {
        if !value.is_finite() || value.floor() < i32::MIN as f64 || value.floor() > i32::MAX as f64 { return None; }
        *out = value.floor() as i32;
    }
    Some(result)
}

/// The source's clipboard shortcut uses Java integer casts (truncation), unlike its HUD.
pub fn copy_coordinates(pos: [f64; 3]) -> Option<String> {
    if pos.iter().any(|v| !v.is_finite()) { return None; }
    Some(format!("X: {} Y: {} Z: {}", pos[0] as i32, pos[1] as i32, pos[2] as i32))
}

/// LunarForge's zoom settings. Host-controlled keybind and smooth-camera effects are separate.
#[derive(Clone, Copy, Debug)]
pub struct ZoomOptions {
    pub toggle: bool,
    pub smooth_camera: bool,
    pub smooth_zoom: bool,
    pub variable: bool,
    pub scroll_speed: f32,
    pub divisor: u8,
    pub sensitivity: f32,
}
impl Default for ZoomOptions {
    fn default() -> Self { Self { toggle: false, smooth_camera: true, smooth_zoom: true, variable: false, scroll_speed: 1.0, divisor: 4, sensitivity: 1.0 } }
}
#[derive(Clone, Copy, Debug)]
pub struct ZoomTick { pub active: bool, pub smooth_camera: bool, pub invalidate_renderer: bool }
#[derive(Debug)]
pub struct Zoom {
    active: bool, down: bool, saved_smooth: bool, variable_factor: f32, previous: f32, current: f32,
}
impl Default for Zoom {
    fn default() -> Self { Self { active: false, down: false, saved_smooth: false, variable_factor: 1.0, previous: 1.0, current: 1.0 } }
}
impl Zoom {
    fn divisor(&self, opts: ZoomOptions) -> f32 {
        opts.divisor.clamp(2, 10) as f32 * if opts.variable { self.variable_factor } else { 1.0 }
    }
    /// Called at the start of a client tick. `smooth_camera` is the current user setting.
    pub fn tick(&mut self, opts: ZoomOptions, enabled: bool, world: bool, screen_open: bool, key_down: bool, smooth_camera: bool) -> ZoomTick {
        let pressed = enabled && world && !screen_open && key_down;
        let mut next = if opts.toggle { if pressed && !self.down { !self.active } else { self.active } } else { pressed };
        if !enabled || !world || screen_open { next = false; }
        self.down = pressed;
        let changed = next != self.active;
        let mut camera = smooth_camera;
        if changed {
            if next { self.saved_smooth = smooth_camera; }
            else { camera = self.saved_smooth; self.variable_factor = 1.0; }
            self.active = next;
        }
        if self.active { camera = opts.smooth_camera || self.saved_smooth; }
        self.previous = self.current;
        let target = if self.active { 1.0 / self.divisor(opts) } else { 1.0 };
        self.current = if opts.smooth_zoom { self.current + (target - self.current) * 0.75 } else { target };
        ZoomTick { active: self.active, smooth_camera: camera, invalidate_renderer: changed }
    }
    /// Returns whether the scroll event should be consumed.
    pub fn scroll(&mut self, opts: ZoomOptions, enabled: bool, screen_open: bool, wheel: i32) -> bool {
        if !enabled || !self.active || !opts.variable || screen_open || wheel == 0 { return false; }
        let lower = 1.4 / opts.divisor.clamp(2, 10) as f32;
        self.variable_factor = (self.variable_factor + wheel.signum() as f32 * opts.scroll_speed.clamp(0.25, 5.0) * 0.5).clamp(lower, 10.0);
        true
    }
    pub fn fov(&self, opts: ZoomOptions, enabled: bool, original: f32, partial_ticks: f32) -> f32 {
        if !enabled { return original; }
        let factor = if opts.smooth_zoom { self.previous + (self.current - self.previous) * partial_ticks }
            else if self.active { 1.0 / self.divisor(opts) } else { 1.0 };
        original * factor
    }
    pub fn sensitivity(&self, opts: ZoomOptions, enabled: bool, original: f32) -> f32 {
        if enabled && self.active { original * opts.sensitivity.clamp(0.1, 2.0) } else { original }
    }
}

/// RMD1 output for the currently supported FPS HUD slice. Its layout is provisional until
/// the host exposes text measurement and the LunarForge HUD editor/config interfaces.
pub fn fps_draw_stream(fps: u32) -> Vec<u8> {
    let mut out = b"RMD1".to_vec();
    out.push(0);
    for v in [0i32, 0, 56, 18] { out.extend(v.to_le_bytes()); }
    out.extend(0x6f000000u32.to_le_bytes());
    let text = fps_text(fps, false);
    out.push(3);
    out.extend(4i32.to_le_bytes()); out.extend(5i32.to_le_bytes());
    out.extend(0xffff_ffffu32.to_le_bytes()); out.push(1);
    out.extend((text.len() as u16).to_le_bytes()); out.extend(text.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fps_and_direction_match_observed_defaults() {
        assert_eq!(fps_text(144, false), "144 FPS");
        assert_eq!(fps_text(144, true), "FPS: 144");
        for (yaw, expected) in [(0., "S"), (90., "W"), (-90., "E"), (180., "N"), (-180., "N"), (22.5, "SW")] {
            assert_eq!(direction(yaw), Some(expected));
        }
        assert_eq!(direction(f32::NAN), None);
    }
    #[test]
    fn click_window_and_screen_filter_match_source() {
        let mut c = ClickCounter::default();
        c.mouse_down(0, 1000, false, Some(false));
        c.mouse_down(0, 1200, false, Some(true));
        c.mouse_down(1, 1300, false, Some(true));
        c.mouse_down(0, 1400, true, Some(false));
        assert_eq!(c.left(2000, false), 2);
        assert_eq!(c.left(2000, true), 1);
        assert_eq!(c.left(2001, false), 1);
        assert_eq!(c.right(2001), 1);
        c.clear_right(); assert_eq!(c.right(2001), 0);
    }
    #[test]
    fn cps_labels_follow_options() {
        let mut counter = ClickCounter::default();
        counter.mouse_down(0, 1000, false, Some(false));
        counter.mouse_down(1, 1000, false, Some(false));
        assert_eq!(cps_text(&mut counter, 1000, CpsOptions::default()), "1 CPS");
        let opts = CpsOptions { right_click: true, reverse: true, ..CpsOptions::default() };
        assert_eq!(cps_text(&mut counter, 1000, opts), "CPS 1  1");
        assert_eq!(cps_text(&mut counter, 1000, CpsOptions { show_text: false, ..opts }), "1  1");
    }
    #[test]
    fn coordinates_floor_but_copy_truncates_toward_zero() {
        let pos = [-1.9, 64.9, -0.1];
        assert_eq!(coordinate_blocks(pos), Some([-2, 64, -1]));
        assert_eq!(copy_coordinates(pos).as_deref(), Some("X: -1 Y: 64 Z: 0"));
        assert_eq!(coordinate_blocks([f64::NAN, 0., 0.]), None);
    }
    #[test]
    fn zoom_interpolates_and_restores_camera() {
        let mut zoom = Zoom::default();
        let opts = ZoomOptions::default();
        let start = zoom.tick(opts, true, true, false, true, false);
        assert!(start.active && start.smooth_camera && start.invalidate_renderer);
        assert_eq!(zoom.fov(opts, true, 100.0, 0.5), 71.875);
        let end = zoom.tick(opts, true, true, true, true, true);
        assert!(!end.active && !end.smooth_camera && end.invalidate_renderer);
        assert_eq!(zoom.fov(opts, false, 100.0, 1.0), 100.0);
    }
    #[test]
    fn zoom_toggle_and_variable_scroll_follow_edges() {
        let mut zoom = Zoom::default();
        let opts = ZoomOptions { toggle: true, variable: true, smooth_zoom: false, ..Default::default() };
        assert!(zoom.tick(opts, true, true, false, true, false).active);
        assert!(zoom.tick(opts, true, true, false, true, true).active);
        assert!(zoom.scroll(opts, true, false, 120));
        assert!((zoom.fov(opts, true, 90.0, 1.0) - 15.0).abs() < 0.0001);
        assert!(!zoom.scroll(opts, true, true, 120));
        zoom.tick(opts, true, true, false, false, true);
        assert!(!zoom.tick(opts, true, true, false, true, true).active);
        assert!(!zoom.scroll(opts, true, false, 120));
    }
    #[test]
    fn draw_stream_has_fixed_bounds_and_text() {
        let b = fps_draw_stream(60);
        assert_eq!(&b[..5], b"RMD1\0");
        assert!(b.ends_with(b"60 FPS"));
    }
}
