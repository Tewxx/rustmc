//! `JsonToNBT`: the 1.8 "mojangson" text form of NBT (`/give` tags, `show_item` hovers).
//! A direct port, quirks included (`[d|D]` also accepts `|` as a suffix, numbers that fail to
//! parse become strings, mismatched list elements are dropped).

use crate::nbt::{Compound, Tag};

#[derive(Debug, Clone, PartialEq)]
pub struct NbtException(pub String);

type Result<T> = std::result::Result<T, NbtException>;

fn err<T>(s: impl Into<String>) -> Result<T> {
    Err(NbtException(s.into()))
}

/// `getTagFromJson`
pub fn parse(json: &str) -> Result<Compound> {
    let s: Vec<char> = json.trim().chars().collect();
    if s.first() != Some(&'{') {
        return err("Invalid tag encountered, expected '{' as first char.");
    }
    if top_tags(&s)? != 1 {
        return err("Encountered multiple top tags, only one expected");
    }
    match any("tag", &s)?.parse() {
        Tag::Compound(c) => Ok(c),
        _ => err("not a compound"),
    }
}

/// `func_179271_b`: the quote at `i` is escaped by an unescaped backslash.
fn escaped(s: &[char], i: usize) -> bool {
    i > 0 && s[i - 1] == '\\' && !escaped(s, i - 1)
}

/// `func_150310_b`: the number of top-level bracketed tags.
fn top_tags(s: &[char]) -> Result<i32> {
    let mut i = 0;
    let mut quoted = false;
    let mut stack = Vec::new();
    for (j, &c) in s.iter().enumerate() {
        if c == '"' {
            if escaped(s, j) {
                if !quoted {
                    return err("Illegal use of \\\"");
                }
            } else {
                quoted = !quoted;
            }
        } else if !quoted {
            if c != '{' && c != '[' {
                if c == '}' && stack.pop() != Some('{') {
                    return err("Unbalanced curly brackets {}");
                }
                if c == ']' && stack.pop() != Some('[') {
                    return err("Unbalanced square brackets []");
                }
            } else {
                if stack.is_empty() {
                    i += 1;
                }
                stack.push(c);
            }
        }
    }
    if quoted {
        return err("Unbalanced quotation");
    }
    if !stack.is_empty() {
        return err("Unbalanced brackets");
    }
    if i == 0 && !s.is_empty() {
        i = 1;
    }
    Ok(i)
}

enum Any {
    Compound(String, Vec<Any>),
    List(String, Vec<Any>),
    Primitive(String, String),
}

impl Any {
    fn name(&self) -> &str {
        match self {
            Any::Compound(n, _) | Any::List(n, _) | Any::Primitive(n, _) => n,
        }
    }

    fn parse(&self) -> Tag {
        match self {
            Any::Compound(_, children) => {
                let mut c = Compound::new();
                for a in children {
                    c.insert(a.name().to_string(), a.parse());
                }
                Tag::Compound(c)
            }
            Any::List(_, children) => {
                // NBTTagList.appendTag: the first element fixes the type; others are dropped.
                let mut ty = 0u8;
                let mut out = Vec::new();
                for a in children {
                    let t = a.parse();
                    let id = t.id();
                    if id == 0 {
                        continue;
                    }
                    if ty == 0 {
                        ty = id;
                    } else if ty != id {
                        continue;
                    }
                    out.push(t);
                }
                Tag::List(ty, out)
            }
            Any::Primitive(_, v) => primitive(v),
        }
    }
}

fn trim(s: &[char]) -> &[char] {
    // String.trim: code points <= ' ' at both ends.
    let start = s.iter().position(|&c| c > ' ').unwrap_or(s.len());
    let end = s.iter().rposition(|&c| c > ' ').map_or(start, |e| e + 1);
    &s[start..end.max(start)]
}

/// `\[[-+\d|,\s]+\]`
fn is_int_array_literal(s: &[char]) -> bool {
    s.len() >= 3 && s[0] == '[' && s[s.len() - 1] == ']' && s[1..s.len() - 1].iter().all(|&c| c == '-' || c == '+' || c.is_ascii_digit() || c == '|' || c == ',' || c.is_whitespace())
}

