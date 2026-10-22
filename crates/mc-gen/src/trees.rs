//! Tree and huge mushroom generators (`WorldGenTrees`, `WorldGenBigTree`, `WorldGenHugeTrees`...).
use crate::biome::{GenBiome, Kind};
use crate::populate::{state, Pop};
use crate::terrain::Generator;
use mc_core::math;
use mc_core::random::JavaRandom;
use mc_core::{BlockPos, Facing};
use mc_data::{ids, Material, StateId};
use mc_logic::Level;

/// A tree generator picked by `genBigTreeChance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tree {
    /// `WorldGenTrees(false)`
    Oak,
    /// `WorldGenForest(false, false)`
    Birch,
    /// `WorldGenForest(false, true)`
    TallBirch,
    /// `WorldGenTaiga2`
    Spruce,
    /// `WorldGenTaiga1`
    Pine,
    Swamp,
    Savanna,
    Canopy,
    /// The biome's persistent `worldGeneratorBigTree`.
    BigOak,
    /// `WorldGenTrees(false, height, jungle, jungle, true)`
    Jungle(i32),
    /// `WorldGenShrub(jungle log, oak leaves)`
    JungleShrub,
    MegaJungle,
    /// `WorldGenMegaPineTree(false, useBaseHeight)`
    MegaPine(bool),
}

fn log(variant: &str) -> StateId {
    state("minecraft:log").set("variant", variant)
}
fn leaves(variant: &str) -> StateId {
    state("minecraft:leaves").set("variant", variant).set_bool("check_decay", false)
}
fn log2(variant: &str) -> StateId {
    state("minecraft:log2").set("variant", variant)
}
fn leaves2(variant: &str) -> StateId {
    state("minecraft:leaves2").set("variant", variant).set_bool("check_decay", false)
}

/// `WorldGenAbstractTree.func_150523_a`: blocks a tree may grow through.
fn can_grow_into(s: StateId) -> bool {
    let b = s.block();
    matches!(b.material, Material::Air | Material::Leaves)
        || matches!(b.id, ids::GRASS | ids::DIRT | ids::LOG | ids::LOG2 | ids::SAPLING | ids::VINE)
}

fn air_or_leaves(p: &mut Pop, pos: BlockPos) -> bool {
    matches!(p.mat(pos), Material::Air | Material::Leaves)
}

fn full_block(p: &mut Pop, pos: BlockPos) -> bool {
    p.state(pos).block().full_block
}

/// `func_175921_a`: turn the soil under the trunk into dirt.
fn set_dirt(p: &mut Pop, pos: BlockPos) {
    if p.id(pos) != ids::DIRT {
        p.put(pos, state("minecraft:dirt"));
    }
}

/// `generate` followed by `func_180711_a` on success, as the decorators call it.
pub fn generate(g: &mut Generator, p: &mut Pop, b: &'static GenBiome, tree: Tree, pos: BlockPos) {
    let ok = match tree {
        Tree::Oak => small(p, pos, 4, log("oak"), leaves("oak"), false),
        Tree::Jungle(h) => small(p, pos, h, log("jungle"), leaves("jungle"), true),
        Tree::Birch => forest(p, pos, false),
        Tree::TallBirch => forest(p, pos, true),
        Tree::Spruce => taiga2(p, pos),
        Tree::Pine => taiga1(p, pos),
        Tree::Swamp => swamp(p, pos),
        Tree::Savanna => savanna(p, pos),
        Tree::Canopy => canopy(p, pos),
        Tree::JungleShrub => shrub(p, pos, log("jungle"), leaves("oak")),
        Tree::MegaJungle => mega_jungle(p, pos),
        Tree::MegaPine(base) => mega_pine(p, pos, base),
        Tree::BigOak => {
            // `genBigTreeChance` of a mutated biome hands out its base biome's generator.
            let owner = if b.kind == Kind::Mutated { b.base } else { b.id };
            let mut limit = g.decor.big_tree_height.get(&owner).copied().unwrap_or(0);
            let ok = big_tree(p, pos, &mut limit);
            g.decor.big_tree_height.insert(owner, limit);
            ok
        }
    };
    if ok {
        if let Tree::MegaPine(_) = tree {
            podzol(p, pos);
        }
    }
}

