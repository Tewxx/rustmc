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
    fn draw_stream_has_fixed_bounds_and_text() {
        let b = fps_draw_stream(60);
        assert_eq!(&b[..5], b"RMD1\0");
        assert!(b.ends_with(b"60 FPS"));
    }
}
