//! Connection framing: `MessageSerializer2` / `MessageDeserializer2` (VarInt length prefix,
//! at most 3 bytes) and `NettyCompressionEncoder` / `Decoder` (zlib above a threshold).
//! Encryption (AES/CFB8) sits below this layer and is not handled here.

use crate::buf::{varint_size, Error, Reader, Result, Writer};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use std::io::{Read, Write};

/// Vanilla limit on a decompressed packet.
pub const MAX_UNCOMPRESSED: usize = 2097152;

/// Frames one packet body (`id + fields`) onto `out`. `threshold < 0` means compression is off.
pub fn encode(body: &[u8], threshold: i32, out: &mut Vec<u8>) {
    let inner: Vec<u8> = if threshold < 0 {
        body.to_vec()
    } else {
        let mut w = Writer::new();
        if (body.len() as i32) < threshold {
            w.varint(0);
            w.bytes(body);
        } else {
            w.varint(body.len() as i32);
            let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
            z.write_all(body).unwrap();
            w.bytes(&z.finish().unwrap());
        }
        w.buf
    };
    debug_assert!(varint_size(inner.len() as i32) <= 3, "unable to fit {} into 3", inner.len());
    let mut w = Writer::new();
    w.varint(inner.len() as i32);
    out.extend_from_slice(&w.buf);
    out.extend_from_slice(&inner);
}

/// Accumulates stream bytes and yields complete packet bodies.
pub struct Decoder {
    buf: Vec<u8>,
    start: usize,
    /// `< 0` while compression is off.
    pub threshold: i32,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self { buf: Vec::new(), start: 0, threshold: -1 }
    }

    pub fn feed(&mut self, data: &[u8]) {
        if self.start > 0 && self.start * 2 > self.buf.len() {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(data);
    }

    /// Next packet body, `Ok(None)` if more bytes are needed.
    pub fn next_packet(&mut self) -> Result<Option<Vec<u8>>> {
        let avail = &self.buf[self.start..];
        let mut len = 0u32;
        let mut header = 0;
        loop {
            if header >= avail.len() {
                return Ok(None);
            }
            let b = avail[header];
            len |= ((b & 0x7f) as u32) << (header * 7);
            header += 1;
            if b & 0x80 == 0 {
                break;
            }
            if header >= 3 {
                return Err(Error::Frame("length wider than 21-bit"));
            }
        }
        let len = len as usize;
        if avail.len() < header + len {
            return Ok(None);
        }
        let frame = &avail[header..header + len];
        self.start += header + len;
        if self.threshold < 0 || frame.is_empty() {
            return Ok(Some(frame.to_vec()));
        }
        let mut r = Reader::new(frame);
        let size = r.varint()?;
        if size == 0 {
            return Ok(Some(r.rest().to_vec()));
        }
        if size < self.threshold {
            return Err(Error::Frame("Badly compressed packet - size is below server threshold"));
        }
        if size as usize > MAX_UNCOMPRESSED {
            return Err(Error::Frame("Badly compressed packet - size is larger than protocol maximum"));
        }
        let mut out = vec![0u8; size as usize];
        ZlibDecoder::new(r.rest()).read_exact(&mut out).map_err(|e| Error::Inflate(e.to_string()))?;
        Ok(Some(out))
    }
}