/// Standard shape check of the small trees: radius 0 at the base, 1 along the trunk, `top`
/// in the top two layers.
fn check_space(p: &mut Pop, pos: BlockPos, height: i32, top: i32, ok: impl Fn(&mut Pop, BlockPos, i32) -> bool) -> bool {
    let mut flag = true;
    for j in pos.y..=pos.y + 1 + height {
        let mut k = 1;
        if j == pos.y {
            k = 0;
        }
        if j >= pos.y + 1 + height - 2 {
            k = top;
        }
        let mut l = pos.x - k;
        while l <= pos.x + k && flag {
            let mut i1 = pos.z - k;
            while i1 <= pos.z + k && flag {
                if (0..256).contains(&j) {
                    if !ok(p, BlockPos::new(l, j, i1), j) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                i1 += 1;
            }
            l += 1;
        }
    }
    flag
}

fn vine(facing_prop: &str) -> StateId {
    state("minecraft:vine").set_bool(facing_prop, true)
}

/// `WorldGenTrees`
fn small(p: &mut Pop, pos: BlockPos, min_height: i32, wood: StateId, leaf: StateId, vines: bool) -> bool {
    let i = p.ni(3) + min_height;
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    if !check_space(p, pos, i, 2, |p, q, _| {
        let s = p.state(q);
        can_grow_into(s)
    }) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT | ids::FARMLAND) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let l2 = 0;
    for i3 in pos.y - 3 + i..=pos.y + i {
        let i4 = i3 - (pos.y + i);
        let j1 = l2 + 1 - i4 / 2;
        for k1 in pos.x - j1..=pos.x + j1 {
            let l1 = k1 - pos.x;
            for i2 in pos.z - j1..=pos.z + j1 {
                let j2 = i2 - pos.z;
                if l1.abs() != j1 || j2.abs() != j1 || p.ni(2) != 0 && i4 != 0 {
                    let bp = BlockPos::new(k1, i3, i2);
                    if matches!(p.mat(bp), Material::Air | Material::Leaves | Material::Vine) {
                        p.put(bp, leaf);
                    }
                }
            }
        }
    }
    for j3 in 0..i {
        let up = pos.offset(0, j3, 0);
        if matches!(p.mat(up), Material::Air | Material::Leaves | Material::Vine) {
            p.put(up, wood);
            if vines && j3 > 0 {
                for (dx, dz, prop) in [(-1, 0, "east"), (1, 0, "west"), (0, -1, "south"), (0, 1, "north")] {
                    let q = pos.offset(dx, j3, dz);
                    if p.ni(3) > 0 && p.air(q) {
                        p.put(q, vine(prop));
                    }
                }
            }
        }
    }
    if vines {
        for k3 in pos.y - 3 + i..=pos.y + i {
            let j4 = k3 - (pos.y + i);
            let k4 = 2 - j4 / 2;
            for l4 in pos.x - k4..=pos.x + k4 {
                for i5 in pos.z - k4..=pos.z + k4 {
                    let m = BlockPos::new(l4, k3, i5);
                    if p.mat(m) == Material::Leaves {
                        hanging_vines(p, m);
                    }
                }
            }
        }
        if p.ni(5) == 0 && i > 5 {
            for l3 in 0..2 {
                for f in Facing::PLANE_HORIZONTAL {
                    if p.ni(4 - l3) == 0 {
                        let o = f.opposite();
                        let age = p.ni(3);
                        let at = pos.offset(o.offset().0, i - 5 + l3, o.offset().2);
                        p.put(at, state("minecraft:cocoa").set("age", &age.to_string()).set("facing", f.name()));
                    }
                }
            }
        }
    }
    true
}

/// The leaf-side vines of jungle and swamp trees: west/east/north/south, each 1 in 4, hanging
/// down up to 4 more blocks.
fn hanging_vines(p: &mut Pop, m: BlockPos) {
    for (q, prop) in [(m.offset(-1, 0, 0), "east"), (m.offset(1, 0, 0), "west"), (m.offset(0, 0, -1), "south"), (m.offset(0, 0, 1), "north")] {
        if p.ni(4) == 0 && p.mat(q) == Material::Air {
            let s = vine(prop);
            p.put(q, s);
            let mut i = 4;
            let mut d = q.down();
            while p.mat(d) == Material::Air && i > 0 {
                p.put(d, s);
                d = d.down();
                i -= 1;
            }
        }
    }
}

/// `WorldGenForest` (birch)
fn forest(p: &mut Pop, pos: BlockPos, tall: bool) -> bool {
    let mut i = p.ni(3) + 5;
    if tall {
        i += p.ni(7);
    }
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    if !check_space(p, pos, i, 2, |p, q, _| {
        let s = p.state(q);
        can_grow_into(s)
    }) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT | ids::FARMLAND) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let leaf = leaves("birch");
    for i2 in pos.y - 3 + i..=pos.y + i {
        let k2 = i2 - (pos.y + i);
        let l2 = 1 - k2 / 2;
        for i3 in pos.x - l2..=pos.x + l2 {
            let j1 = i3 - pos.x;
            for k1 in pos.z - l2..=pos.z + l2 {
                let l1 = k1 - pos.z;
                if j1.abs() != l2 || l1.abs() != l2 || p.ni(2) != 0 && k2 != 0 {
                    let bp = BlockPos::new(i3, i2, k1);
                    if air_or_leaves(p, bp) {
                        p.put(bp, leaf);
                    }
                }
            }
        }
    }
    for j2 in 0..i {
        let up = pos.offset(0, j2, 0);
        if air_or_leaves(p, up) {
            p.put(up, log("birch"));
        }
    }
    true
}