/// `func_150316_a`
fn any(name: &str, value: &[char]) -> Result<Any> {
    let v = trim(value);
    if v.first() == Some(&'{') {
        let mut rest: &[char] = &v[1..v.len().saturating_sub(1).max(1)];
        let mut children = Vec::new();
        while !rest.is_empty() {
            let s1 = split_entry(rest, true)?;
            if !s1.is_empty() {
                children.push(entry(&rest[..s1.len()], false)?);
            }
            if rest.len() < s1.len() + 1 {
                break;
            }
            let c1 = rest[s1.len()];
            if !matches!(c1, ',' | '{' | '}' | '[' | ']') {
                return err(format!("Unexpected token '{c1}'"));
            }
            rest = &rest[s1.len() + 1..];
        }
        Ok(Any::Compound(name.to_string(), children))
    } else if v.first() == Some(&'[') && !is_int_array_literal(v) {
        let mut rest: &[char] = &v[1..v.len().saturating_sub(1).max(1)];
        let mut children = Vec::new();
        while !rest.is_empty() {
            let s = split_entry(rest, false)?;
            if !s.is_empty() {
                children.push(entry(&rest[..s.len()], true)?);
            }
            if rest.len() < s.len() + 1 {
                break;
            }
            let c0 = rest[s.len()];
            if !matches!(c0, ',' | '{' | '}' | '[' | ']') {
                return err(format!("Unexpected token '{c0}'"));
            }
            rest = &rest[s.len() + 1..];
        }
        Ok(Any::List(name.to_string(), children))
    } else {
        Ok(Any::Primitive(name.to_string(), v.iter().collect()))
    }
}

/// `func_179270_a`
fn entry(s: &[char], list: bool) -> Result<Any> {
    let name = entry_name(s, list)?;
    let value = entry_value(s, list)?;
    any(&name, &value)
}

/// `func_150314_a`: the next entry, up to its separator.
fn split_entry(s: &[char], compound: bool) -> Result<Vec<char>> {
    let mut i = find_unquoted(s, ':');
    let j = find_unquoted(s, ',');
    if compound {
        if i.is_none() {
            return err("Unable to locate name/value separator");
        }
        if let (Some(j), Some(ii)) = (j, i) {
            if j < ii {
                return err("Name error");
            }
        }
    } else {
        // `i == -1 || i > j` on Java ints: a name only counts before a later comma.
        i = match (i, j) {
            (Some(ii), Some(jj)) if ii < jj => Some(ii),
            _ => None,
        };
    }
    entry_end(s, i)
}

/// `func_179269_a`
fn entry_end(s: &[char], sep: Option<usize>) -> Result<Vec<char>> {
    let mut stack = Vec::new();
    let mut i = sep.map_or(0, |p| p + 1);
    let mut quoted = false;
    let mut had_quote = false;
    let mut nonspace = false;
    let mut j = 0usize;
    while i < s.len() {
        let c0 = s[i];
        if c0 == '"' {
            if escaped(s, i) {
                if !quoted {
                    return err("Illegal use of \\\"");
                }
            } else {
                quoted = !quoted;
                if quoted && !nonspace {
                    had_quote = true;
                }
                if !quoted {
                    j = i;
                }
            }
        } else if !quoted {
            if c0 != '{' && c0 != '[' {
                if c0 == '}' && stack.pop() != Some('{') {
                    return err("Unbalanced curly brackets {}");
                }
                if c0 == ']' && stack.pop() != Some('[') {
                    return err("Unbalanced square brackets []");
                }
                if c0 == ',' && stack.is_empty() {
                    return Ok(s[..i].to_vec());
                }
            } else {
                stack.push(c0);
            }
        }
        if !c0.is_whitespace() {
            if !quoted && had_quote && j != i {
                return Ok(s[..j + 1].to_vec());
            }
            nonspace = true;
        }
        i += 1;
    }
    Ok(s[..i].to_vec())
}

/// `func_150313_b`
fn entry_name(s: &[char], list: bool) -> Result<String> {
    let mut s = s;
    if list {
        s = trim(s);
        if s.first() == Some(&'{') || s.first() == Some(&'[') {
            return Ok(String::new());
        }
    }
    match find_unquoted(s, ':') {
        None if list => Ok(String::new()),
        None => err("Unable to locate name/value separator"),
        Some(i) => Ok(trim(&s[..i]).iter().collect()),
    }
}

/// `func_150311_c`
fn entry_value(s: &[char], list: bool) -> Result<Vec<char>> {
    let mut s = s;
    if list {
        s = trim(s);
        if s.first() == Some(&'{') || s.first() == Some(&'[') {
            return Ok(s.to_vec());
        }
    }
    match find_unquoted(s, ':') {
        None if list => Ok(s.to_vec()),
        None => err("Unable to locate name/value separator"),
        Some(i) => Ok(trim(&s[i + 1..]).to_vec()),
    }
}

/// `func_150312_a`: the first unquoted `c` before any bracket.
fn find_unquoted(s: &[char], c: char) -> Option<usize> {
    let mut outside = true;
    for (i, &c0) in s.iter().enumerate() {
        if c0 == '"' {
            if !escaped(s, i) {
                outside = !outside;
            }
        } else if outside {
            if c0 == c {
                return Some(i);
            }
            if c0 == '{' || c0 == '[' {
                return None;
            }
        }
    }
    None
}

