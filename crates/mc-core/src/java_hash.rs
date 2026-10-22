//! Iteration order of JDK 8 `java.util.HashMap` / `HashSet`.
//!
//! Vanilla iterates hash maps in several places where the order is visible (potion effect lists,
//! attribute modifier products). A JDK 8 map walks its buckets in index order, and each bucket in
//! insertion order (resizes split buckets without reordering). The table starts at 16 and doubles
//! whenever the size exceeds 3/4 of it; it never shrinks. Treeified buckets (8+ collisions in a
//! table of 64+) are not modelled.

/// `HashMap.hash`: spreads the high bits of `hashCode()` down.
#[inline]
pub fn spread(h: i32) -> u32 {
    let h = h as u32;
    h ^ (h >> 16)
}

/// `Integer.hashCode`
#[inline]
pub fn int_hash(v: i32) -> i32 {
    v
}

/// `UUID.hashCode`
#[inline]
pub fn uuid_hash(msb: u64, lsb: u64) -> i32 {
    let hilo = msb ^ lsb;
    ((hilo >> 32) as i32) ^ (hilo as i32)
}

/// `HashMap.tableSizeFor`
fn table_size_for(c: usize) -> usize {
    c.max(1).next_power_of_two()
}

/// A map that iterates like a JDK 8 `HashMap` built by the same insert/remove sequence.
#[derive(Clone, Debug)]
pub struct JavaHashMap<K, V> {
    entries: Vec<(i32, K, V)>,
    cap: usize,
}

impl<K: PartialEq, V> Default for JavaHashMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq, V> JavaHashMap<K, V> {
    pub fn new() -> Self {
        Self { entries: Vec::new(), cap: 16 }
    }

    /// `new HashSet<>(collection)` / `Sets.newHashSet(collection)` sizing:
    /// `max(size / .75 + 1, 16)` rounded up to a power of two.
    pub fn with_expected(n: usize) -> Self {
        Self { entries: Vec::with_capacity(n), cap: table_size_for(((n as f32 / 0.75) as usize + 1).max(16)) }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `put`: replaces the value in place, or appends and resizes past the 0.75 load factor.
    pub fn insert(&mut self, hash: i32, k: K, v: V) -> Option<V> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.1 == k) {
            return Some(std::mem::replace(&mut e.2, v));
        }
        self.entries.push((hash, k, v));
        if self.entries.len() > self.cap * 3 / 4 {
            self.cap *= 2;
        }
        None
    }

    pub fn remove(&mut self, k: &K) -> Option<V> {
        let i = self.entries.iter().position(|e| e.1 == *k)?;
        Some(self.entries.remove(i).2)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.entries.iter().find(|e| e.1 == *k).map(|e| &e.2)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.entries.iter_mut().find(|e| e.1 == *k).map(|e| &mut e.2)
    }

    pub fn contains_key(&self, k: &K) -> bool {
        self.entries.iter().any(|e| e.1 == *k)
    }

    /// Entries in Java iteration order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let mask = self.cap as u32 - 1;
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| spread(self.entries[i].0) & mask);
        order.into_iter().map(move |i| (&self.entries[i].1, &self.entries[i].2))
    }

    /// Mutable access in insertion order (for updates that do not depend on the order).
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|e| &mut e.2)
    }

    pub fn hash_of(&self, k: &K) -> Option<i32> {
        self.entries.iter().find(|e| e.1 == *k).map(|e| e.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_keys_iterate_by_bucket() {
        let mut m = JavaHashMap::new();
        for id in [17, 1, 5, 3] {
            m.insert(int_hash(id), id, ());
        }
        // 17 and 1 share bucket 1 (17 inserted first), then 3, then 5.
        let keys: Vec<i32> = m.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec![17, 1, 3, 5]);
    }

    #[test]
    fn resize_past_load_factor() {
        let mut m = JavaHashMap::new();
        for id in 0..13 {
            m.insert(int_hash(id * 16), id * 16, ());
        }
        // 13 > 12 doubles the table to 32: multiples of 32 come before odd multiples of 16.
        let keys: Vec<i32> = m.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys[..7], [0, 32, 64, 96, 128, 160, 192]);
        assert_eq!(keys[7..], [16, 48, 80, 112, 144, 176]);
    }

    #[test]
    fn uuid_hash_matches_java() {
        // UUID.fromString("662A6B8D-DA3E-4C1C-8813-96EA6097278D").hashCode()
        let (m, l) = (0x662A6B8D_DA3E_4C1Cu64, 0x8813_96EA6097278Du64);
        let hilo = m ^ l;
        assert_eq!(uuid_hash(m, l), ((hilo >> 32) as i32) ^ (hilo as i32));
    }
}
