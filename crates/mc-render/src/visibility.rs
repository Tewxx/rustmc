//! Visibility between the six faces of a section (`VisGraph` / `SetVisibility`).
use mc_core::Facing;

pub const ALL_VISIBLE: u64 = (1u64 << 36) - 1;

pub struct VisGraph {
    opaque: [u64; 64],
    count: usize,
}

impl Default for VisGraph {
    fn default() -> Self { Self { opaque: [0; 64], count: 0 } }
}

impl VisGraph {
    pub fn set_opaque(&mut self, x: usize, y: usize, z: usize) {
        let i = x | (z << 4) | (y << 8);
        let bit = 1u64 << (i & 63);
        if self.opaque[i >> 6] & bit == 0 { self.opaque[i >> 6] |= bit; self.count += 1; }
    }

    /// Java's bit positions are `from.index() + 6 * to.index()`.
    pub fn compute(mut self) -> u64 {
        if self.count < 256 { return ALL_VISIBLE; }
        if self.count == 4096 { return 0; }
        let mut visible = 0u64;
        let mut queue = Vec::with_capacity(4096);
        for start in 0..4096 {
            let x = start & 15; let z = (start >> 4) & 15; let y = start >> 8;
            if x != 0 && x != 15 && y != 0 && y != 15 && z != 0 && z != 15 { continue; }
            if self.opaque[start >> 6] & (1 << (start & 63)) != 0 { continue; }
            self.opaque[start >> 6] |= 1 << (start & 63);
            queue.clear(); queue.push(start); let mut head = 0; let mut faces = 0u8;
            while head < queue.len() {
                let i = queue[head]; head += 1;
                let x = i & 15; let z = (i >> 4) & 15; let y = i >> 8;
                if x == 0 { faces |= 1 << Facing::West.index(); }
                if x == 15 { faces |= 1 << Facing::East.index(); }
                if y == 0 { faces |= 1 << Facing::Down.index(); }
                if y == 15 { faces |= 1 << Facing::Up.index(); }
                if z == 0 { faces |= 1 << Facing::North.index(); }
                if z == 15 { faces |= 1 << Facing::South.index(); }
                for (valid, next) in [
                    (x > 0, i.wrapping_sub(1)), (x < 15, i + 1),
                    (z > 0, i.wrapping_sub(16)), (z < 15, i + 16),
                    (y > 0, i.wrapping_sub(256)), (y < 15, i + 256),
                ] {
                    if valid && self.opaque[next >> 6] & (1 << (next & 63)) == 0 {
                        self.opaque[next >> 6] |= 1 << (next & 63);
                        queue.push(next);
                    }
                }
            }
            for a in 0..6 { for b in 0..6 {
                if faces & (1 << a) != 0 && faces & (1 << b) != 0 { visible |= 1 << (a + 6*b); }
            }}
        }
        visible
    }
}

pub fn connects(mask: u64, from: Facing, to: Facing) -> bool {
    mask & (1 << (from.index() + 6 * to.index())) != 0
}

type SectionKey = (i32, i32, i32);

struct Walk {
    start: SectionKey,
    radius: i32,
    seen: std::collections::HashMap<SectionKey, u8>,
    queue: std::collections::VecDeque<(SectionKey, Option<Facing>)>,
}

impl Walk {
    fn new(start: SectionKey, radius: i32) -> Self {
        let mut seen = std::collections::HashMap::new(); seen.insert(start, 63);
        let mut queue = std::collections::VecDeque::new(); queue.push_back((start, None));
        Self { start, radius, seen, queue }
    }

