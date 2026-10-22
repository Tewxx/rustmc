//! Potion effects (`Potion`, `PotionEffect`, `EntityLivingBase.activePotionsMap`) and entity
//! attributes (`ModifiableAttributeInstance`, `S20PacketEntityProperties`).
use mc_core::java_hash::{int_hash, uuid_hash, JavaHashMap};

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
}

const fn p(id: u8, name: &'static str, bad: bool, liquid_color: u32, ix: i32, iy: i32) -> Potion {
    Potion { id, name, bad, liquid_color, icon: if ix < 0 { -1 } else { ix + iy * 8 } }
}

/// `Potion.potionTypes` (ids 1..=23).
pub const POTIONS: [Potion; 23] = [
    p(1, "potion.moveSpeed", false, 8171462, 0, 0),
    p(2, "potion.moveSlowdown", true, 5926017, 1, 0),
    p(3, "potion.digSpeed", false, 14270531, 2, 0),
    p(4, "potion.digSlowDown", true, 4866583, 3, 0),
    p(5, "potion.damageBoost", false, 9643043, 4, 0),
    p(6, "potion.heal", false, 16262179, -1, 0),
    p(7, "potion.harm", true, 4393481, -1, 0),
    p(8, "potion.jump", false, 2293580, 2, 1),
    p(9, "potion.confusion", true, 5578058, 3, 1),
    p(10, "potion.regeneration", false, 13458603, 7, 0),
    p(11, "potion.resistance", false, 10044730, 6, 1),
    p(12, "potion.fireResistance", false, 14981690, 7, 1),
    p(13, "potion.waterBreathing", false, 3035801, 0, 2),
    p(14, "potion.invisibility", false, 8356754, 0, 1),
    p(15, "potion.blindness", true, 2039587, 5, 1),
    p(16, "potion.nightVision", false, 2039713, 4, 1),
    p(17, "potion.hunger", true, 5797459, 1, 1),
    p(18, "potion.weakness", true, 4738376, 5, 0),
    p(19, "potion.poison", true, 5149489, 6, 0),
    p(20, "potion.wither", true, 3484199, 1, 2),
    p(21, "potion.healthBoost", false, 16284963, 2, 2),
    p(22, "potion.absorption", false, 2445989, 2, 2),
    p(23, "potion.saturation", false, 16262179, -1, 0),
];

pub const SPEED: u8 = 1;
pub const SLOWNESS: u8 = 2;
pub const HASTE: u8 = 3;
pub const MINING_FATIGUE: u8 = 4;
pub const JUMP_BOOST: u8 = 8;
pub const NAUSEA: u8 = 9;
pub const REGENERATION: u8 = 10;
pub const INVISIBILITY: u8 = 14;
pub const BLINDNESS: u8 = 15;
pub const NIGHT_VISION: u8 = 16;
pub const POISON: u8 = 19;
pub const WITHER: u8 = 20;
pub const HUNGER: u8 = 17;

pub fn potion(id: u8) -> Option<&'static Potion> {
    POTIONS.get((id as usize).wrapping_sub(1))
}

/// `PotionEffect`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PotionEffect {
    pub id: u8,
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub show_particles: bool,
    /// `isPotionDurationMax`: shown as `**:**`.
    pub duration_max: bool,
}

impl PotionEffect {
    /// `combine`
    fn combine(&mut self, o: &PotionEffect) {
        if o.amplifier > self.amplifier {
            self.amplifier = o.amplifier;
            self.duration = o.duration;
        } else if o.amplifier == self.amplifier && self.duration < o.duration {
            self.duration = o.duration;
        } else if !o.ambient && self.ambient {
            self.ambient = o.ambient;
        }
        self.show_particles = o.show_particles;
    }

    /// `Potion.isReady(duration, amplifier)`
    fn is_ready(&self) -> bool {
        let every = |base: i32| {
            let k = base >> self.amplifier.clamp(0, 31);
            k <= 0 || self.duration % k == 0
        };
        match self.id {
            REGENERATION => every(50),
            POISON => every(25),
            WITHER => every(40),
            HUNGER => true,
            _ => false,
        }
    }

    /// `Potion.getDurationString`: `**:**` or `StringUtils.ticksToElapsedTime`.
    pub fn duration_string(&self) -> String {
        if self.duration_max {
            return "**:**".into();
        }
        let s = self.duration / 20;
        let (m, s) = (s / 60, s % 60);
        if s < 10 {
            format!("{m}:0{s}")
        } else {
            format!("{m}:{s}")
        }
    }
}

