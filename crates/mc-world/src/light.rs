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

    /// `Block.getLightOpacity`: water 3, leaves 1, so `generateSkylightMap` / `getRawLight`
    /// knock exactly that much off the sky light per block.
    #[test]
    fn water_and_leaves_attenuate_sky_light_by_their_opacity() {
        let mut w = flat_world(1, 64);
        for y in 64..=66 {
            w.set_block(BlockPos::new(5, y, 5), 9, 0); // still water
        }
        assert_eq!(w.light(5, 67, 5).0, 15);
        assert_eq!(w.light(5, 66, 5).0, 12);
        assert_eq!(w.light(5, 65, 5).0, 9);
        assert_eq!(w.light(5, 64, 5).0, 6);

        let mut w = flat_world(1, 64);
        for y in 64..=66 {
            w.set_block(BlockPos::new(5, y, 5), 18, 0); // oak leaves
        }
        assert_eq!(w.light(5, 67, 5).0, 15);
        assert_eq!(w.light(5, 66, 5).0, 14);
        assert_eq!(w.light(5, 65, 5).0, 13);
        assert_eq!(w.light(5, 64, 5).0, 12);
    }

    /// `Block.getMixedBrightnessForBlock`: a slab whose combined light is 0 borrows the light of
    /// the block below (a slab over a torch is unlit by propagation but not by rendering).
    /// `Block.getMixedBrightnessForBlock`: a slab whose combined light is 0 retries at the
    /// block below, which also feeds that block's own emission into `getCombinedLight`. This is
    /// observable when the slab's neighbour light was never computed (a stale light array).
    #[test]
    fn slab_brightness_falls_back_to_the_block_below() {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        c.set(8, 63, 8, 50, 5); // torch
        c.set(8, 64, 8, 44, 0); // stone slab, half=bottom
        w.insert(c);
        assert_eq!(w.light(8, 64, 8), (0, 0));
        assert_eq!(w.combined_light(8, 64, 8, 0), 0);
        // Without the slab retry this would be 0; the torch's emission wins below.
        assert_eq!(w.combined_light(8, 63, 8, 14), 14 << 4);
        assert_eq!(w.mixed_brightness(8, 64, 8), 14 << 4);
    }

    /// `Entity.getBrightnessForRender`: `getCombinedLight` at the eye block, and the unlit `0`
    /// when that position is outside a loaded chunk.
    #[test]
    fn entity_brightness_is_combined_light_at_a_loaded_position() {
        let w = flat_world(1, 64);
        assert_eq!(w.brightness_for_render(BlockPos::new(5, 70, 5)), (15, 0));
        assert_eq!(w.brightness_for_render(BlockPos::new(5, 30, 5)), (0, 0));
        assert_eq!(w.brightness_for_render(BlockPos::new(4000, 70, 4000)), (0, 0));
    }
}