/// Space check of the spruces: radius 0 below `j`, else `r`.
fn check_space_spruce(p: &mut Pop, pos: BlockPos, i: i32, j: i32, r: i32, ok: fn(StateId) -> bool) -> bool {
    let mut flag = true;
    let mut i1 = pos.y;
    while i1 <= pos.y + 1 + i && flag {
        let j1 = if i1 - pos.y < j { 0 } else { r };
        let mut k1 = pos.x - j1;
        while k1 <= pos.x + j1 && flag {
            let mut l1 = pos.z - j1;
            while l1 <= pos.z + j1 && flag {
                if (0..256).contains(&i1) {
                    let s = p.state(BlockPos::new(k1, i1, l1));
                    if !ok(s) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                l1 += 1;
            }
            k1 += 1;
        }
        i1 += 1;
    }
    flag
}

/// `WorldGenTaiga1` (pine)
fn taiga1(p: &mut Pop, pos: BlockPos) -> bool {
    let i = p.ni(5) + 7;
    let j = i - p.ni(2) - 3;
    let k = i - j;
    let l = 1 + p.ni(k + 1);
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    if !check_space_spruce(p, pos, i, j, l, can_grow_into) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let leaf = leaves("spruce");
    let mut k2 = 0;
    let mut l2 = pos.y + i;
    while l2 >= pos.y + j {
        for j3 in pos.x - k2..=pos.x + k2 {
            let k3 = j3 - pos.x;
            for i2 in pos.z - k2..=pos.z + k2 {
                let j2 = i2 - pos.z;
                if k3.abs() != k2 || j2.abs() != k2 || k2 <= 0 {
                    let bp = BlockPos::new(j3, l2, i2);
                    if !full_block(p, bp) {
                        p.put(bp, leaf);
                    }
                }
            }
        }
        if k2 >= 1 && l2 == pos.y + j + 1 {
            k2 -= 1;
        } else if k2 < l {
            k2 += 1;
        }
        l2 -= 1;
    }
    for i3 in 0..i - 1 {
        let up = pos.offset(0, i3, 0);
        if air_or_leaves(p, up) {
            p.put(up, log("spruce"));
        }
    }
    true
}

/// `WorldGenTaiga2` (spruce)
fn taiga2(p: &mut Pop, pos: BlockPos) -> bool {
    let i = p.ni(4) + 6;
    let j = 1 + p.ni(2);
    let k = i - j;
    let l = 2 + p.ni(2);
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    if !check_space_spruce(p, pos, i, j, l, |s| matches!(s.block().material, Material::Air | Material::Leaves)) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT | ids::FARMLAND) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let leaf = leaves("spruce");
    let mut i3 = p.ni(2);
    let mut j3 = 1;
    let mut k3 = 0;
    for l3 in 0..=k {
        let j4 = pos.y + i - l3;
        for i2 in pos.x - i3..=pos.x + i3 {
            let j2 = i2 - pos.x;
            for k2 in pos.z - i3..=pos.z + i3 {
                let l2 = k2 - pos.z;
                if j2.abs() != i3 || l2.abs() != i3 || i3 <= 0 {
                    let bp = BlockPos::new(i2, j4, k2);
                    if !full_block(p, bp) {
                        p.put(bp, leaf);
                    }
                }
            }
        }
        if i3 >= j3 {
            i3 = k3;
            k3 = 1;
            j3 += 1;
            if j3 > l {
                j3 = l;
            }
        } else {
            i3 += 1;
        }
    }
    let i4 = p.ni(3);
    for k4 in 0..i - i4 {
        let up = pos.offset(0, k4, 0);
        if air_or_leaves(p, up) {
            p.put(up, log("spruce"));
        }
    }
    true
}

/// `WorldGenSwamp`
fn swamp(p: &mut Pop, mut pos: BlockPos) -> bool {
    let i = p.ni(4) + 5;
    while p.mat(pos.down()) == Material::Water {
        pos = pos.down();
    }
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    let base_y = pos.y;
    if !check_space(p, pos, i, 3, |p, q, j| {
        let s = p.state(q);
        let b = s.block();
        if b.material != Material::Air && b.material != Material::Leaves {
            if b.id != ids::WATER && b.id != ids::FLOWING_WATER {
                return false;
            } else if j > base_y {
                return false;
            }
        }
        true
    }) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let leaf = leaves("oak");
    for l1 in pos.y - 3 + i..=pos.y + i {
        let k2 = l1 - (pos.y + i);
        let i3 = 2 - k2 / 2;
        for k3 in pos.x - i3..=pos.x + i3 {
            let l3 = k3 - pos.x;
            for j1 in pos.z - i3..=pos.z + i3 {
                let k1 = j1 - pos.z;
                if l3.abs() != i3 || k1.abs() != i3 || p.ni(2) != 0 && k2 != 0 {
                    let bp = BlockPos::new(k3, l1, j1);
                    if !full_block(p, bp) {
                        p.put(bp, leaf);
                    }
                }
            }
        }
    }
    for i2 in 0..i {
        let up = pos.offset(0, i2, 0);
        let s = p.state(up);
        let b = s.block();
        if matches!(b.material, Material::Air | Material::Leaves) || b.id == ids::FLOWING_WATER || b.id == ids::WATER {
            p.put(up, log("oak"));
        }
    }
    for j2 in pos.y - 3 + i..=pos.y + i {
        let l2 = j2 - (pos.y + i);
        let j3 = 2 - l2 / 2;
        for i4 in pos.x - j3..=pos.x + j3 {
            for j4 in pos.z - j3..=pos.z + j3 {
                let m = BlockPos::new(i4, j2, j4);
                if p.mat(m) == Material::Leaves {
                    hanging_vines(p, m);
                }
            }
        }
    }
    true
}

/// `WorldGenShrub`
fn shrub(p: &mut Pop, mut pos: BlockPos, wood: StateId, leaf: StateId) -> bool {
    while air_or_leaves(p, pos) && pos.y > 0 {
        pos = pos.down();
    }
    let b = p.id(pos);
    if b == ids::DIRT || b == ids::GRASS {
        pos = pos.up();
        p.put(pos, wood);
        for i in pos.y..=pos.y + 2 {
            let j = i - pos.y;
            let k = 2 - j;
            for l in pos.x - k..=pos.x + k {
                let i1 = l - pos.x;
                for j1 in pos.z - k..=pos.z + k {
                    let k1 = j1 - pos.z;
                    if i1.abs() != k || k1.abs() != k || p.ni(2) != 0 {
                        let bp = BlockPos::new(l, i, j1);
                        if !full_block(p, bp) {
                            p.put(bp, leaf);
                        }
                    }
                }
            }
        }
    }
    true
}

