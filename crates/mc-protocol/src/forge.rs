//! Forge 1.8.9 FML handshake payloads (protocol 2).
//!
//! Transport is carried by play-state plugin messages. The caller must validate the server's
//! mod list and complete registry snapshot before acknowledging them.
use crate::{Reader, Writer};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub name: String,
    pub ids: BTreeMap<String, i32>,
    pub substitutions: BTreeSet<String>,
    pub dummied: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    ServerHello { version: u8, dimension: Option<i32> },
    ClientHello(u8),
    ModList(BTreeMap<String, String>),
    RegistryData { more: bool, registry: Registry },
    Ack(u8),
    Reset,
}

fn count(r: &mut Reader<'_>, max_bytes: usize) -> Result<usize, String> {
    let mut n = 0usize;
    for shift in 0..max_bytes {
        let b = r.u8().map_err(|e| e.to_string())?;
        n |= ((b & 0x7f) as usize) << (shift * 7);
        if b & 0x80 == 0 { return Ok(n); }
    }
    Err("oversized Forge VarInt".into())
}
fn string(r: &mut Reader<'_>) -> Result<String, String> {
    let len = count(r, 2)?;
    let bytes = r.take(len).map_err(|e| e.to_string())?;
    String::from_utf8(bytes.to_vec()).map_err(|_| "invalid UTF-8 in Forge payload".into())
}
fn strings(r: &mut Reader<'_>) -> Result<BTreeSet<String>, String> {
    let n = count(r, 3)?;
    if n > r.remaining() { return Err("invalid Forge list length".into()); }
    let mut out = BTreeSet::new();
    for _ in 0..n { if !out.insert(string(r)?) { return Err("duplicate Forge registry entry".into()); } }
    Ok(out)
}

pub fn decode(data: &[u8]) -> Result<Message, String> {
    let mut r = Reader::new(data);
    let id = r.u8().map_err(|e| e.to_string())?;
    let msg = match id {
        0 => { let version = r.u8().map_err(|e| e.to_string())?; let dimension = if version > 1 { Some(r.i32().map_err(|e| e.to_string())?) } else { None }; Message::ServerHello { version, dimension } }
        1 => Message::ClientHello(r.u8().map_err(|e| e.to_string())?),
        2 => {
            let n = count(&mut r, 2)?;
            if n > r.remaining() / 2 { return Err("invalid Forge mod count".into()); }
            let mut mods = BTreeMap::new();
            for _ in 0..n { let key = string(&mut r)?; let value = string(&mut r)?; if mods.insert(key, value).is_some() { return Err("duplicate Forge mod".into()); } }
            Message::ModList(mods)
        }
        3 => {
            let more = r.bool().map_err(|e| e.to_string())?;
            let name = string(&mut r)?;
            let n = count(&mut r, 3)?;
            if n > r.remaining() / 2 { return Err("invalid Forge registry count".into()); }
            let mut ids = BTreeMap::new();
            for _ in 0..n { let key = string(&mut r)?; let value = count(&mut r, 3)? as i32; if ids.insert(key, value).is_some() { return Err("duplicate Forge registry id".into()); } }
            let substitutions = strings(&mut r)?;
            let dummied = if r.remaining() > 0 { strings(&mut r)? } else { BTreeSet::new() };
            Message::RegistryData { more, registry: Registry { name, ids, substitutions, dummied } }
        }
        255 => Message::Ack(r.u8().map_err(|e| e.to_string())?),
        254 => Message::Reset,
        _ => return Err(format!("unknown Forge handshake discriminator {id}")),
    };
    if r.remaining() != 0 { return Err("trailing Forge payload bytes".into()); }
    Ok(msg)
}

fn encode(msg: &Message) -> Vec<u8> {
    let mut w = Writer::new();
    match msg {
        Message::ClientHello(v) => { w.u8(1); w.u8(*v); }
        Message::ModList(mods) => { w.u8(2); w.varint(mods.len() as i32); for (k,v) in mods { w.string(k); w.string(v); } }
        Message::Ack(phase) => { w.u8(255); w.u8(*phase); }
        _ => panic!("not a client handshake message"),
    }
    w.buf
}

