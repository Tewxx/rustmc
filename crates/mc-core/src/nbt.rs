//! Uncompressed NBT (1.8 tag set: ids 0–11). Strings use Java's modified UTF-8.
//! Callers handle gzip/zlib.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// Element type id (vanilla keeps it even when empty), then elements.
    List(u8, Vec<Tag>),
    Compound(Compound),
    IntArray(Vec<i32>),
}

pub type Compound = BTreeMap<String, Tag>;

#[derive(Debug)]
pub enum NbtError {
    Eof,
    BadTag(u8),
    BadString,
    TooDeep,
}

impl fmt::Display for NbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NbtError::Eof => write!(f, "unexpected end of NBT data"),
            NbtError::BadTag(t) => write!(f, "invalid NBT tag id {t}"),
            NbtError::BadString => write!(f, "invalid modified UTF-8 string"),
            NbtError::TooDeep => write!(f, "NBT nested too deeply"),
        }
    }
}

impl std::error::Error for NbtError {}

// Vanilla's NBTSizeTracker limits depth to 512.
const MAX_DEPTH: usize = 512;

impl Tag {
    pub fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
        }
    }

    pub fn as_compound(&self) -> Option<&Compound> {
        match self {
            Tag::Compound(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(_, v) => Some(v),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Tag::ByteArray(v) => Some(v),
            _ => None,
        }
    }

    /// Numeric tags coerced to i64, like `NBTTagCompound.getInteger` on any numeric type.
    pub fn as_i64(&self) -> Option<i64> {
        Some(match *self {
            Tag::Byte(v) => v as i64,
            Tag::Short(v) => v as i64,
            Tag::Int(v) => v as i64,
            Tag::Long(v) => v,
            Tag::Float(v) => v as i64,
            Tag::Double(v) => v as i64,
            _ => return None,
        })
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NbtError> {
        let end = self.pos.checked_add(n).ok_or(NbtError::Eof)?;
        let s = self.buf.get(self.pos..end).ok_or(NbtError::Eof)?;
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, NbtError> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, NbtError> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, NbtError> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn len(&mut self) -> Result<usize, NbtError> {
        let n = self.i32()?;
        // Java would throw NegativeArraySizeException.
        usize::try_from(n).map_err(|_| NbtError::Eof)
    }

    fn string(&mut self) -> Result<String, NbtError> {
        let n = self.i16()? as u16 as usize;
        decode_mutf8(self.take(n)?)
    }

    fn payload(&mut self, id: u8, depth: usize) -> Result<Tag, NbtError> {
        if depth > MAX_DEPTH {
            return Err(NbtError::TooDeep);
        }
        Ok(match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.to_vec())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.payload(elem, depth + 1)?);
                }
                Tag::List(elem, v)
            }
            10 => {
                let mut c = Compound::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    c.insert(name, self.payload(t, depth + 1)?);
                }
                Tag::Compound(c)
            }
            11 => {
                let n = self.len()?;
                let bytes = self.take(n.checked_mul(4).ok_or(NbtError::Eof)?)?;
                Tag::IntArray(bytes.chunks_exact(4).map(|b| i32::from_be_bytes(b.try_into().unwrap())).collect())
            }
            t => return Err(NbtError::BadTag(t)),
        })
    }
}

/// Reads a named root tag (normally a compound). Returns `(name, tag)`.
pub fn read(buf: &[u8]) -> Result<(String, Tag), NbtError> {
    let mut r = Reader { buf, pos: 0 };
    let id = r.u8()?;
    if id == 0 {
        return Ok((String::new(), Tag::Compound(Compound::new())));
    }
    let name = r.string()?;
    Ok((name, r.payload(id, 0)?))
}

pub fn write(name: &str, tag: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(tag.id());
    write_string(&mut out, name);
    write_payload(&mut out, tag);
    out
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let b = encode_mutf8(s);
    out.extend_from_slice(&(b.len() as u16).to_be_bytes());
    out.extend_from_slice(&b);
}

fn write_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend_from_slice(v);
        }
        Tag::String(s) => write_string(out, s),
        Tag::List(elem, v) => {
            // Vanilla writes the first element's type, or 0 for an empty list.
            out.push(v.first().map_or(*elem, Tag::id));
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for t in v {
                write_payload(out, t);
            }
        }
        Tag::Compound(c) => {
            for (k, t) in c {
                out.push(t.id());
                write_string(out, k);
                write_payload(out, t);
            }
            out.push(0);
        }
        Tag::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for i in v {
                out.extend_from_slice(&i.to_be_bytes());
            }
        }
    }
}

/// Java modified UTF-8: NUL as 0xC0 0x80, supplementary chars as surrogate pairs.
fn encode_mutf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for u in s.encode_utf16() {
        match u {
            0x0001..=0x007F => out.push(u as u8),
            0x0000 | 0x0080..=0x07FF => {
                out.push(0xC0 | (u >> 6) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
            _ => {
                out.push(0xE0 | (u >> 12) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        }
    }
    out
}

fn decode_mutf8(b: &[u8]) -> Result<String, NbtError> {
    let mut units = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as u16;
        if c < 0x80 {
            units.push(c);
            i += 1;
        } else if c & 0xE0 == 0xC0 {
            let c2 = *b.get(i + 1).ok_or(NbtError::BadString)? as u16;
            units.push(((c & 0x1F) << 6) | (c2 & 0x3F));
            i += 2;
        } else if c & 0xF0 == 0xE0 {
            let c2 = *b.get(i + 1).ok_or(NbtError::BadString)? as u16;
            let c3 = *b.get(i + 2).ok_or(NbtError::BadString)? as u16;
            units.push(((c & 0x0F) << 12) | ((c2 & 0x3F) << 6) | (c3 & 0x3F));
            i += 3;
        } else {
            return Err(NbtError::BadString);
        }
    }
    // Java strings may hold lone surrogates; replace those rather than fail.
    Ok(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut c = Compound::new();
        c.insert("byte".into(), Tag::Byte(-3));
        c.insert("str".into(), Tag::String("h\u{0}é😀".into()));
        c.insert("list".into(), Tag::List(3, vec![Tag::Int(1), Tag::Int(2)]));
        c.insert("ints".into(), Tag::IntArray(vec![-1, 7]));
        c.insert("d".into(), Tag::Double(1.5));
        let root = Tag::Compound(c);
        let bytes = write("root", &root);
        assert_eq!(read(&bytes).unwrap(), ("root".to_string(), root));
    }

    #[test]
    fn nul_is_two_bytes() {
        assert_eq!(encode_mutf8("\0"), vec![0xC0, 0x80]);
        assert_eq!(encode_mutf8("😀").len(), 6);
    }

    #[test]
    fn truncated_input_errors() {
        let bytes = write("", &Tag::Compound(Compound::from([("a".into(), Tag::Long(5))])));
        assert!(read(&bytes[..bytes.len() - 3]).is_err());
    }
}
