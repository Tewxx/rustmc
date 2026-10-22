//! `PacketBuffer`: the primitive encodings shared by every packet.

use mc_core::nbt::{self, Compound, Tag};
use mc_core::BlockPos;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Ran out of bytes (`IndexOutOfBoundsException` in Netty).
    Eof,
    /// `VarInt too big` / `VarLong too big`
    VarTooBig,
    /// String byte length above `max * 4`, negative, or char length above `max`.
    StringTooLong { len: usize, max: usize },
    /// String longer than 32767 bytes when encoding.
    StringTooBig(usize),
    BadPacketId(i32),
    /// Bytes left over after `readPacketData` ("Packet was larger than I expected").
    Trailing(usize),
    Nbt(String),
    /// An enum ordinal or similar field out of range.
    BadValue(&'static str, i64),
    /// Frame or compression errors.
    Frame(&'static str),
    Inflate(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Eof => write!(f, "unexpected end of packet"),
            Error::VarTooBig => write!(f, "VarInt too big"),
            Error::StringTooLong { len, max } => {
                write!(f, "The received string length is longer than maximum allowed ({len} > {max})")
            }
            Error::StringTooBig(n) => write!(f, "String too big (was {n} bytes encoded, max 32767)"),
            Error::BadPacketId(id) => write!(f, "Bad packet id {id}"),
            Error::Trailing(n) => write!(f, "Packet was larger than I expected, found {n} bytes extra"),
            Error::Nbt(e) => write!(f, "NBT: {e}"),
            Error::BadValue(what, v) => write!(f, "invalid {what}: {v}"),
            Error::Frame(e) => write!(f, "{e}"),
            Error::Inflate(e) => write!(f, "inflate: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// `ItemStack` on the wire: `id` (-1 for empty), count, damage, optional tag.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub id: i16,
    pub count: i8,
    pub damage: i16,
    pub nbt: Option<Compound>,
}

/// UUID as Java's `(mostSignificantBits, leastSignificantBits)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Uuid(pub u64, pub u64);

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (m, l) = (self.0, self.1);
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            m >> 32,
            (m >> 16) & 0xffff,
            m & 0xffff,
            l >> 48,
            l & 0xffff_ffff_ffff
        )
    }
}

impl Uuid {
    /// `UUID.fromString` for the canonical dashed form.
    pub fn parse(s: &str) -> Option<Uuid> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 5 {
            return None;
        }
        let p: Vec<u64> = parts.iter().map(|p| u64::from_str_radix(p, 16).ok()).collect::<Option<_>>()?;
        Some(Uuid(p[0] << 32 | (p[1] & 0xffff) << 16 | (p[2] & 0xffff), (p[3] & 0xffff) << 48 | (p[4] & 0xffff_ffff_ffff)))
    }

    /// `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`: offline-mode player id.
    pub fn offline_player(name: &str) -> Uuid {
        let mut d = md5(format!("OfflinePlayer:{name}").as_bytes());
        d[6] = (d[6] & 0x0f) | 0x30;
        d[8] = (d[8] & 0x3f) | 0x80;
        Uuid(u64::from_be_bytes(d[0..8].try_into().unwrap()), u64::from_be_bytes(d[8..16].try_into().unwrap()))
    }
}

/// Minimal MD5 (RFC 1321) for offline UUIDs.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14,
        20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6,
        10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// `DataWatcher.WatchableObject` value types 0–7.
#[derive(Clone, Debug, PartialEq)]
pub enum MetaValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(String),
    Item(Option<Slot>),
    Pos(i32, i32, i32),
    Rotations(f32, f32, f32),
}

/// One watched entry: `(index 0–31, value)`.
pub type Metadata = Vec<(u8, MetaValue)>;