fn random_horizontal(p: &mut Pop) -> Facing {
    Facing::PLANE_HORIZONTAL[p.ni(4) as usize]
}

/// `WorldGenSavannaTree`
fn savanna(p: &mut Pop, pos: BlockPos) -> bool {
    let i = p.ni(3) + p.ni(3) + 5;
    if !(pos.y >= 1 && pos.y + i + 1 <= 256) {
        return false;
    }
    if !check_space(p, pos, i, 2, |p, q, _| {
        let s = p.state(q);
        can_grow_into(s)
    }) {
        return false;
    }
    let below = p.id(pos.down());
    if !(matches!(below, ids::GRASS | ids::DIRT) && pos.y < 256 - i - 1) {
        return false;
    }
    set_dirt(p, pos.down());
    let wood = log2("acacia");
    let leaf = leaves2("acacia");
    let put_leaf = |p: &mut Pop, q: BlockPos| {
        if air_or_leaves(p, q) {
            p.put(q, leaf);
        }
    };
    let f = random_horizontal(p);
    let k2 = i - p.ni(4) - 1;
    let mut l2 = 3 - p.ni(3);
    let (mut i3, mut j1) = (pos.x, pos.z);
    let mut k1 = 0;
    for l1 in 0..i {
        let i2 = pos.y + l1;
        if l1 >= k2 && l2 > 0 {
            i3 += f.offset().0;
            j1 += f.offset().2;
            l2 -= 1;
        }
        let bp = BlockPos::new(i3, i2, j1);
        if air_or_leaves(p, bp) {
            p.put(bp, wood);
            k1 = i2;
        }
    }
    let mut b2 = BlockPos::new(i3, k1, j1);
    for j3 in -3i32..=3 {
        for i4 in -3i32..=3 {
            if j3.abs() != 3 || i4.abs() != 3 {
                put_leaf(p, b2.offset(j3, 0, i4));
            }
        }
    }
    b2 = b2.up();
    for k3 in -1..=1 {
        for j4 in -1..=1 {
            put_leaf(p, b2.offset(k3, 0, j4));
        }
    }
    put_leaf(p, b2.offset(2, 0, 0));
    put_leaf(p, b2.offset(-2, 0, 0));
    put_leaf(p, b2.offset(0, 0, 2));
    put_leaf(p, b2.offset(0, 0, -2));
    let (mut i3, mut j1) = (pos.x, pos.z);
    let f1 = random_horizontal(p);
    if f1 != f {
        let l3 = k2 - p.ni(2) - 1;
        let mut k4 = 1 + p.ni(3);
        let mut k1 = 0;
        let mut l4 = l3;
        while l4 < i && k4 > 0 {
            if l4 >= 1 {
                let j2 = pos.y + l4;
                i3 += f1.offset().0;
                j1 += f1.offset().2;
                let bp = BlockPos::new(i3, j2, j1);
                if air_or_leaves(p, bp) {
                    p.put(bp, wood);
                    k1 = j2;
                }
            }
            l4 += 1;
            k4 -= 1;
        }
        if k1 > 0 {
            let mut b3 = BlockPos::new(i3, k1, j1);
            for i5 in -2i32..=2 {
                for k5 in -2i32..=2 {
                    if i5.abs() != 2 || k5.abs() != 2 {
                        put_leaf(p, b3.offset(i5, 0, k5));
                    }
                }
            }
            b3 = b3.up();
            for j5 in -1..=1 {
                for l5 in -1..=1 {
                    put_leaf(p, b3.offset(j5, 0, l5));
                }
            }
        }
    }
    true
}

