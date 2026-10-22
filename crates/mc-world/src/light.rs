//! Tests for the vanilla light engine in [`crate::World`]; the engine itself lives there because
//! it is spread over `World` and `Chunk` in vanilla too.

#[cfg(test)]
mod tests {
    use crate::chunk::Chunk;
    use crate::World;
    use mc_core::BlockPos;

    fn flat_world(r: i32, ground: i32) -> World {
        let mut w = World::default();
        for cx in -r..=r {
            for cz in -r..=r {
                let mut c = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..ground {
                            c.set(x, y, z, 1, 0);
                        }
                    }
                }
                c.recount();
                c.generate_skylight_map();
                c.terrain_populated = true;
                w.insert(c);
            }
        }
        for cx in -r..=r {
            for cz in -r..=r {
                w.tick_chunk_light(cx, cz, false, false);
            }
        }
        w
    }

    #[test]
    fn open_sky_is_full_and_caves_are_dark() {
        let mut w = flat_world(2, 64);
        assert_eq!(w.light(5, 64, 5), (15, 0));
        assert_eq!(w.light(5, 30, 5), (0, 0));
        // Dig a one-block pocket: still dark.
        w.set_block(BlockPos::new(8, 30, 8), 0, 0);
        assert_eq!(w.light(8, 30, 8), (0, 0));
    }

    #[test]
    fn light_spreads_under_an_overhang() {
        let mut w = flat_world(2, 64);
        for x in 0..8 {
            for z in -16..32 {
                w.set_block(BlockPos::new(x, 67, z), 1, 0);
            }
        }
        assert_eq!(w.light(8, 65, 5).0, 15);
        assert_eq!(w.light(7, 65, 5).0, 14);
        assert_eq!(w.light(5, 65, 5).0, 12);
        // Remove the roof again: back to full sky.
        for x in 0..8 {
            for z in -16..32 {
                w.set_block(BlockPos::new(x, 67, z), 0, 0);
            }
        }
        assert_eq!(w.light(5, 65, 5).0, 15);
    }

    #[test]
    fn glowstone_emits_and_removal_clears() {
        let mut w = flat_world(2, 64);
        w.set_block(BlockPos::new(8, 100, 8), 89, 0);
        assert_eq!(w.light(9, 100, 8).1, 14);
        assert_eq!(w.light(12, 100, 8).1, 11);
        assert_eq!(w.light(-5, 100, 8).1, 2);
        w.set_block(BlockPos::new(8, 100, 8), 0, 0);
        assert_eq!(w.light(9, 100, 8).1, 0);
        assert_eq!(w.light(-5, 100, 8).1, 0);
    }

    #[test]
    fn torch_light_crosses_chunk_borders() {
        let mut w = flat_world(2, 64);
        w.set_block(BlockPos::new(15, 64, 8), 50, 5);
        assert_eq!(w.light(15, 64, 8).1, 14);
        assert_eq!(w.light(16, 64, 8).1, 13);
        assert_eq!(w.light(20, 64, 8).1, 9);
    }
}