    fn advance(&mut self, masks: &std::collections::HashMap<SectionKey, u64>, budget: usize) -> bool {
        for _ in 0..budget {
            let Some((key, incoming)) = self.queue.pop_front() else { return true };
            let mask = if key == self.start { ALL_VISIBLE } else { masks.get(&key).copied().unwrap_or(ALL_VISIBLE) };
            for to in Facing::ALL {
                if let Some(from) = incoming { if !connects(mask, from, to) { continue; } }
                let (dx, dy, dz) = to.offset();
                let next = (key.0.saturating_add(dx), key.1.saturating_add(dy), key.2.saturating_add(dz));
                if !(0..16).contains(&next.1) || (next.0 as i64 - self.start.0 as i64).abs() > self.radius as i64 ||
                    (next.2 as i64 - self.start.2 as i64).abs() > self.radius as i64 { continue; }
                let from = to.opposite();
                let entry = self.seen.entry(next).or_insert(0);
                let bit = 1 << from.index();
                if *entry & bit != 0 { continue; }
                if masks.get(&next).copied().unwrap_or(ALL_VISIBLE) == ALL_VISIBLE { *entry = 63; }
                else { *entry |= bit; }
                self.queue.push_back((next, Some(from)));
            }
        }
        self.queue.is_empty()
    }
}

/// Reachable sections, treating absent visibility data and the camera section as open.
pub fn reachable(start: SectionKey, radius: i32, masks: &std::collections::HashMap<SectionKey, u64>) -> std::collections::HashSet<SectionKey> {
    let mut walk = Walk::new(start, radius);
    while !walk.advance(masks, usize::MAX) {}
    walk.seen.into_keys().collect()
}

/// Sections outside the traversed region remain visible; custom models can extend far beyond
/// their nominal section and the renderer has no CPU far-plane culling.
pub fn culls(visible: &std::collections::HashSet<SectionKey>, start: SectionKey, radius: i32, key: SectionKey) -> bool {
    (0..16).contains(&key.1) && (key.0 as i64 - start.0 as i64).abs() <= radius as i64 &&
        (key.2 as i64 - start.2 as i64).abs() <= radius as i64 && !visible.contains(&key)
}

/// Conservative cache. Dirty/missing sections are open until fresh geometry arrives.
pub struct CaveCull {
    masks: std::collections::HashMap<(i32, i32, i32), u64>,
    cached: Option<(SectionKey, i32, std::sync::Arc<std::collections::HashSet<SectionKey>>)>,
    walk: Option<Walk>,
    changed: std::time::Instant,
}

impl Default for CaveCull {
    fn default() -> Self { Self { masks: Default::default(), cached: None, walk: None, changed: std::time::Instant::now() } }
}

impl CaveCull {
    pub fn clear(&mut self) { self.masks.clear(); self.cached = None; self.walk = None; self.changed = std::time::Instant::now(); }

    pub fn update(&mut self, key: (i32, i32, i32), mask: Option<u64>) {
        let old = self.masks.get(&key).copied();
        let new = mask.filter(|&m| m != ALL_VISIBLE);
        if old == new { return; }
        if let Some(m) = new { self.masks.insert(key, m); } else { self.masks.remove(&key); }
        self.cached = None;
        self.walk = None;
        self.changed = std::time::Instant::now();
    }

