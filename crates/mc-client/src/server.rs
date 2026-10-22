//! The integrated server's world ticking (`WorldServer`): the scheduled tick list
//! (`NextTickListEntry` order: time, priority, insertion), random block ticks driven by
//! `updateLCG`, and block updates that notify neighbours, all through [`mc_logic::Level`].
use mc_core::{BlockPos, JavaRandom};
use mc_data::StateId;
use mc_logic::{blocks, flags, Level, Rng};
use mc_world::World;
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub struct TickList {
    /// `(time, priority, id)` → `(pos, block)`
    entries: BTreeMap<(i64, i32, u64), (BlockPos, u16)>,
    pending: HashSet<(BlockPos, u16)>,
    next_id: u64,
}


/// Integrated-server state that lives across ticks.
pub struct Server {
    pub ticks: TickList,
    pub rand: JavaRandom,
    /// `World.updateLCG`
    update_lcg: i32,
}

impl Default for Server {
    fn default() -> Self {
        // updateLCG starts as (new Random()).nextInt(); any value works.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        let mut rand = JavaRandom::new(seed);
        let update_lcg = rand.next_int();
        Server { ticks: TickList::default(), rand, update_lcg }
    }
}

/// What a batch of world logic produced for the client to apply.
#[derive(Default)]
pub struct Effects {
    /// Changed positions (remesh).
    pub edits: Vec<BlockPos>,
    /// `dropBlockAsItemWithChance` calls: (pos, state, chance, fortune).
    pub drops: Vec<(BlockPos, StateId, f32, i32)>,
    /// `playSoundEffect`: (pos, name, volume, pitch).
    pub sounds: Vec<([f64; 3], String, f32, f32)>,
}

/// A `Level` view over the client world plus the server's tick state.
pub struct Lv<'a> {
    pub world: &'a mut World,
    pub server: &'a mut Server,
    pub fx: &'a mut Effects,
    pub total_time: i64,
    pub rules: Option<&'a crate::level::GameRules>,
}

impl Level for Lv<'_> {
    fn state(&mut self, pos: BlockPos) -> StateId {
        self.world.state_at(pos.x, pos.y, pos.z)
    }

    /// `World.setBlockState`: chunk store with `breakBlock`/`onBlockAdded`, light, and with
    /// flag 1 `notifyNeighborsOfStateChange`.
    fn set_state(&mut self, pos: BlockPos, state: StateId, fl: i32) -> bool {
        if !mc_world::is_valid(pos) {
            return false;
        }
        let Some(p) = self.world.chunk_store(pos, state) else { return false };
        let old = p.old;
        if old.block().id != state.block().id {
            blocks::break_block(self, pos, old);
        }
        if self.world.chunk_finish_set(pos, state, p).is_none() {
            return false;
        }
        if old.block().id != state.block().id {
            blocks::on_block_added(self, pos, state);
        }
        let (nb, ob) = (state.block(), old.block());
        if nb.light_opacity != ob.light_opacity || nb.light_value != ob.light_value {
            self.world.check_light(pos);
        }
        if fl & flags::NO_RERENDER == 0 || fl & flags::UPDATE != 0 {
            self.world.mark_block_for_update(pos);
        }
        self.fx.edits.push(pos);
        if fl & flags::NOTIFY != 0 {
            self.notify_neighbors(pos, old.block().id);
        }
        true
    }

    fn light_for(&mut self, sky: bool, pos: BlockPos) -> i32 {
        self.world.light_for(sky, pos)
    }

    fn light(&mut self, pos: BlockPos) -> i32 {
        if !(0..256).contains(&pos.y) {
            return if pos.y < 0 { 0 } else { 15 };
        }
        let (s, b) = self.world.light(pos.x, pos.y, pos.z);
        s.max(b) as i32
    }

    fn is_area_loaded(&mut self, from: BlockPos, to: BlockPos) -> bool {
        self.world.is_area_loaded(from.x, from.y, from.z, to.x, to.y, to.z)
    }

    /// `WorldServer.updateBlockTick` (priority 0)
    fn schedule_update(&mut self, pos: BlockPos, block: u16, delay: i32) {
        if !self.is_area_loaded(pos.offset(-8, -8, -8), pos.offset(8, 8, 8)) {
            return;
        }
        if mc_data::Block::by_id(block).is_none_or(|b| b.material == mc_data::Material::Air) {
            return;
        }
        let t = &mut self.server.ticks;
        if t.pending.insert((pos, block)) {
            t.next_id += 1;
            t.entries.insert((self.total_time + delay as i64, 0, t.next_id), (pos, block));
        }
    }

    fn rng(&mut self, _which: Rng) -> &mut JavaRandom {
        &mut self.server.rand
    }

    fn drop_block_as_item(&mut self, pos: BlockPos, state: StateId, chance: f32, fortune: i32) {
        self.fx.drops.push((pos, state, chance, fortune));
    }

    fn play_sound(&mut self, x: f64, y: f64, z: f64, name: &str, volume: f32, pitch: f32) {
        self.fx.sounds.push(([x, y, z], name.to_string(), volume, pitch));
    }

    fn game_rule(&self, name: &str) -> bool {
        self.rules.is_none_or(|r| r.get_bool(name))
    }

    fn grow_tree(&mut self, pos: BlockPos, sapling: StateId) -> bool {
        let ok = mc_gen::populate::grow_sapling(self.world, &mut self.server.rand, pos, sapling);
        self.fx.edits.push(pos);
        ok
    }

    fn as_level(&mut self) -> &mut dyn Level {
        self
    }
}

