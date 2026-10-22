//! Login encryption (`CryptManager`): SHA-1 server hash, RSA/ECB/PKCS1Padding for the shared
//! secret and verify token, and the AES/CFB8 stream cipher both directions use afterwards.
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use num_bigint::{BigInt, BigUint, Sign};

/// SHA-1 (FIPS 180-1).
#[derive(Clone)]
pub struct Sha1 {
    h: [u32; 5],
    buf: Vec<u8>,
    len: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    pub fn new() -> Self {
        Self { h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0], buf: Vec::new(), len: 0 }
    }
    pub fn update(&mut self, data: &[u8]) {
        self.len += data.len() as u64;
        self.buf.extend_from_slice(data);
        let full = self.buf.len() / 64 * 64;
        let blocks: Vec<u8> = self.buf.drain(..full).collect();
        for b in blocks.chunks(64) {
            self.block(b);
        }
    }
    fn block(&mut self, b: &[u8]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut bb, mut c, mut d, mut e] = self.h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((bb & c) | (!bb & d), 0x5A827999),
                20..=39 => (bb ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((bb & c) | (bb & d) | (c & d), 0x8F1BBCDC),
                _ => (bb ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = bb.rotate_left(30);
            bb = a;
            a = t;
        }
        for (h, v) in self.h.iter_mut().zip([a, bb, c, d, e]) {
            *h = h.wrapping_add(v);
        }
    }
    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.len * 8;
        let mut pad = vec![0x80u8];
        while (self.buf.len() + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_be_bytes());
        let len = self.len;
        self.update(&pad);
        self.len = len;
        let mut out = [0u8; 20];
        for (i, h) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
        }
        out
    }
}

/// `new BigInteger(CryptManager.getServerIdHash(...)).toString(16)`: the session server hash.
pub fn server_hash(server_id: &str, secret: &[u8], public_key: &[u8]) -> String {
    let mut s = Sha1::new();
    s.update(server_id.as_bytes());
    s.update(secret);
    s.update(public_key);
    BigInt::from_signed_bytes_be(&s.finish()).to_str_radix(16)
}

/// AES/CFB8 with the shared secret as key and IV, one instance per direction.
pub struct Cfb8 {
    aes: Aes128,
    iv: [u8; 16],
}

impl Cfb8 {
    pub fn new(secret: &[u8; 16]) -> Self {
        Self { aes: Aes128::new(secret.into()), iv: *secret }
    }
    fn keystream(&self) -> u8 {
        let mut block = self.iv.into();
        self.aes.encrypt_block(&mut block);
        block[0]
    }
    fn shift(&mut self, c: u8) {
        self.iv.copy_within(1.., 0);
        self.iv[15] = c;
    }
    pub fn encrypt(&mut self, data: &mut [u8]) {
        for b in data {
            *b ^= self.keystream();
            self.shift(*b);
        }
    }
    pub fn decrypt(&mut self, data: &mut [u8]) {
        for b in data {
            let c = *b;
            *b ^= self.keystream();
            self.shift(c);
        }
    }
}

/// Minimal DER walker for `SubjectPublicKeyInfo { AlgorithmIdentifier, BIT STRING { RSAPublicKey } }`.
fn der(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *data.first()?;
    let mut len = *data.get(1)? as usize;
    let mut i = 2;
    if len & 0x80 != 0 {
        let n = len & 0x7f;
        len = 0;
        for _ in 0..n {
            len = len << 8 | *data.get(i)? as usize;
            i += 1;
        }
    }
    let body = data.get(i..i + len)?;
    Some((tag, body, &data[i + len..]))
}

/// RSA modulus and exponent from an X.509 encoded public key.
pub fn parse_public_key(x509: &[u8]) -> Option<(BigUint, BigUint)> {
    let (0x30, spki, _) = der(x509)? else { return None };
    let (0x30, _, rest) = der(spki)? else { return None };
    let (0x03, bits, _) = der(rest)? else { return None };
    let (0x30, key, _) = der(bits.get(1..)?)? else { return None };
    let (0x02, n, rest) = der(key)? else { return None };
    let (0x02, e, _) = der(rest)? else { return None };
    Some((BigUint::from_bytes_be(n), BigUint::from_bytes_be(e)))
}

/// `RSA/ECB/PKCS1Padding` encryption; `rand` supplies the nonzero padding bytes.
pub fn rsa_encrypt(x509: &[u8], msg: &[u8], mut rand: impl FnMut() -> u8) -> Option<Vec<u8>> {
    let (n, e) = parse_public_key(x509)?;
    let k = (n.bits() as usize).div_ceil(8);
    if msg.len() + 11 > k {
        return None;
    }
    let mut em = vec![0u8, 2];
    while em.len() < k - msg.len() - 1 {
        let b = rand();
        if b != 0 {
            em.push(b);
        }
    }
    em.push(0);
    em.extend_from_slice(msg);
    let c = BigUint::from_bytes_be(&em).modpow(&e, &n);
    let bytes = c.to_bytes_be();
    let mut out = vec![0u8; k - bytes.len()];
    out.extend_from_slice(&bytes);
    let _ = Sign::Plus;
    Some(out)
}

/// Random bytes from the OS-seeded hasher keys (shared secret, padding).
pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    let mut out = Vec::with_capacity(n);
    let mut ctr = 0u64;
    while out.len() < n {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(ctr);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        out.extend_from_slice(&h.finish().to_le_bytes());
        ctr += 1;
    }
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_vectors() {
        let h = |s: &[u8]| {
            let mut x = Sha1::new();
            x.update(s);
            hex(&x.finish())
        };
        assert_eq!(h(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(h(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(h(&[b'a'; 1000]), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    }

    #[test]
    fn minecraft_hex_digests() {
        // wiki.vg reference values for sha1(name) as Java's signed BigInteger.
        let d = |s: &str| {
            let mut x = Sha1::new();
            x.update(s.as_bytes());
            BigInt::from_signed_bytes_be(&x.finish()).to_str_radix(16)
        };
        assert_eq!(d("Notch"), "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48");
        assert_eq!(d("jeb_"), "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1");
        assert_eq!(d("simon"), "88e16a1019277b15d58faf0541e11910eb756f6");
    }

    #[test]
    fn cfb8_roundtrip_and_stream() {
        let key = [7u8; 16];
        let mut data = b"hello minecraft protocol".to_vec();
        let orig = data.clone();
        let (mut e, mut d) = (Cfb8::new(&key), Cfb8::new(&key));
        let (a, b) = data.split_at_mut(5);
        e.encrypt(a);
        e.encrypt(b);
        assert_ne!(data, orig);
        d.decrypt(&mut data);
        assert_eq!(data, orig);
    }
}
