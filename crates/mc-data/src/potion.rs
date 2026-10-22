//! The `Potion` registry and `PotionHelper`: potion damage values to effects, colors and names.

/// One `Potion` registry entry.
#[derive(Clone, Copy, Debug)]
pub struct Potion {
    pub id: u8,
    /// Translation key, e.g. `potion.moveSpeed`.
    pub name: &'static str,
    pub bad: bool,
    pub liquid_color: u32,
    /// `statusIconIndex` into `textures/gui/container/inventory.png` (18×18 cells from 0,198), or -1.
    pub icon: i32,
    /// `getEffectiveness`: 0.5 for bad effects, 1.0 otherwise, unless `setEffectiveness` changed it.
    pub effectiveness: f64,
    /// `isInstant` (`PotionHealth`: instant health, instant damage, saturation).
    pub instant: bool,
}

const fn p(id: u8, name: &'static str, bad: bool, liquid_color: u32, ix: i32, iy: i32) -> Potion {
    Potion {
        id,
        name,
        bad,
        liquid_color,
        icon: if ix < 0 { -1 } else { ix + iy * 8 },
        effectiveness: if bad { 0.5 } else { 1.0 },
        instant: matches!(id, 6 | 7 | 23),
    }
}

const fn eff(mut p: Potion, e: f64) -> Potion {
    p.effectiveness = e;
    p
}

/// `Potion.potionTypes` (ids 1..=23).
pub const POTIONS: [Potion; 23] = [
    p(1, "potion.moveSpeed", false, 8171462, 0, 0),
    p(2, "potion.moveSlowdown", true, 5926017, 1, 0),
    eff(p(3, "potion.digSpeed", false, 14270531, 2, 0), 1.5),
    p(4, "potion.digSlowDown", true, 4866583, 3, 0),
    p(5, "potion.damageBoost", false, 9643043, 4, 0),
    p(6, "potion.heal", false, 16262179, -1, 0),
    p(7, "potion.harm", true, 4393481, -1, 0),
    p(8, "potion.jump", false, 2293580, 2, 1),
    eff(p(9, "potion.confusion", true, 5578058, 3, 1), 0.25),
    eff(p(10, "potion.regeneration", false, 13458603, 7, 0), 0.25),
    p(11, "potion.resistance", false, 10044730, 6, 1),
    p(12, "potion.fireResistance", false, 14981690, 7, 1),
    p(13, "potion.waterBreathing", false, 3035801, 0, 2),
    p(14, "potion.invisibility", false, 8356754, 0, 1),
    eff(p(15, "potion.blindness", true, 2039587, 5, 1), 0.25),
    p(16, "potion.nightVision", false, 2039713, 4, 1),
    p(17, "potion.hunger", true, 5797459, 1, 1),
    p(18, "potion.weakness", true, 4738376, 5, 0),
    eff(p(19, "potion.poison", true, 5149489, 6, 0), 0.25),
    eff(p(20, "potion.wither", true, 3484199, 1, 2), 0.25),
    p(21, "potion.healthBoost", false, 16284963, 2, 2),
    p(22, "potion.absorption", false, 2445989, 2, 2),
    p(23, "potion.saturation", false, 16262179, -1, 0),
];

pub fn by_id(id: u8) -> Option<&'static Potion> {
    POTIONS.get((id as usize).wrapping_sub(1))
}

/// A `PotionEffect` as `getPotionEffects` builds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brewed {
    pub id: u8,
    pub duration: i32,
    pub amplifier: i32,
}

/// `potionRequirements`
fn requirement(id: u8) -> Option<&'static str> {
    Some(match id {
        10 => "0 & !1 & !2 & !3 & 0+6",
        1 => "!0 & 1 & !2 & !3 & 1+6",
        12 => "0 & 1 & !2 & !3 & 0+6",
        6 => "0 & !1 & 2 & !3",
        19 => "!0 & !1 & 2 & !3 & 2+6",
        18 => "!0 & !1 & !2 & 3 & 3+6",
        7 => "!0 & !1 & 2 & 3",
        2 => "!0 & 1 & !2 & 3 & 3+6",
        5 => "0 & !1 & !2 & 3 & 3+6",
        16 => "!0 & 1 & 2 & !3 & 2+6",
        14 => "!0 & 1 & 2 & 3 & 2+6",
        13 => "0 & !1 & 2 & 3 & 2+6",
        8 => "0 & 1 & !2 & 3 & 3+6",
        _ => return None,
    })
}

/// `potionAmplifiers`
fn amplifier(id: u8) -> Option<&'static str> {
    matches!(id, 1 | 3 | 5 | 10 | 7 | 6 | 11 | 19 | 8).then_some("5")
}