impl Lv<'_> {
    /// `WorldServer.tickUpdates(false)`: runs due scheduled ticks, at most 1000.
    pub fn tick_updates(&mut self) {
        let mut due = Vec::new();
        while due.len() < 1000 {
            let Some((&k, _)) = self.server.ticks.entries.first_key_value() else { break };
            if k.0 > self.total_time {
                break;
            }
            let (pos, block) = self.server.ticks.entries.remove(&k).unwrap();
            self.server.ticks.pending.remove(&(pos, block));
            due.push((pos, block));
        }
        for (pos, block) in due {
            if self.is_area_loaded(pos.offset(-8, -8, -8), pos.offset(8, 8, 8)) {
                let state = self.state(pos);
                if state.block().id == block {
                    blocks::update_tick(self, pos, state, Rng::World);
                }
            }
        }
    }

    /// `WorldServer.updateBlocks`' random ticks: `randomTickSpeed` positions per section of
    /// each active chunk, chosen by `updateLCG`.
    pub fn random_ticks(&mut self, chunks: &[(i32, i32)], speed: i32) {
        for &(cx, cz) in chunks {
            for sy in 0..16 {
                let ticking = self.world.chunk(cx, cz).and_then(|c| c.sections[sy].as_ref()).is_some_and(|s| s.tick_count > 0);
                if !ticking {
                    continue;
                }
                for _ in 0..speed {
                    self.server.update_lcg = self.server.update_lcg.wrapping_mul(3).wrapping_add(1013904223);
                    let k = self.server.update_lcg >> 2;
                    let pos = BlockPos::new(cx * 16 + (k & 15), sy as i32 * 16 + (k >> 16 & 15), cz * 16 + (k >> 8 & 15));
                    let state = self.state(pos);
                    if state.block().ticks_randomly {
                        blocks::random_tick(self, pos, state);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_data::ids;

    fn world() -> World {
        let mut w = World::default();
        for x in -2..=2 {
            for z in -2..=2 {
                w.insert(mc_world::chunk::Chunk::new(x, z));
            }
        }
        for x in -8..8 {
            for z in -8..8 {
                w.set_block(BlockPos::new(x, 10, z), 1, 0);
            }
        }
        w
    }

    #[test]
    fn placed_water_flows_through_scheduled_ticks() {
        let (mut w, mut s) = (world(), Server::default());
        let mut fx = Effects::default();
        let mut lv = Lv { world: &mut w, server: &mut s, fx: &mut fx, total_time: 0, rules: None };
        let src = BlockPos::new(0, 11, 0);
        lv.set_state(src, mc_logic::st(ids::FLOWING_WATER, 0), flags::DEFAULT);
        for t in 0..40 {
            lv.total_time = t;
            lv.tick_updates();
        }
        assert_ne!(lv.state(BlockPos::new(3, 11, 0)).block().material, mc_data::Material::Air);
        assert!(lv.state(BlockPos::new(9, 11, 0)).is_air());
    }

    #[test]
    fn grass_spreads_by_random_ticks() {
        let (mut w, mut s) = (world(), Server::default());
        for x in -8..8 {
            for z in -8..8 {
                w.set_block(BlockPos::new(x, 10, z), ids::DIRT, 0);
            }
        }
        // The updateLCG cycle only visits some cells, so seed grass widely.
        for x in (-8..8).step_by(4) {
            for z in (-8..8).step_by(4) {
                w.set_block(BlockPos::new(x, 10, z), ids::GRASS, 0);
            }
        }
        s.update_lcg = 12345;
        let mut fx = Effects::default();
        let mut lv = Lv { world: &mut w, server: &mut s, fx: &mut fx, total_time: 0, rules: None };
        for _ in 0..3000 {
            lv.random_ticks(&[(0, 0), (-1, 0), (0, -1), (-1, -1)], 3);
        }
        let grass = (-8..8).flat_map(|x| (-8..8).map(move |z| (x, z))).filter(|&(x, z)| lv.world.state_at(x, 10, z).block().id == ids::GRASS).count();
        assert!(grass > 16, "{grass}");
    }

    #[test]
    fn saplings_grow_into_trees() {
        let (mut w, mut s) = (world(), Server::default());
        for x in -8..8 {
            for z in -8..8 {
                w.set_block(BlockPos::new(x, 10, z), ids::DIRT, 0);
            }
        }
        let sap = mc_logic::st(ids::SAPLING, 0).set("stage", "1");
        w.set_block_state(BlockPos::new(0, 11, 0), sap);
        let mut fx = Effects::default();
        let mut lv = Lv { world: &mut w, server: &mut s, fx: &mut fx, total_time: 0, rules: None };
        for _ in 0..500 {
            let st = lv.state(BlockPos::new(0, 11, 0));
            if st.block().id != ids::SAPLING {
                break;
            }
            mc_logic::blocks::random_tick(&mut lv, BlockPos::new(0, 11, 0), st);
        }
        assert!(matches!(lv.state(BlockPos::new(0, 11, 0)).block().id, ids::LOG | ids::LOG2));
    }

    #[test]
    fn fire_spreads_and_burns_planks() {
        let (mut w, mut s) = (world(), Server::default());
        for x in -3..=3 {
            w.set_block(BlockPos::new(x, 11, 0), 5, 0);
        }
        let mut fx = Effects::default();
        let mut lv = Lv { world: &mut w, server: &mut s, fx: &mut fx, total_time: 0, rules: None };
        lv.set_state(BlockPos::new(0, 12, 0), mc_logic::st(ids::FIRE, 0), flags::DEFAULT);
        for t in 0..20000 {
            lv.total_time = t;
            lv.tick_updates();
        }
        let planks = (-3..=3).filter(|&x| lv.state(BlockPos::new(x, 11, 0)).block().id == 5).count();
        assert!(planks < 7, "{planks}");
    }

    #[test]
    fn orphaned_leaves_decay() {
        let (mut w, mut s) = (world(), Server::default());
        let leaf = mc_logic::st(ids::LEAVES, 0).set_bool("check_decay", true);
        w.set_block_state(BlockPos::new(0, 12, 0), leaf);
        let mut fx = Effects::default();
        let mut lv = Lv { world: &mut w, server: &mut s, fx: &mut fx, total_time: 0, rules: None };
        mc_logic::blocks::random_tick(&mut lv, BlockPos::new(0, 12, 0), leaf);
        assert!(lv.state(BlockPos::new(0, 12, 0)).is_air());
        assert_eq!(fx.drops.len(), 1);
    }
}
