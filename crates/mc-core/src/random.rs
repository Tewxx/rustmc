//! Bit-exact `java.util.Random`: the 48-bit LCG every vanilla system draws from
//! (worldgen, block ticks, particles, mob AI). Call order matters as much as the algorithm.

const MULTIPLIER: i64 = 0x5DEECE66D;
const ADDEND: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;
const DOUBLE_UNIT: f64 = 1.0 / (1u64 << 53) as f64;
const FLOAT_UNIT: f32 = 1.0 / (1u32 << 24) as f32;

#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: i64,
    next_gaussian: Option<f64>,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self { seed: (seed ^ MULTIPLIER) & MASK, next_gaussian: None }
    }

    /// `setSeed`: also clears the cached Gaussian, like Java.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULTIPLIER) & MASK;
        self.next_gaussian = None;
    }

    /// Current scrambled internal state (what `Random.seed` holds).
    pub fn raw_seed(&self) -> i64 {
        self.seed
    }

    #[inline]
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    #[inline]
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`. Panics on `bound <= 0`, where Java throws.
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            // Rejection when the last bucket is partial; relies on i32 overflow like Java.
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * FLOAT_UNIT
    }

    pub fn next_double(&mut self) -> f64 {
        (((self.next(26) as i64) << 27) + self.next(27) as i64) as f64 * DOUBLE_UNIT
    }

    /// Polar Box–Muller with Java's caching of the second value.
    /// Uses `StrictMath.log`/`sqrt` in Java; see [`crate::strict_math::log`].
    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let m = (-2.0 * crate::strict_math::log(s) / s).sqrt();
                self.next_gaussian = Some(v2 * m);
                return v1 * m;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values produced by `java.util.Random` on JDK 8.

    #[test]
    fn matches_java_sequence() {
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_int(), -1155484576);
        assert_eq!(r.next_int_bounded(10), 8);
        assert_eq!(r.next_int_bounded(16), 3);
        assert_eq!(r.next_int_bounded(1_000_000_007), 302116440);
        assert_eq!(r.next_long(), -6688467811848818630);
        assert!(r.next_boolean());
        assert_eq!(r.next_float().to_bits(), 1039114536);
        assert_eq!(r.next_double().to_bits() as i64, 4603557428926125371);
        assert_eq!(r.next_gaussian().to_bits() as i64, -4613639202719167937);
        assert_eq!(r.next_gaussian().to_bits() as i64, -4615837303083591803);
        r.set_seed(-123456789012345);
        assert_eq!(r.next_int_bounded(7), 0);
    }

    #[test]
    fn bulk_mixed_draws_match_java() {
        let mut n = JavaRandom::new(42);
        let mut y: i64 = 0;
        for i in 0..100000 {
            let a = n.next_int_bounded(1 + (i % 1000)) as i64;
            let b = n.next_long();
            let c = n.next_double().to_bits() as i64;
            let d = n.next_float().to_bits() as i32 as i64;
            y = y.wrapping_mul(31).wrapping_add(a).wrapping_add(b).wrapping_add(c).wrapping_add(d);
        }
        assert_eq!(y, -2498592731544558166);
    }

    #[test]
    fn bulk_gaussians_match_java() {
        let mut g = JavaRandom::new(8675309);
        let mut x: i64 = 0;
        for _ in 0..100000 {
            x = x.wrapping_mul(31).wrapping_add(g.next_gaussian().to_bits() as i64);
        }
        assert_eq!(x, -1003008187085470269);
    }
}