/// `[-+]?[0-9]+`
fn is_int(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

/// `[-+]?[0-9]*\.?[0-9]+`
fn is_decimal(s: &str) -> bool {
    let d = s.strip_prefix(['-', '+']).unwrap_or(s);
    match d.split_once('.') {
        Some((a, b)) => a.bytes().all(|x| x.is_ascii_digit()) && !b.is_empty() && b.bytes().all(|x| x.is_ascii_digit()),
        None => !d.is_empty() && d.bytes().all(|x| x.is_ascii_digit()),
    }
}

fn with_suffix<'a>(s: &'a str, suffixes: &[char]) -> Option<&'a str> {
    let last = s.chars().last()?;
    suffixes.contains(&last).then(|| &s[..s.len() - last.len_utf8()])
}

/// `Primitive.parse`
fn primitive(v: &str) -> Tag {
    let unescape_quotes = |s: &str| s.replace("\\\"", "\"");
    // Each typed pattern matches first; a value its parser rejects becomes a string.
    macro_rules! typed {
        ($sfx:expr, $check:ident, $ty:ty, $variant:ident) => {
            if let Some(body) = with_suffix(v, $sfx).filter(|b| $check(b)) {
                return match body.parse::<$ty>() {
                    Ok(n) => Tag::$variant(n),
                    Err(_) => Tag::String(unescape_quotes(v)),
                };
            }
        };
    }
    typed!(&['d', '|', 'D'], is_decimal, f64, Double);
    typed!(&['f', '|', 'F'], is_decimal, f32, Float);
    typed!(&['b', '|', 'B'], is_int, i8, Byte);
    typed!(&['l', '|', 'L'], is_int, i64, Long);
    typed!(&['s', '|', 'S'], is_int, i16, Short);
    if is_int(v) {
        return match v.parse::<i32>() {
            Ok(n) => Tag::Int(n),
            Err(_) => Tag::String(unescape_quotes(v)),
        };
    }
    if is_decimal(v) {
        return match v.parse::<f64>() {
            Ok(n) => Tag::Double(n),
            Err(_) => Tag::String(unescape_quotes(v)),
        };
    }
    if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
        return Tag::Byte(v.eq_ignore_ascii_case("true") as i8);
    }
    if v.starts_with('[') && v.ends_with(']') && v.len() >= 2 {
        let inner = &v[1..v.len() - 1];
        let parts: Vec<&str> = inner.split(',').filter(|p| !p.is_empty()).collect();
        let ints: std::result::Result<Vec<i32>, _> = parts.iter().map(|p| p.trim().parse::<i32>()).collect();
        return match ints {
            Ok(a) => Tag::IntArray(a),
            Err(_) => Tag::String(v.to_string()),
        };
    }
    let mut s = v;
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = &s[1..s.len() - 1];
    }
    let s = unescape_quotes(s);
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '\\' && chars[i + 1] == '\\' {
            out.push('\\');
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Tag::String(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_tag() {
        let c = parse(r#"{id:"minecraft:diamond_sword",Count:1b,Damage:0s,tag:{display:{Name:"Excalibur",Lore:["a","b"]},ench:[{id:16s,lvl:5s}]}}"#).unwrap();
        assert_eq!(c["id"], Tag::String("minecraft:diamond_sword".into()));
        assert_eq!(c["Count"], Tag::Byte(1));
        assert_eq!(c["Damage"], Tag::Short(0));
        let tag = c["tag"].as_compound().unwrap();
        let display = tag["display"].as_compound().unwrap();
        assert_eq!(display["Name"], Tag::String("Excalibur".into()));
        assert_eq!(display["Lore"], Tag::List(8, vec![Tag::String("a".into()), Tag::String("b".into())]));
        let ench = tag["ench"].as_list().unwrap();
        assert_eq!(ench[0].as_compound().unwrap()["lvl"], Tag::Short(5));
    }

    #[test]
    fn primitives_and_quirks() {
        assert_eq!(primitive("1.5d"), Tag::Double(1.5));
        assert_eq!(primitive("3|"), Tag::Double(3.0));
        assert_eq!(primitive("300b"), Tag::String("300b".into()));
        assert_eq!(primitive("12"), Tag::Int(12));
        assert_eq!(primitive(".5"), Tag::Double(0.5));
        assert_eq!(primitive("true"), Tag::Byte(1));
        assert_eq!(primitive("[1,2,3]"), Tag::IntArray(vec![1, 2, 3]));
        assert_eq!(primitive(r#""a\"b""#), Tag::String("a\"b".into()));
        assert!(parse("[1]").is_err());
        assert!(parse("{a:1}{b:2}").is_err());
    }
}