/// A single `FML|MP` server payload. Forge sends a channel/part-count/length preamble,
/// then parts numbered from zero. Bound the retained data while mod transport is unavailable.
pub const MAX_MULTIPART_BYTES: usize = 16 * 1024 * 1024;
pub struct Multipart {
    channel: String,
    parts: u8,
    next: u8,
    length: usize,
    data: Vec<u8>,
}
impl Multipart {
    pub fn new(preamble: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(preamble);
        let channel = r.string(20).map_err(|e| e.to_string())?;
        let parts = r.u8().map_err(|e| e.to_string())?;
        let length = r.i32().map_err(|e| e.to_string())?;
        if r.remaining() != 0 || channel.is_empty() || channel == "FML|MP" || channel.contains('\0') || parts == 0 || length <= 0 || length as usize > MAX_MULTIPART_BYTES {
            return Err("invalid or unsupported Forge multipart preamble".into());
        }
        Ok(Self { channel, parts, next: 0, length: length as usize, data: Vec::new() })
    }
    /// Returns the reassembled channel and payload only after all declared bytes arrive.
    pub fn push(&mut self, part: &[u8]) -> Result<Option<(String, Vec<u8>)>, String> {
        let (&number, payload) = part.split_first().ok_or("empty Forge multipart packet")?;
        if self.next >= self.parts || number != self.next || payload.is_empty() || payload.len() > self.length.saturating_sub(self.data.len()) {
            return Err("invalid Forge multipart sequence or length".into());
        }
        self.data.extend_from_slice(payload);
        self.next += 1;
        if self.next == self.parts {
            if self.data.len() != self.length { return Err("incomplete Forge multipart payload".into()); }
            Ok(Some((self.channel.clone(), std::mem::take(&mut self.data))))
        } else {
            if self.data.len() == self.length { return Err("Forge multipart payload finished before its last part".into()); }
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase { Hello, Mods, Registries, Pending, Complete, Done }

/// Ordered FML handshake. `validate` is called for each complete registry before the final ack.
pub struct ClientHandshake {
    phase: Phase,
    pub dimension: Option<i32>,
    pub server_mods: BTreeMap<String, String>,
    registries: BTreeMap<String, Registry>,
    client_mods: BTreeMap<String, String>,
}
impl ClientHandshake {
    pub fn new(client_mods: BTreeMap<String, String>) -> Self {
        Self { phase: Phase::Hello, dimension: None, server_mods: BTreeMap::new(), registries: BTreeMap::new(), client_mods }
    }
    pub fn is_complete(&self) -> bool { self.phase == Phase::Done }
    pub fn accept(&mut self, data: &[u8], mut validate: impl FnMut(&BTreeMap<String, Registry>) -> Result<(), String>) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
        let msg = decode(data)?;
        match (self.phase, msg) {
            (Phase::Hello, Message::ServerHello { version: 1 | 2, dimension }) => {
                self.dimension = dimension; self.phase = Phase::Mods;
                Ok(vec![("REGISTER", b"FML|HS\0FML\0FML|MP\0FML\0FORGE".to_vec()), ("FML|HS", encode(&Message::ClientHello(2))), ("FML|HS", encode(&Message::ModList(self.client_mods.clone())))])
            }
            (Phase::Mods, Message::ModList(mods)) => { self.server_mods = mods; self.phase = Phase::Registries; Ok(vec![("FML|HS", encode(&Message::Ack(2)))]) }
            (Phase::Registries, Message::RegistryData { more, registry }) => {
                if self.registries.len() >= 16 { return Err("too many Forge registries".into()); }
                if self.registries.insert(registry.name.clone(), registry).is_some() { return Err("duplicate Forge registry".into()); }
                if more { Ok(vec![]) } else {
                    validate(&self.registries)?; self.phase = Phase::Pending;
                    Ok(vec![("FML|HS", encode(&Message::Ack(3)))])
                }
            }
            (Phase::Pending, Message::Ack(2)) => { self.phase = Phase::Complete; Ok(vec![("FML|HS", encode(&Message::Ack(4)))]) }
            (Phase::Complete, Message::Ack(3)) => { self.phase = Phase::Done; Ok(vec![("FML|HS", encode(&Message::Ack(5)))]) }
            (Phase::Done, Message::Reset) => { self.phase = Phase::Hello; self.registries.clear(); self.server_mods.clear(); self.dimension = None; Ok(vec![]) }
            (phase, _) => Err(format!("unexpected Forge handshake message in {phase:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty_registry(more: bool) -> Vec<u8> {
        let mut w = Writer::new(); w.u8(3); w.bool(more); w.string("minecraft:blocks"); w.varint(0); w.varint(0); w.varint(0); w.buf
    }
    #[test]
    fn complete_remote_handshake_and_reset() {
        let mut h = ClientHandshake::new(BTreeMap::new());
        let hello = h.accept(&[0,2,0,0,0,1], |_| Ok(())).unwrap();
        assert_eq!(h.dimension, Some(1));
        assert_eq!(hello, vec![("REGISTER", b"FML|HS\0FML\0FML|MP\0FML\0FORGE".to_vec()), ("FML|HS", vec![1,2]), ("FML|HS", vec![2,0])]);
        assert_eq!(h.accept(&[2,0], |_| Ok(())).unwrap()[0].1, vec![255,2]);
        assert_eq!(h.accept(&empty_registry(false), |r| { assert!(r.contains_key("minecraft:blocks")); Ok(()) }).unwrap()[0].1, vec![255,3]);
        assert_eq!(h.accept(&[255,2], |_| Ok(())).unwrap()[0].1, vec![255,4]);
        assert_eq!(h.accept(&[255,3], |_| Ok(())).unwrap()[0].1, vec![255,5]);
        assert!(h.is_complete());
        assert!(h.accept(&[254], |_| Ok(())).unwrap().is_empty());
        assert!(!h.is_complete());
    }
    fn preamble(channel: &str, parts: u8, len: i32) -> Vec<u8> {
        let mut w = Writer::new(); w.string(channel); w.u8(parts); w.i32(len); w.buf
    }
    #[test]
    fn multipart_reassembles_and_rejects_bad_sequences() {
        let mut m = Multipart::new(&preamble("FML|HS", 2, 5)).unwrap();
        assert_eq!(m.push(&[0, 1, 2]).unwrap(), None);
        assert_eq!(m.push(&[1, 3, 4, 5]).unwrap(), Some(("FML|HS".into(), vec![1, 2, 3, 4, 5])));
        assert!(m.push(&[2, 6]).is_err());
        assert!(Multipart::new(&preamble("FML|MP", 1, 1)).is_err());
        assert!(Multipart::new(&preamble("FML", 0, 1)).is_err());
        assert!(Multipart::new(&preamble("FML", 1, MAX_MULTIPART_BYTES as i32 + 1)).is_err());
        assert!(Multipart::new(&preamble("FML", 2, 2)).unwrap().push(&[1, 3]).is_err());
        assert!(Multipart::new(&preamble("FML", 1, 2)).unwrap().push(&[0, 3]).is_err());
        assert!(Multipart::new(&preamble("FML", 1, 1)).unwrap().push(&[0, 3, 4]).is_err());
    }
    #[test]
    fn malformed_and_out_of_order_messages_fail() {
        assert!(decode(&[0,2]).is_err());
        assert!(decode(&[2,0x80,0x80,0]).is_err());
        assert!(decode(&[2,1,1,0xff,0]).is_err());
        let mut h = ClientHandshake::new(BTreeMap::new());
        assert!(h.accept(&[255,2], |_| Ok(())).is_err());
        assert!(h.accept(&[0,2,0,0,0,0], |_| Ok(())).is_ok());
        assert!(h.accept(&[2,0], |_| Ok(())).is_ok());
        assert!(h.accept(&empty_registry(false), |_| Err("missing block".into())).is_err());
        assert!(!h.is_complete());
    }
}