/// `PacketBuffer.getVarIntSize`
pub fn varint_size(v: i32) -> usize {
    for i in 1..5 {
        if (v as u32) & (u32::MAX << (i * 7)) == 0 {
            return i;
        }
    }
    5
}

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }
    pub fn bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }
    /// `writeVarIntToBuffer`
    pub fn varint(&mut self, v: i32) {
        let mut v = v as u32;
        while v & !0x7f != 0 {
            self.buf.push((v & 0x7f | 0x80) as u8);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }
    /// `writeVarLong`
    pub fn varlong(&mut self, v: i64) {
        let mut v = v as u64;
        while v & !0x7f != 0 {
            self.buf.push((v & 0x7f | 0x80) as u8);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }
    /// `writeString`. Vanilla throws past 32767 bytes; we truncate at a char boundary instead
    /// of panicking so a bad chat line can't take down the connection thread.
    pub fn string(&mut self, s: &str) {
        let mut b = s.as_bytes();
        if b.len() > 32767 {
            let mut n = 32767;
            while !s.is_char_boundary(n) {
                n -= 1;
            }
            b = &b[..n];
        }
        self.varint(b.len() as i32);
        self.buf.extend_from_slice(b);
    }
    /// `writeByteArray`
    pub fn byte_array(&mut self, v: &[u8]) {
        self.varint(v.len() as i32);
        self.buf.extend_from_slice(v);
    }
    /// `writeBlockPos`: `BlockPos.toLong`
    pub fn pos(&mut self, p: BlockPos) {
        self.i64(p.to_long());
    }
    /// `writeUuid`
    pub fn uuid(&mut self, u: Uuid) {
        self.i64(u.0 as i64);
        self.i64(u.1 as i64);
    }
    /// `writeNBTTagCompoundToBuffer`: a single 0 byte for null, else an unnamed root compound.
    pub fn nbt(&mut self, c: Option<&Compound>) {
        match c {
            None => self.u8(0),
            Some(c) => self.buf.extend_from_slice(&nbt::write("", &Tag::Compound(c.clone()))),
        }
    }
    /// `writeItemStackToBuffer`
    pub fn slot(&mut self, s: Option<&Slot>) {
        match s {
            None => self.i16(-1),
            Some(s) => {
                self.i16(s.id);
                self.i8(s.count);
                self.i16(s.damage);
                self.nbt(s.nbt.as_ref());
            }
        }
    }
    /// `DataWatcher.writeWatchedListToPacketBuffer`
    pub fn metadata(&mut self, m: &Metadata) {
        for (idx, v) in m {
            let ty = match v {
                MetaValue::Byte(_) => 0,
                MetaValue::Short(_) => 1,
                MetaValue::Int(_) => 2,
                MetaValue::Float(_) => 3,
                MetaValue::String(_) => 4,
                MetaValue::Item(_) => 5,
                MetaValue::Pos(..) => 6,
                MetaValue::Rotations(..) => 7,
            };
            self.u8(((ty << 5) | (idx & 31)) as u8);
            match v {
                MetaValue::Byte(b) => self.i8(*b),
                MetaValue::Short(s) => self.i16(*s),
                MetaValue::Int(i) => self.i32(*i),
                MetaValue::Float(f) => self.f32(*f),
                MetaValue::String(s) => self.string(s),
                MetaValue::Item(s) => self.slot(s.as_ref()),
                MetaValue::Pos(x, y, z) => {
                    self.i32(*x);
                    self.i32(*y);
                    self.i32(*z);
                }
                MetaValue::Rotations(x, y, z) => {
                    self.f32(*x);
                    self.f32(*y);
                    self.f32(*z);
                }
            }
        }
        self.u8(127);
    }
}

pub struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Eof);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    /// Netty `readBoolean`: any non-zero byte is true.
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.arr()?))
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.arr()?))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.arr()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.arr()?))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_be_bytes(self.arr()?))
    }
    /// `readVarIntFromBuffer`: at most 5 bytes.
    pub fn varint(&mut self) -> Result<i32> {
        let mut v = 0u32;
        let mut n = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u32) << (n * 7);
            n += 1;
            if n > 5 {
                return Err(Error::VarTooBig);
            }
            if b & 0x80 == 0 {
                return Ok(v as i32);
            }
        }
    }
    /// `readVarLong`: at most 10 bytes.
    pub fn varlong(&mut self) -> Result<i64> {
        let mut v = 0u64;
        let mut n = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << (n * 7);
            n += 1;
            if n > 10 {
                return Err(Error::VarTooBig);
            }
            if b & 0x80 == 0 {
                return Ok(v as i64);
            }
        }
    }
    /// `readStringFromBuffer(maxLength)`; `maxLength` counts UTF-16 units like Java.
    pub fn string(&mut self, max: usize) -> Result<String> {
        let n = self.varint()?;
        if n < 0 || n as usize > max * 4 {
            return Err(Error::StringTooLong { len: n.max(0) as usize, max: max * 4 });
        }
        let s = String::from_utf8_lossy(self.take(n as usize)?).into_owned();
        let units = s.encode_utf16().count();
        if units > max {
            return Err(Error::StringTooLong { len: units, max });
        }
        Ok(s)
    }
    /// `readChatComponent`: the JSON text (callers parse it).
    pub fn chat(&mut self) -> Result<String> {
        self.string(32767)
    }
    /// `readByteArray`
    pub fn byte_array(&mut self) -> Result<Vec<u8>> {
        let n = self.varint()?;
        if n < 0 {
            return Err(Error::BadValue("byte array length", n as i64));
        }
        Ok(self.take(n as usize)?.to_vec())
    }
    pub fn pos(&mut self) -> Result<BlockPos> {
        Ok(BlockPos::from_long(self.i64()?))
    }
    pub fn uuid(&mut self) -> Result<Uuid> {
        Ok(Uuid(self.i64()? as u64, self.i64()? as u64))
    }
    /// `readNBTTagCompoundFromBuffer`
    pub fn nbt(&mut self) -> Result<Option<Compound>> {
        if self.remaining() == 0 {
            return Err(Error::Eof);
        }
        if self.buf[self.pos] == 0 {
            self.pos += 1;
            return Ok(None);
        }
        let len = nbt_len(&self.buf[self.pos..]).ok_or(Error::Eof)?;
        if len > 2097152 {
            return Err(Error::Nbt("Tried to read NBT tag that was too big".into()));
        }
        let data = self.take(len)?;
        match nbt::read(data) {
            Ok((_, Tag::Compound(c))) => Ok(Some(c)),
            Ok(_) => Err(Error::Nbt("Root tag must be a named compound tag".into())),
            Err(e) => Err(Error::Nbt(e.to_string())),
        }
    }
    /// `readItemStackFromBuffer`
    pub fn slot(&mut self) -> Result<Option<Slot>> {
        let id = self.i16()?;
        if id < 0 {
            return Ok(None);
        }
        let count = self.i8()?;
        let damage = self.i16()?;
        let nbt = self.nbt()?;
        Ok(Some(Slot { id, count, damage, nbt }))
    }
    /// `DataWatcher.readWatchedListFromPacketBuffer` (an empty list reads as empty, not null).
    pub fn metadata(&mut self) -> Result<Metadata> {
        let mut out = Vec::new();
        loop {
            let b = self.u8()?;
            if b == 127 {
                return Ok(out);
            }
            let idx = b & 31;
            let v = match b >> 5 {
                0 => MetaValue::Byte(self.i8()?),
                1 => MetaValue::Short(self.i16()?),
                2 => MetaValue::Int(self.i32()?),
                3 => MetaValue::Float(self.f32()?),
                4 => MetaValue::String(self.string(32767)?),
                5 => MetaValue::Item(self.slot()?),
                6 => MetaValue::Pos(self.i32()?, self.i32()?, self.i32()?),
                _ => MetaValue::Rotations(self.f32()?, self.f32()?, self.f32()?),
            };
            out.push((idx, v));
        }
    }
}

