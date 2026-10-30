//! Loading the initial world and finding the spawn.

use crate::*;

/// Spawn X/Z from `level.dat` (gzipped NBT).
#[cfg(feature = "singleplayer")]
pub(crate) fn read_spawn(world: &Path) -> Option<(i32, i32)> {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&std::fs::read(world.join("level.dat")).ok()?[..])
        .read_to_end(&mut raw)
        .ok()?;
    let (_, root) = nbt::read(&raw).ok()?;
    let data = root.as_compound()?.get("Data")?.as_compound()?;
    Some((data.get("SpawnX")?.as_i64()? as i32, data.get("SpawnZ")?.as_i64()? as i32))
}

#[cfg(feature = "singleplayer")]
pub(crate) fn load_world(args: &Args) -> (World, (i32, i32), i64) {
    let mut world = World::default();
    // A fixed default: the demo world (and so its quad count / benchmarks) must not change per run.
    let seed = args.seed.unwrap_or(0x5eed);
    let mut generator = mc_gen::Generator::new(seed);
    if args.world.is_none() {
        println!("seed {seed}");
    }
    let center = args.pos.or_else(|| args.world.as_deref().and_then(read_spawn)).unwrap_or((8, 8));
    let (ccx, ccz) = (center.0 >> 4, center.1 >> 4);
    for cx in ccx - args.radius..=ccx + args.radius {
        for cz in ccz - args.radius..=ccz + args.radius {
            let chunk = match &args.world {
                Some(dir) => match mc_world::region::load_chunk(dir, cx, cz) {
                    Ok(Some(c)) => c,
                    Ok(None) => continue,
                    Err(e) => {
                        eprintln!("chunk {cx},{cz}: {e}");
                        continue;
                    }
                },
                None => {
                    let mut c = generator.provide_chunk(cx, cz);
                    c.terrain_populated = true;
                    c
                }
            };
            world.insert(chunk);
        }
    }
    if args.world.is_none() {
        // Chunk.populateChunk: a chunk is decorated once it and its +x, +z neighbours exist,
        // in load order (x outer, z inner).
        let t = Instant::now();
        for cx in ccx - args.radius..ccx + args.radius {
            for cz in ccz - args.radius..ccz + args.radius {
                mc_gen::populate::populate(&mut generator, &mut world, cx, cz);
            }
        }
        println!("populated chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    let keys: Vec<(i32, i32)> = world.chunks.keys().copied().collect();
    for (cx, cz) in keys {
        world.tick_chunk_light(cx, cz, false, false);
    }
    world.track_dirty = true;
    world.take_dirty();
    println!("lit chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    (world, center, seed)
}

/// Builds section meshes on all cores (`ChunkRenderDispatcher`'s worker threads), each worker
/// with its own geometry scratch; results come back in job order.
pub(crate) fn mesh_parallel(world: &World, models: &mc_render::models::BlockModels, jobs: &[(i32, i32, i32)], smooth: bool, fancy: bool) -> Vec<((i32, i32, i32), mesh::ChunkMesh)> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(jobs.len().max(1));
    let build = |part: &[(i32, i32, i32)]| {
        let mut g = BlockGeometry::new();
        part.iter().map(|&(x, sy, z)| ((x, sy, z), mesh::build_section(world, models, x, sy, z, smooth, fancy, &mut g))).collect::<Vec<_>>()
    };
    if threads <= 1 {
        return build(jobs);
    }
    let size = jobs.len().div_ceil(threads);
    std::thread::scope(|sc| {
        let handles: Vec<_> = jobs.chunks(size).map(|part| sc.spawn(move || build(part))).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

pub(crate) fn find_player_spawn(world: &World, center: (i32, i32)) -> mc_core::Vec3 {
    let mut candidates = Vec::new();
    for x in center.0 - 16..=center.0 + 16 {
        for z in center.1 - 16..=center.1 + 16 {
            candidates.push((x, z));
        }
    }
    candidates.sort_by_key(|&(x, z)| (x - center.0).pow(2) + (z - center.1).pow(2));
    let mut geometry = BlockGeometry::new();
    for (x, z) in candidates {
        if !world.chunks.contains_key(&(x >> 4, z >> 4)) {
            continue;
        }
        for y in (0..254).rev() {
            if !matches!(world.get(x, y, z).0, 1 | 2 | 3 | 12 | 13 | 24 | 80) {
                continue;
            }
            let p = player::Player::new(mc_core::Vec3::new(x as f64 + 0.5, y as f64 + 1., z as f64 + 0.5));
            if world.light(x, y + 1, z).0 == 15 && player::collisions(world, &mut geometry, &p.bounds().expand(0.7, 0., 0.7)).is_empty() {
                return p.pos;
            }
        }
    }
    let y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);
    mc_core::Vec3::new(center.0 as f64 + 0.5, y as f64 + 1., center.1 as f64 + 0.5)
}