/// `EntityLivingBase.activePotionsMap` (a `HashMap<Integer, PotionEffect>`).
#[derive(Clone, Debug, Default)]
pub struct Effects {
    map: JavaHashMap<u8, PotionEffect>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn get(&self, id: u8) -> Option<&PotionEffect> {
        self.map.get(&id)
    }

    pub fn has(&self, id: u8) -> bool {
        self.map.contains_key(&id)
    }

    /// Amplifier + 1, or 0 without the effect.
    pub fn level(&self, id: u8) -> i32 {
        self.get(id).map_or(0, |e| e.amplifier + 1)
    }

    /// `getActivePotionEffects` in Java iteration order.
    pub fn iter(&self) -> impl Iterator<Item = &PotionEffect> {
        self.map.iter().map(|(_, v)| v)
    }

    /// `addPotionEffect` (every potion applies to players).
    pub fn add(&mut self, e: PotionEffect) {
        if let Some(cur) = self.map.get_mut(&e.id) {
            cur.combine(&e);
        } else {
            self.map.insert(int_hash(e.id as i32), e.id, e);
        }
    }

    /// `removePotionEffectClient`
    pub fn remove(&mut self, id: u8) -> Option<PotionEffect> {
        self.map.remove(&id)
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Client half of `updatePotionEffects`: count durations down (expired effects stay until the
    /// server removes them). Returns the health regeneration heals (`performEffect`).
    pub fn tick_client(&mut self) -> f32 {
        let mut heal = 0.0;
        for e in self.map.values_mut() {
            if e.duration > 0 {
                if e.is_ready() && e.id == REGENERATION {
                    heal += 1.0;
                }
                e.duration -= 1;
            }
        }
        heal
    }

    /// `EntityLivingBase.getArmSwingAnimationEnd`
    pub fn swing_end(&self) -> i32 {
        if let Some(e) = self.get(HASTE) {
            6 - (1 + e.amplifier)
        } else if let Some(e) = self.get(MINING_FATIGUE) {
            6 + (1 + e.amplifier) * 2
        } else {
            6
        }
    }

    /// The potion part of `EntityPlayer.getToolDigEfficiency`.
    pub fn dig_multiplier(&self, mut f: f32) -> f32 {
        if let Some(e) = self.get(HASTE) {
            f *= 1.0 + (e.amplifier + 1) as f32 * 0.2;
        }
        if let Some(e) = self.get(MINING_FATIGUE) {
            f *= match e.amplifier {
                0 => 0.3,
                1 => 0.09,
                2 => 0.0027,
                _ => 8.1e-4,
            };
        }
        f
    }
}

/// `AttributeModifier`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modifier {
    pub uuid: (u64, u64),
    pub amount: f64,
    pub op: u8,
}

/// `ModifiableAttributeInstance` of a `RangedAttribute`.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub base: f64,
    min: f64,
    max: f64,
    by_op: [JavaHashMap<(u64, u64), Modifier>; 3],
}

impl Attribute {
    pub fn new(base: f64, min: f64, max: f64) -> Self {
        Self { base, min, max, by_op: Default::default() }
    }

    pub fn modifier(&self, uuid: (u64, u64)) -> Option<&Modifier> {
        self.by_op.iter().find_map(|m| m.get(&uuid))
    }

    /// `applyModifier` (vanilla throws on a duplicate UUID; this replaces it).
    pub fn apply(&mut self, m: Modifier) {
        self.remove(m.uuid);
        if let Some(set) = self.by_op.get_mut(m.op as usize) {
            set.insert(uuid_hash(m.uuid.0, m.uuid.1), m.uuid, m);
        }
    }

    pub fn remove(&mut self, uuid: (u64, u64)) {
        for set in &mut self.by_op {
            set.remove(&uuid);
        }
    }

    pub fn remove_all(&mut self) {
        for set in &mut self.by_op {
            set.clear();
        }
    }