/// `potionPrefixes`
const PREFIXES: [&str; 32] = [
    "potion.prefix.mundane",
    "potion.prefix.uninteresting",
    "potion.prefix.bland",
    "potion.prefix.clear",
    "potion.prefix.milky",
    "potion.prefix.diffuse",
    "potion.prefix.artless",
    "potion.prefix.thin",
    "potion.prefix.awkward",
    "potion.prefix.flat",
    "potion.prefix.bulky",
    "potion.prefix.bungling",
    "potion.prefix.buttered",
    "potion.prefix.smooth",
    "potion.prefix.suave",
    "potion.prefix.debonair",
    "potion.prefix.thick",
    "potion.prefix.elegant",
    "potion.prefix.fancy",
    "potion.prefix.charming",
    "potion.prefix.dashing",
    "potion.prefix.refined",
    "potion.prefix.cordial",
    "potion.prefix.sparkling",
    "potion.prefix.potent",
    "potion.prefix.foul",
    "potion.prefix.odorless",
    "potion.prefix.rank",
    "potion.prefix.harsh",
    "potion.prefix.acrid",
    "potion.prefix.gross",
    "potion.prefix.stinky",
];

pub fn check_flag(v: i32, bit: i32) -> bool {
    v & (1 << bit) != 0
}

/// `getPotionPrefix`
pub fn prefix(meta: i32) -> &'static str {
    let f = |b: i32, v: usize| if check_flag(meta, b) { v } else { 0 };
    PREFIXES[f(5, 16) | f(4, 8) | f(3, 4) | f(2, 2) | f(1, 1)]
}

/// `ItemPotion.isSplash`
pub fn is_splash(meta: i32) -> bool {
    meta & 16384 != 0
}

fn count_set_flags(mut v: i32) -> i32 {
    let mut i = 0;
    while v > 0 {
        v &= v - 1;
        i += 1;
    }
    i
}

/// `getPotionEffect`
fn potion_effect(not: bool, has_mul: bool, neg: bool, cmp: i32, bit: i32, mul: i32, meta: i32) -> i32 {
    let mut i = 0;
    if not {
        i = (!check_flag(meta, bit)) as i32;
    } else if cmp != -1 {
        let n = count_set_flags(meta);
        if (cmp == 0 && n == bit) || (cmp == 1 && n > bit) || (cmp == 2 && n < bit) {
            i = 1;
        }
    } else {
        i = check_flag(meta, bit) as i32;
    }
    if has_mul {
        i *= mul;
    }
    if neg {
        i *= -1;
    }
    i
}

/// `parsePotionEffects(s, start, end, meta)`
#[allow(unused_assignments)]
fn parse(s: &[u8], start: usize, end: i32, meta: i32) -> i32 {
    if !(start < s.len() && end >= 0 && (start as i32) < end) {
        return 0;
    }
    let end_u = end as usize;
    let find = |c: u8| s[start..].iter().position(|&x| x == c).map(|p| p + start);
    if let Some(i) = find(b'|').filter(|&i| i < end_u) {
        let l = parse(s, start, i as i32 - 1, meta);
        if l > 0 {
            return l;
        }
        let r = parse(s, i + 1, end, meta);
        return if r > 0 { r } else { 0 };
    }
    if let Some(j) = find(b'&').filter(|&j| j < end_u) {
        let l = parse(s, start, j as i32 - 1, meta);
        if l <= 0 {
            return 0;
        }
        let r = parse(s, j + 1, end, meta);
        return if r <= 0 { 0 } else { l.max(r) };
    }
    let (mut star, mut has_mul, mut has_num, mut not, mut neg) = (false, false, false, false, false);
    let (mut cmp, mut bit, mut mul, mut total) = (-1, 0, 0, 0);
    macro_rules! flush {
        () => {
            if has_num {
                total += potion_effect(not, has_mul, neg, cmp, bit, mul, meta);
                not = false;
                neg = false;
                star = false;
                has_mul = false;
                has_num = false;
                mul = 0;
                bit = 0;
                cmp = -1;
            }
        };
    }
    for &c in &s[start..end_u.min(s.len())] {
        match c {
            b'0'..=b'9' => {
                if star {
                    mul = (c - b'0') as i32;
                    has_mul = true;
                } else {
                    bit = bit * 10 + (c - b'0') as i32;
                    has_num = true;
                }
            }
            b'*' => star = true,
            b'!' => {
                flush!();
                not = true;
            }
            b'-' => {
                flush!();
                neg = true;
            }
            b'=' | b'<' | b'>' => {
                flush!();
                cmp = match c {
                    b'=' => 0,
                    b'<' => 2,
                    _ => 1,
                };
            }
            b'+' => flush!(),
            _ => {}
        }
    }
    flush!();
    total
}