/// Byte length of the named root tag at the start of `b`, without decoding it.
fn nbt_len(b: &[u8]) -> Option<usize> {
    fn get(b: &[u8], p: usize, n: usize) -> Option<&[u8]> {
        b.get(p..p.checked_add(n)?)
    }
    fn be_u16(b: &[u8], p: usize) -> Option<usize> {
        Some(u16::from_be_bytes(get(b, p, 2)?.try_into().ok()?) as usize)
    }
    fn be_i32(b: &[u8], p: usize) -> Option<usize> {
        let v = i32::from_be_bytes(get(b, p, 4)?.try_into().ok()?);
        usize::try_from(v).ok()
    }
    fn payload(b: &[u8], p: usize, id: u8, depth: usize) -> Option<usize> {
        if depth > 512 {
            return None;
        }
        Some(match id {
            1 => p + 1,
            2 => p + 2,
            3 | 5 => p + 4,
            4 | 6 => p + 8,
            7 => p + 4 + be_i32(b, p)?,
            8 => p + 2 + be_u16(b, p)?,
            9 => {
                let et = *b.get(p)?;
                let n = be_i32(b, p + 1)?;
                let mut q = p + 5;
                for _ in 0..n {
                    q = payload(b, q, et, depth + 1)?;
                    if q > b.len() {
                        return None;
                    }
                }
                q
            }
            10 => {
                let mut q = p;
                loop {
                    let t = *b.get(q)?;
                    q += 1;
                    if t == 0 {
                        break q;
                    }
                    q += 2 + be_u16(b, q)?;
                    q = payload(b, q, t, depth + 1)?;
                }
            }
            11 => p + 4 + be_i32(b, p)?.checked_mul(4)?,
            _ => return None,
        })
    }
    let id = *b.first()?;
    let p = 3 + be_u16(b, 1)?;
    let end = payload(b, p, id, 0)?;
    (end <= b.len()).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_match_vanilla() {
        for (v, bytes) in [
            (0, vec![0x00]),
            (1, vec![0x01]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (300, vec![0xac, 0x02]),
            (2147483647, vec![0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, vec![0xff, 0xff, 0xff, 0xff, 0x0f]),
        ] {
            let mut w = Writer::new();
            w.varint(v);
            assert_eq!(w.buf, bytes, "{v}");
            assert_eq!(varint_size(v), bytes.len());
            assert_eq!(Reader::new(&bytes).varint().unwrap(), v);
        }
    }

    #[test]
    fn offline_uuid_matches_java() {
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))
        assert_eq!(Uuid::offline_player("Notch").to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        let u = Uuid::offline_player("Steve");
        assert_eq!(Uuid::parse(&u.to_string()), Some(u));
    }

    #[test]
    fn string_limits_count_utf16_units() {
        let mut w = Writer::new();
        w.string("héllo");
        assert_eq!(Reader::new(&w.buf).string(5).unwrap(), "héllo");
        assert!(Reader::new(&w.buf).string(4).is_err());
    }

    #[test]
    fn slot_and_nbt_round_trip() {
        let mut tag = Compound::new();
        tag.insert("Unbreakable".into(), Tag::Byte(1));
        let mut list = Compound::new();
        list.insert("lvl".into(), Tag::Short(3));
        tag.insert("ench".into(), Tag::List(10, vec![Tag::Compound(list)]));
        let s = Slot { id: 276, count: 1, damage: 5, nbt: Some(tag) };
        let mut w = Writer::new();
        w.slot(Some(&s));
        w.slot(None);
        w.i32(42);
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.slot().unwrap(), Some(s));
        assert_eq!(r.slot().unwrap(), None);
        assert_eq!(r.i32().unwrap(), 42);
    }
}