    /// `computeValue`: each operation iterates a fresh `Sets.newHashSet` copy of its modifiers.
    pub fn value(&self) -> f64 {
        let ordered = |op: usize| {
            let src = &self.by_op[op];
            let mut copy = JavaHashMap::with_expected(src.len());
            for (k, m) in src.iter() {
                copy.insert(src.hash_of(k).unwrap_or(0), *k, *m);
            }
            copy.iter().map(|(_, m)| m.amount).collect::<Vec<f64>>()
        };
        let mut d0 = self.base;
        for a in ordered(0) {
            d0 += a;
        }
        let mut d1 = d0;
        for a in ordered(1) {
            d1 += d0 * a;
        }
        for a in ordered(2) {
            d1 *= 1.0 + a;
        }
        // MathHelper.clamp_double
        if d1 < self.min {
            self.min
        } else if d1 > self.max {
            self.max
        } else {
            d1
        }
    }
}

pub const MOVEMENT_SPEED: &str = "generic.movementSpeed";
pub const MAX_HEALTH: &str = "generic.maxHealth";
/// `EntityLivingBase.sprintingSpeedBoostModifier`
pub const SPRINT_UUID: (u64, u64) = (0x662A6B8D_DA3E_4C1C, 0x8813_96EA6097278D);
pub const SPRINT_BOOST: f64 = 0.30000001192092896;

/// `BaseAttributeMap` of a player: the attributes `EntityPlayer.applyEntityAttributes` registers,
/// plus any the server sends.
#[derive(Clone, Debug)]
pub struct Attributes {
    map: Vec<(String, Attribute)>,
}

impl Default for Attributes {
    fn default() -> Self {
        let max = f64::MAX;
        Self {
            map: vec![
                (MAX_HEALTH.into(), Attribute::new(20.0, 0.0, 1024.0)),
                ("generic.knockbackResistance".into(), Attribute::new(0.0, 0.0, 1.0)),
                (MOVEMENT_SPEED.into(), Attribute::new(0.10000000149011612, 0.0, max)),
                ("generic.attackDamage".into(), Attribute::new(1.0, 0.0, 2048.0)),
            ],
        }
    }
}

impl Attributes {
    pub fn get(&self, name: &str) -> Option<&Attribute> {
        self.map.iter().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Attribute> {
        self.map.iter_mut().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    pub fn value(&self, name: &str) -> f64 {
        self.get(name).map_or(0.0, Attribute::value)
    }

    /// `handleEntityProperties` for one snapshot: unknown names register with the packet's range.
    pub fn apply_snapshot(&mut self, name: &str, base: f64, mods: impl IntoIterator<Item = Modifier>) {
        if self.get(name).is_none() {
            self.map.push((name.into(), Attribute::new(0.0, 2.2250738585072014E-308, f64::MAX)));
        }
        let a = self.get_mut(name).unwrap();
        a.base = base;
        a.remove_all();
        for m in mods {
            a.apply(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprint_speed_value() {
        let mut a = Attributes::default();
        assert_eq!(a.value(MOVEMENT_SPEED), 0.10000000149011612);
        a.get_mut(MOVEMENT_SPEED).unwrap().apply(Modifier { uuid: SPRINT_UUID, amount: SPRINT_BOOST, op: 2 });
        assert_eq!(a.value(MOVEMENT_SPEED), 0.10000000149011612 * (1.0 + 0.30000001192092896));
    }

    #[test]
    fn effects_combine_and_tick() {
        let mut e = Effects::default();
        let base = PotionEffect { id: SPEED, duration: 100, amplifier: 0, ambient: false, show_particles: true, duration_max: false };
        e.add(base);
        e.add(PotionEffect { duration: 50, amplifier: 1, ..base });
        assert_eq!(e.get(SPEED).unwrap().amplifier, 1);
        assert_eq!(e.get(SPEED).unwrap().duration, 50);
        for _ in 0..60 {
            e.tick_client();
        }
        // Expired effects stay active on the client until S1E removes them.
        assert!(e.has(SPEED));
        assert_eq!(e.get(SPEED).unwrap().duration, 0);
        assert_eq!(e.get(SPEED).unwrap().duration_string(), "0:00");
    }

    #[test]
    fn regeneration_ready_interval() {
        let mut e = Effects::default();
        e.add(PotionEffect { id: REGENERATION, duration: 100, amplifier: 1, ambient: false, show_particles: true, duration_max: false });
        // 50 >> 1 = 25: heals at durations 100, 75, 50, 25.
        let heals: f32 = (0..100).map(|_| e.tick_client()).sum();
        assert_eq!(heals, 4.0);
    }
}
