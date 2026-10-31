//! Bounded RMD1 HUD command decoding.
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Rect { left: i32, top: i32, right: i32, bottom: i32, argb: u32 },
    Text { x: i32, y: i32, argb: u32, shadow: bool, text: String },
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Draw>> {
    if bytes.len() > 256 * 1024 || !bytes.starts_with(b"RMD1") { bail!("invalid HUD stream"); }
    let mut i = 4;
    let mut out = Vec::new();
    fn take<'a>(data: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8]> {
        let end = at.checked_add(n).ok_or_else(|| anyhow::anyhow!("invalid length"))?;
        let result = data.get(*at..end).ok_or_else(|| anyhow::anyhow!("truncated HUD stream"))?;
        *at = end; Ok(result)
    }
    fn word(data: &[u8], at: &mut usize) -> Result<u32> { Ok(u32::from_le_bytes(take(data, at, 4)?.try_into().unwrap())) }
    while i < bytes.len() {
        if out.len() >= 16_384 { bail!("too many HUD commands"); }
        let op = take(bytes, &mut i, 1)?[0];
        let command = match op {
            0 => Draw::Rect { left: word(bytes, &mut i)? as i32, top: word(bytes, &mut i)? as i32, right: word(bytes, &mut i)? as i32, bottom: word(bytes, &mut i)? as i32, argb: word(bytes, &mut i)? },
            3 => {
                let x = word(bytes, &mut i)? as i32; let y = word(bytes, &mut i)? as i32; let argb = word(bytes, &mut i)?;
                let shadow = match take(bytes, &mut i, 1)?[0] { 0 => false, 1 => true, _ => bail!("invalid shadow flag") };
                let len = u16::from_le_bytes(take(bytes, &mut i, 2)?.try_into().unwrap()) as usize;
                let text = std::str::from_utf8(take(bytes, &mut i, len)?)?.to_owned();
                Draw::Text { x, y, argb, shadow, text }
            }
            _ => bail!("unsupported HUD command"),
        };
        out.push(command);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_and_invalid_streams() {
        let mut b = b"RMD1\0".to_vec();
        for x in [1u32, 2, 3, 4, 0xff010203] { b.extend(x.to_le_bytes()); }
        assert_eq!(decode(&b).unwrap(), vec![Draw::Rect { left: 1, top: 2, right: 3, bottom: 4, argb: 0xff010203 }]);
        b.pop(); assert!(decode(&b).is_err());
        assert!(decode(b"RMD1\xff").is_err());
    }
}