/// `WorldGenCanopyTree` (dark oak)
fn canopy(p: &mut Pop, pos: BlockPos) -> bool {
    let i = p.ni(3) + p.ni(2) + 6;
    let (j, k, l) = (pos.x, pos.y, pos.z);
    if !(k >= 1 && k + i + 1 < 256) {
        return false;
    }
    let below = pos.down();
    if !matches!(p.id(below), ids::GRASS | ids::DIRT) {
        return false;
    }
    for ly in 0..=i + 1 {
        let mut i1 = 1;
        if ly == 0 {
            i1 = 0;
        }
        if ly >= i - 1 {
            i1 = 2;
        }
        for j1 in -i1..=i1 {
            for k1 in -i1..=i1 {
                let s = p.state(BlockPos::new(j + j1, k + ly, l + k1));
                if !can_grow_into(s) {
                    return false;
                }
            }
        }
    }
    set_dirt(p, below);
    set_dirt(p, below.offset(1, 0, 0));
    set_dirt(p, below.offset(0, 0, 1));
    set_dirt(p, below.offset(1, 0, 1));
    let wood = log2("dark_oak");
    let leaf = leaves2("dark_oak");
    let put_log = |p: &mut Pop, q: BlockPos| {
        let s = p.state(q);
        if can_grow_into(s) {
            p.put(q, wood);
        }
    };
    let put_leaf = |p: &mut Pop, x: i32, y: i32, z: i32| {
        let q = BlockPos::new(x, y, z);
        if p.mat(q) == Material::Air {
            p.put(q, leaf);
        }
    };
    let f = random_horizontal(p);
    let i1 = i - p.ni(4);
    let mut j1 = 2 - p.ni(3);
    let (mut k1, mut l1) = (j, l);
    let i2 = k + i - 1;
    for j2 in 0..i {
        if j2 >= i1 && j1 > 0 {
            k1 += f.offset().0;
            l1 += f.offset().2;
            j1 -= 1;
        }
        let b1 = BlockPos::new(k1, k + j2, l1);
        if air_or_leaves(p, b1) {
            put_log(p, b1);
            put_log(p, b1.offset(1, 0, 0));
            put_log(p, b1.offset(0, 0, 1));
            put_log(p, b1.offset(1, 0, 1));
        }
    }
    for i3 in -2..=0 {
        for l3 in -2..=0 {
            let mut k4 = -1;
            put_leaf(p, k1 + i3, i2 + k4, l1 + l3);
            put_leaf(p, 1 + k1 - i3, i2 + k4, l1 + l3);
            put_leaf(p, k1 + i3, i2 + k4, 1 + l1 - l3);
            put_leaf(p, 1 + k1 - i3, i2 + k4, 1 + l1 - l3);
            if (i3 > -2 || l3 > -1) && (i3 != -1 || l3 != -2) {
                k4 = 1;
                put_leaf(p, k1 + i3, i2 + k4, l1 + l3);
                put_leaf(p, 1 + k1 - i3, i2 + k4, l1 + l3);
                put_leaf(p, k1 + i3, i2 + k4, 1 + l1 - l3);
                put_leaf(p, 1 + k1 - i3, i2 + k4, 1 + l1 - l3);
            }
        }
    }
    if p.rand.next_boolean() {
        put_leaf(p, k1, i2 + 2, l1);
        put_leaf(p, k1 + 1, i2 + 2, l1);
        put_leaf(p, k1 + 1, i2 + 2, l1 + 1);
        put_leaf(p, k1, i2 + 2, l1 + 1);
    }
    for j3 in -3i32..=4 {
        for i4 in -3i32..=4 {
            if (j3 != -3 || i4 != -3) && (j3 != -3 || i4 != 4) && (j3 != 4 || i4 != -3) && (j3 != 4 || i4 != 4) && (j3.abs() < 3 || i4.abs() < 3) {
                put_leaf(p, k1 + j3, i2, l1 + i4);
            }
        }
    }
    for k3 in -1..=2 {
        for j4 in -1..=2 {
            if (k3 < 0 || k3 > 1 || j4 < 0 || j4 > 1) && p.ni(3) <= 0 {
                let l4 = p.ni(3) + 2;
                for i5 in 0..l4 {
                    put_log(p, BlockPos::new(j + k3, i2 - i5 - 1, l + j4));
                }
                for j5 in -1..=1 {
                    for l2 in -1..=1 {
                        put_leaf(p, k1 + k3 + j5, i2, l1 + j4 + l2);
                    }
                }
                for k5 in -2i32..=2 {
                    for l5 in -2i32..=2 {
                        if k5.abs() != 2 || l5.abs() != 2 {
                            put_leaf(p, k1 + k3 + k5, i2 - 1, l1 + j4 + l5);
                        }
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// WorldGenHugeTrees

fn huge_height(p: &mut Pop, base: i32, extra: i32) -> i32 {
    let mut i = p.ni(3) + base;
    if extra > 1 {
        i += p.ni(extra);
    }
    i
}

/// `func_175929_a`: space check and dirt under the 2x2 trunk.
fn huge_check(p: &mut Pop, pos: BlockPos, h: i32) -> bool {
    if !(pos.y >= 1 && pos.y + h + 1 <= 256) {
        return false;
    }
    let mut flag = true;
    for i in 0..=1 + h {
        let j = if i == 0 { 1 } else { 2 };
        let mut k = -j;
        while k <= j && flag {
            let mut l = -j;
            while l <= j && flag {
                if pos.y + i < 0 || pos.y + i >= 256 || !can_grow_into(p.state(pos.offset(k, i, l))) {
                    flag = false;
                }
                l += 1;
            }
            k += 1;
        }
    }
    if !flag {
        return false;
    }
    let below = pos.down();
    if matches!(p.id(below), ids::GRASS | ids::DIRT) && pos.y >= 2 {
        set_dirt(p, below);
        set_dirt(p, below.offset(1, 0, 0));
        set_dirt(p, below.offset(0, 0, 1));
        set_dirt(p, below.offset(1, 0, 1));
        true
    } else {
        false
    }
}

/// `func_175925_a`: a leaf layer around a 2x2 trunk.
fn huge_layer(p: &mut Pop, c: BlockPos, r: i32, leaf: StateId) {
    let i = r * r;
    for j in -r..=r + 1 {
        for k in -r..=r + 1 {
            let (l, i1) = (j - 1, k - 1);
            if j * j + k * k <= i || l * l + i1 * i1 <= i || j * j + i1 * i1 <= i || l * l + k * k <= i {
                let bp = c.offset(j, 0, k);
                if air_or_leaves(p, bp) {
                    p.put(bp, leaf);
                }
            }
        }
    }
}

/// `func_175928_b`: a round leaf layer around one block.
fn huge_layer_single(p: &mut Pop, c: BlockPos, r: i32, leaf: StateId) {
    let i = r * r;
    for j in -r..=r {
        for k in -r..=r {
            if j * j + k * k <= i {
                let bp = c.offset(j, 0, k);
                if air_or_leaves(p, bp) {
                    p.put(bp, leaf);
                }
            }
        }
    }
}

/// `WorldGenMegaJungle(false, 10, 20, jungle, jungle)`
fn mega_jungle(p: &mut Pop, pos: BlockPos) -> bool {
    let i = huge_height(p, 10, 20);
    if !huge_check(p, pos, i) {
        return false;
    }
    let wood = log("jungle");
    let leaf = leaves("jungle");
    let top = pos.offset(0, i, 0);
    for j in -2..=0 {
        huge_layer(p, top.offset(0, j, 0), 2 + 1 - j, leaf);
    }
    let mut j = pos.y + i - 2 - p.ni(4);
    while j > pos.y + i / 2 {
        let f = p.nf() * std::f32::consts::PI * 2.0;
        let mut k = pos.x + (0.5 + math::cos(f) * 4.0) as i32;
        let mut l = pos.z + (0.5 + math::sin(f) * 4.0) as i32;
        for i1 in 0..5 {
            k = pos.x + (1.5 + math::cos(f) * i1 as f32) as i32;
            l = pos.z + (1.5 + math::sin(f) * i1 as f32) as i32;
            p.put(BlockPos::new(k, j - 3 + i1 / 2, l), wood);
        }
        let j2 = 1 + p.ni(2);
        let j1 = j;
        for k1 in j - j2..=j1 {
            let l1 = k1 - j1;
            huge_layer_single(p, BlockPos::new(k, k1, l), 1 - l1, leaf);
        }
        j -= 2 + p.ni(4);
    }
    let vine_at = |p: &mut Pop, q: BlockPos, prop: &str| {
        if p.ni(3) > 0 && p.air(q) {
            p.put(q, vine(prop));
        }
    };
    for i2 in 0..i {
        let b = pos.offset(0, i2, 0);
        if can_grow_into(p.state(b)) {
            p.put(b, wood);
            if i2 > 0 {
                vine_at(p, b.offset(-1, 0, 0), "east");
                vine_at(p, b.offset(0, 0, -1), "south");
            }
        }
        if i2 < i - 1 {
            let b1 = b.offset(1, 0, 0);
            if can_grow_into(p.state(b1)) {
                p.put(b1, wood);
                if i2 > 0 {
                    vine_at(p, b1.offset(1, 0, 0), "west");
                    vine_at(p, b1.offset(0, 0, -1), "south");
                }
            }
            let b2 = b.offset(1, 0, 1);
            if can_grow_into(p.state(b2)) {
                p.put(b2, wood);
                if i2 > 0 {
                    vine_at(p, b2.offset(1, 0, 0), "west");
                    vine_at(p, b2.offset(0, 0, 1), "north");
                }
            }
            let b3 = b.offset(0, 0, 1);
            if can_grow_into(p.state(b3)) {
                p.put(b3, wood);
                if i2 > 0 {
                    vine_at(p, b3.offset(-1, 0, 0), "east");
                    vine_at(p, b3.offset(0, 0, 1), "north");
                }
            }
        }
    }
    true
}

/// `WorldGenMegaPineTree(false, use_base_height)`: base height 13, extra 15.
fn mega_pine(p: &mut Pop, pos: BlockPos, use_base_height: bool) -> bool {
    let i = huge_height(p, 13, 15);
    if !huge_check(p, pos, i) {
        return false;
    }
    let wood = log("spruce");
    let leaf = leaves("spruce");
    // func_150541_c
    let top = pos.y + i;
    let n = p.ni(5) + if use_base_height { 13 } else { 3 };
    let mut prev = 0;
    for k in top - n..=top {
        let l = top - k;
        let i1 = math::floor_float(l as f32 / n as f32 * 3.5);
        let extra = if l > 0 && i1 == prev && (k & 1) == 0 { 1 } else { 0 };
        huge_layer(p, BlockPos::new(pos.x, k, pos.z), i1 + extra, leaf);
        prev = i1;
    }
    for j in 0..i {
        let b = pos.offset(0, j, 0);
        if air_or_leaves(p, b) {
            p.put(b, wood);
        }
        if j < i - 1 {
            for (dx, dz) in [(1, 0), (1, 1), (0, 1)] {
                let q = pos.offset(dx, j, dz);
                if air_or_leaves(p, q) {
                    p.put(q, wood);
                }
            }
        }
    }
    true
}

/// `WorldGenMegaPineTree.func_180711_a`: podzol patches around the trunk.
fn podzol(p: &mut Pop, pos: BlockPos) {
    podzol_disc(p, pos.offset(-1, 0, -1));
    podzol_disc(p, pos.offset(2, 0, -1));
    podzol_disc(p, pos.offset(-1, 0, 2));
    podzol_disc(p, pos.offset(2, 0, 2));
    for _ in 0..5 {
        let j = p.ni(64);
        let (k, l) = (j % 8, j / 8);
        if k == 0 || k == 7 || l == 0 || l == 7 {
            podzol_disc(p, pos.offset(-3 + k, 0, -3 + l));
        }
    }
}

fn podzol_disc(p: &mut Pop, c: BlockPos) {
    let pz = state("minecraft:dirt").set("variant", "podzol");
    for i in -2i32..=2 {
        for j in -2i32..=2 {
            if i.abs() != 2 || j.abs() != 2 {
                let q = c.offset(i, 0, j);
                for y in (-3..=2).rev() {
                    let b = q.offset(0, y, 0);
                    let id = p.id(b);
                    if id == ids::GRASS || id == ids::DIRT {
                        p.put(b, pz);
                        break;
                    }
                    if p.mat(b) != Material::Air && y < 0 {
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// WorldGenBigTree

struct BigTree<'r> {
    rand: JavaRandom,
    base: BlockPos,
    height_limit: &'r mut i32,
    height: i32,
    leaf_distance_limit: i32,
    nodes: Vec<(BlockPos, i32)>,
}

const HEIGHT_ATTENUATION: f64 = 0.618;
const BRANCH_SLOPE: f64 = 0.381;

/// `BlockPos.add(double, double, double)`
fn add_d(b: BlockPos, x: f64, y: f64, z: f64) -> BlockPos {
    BlockPos::new(math::floor_double(b.x as f64 + x), math::floor_double(b.y as f64 + y), math::floor_double(b.z as f64 + z))
}

fn greatest(d: BlockPos) -> i32 {
    let (i, j, k) = (d.x.abs(), d.y.abs(), d.z.abs());
    if k > i && k > j {
        k
    } else if j > i {
        j
    } else {
        i
    }
}

impl BigTree<'_> {
    fn layer_size(&self, y: i32) -> f32 {
        let hl = *self.height_limit;
        if (y as f32) < hl as f32 * 0.3 {
            return -1.0;
        }
        let f = hl as f32 / 2.0;
        let f1 = f - y as f32;
        let mut f2 = ((f * f - f1 * f1) as f64).sqrt() as f32;
        if f1 == 0.0 {
            f2 = f;
        } else if f1.abs() >= f {
            return 0.0;
        }
        f2 * 0.5
    }

    fn leaf_size(&self, y: i32) -> f32 {
        if y >= 0 && y < self.leaf_distance_limit {
            if y != 0 && y != self.leaf_distance_limit - 1 {
                3.0
            } else {
                2.0
            }
        } else {
            -1.0
        }
    }

    fn check_line(&self, p: &mut Pop, a: BlockPos, b: BlockPos) -> i32 {
        let d = BlockPos::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let i = greatest(d);
        let f = d.x as f32 / i as f32;
        let f1 = d.y as f32 / i as f32;
        let f2 = d.z as f32 / i as f32;
        if i == 0 {
            return -1;
        }
        for j in 0..=i {
            let q = add_d(a, (0.5 + j as f32 * f) as f64, (0.5 + j as f32 * f1) as f64, (0.5 + j as f32 * f2) as f64);
            if !can_grow_into(p.state(q)) {
                return j;
            }
        }
        -1
    }

    fn limb(&self, p: &mut Pop, a: BlockPos, b: BlockPos) {
        let d = BlockPos::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let i = greatest(d);
        let f = d.x as f32 / i as f32;
        let f1 = d.y as f32 / i as f32;
        let f2 = d.z as f32 / i as f32;
        for j in 0..=i {
            let q = add_d(a, (0.5 + j as f32 * f) as f64, (0.5 + j as f32 * f1) as f64, (0.5 + j as f32 * f2) as f64);
            let (ax, az) = ((q.x - a.x).abs(), (q.z - a.z).abs());
            let m = ax.max(az);
            let axis = if m > 0 {
                if ax == m {
                    "x"
                } else {
                    "z"
                }
            } else {
                "y"
            };
            p.put(q, state("minecraft:log").set("axis", axis));
        }
    }

    fn cross_section(&self, p: &mut Pop, c: BlockPos, r: f32, leaf: StateId) {
        let i = (r as f64 + 0.618) as i32;
        for j in -i..=i {
            for k in -i..=i {
                if ((j.abs() as f64 + 0.5).powi(2) + (k.abs() as f64 + 0.5).powi(2)) <= (r * r) as f64 {
                    let q = c.offset(j, 0, k);
                    if air_or_leaves(p, q) {
                        p.put(q, leaf);
                    }
                }
            }
        }
    }

    fn generate_leaf_node_list(&mut self, p: &mut Pop) {
        let hl = *self.height_limit;
        self.height = (hl as f64 * HEIGHT_ATTENUATION) as i32;
        if self.height >= hl {
            self.height = hl - 1;
        }
        let mut i = (1.382 + (1.0 * hl as f64 / 13.0).powf(2.0)) as i32;
        if i < 1 {
            i = 1;
        }
        let j = self.base.y + self.height;
        let mut k = hl - self.leaf_distance_limit;
        self.nodes.push((self.base.offset(0, k, 0), j));
        while k >= 0 {
            let f = self.layer_size(k);
            if f >= 0.0 {
                for _ in 0..i {
                    let d0 = 1.0 * f as f64 * (self.rand.next_float() as f64 + 0.328);
                    let d1 = (self.rand.next_float() * 2.0) as f64 * std::f64::consts::PI;
                    let d2 = d0 * d1.sin() + 0.5;
                    let d3 = d0 * d1.cos() + 0.5;
                    let bp = add_d(self.base, d2, (k - 1) as f64, d3);
                    let bp1 = bp.offset(0, self.leaf_distance_limit, 0);
                    if self.check_line(p, bp, bp1) == -1 {
                        let i1 = self.base.x - bp.x;
                        let j1 = self.base.z - bp.z;
                        let d4 = bp.y as f64 - ((i1 * i1 + j1 * j1) as f64).sqrt() * BRANCH_SLOPE;
                        let k1 = if d4 > j as f64 { j } else { d4 as i32 };
                        let bp2 = BlockPos::new(self.base.x, k1, self.base.z);
                        if self.check_line(p, bp2, bp) == -1 {
                            self.nodes.push((bp, bp2.y));
                        }
                    }
                }
            }
            k -= 1;
        }
    }
}

/// `WorldGenBigTree.generate` after `func_175904_e` (leaf distance 5). `height_limit` is the
/// generator's persistent `heightLimit`.
fn big_tree(p: &mut Pop, pos: BlockPos, height_limit: &mut i32) -> bool {
    let seed = p.rand.next_long();
    let mut t = BigTree { rand: JavaRandom::new(seed), base: pos, height_limit, height: 0, leaf_distance_limit: 5, nodes: Vec::new() };
    if *t.height_limit == 0 {
        *t.height_limit = 5 + t.rand.next_int_bounded(12);
    }
    // validTreeLocation
    if !matches!(p.id(pos.down()), ids::DIRT | ids::GRASS | ids::FARMLAND) {
        return false;
    }
    let i = t.check_line(p, pos, pos.offset(0, *t.height_limit - 1, 0));
    if i != -1 {
        if i < 6 {
            return false;
        }
        *t.height_limit = i;
    }
    t.generate_leaf_node_list(p);
    let leaf = state("minecraft:leaves").set_bool("check_decay", false);
    let nodes = std::mem::take(&mut t.nodes);
    for &(n, _) in &nodes {
        for i in 0..t.leaf_distance_limit {
            let r = t.leaf_size(i);
            t.cross_section(p, n.offset(0, i, 0), r, leaf);
        }
    }
    t.limb(p, pos, pos.offset(0, t.height, 0));
    let hl = *t.height_limit;
    for &(n, by) in &nodes {
        let b = BlockPos::new(pos.x, by, pos.z);
        if b != n && (by - pos.y) as f64 >= hl as f64 * 0.2 {
            t.limb(p, b, n);
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------

/// `WorldGenBigMushroom`; `kind` is the generator's mushroom block id (0 until first chosen).
pub fn big_mushroom(p: &mut Pop, kind: &mut u16, pos: BlockPos) -> bool {
    if *kind == 0 {
        *kind = if p.rand.next_boolean() { ids::BROWN_MUSHROOM_BLOCK } else { ids::RED_MUSHROOM_BLOCK };
    }
    let brown = *kind == ids::BROWN_MUSHROOM_BLOCK;
    let i = p.ni(3) + 4;
    if !(pos.y >= 1 && pos.y + i + 1 < 256) {
        return false;
    }
    let mut flag = true;
    for j in pos.y..=pos.y + 1 + i {
        let k = if j <= pos.y + 3 { 0 } else { 3 };
        let mut l = pos.x - k;
        while l <= pos.x + k && flag {
            let mut i1 = pos.z - k;
            while i1 <= pos.z + k && flag {
                if (0..256).contains(&j) {
                    if !air_or_leaves(p, BlockPos::new(l, j, i1)) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                i1 += 1;
            }
            l += 1;
        }
    }
    if !flag {
        return false;
    }
    if !matches!(p.id(pos.down()), ids::DIRT | ids::GRASS | ids::MYCELIUM) {
        return false;
    }
    // BlockHugeMushroom.EnumType metadata
    const NORTH_WEST: u8 = 1;
    const NORTH_EAST: u8 = 3;
    const CENTER: u8 = 5;
    const SOUTH_WEST: u8 = 7;
    const SOUTH_EAST: u8 = 9;
    const ALL_INSIDE: u8 = 0;
    const STEM: u8 = 10;
    let top = pos.y + i;
    let k2 = if brown { top } else { top - 3 };
    for l2 in k2..=top {
        let mut j3 = 1;
        if l2 < top {
            j3 += 1;
        }
        if brown {
            j3 = 3;
        }
        let (k3, l3, j1, k1) = (pos.x - j3, pos.x + j3, pos.z - j3, pos.z + j3);
        for l1 in k3..=l3 {
            for i2 in j1..=k1 {
                let mut j2: i32 = 5;
                if l1 == k3 {
                    j2 -= 1;
                } else if l1 == l3 {
                    j2 += 1;
                }
                if i2 == j1 {
                    j2 -= 3;
                } else if i2 == k1 {
                    j2 += 3;
                }
                let mut t = j2 as u8;
                if brown || l2 < top {
                    if (l1 == k3 || l1 == l3) && (i2 == j1 || i2 == k1) {
                        continue;
                    }
                    if l1 == pos.x - (j3 - 1) && i2 == j1 {
                        t = NORTH_WEST;
                    }
                    if l1 == k3 && i2 == pos.z - (j3 - 1) {
                        t = NORTH_WEST;
                    }
                    if l1 == pos.x + (j3 - 1) && i2 == j1 {
                        t = NORTH_EAST;
                    }
                    if l1 == l3 && i2 == pos.z - (j3 - 1) {
                        t = NORTH_EAST;
                    }
                    if l1 == pos.x - (j3 - 1) && i2 == k1 {
                        t = SOUTH_WEST;
                    }
                    if l1 == k3 && i2 == pos.z + (j3 - 1) {
                        t = SOUTH_WEST;
                    }
                    if l1 == pos.x + (j3 - 1) && i2 == k1 {
                        t = SOUTH_EAST;
                    }
                    if l1 == l3 && i2 == pos.z + (j3 - 1) {
                        t = SOUTH_EAST;
                    }
                }
                if t == CENTER && l2 < top {
                    t = ALL_INSIDE;
                }
                if pos.y >= top - 1 || t != ALL_INSIDE {
                    let bp = BlockPos::new(l1, l2, i2);
                    if !full_block(p, bp) {
                        p.put(bp, mc_logic::st(*kind, t));
                    }
                }
            }
        }
    }
    for i3 in 0..i {
        let up = pos.offset(0, i3, 0);
        if !full_block(p, up) {
            p.put(up, mc_logic::st(*kind, STEM));
        }
    }
    true
}