/// Java `Math.round(double)`
fn round(v: f64) -> i32 {
    (v + 0.5).floor() as i64 as i32
}

/// `getPotionEffects(meta, false)` (no potion is ever marked usable, so the flag never filters).
pub fn effects(meta: i32) -> Vec<Brewed> {
    let mut out = Vec::new();
    for p in &POTIONS {
        let Some(req) = requirement(p.id) else { continue };
        let mut i = parse(req.as_bytes(), 0, req.len() as i32, meta);
        if i <= 0 {
            continue;
        }
        let j = amplifier(p.id).map_or(0, |a| parse(a.as_bytes(), 0, a.len() as i32, meta).max(0));
        if p.instant {
            i = 1;
        } else {
            i = 1200 * (i * 3 + (i - 1) * 2);
            i >>= j;
            i = round(i as f64 * p.effectiveness);
            if is_splash(meta) {
                i = round(i as f64 * 0.75 + 0.5);
            }
        }
        out.push(Brewed { id: p.id, duration: i, amplifier: j });
    }
    out
}

/// `calcPotionLiquidColor` over (id, amplifier, showParticles).
pub fn liquid_color(effects: impl IntoIterator<Item = (u8, i32, bool)>) -> u32 {
    let (mut r, mut g, mut b, mut n) = (0f32, 0f32, 0f32, 0f32);
    let mut any = false;
    for (id, amp, particles) in effects {
        any = true;
        if !particles {
            continue;
        }
        let Some(p) = by_id(id) else { continue };
        let c = p.liquid_color;
        for _ in 0..=amp {
            r += ((c >> 16) & 255) as f32 / 255.0;
            g += ((c >> 8) & 255) as f32 / 255.0;
            b += (c & 255) as f32 / 255.0;
            n += 1.0;
        }
    }
    if !any {
        return 3694022;
    }
    if n == 0.0 {
        return 0;
    }
    ((r / n * 255.0) as i32 as u32) << 16 | ((g / n * 255.0) as i32 as u32) << 8 | (b / n * 255.0) as i32 as u32
}

/// `getLiquidColor(meta, false)`
pub fn meta_color(meta: i32) -> u32 {
    liquid_color(effects(meta).iter().map(|e| (e.id, e.amplifier, true)))
}

/// `applyIngredient`: a brewing ingredient's bit operations on a potion damage value.
pub fn apply_ingredient(mut meta: i32, s: &str) -> i32 {
    let (mut has_num, mut toggle, mut clear, mut require, mut bit) = (false, false, false, false, 0);
    let op = |meta: i32, bit: i32, clear: bool, toggle: bool, require: bool| -> i32 {
        if require {
            if !check_flag(meta, bit) {
                return 0;
            }
            meta
        } else if clear {
            meta & !(1 << bit)
        } else if toggle {
            meta ^ (1 << bit)
        } else {
            meta | (1 << bit)
        }
    };
    for c in s.bytes() {
        match c {
            b'0'..=b'9' => {
                bit = bit * 10 + (c - b'0') as i32;
                has_num = true;
            }
            b'!' | b'-' | b'+' | b'&' => {
                if has_num {
                    meta = op(meta, bit, clear, toggle, require);
                    (require, toggle, clear, has_num, bit) = (false, false, false, false, 0);
                }
                match c {
                    b'!' => toggle = true,
                    b'-' => clear = true,
                    b'&' => require = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if has_num {
        meta = op(meta, bit, clear, toggle, require);
    }
    meta & 32767
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_potions() {
        // Potion of Healing (8197), Swiftness II (8226), splash Poison (16388), long Fire Resistance (8259).
        assert_eq!(effects(8197), vec![Brewed { id: 6, duration: 1, amplifier: 0 }]);
        assert_eq!(effects(8226), vec![Brewed { id: 1, duration: 1800, amplifier: 1 }]);
        assert_eq!(effects(16388), vec![Brewed { id: 19, duration: 676, amplifier: 0 }]);
        assert_eq!(effects(8259), vec![Brewed { id: 12, duration: 9600, amplifier: 0 }]);
        assert_eq!(effects(0), vec![]);
        assert_eq!(effects(16), vec![]);
        assert_eq!(prefix(16), "potion.prefix.awkward");
        assert_eq!(meta_color(0), 3694022);
        assert_eq!(meta_color(8197), 16262179);
        // Nether wart on water -> awkward; then sugar -> swiftness base.
        assert_eq!(apply_ingredient(0, "+4"), 16);
        assert_eq!(apply_ingredient(16, "-0+1-2-3&4-4+13"), 8194);
    }
}