    /// While visibility is changing, draw everything instead of blocking the render thread.
    pub fn visible(&mut self, start: (i32, i32, i32), radius: i32) -> Option<std::sync::Arc<std::collections::HashSet<(i32, i32, i32)>>> {
        if self.masks.is_empty() || !(0..16).contains(&start.1) || !(1..=32).contains(&radius) ||
            self.changed.elapsed() < std::time::Duration::from_millis(100) { return None; }
        if let Some((s, r, visible)) = &self.cached {
            if *s == start && *r == radius { return Some(visible.clone()); }
        }
        if self.walk.as_ref().is_none_or(|w| w.start != start || w.radius != radius) {
            self.walk = Some(Walk::new(start, radius));
        }
        if self.walk.as_mut().unwrap().advance(&self.masks, 2048) {
            let walk = self.walk.take().unwrap();
            let visible = std::sync::Arc::new(walk.seen.into_keys().collect());
            self.cached = Some((start, radius, visible));
        }
        self.cached.as_ref().filter(|(s, r, _)| *s == start && *r == radius).map(|(_, _, visible)| visible.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run with `cargo test --release -p mc-render visibility_headless_bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual headless CPU benchmark"]
    fn visibility_headless_bench() {
        for radius in [8, 18, 32] {
            let mut masks = std::collections::HashMap::new();
            for y in 0..16 { for z in -radius..=radius { masks.insert((1, y, z), 0); } }
            let mut walk = Walk::new((0, 8, 0), radius);
            let started = std::time::Instant::now(); let mut worst = std::time::Duration::ZERO; let mut passes = 0;
            loop {
                let t = std::time::Instant::now(); let done = walk.advance(&masks, 2048);
                worst = worst.max(t.elapsed()); passes += 1;
                if done { break; }
            }
            let all = ((radius * 2 + 1).pow(2) * 16) as usize;
            eprintln!("cave traversal radius {radius}: reached {}/{} sections, {} steps, {:.2} ms total, {:.2} ms worst step (CPU only)",
                walk.seen.len(), all, passes, started.elapsed().as_secs_f64()*1000., worst.as_secs_f64()*1000.);
            assert!(walk.seen.len() < all);
        }
    }

    #[test]
    fn traversal_preserves_distinct_entry_faces() {
        let mut masks = std::collections::HashMap::new();
        for x in -2..=2 { for y in 0..16 { for z in -2..=2 { masks.insert((x, y, z), 0); } } }
        let start = (0, 8, 0);
        for key in [(0, 8, -1), (1, 8, -1), (2, 8, 0), (1, 8, 1)] { masks.remove(&key); }
        let pair = |a: Facing, b: Facing| (1u64 << (a.index() + 6*b.index())) | (1u64 << (b.index() + 6*a.index()));
        masks.insert((1, 8, 0), pair(Facing::West, Facing::East) | pair(Facing::North, Facing::South));
        let seen = reachable(start, 2, &masks);
        assert!(seen.contains(&(2, 8, 0)));
        assert!(seen.contains(&(1, 8, 1)));
    }

    #[test]
    fn dirty_graphs_disable_culling_until_rebuilt() {
        let mut c = CaveCull::default();
        for y in 0..16 { for z in -2..=2 { c.update((1, y, z), Some(0)); } }
        c.changed = std::time::Instant::now() - std::time::Duration::from_millis(101);
        let start = (0, 8, 0);
        let visible = loop { if let Some(v) = c.visible(start, 2) { break v; } };
        assert!(culls(&visible, start, 2, (2, 8, 0)));
        c.update((1, 8, 0), None);
        assert!(c.visible(start, 2).is_none());
        c.changed = std::time::Instant::now() - std::time::Duration::from_millis(101);
        let visible = loop { if let Some(v) = c.visible(start, 2) { break v; } };
        assert!(!culls(&visible, start, 2, (2, 8, 0)));
        assert!(c.visible(start, 33).is_none());
    }

    #[test]
    fn traversal_handles_closed_and_unknown_sections_conservatively() {
        let mut masks = std::collections::HashMap::new();
        for y in 0..16 { for z in -2..=2 { masks.insert((1, y, z), 0); } }
        let start = (0, 8, 0);
        let closed = reachable(start, 2, &masks);
        assert!(closed.contains(&(1, 8, 0)));
        assert!(!closed.contains(&(2, 8, 0)));
        assert!(culls(&closed, start, 2, (2, 8, 0)));
        assert!(!culls(&closed, start, 2, (3, 8, 0)));
        masks.remove(&(1, 8, 0));
        let unknown = reachable(start, 2, &masks);
        assert!(unknown.contains(&(2, 8, 0)));
        masks.insert(start, 0);
        assert!(reachable(start, 2, &masks).contains(&(1, 8, 0)));
    }

    #[test]
    fn matches_java_visgraph_fixtures() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../tests/data/visgraph.json")).unwrap();
        for row in fixtures.as_array().unwrap() {
            let mut graph = VisGraph::default();
            for v in row["opaque"].as_array().unwrap() {
                let i = v.as_u64().unwrap() as usize;
                graph.set_opaque(i & 15, i >> 8, i >> 4 & 15);
            }
            assert_eq!(graph.compute(), row["mask"].as_u64().unwrap(), "{}", row["name"]);
        }
    }
}
