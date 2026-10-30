//! `I18n` over `lang/en_US.lang` (`key=value` lines, Java `String.format` args).

use mc_assets::AssetSource;
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Lang {
    map: HashMap<String, String>,
}

impl Lang {
    pub fn load(assets: &AssetSource, code: &str) -> Self {
        let mut map = HashMap::new();
        // `LanguageManager`: every pack's file, lowest first, so a partial pack file only overrides.
        for bytes in assets.read_all(&format!("lang/{code}.lang")) {
            for line in String::from_utf8_lossy(&bytes).lines() {
                if line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        Self { map }
    }

    /// `I18n.format(key)`: the key itself when missing, like vanilla.
    pub fn get(&self, key: &str) -> String {
        self.map.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    /// `I18n.format(key, args...)` for `%s`/`%d`/`%N$s` placeholders.
    pub fn format(&self, key: &str, args: &[&str]) -> String {
        let pattern = self.get(key);
        let mut out = String::new();
        let mut next = 0;
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            let mut spec = String::new();
            while let Some(&d) = chars.peek() {
                spec.push(d);
                chars.next();
                if d.is_ascii_alphabetic() || d == '%' {
                    break;
                }
            }
            if spec == "%" {
                out.push('%');
                continue;
            }
            let idx = match spec.split_once('$') {
                Some((n, _)) => n.parse::<usize>().unwrap_or(1) - 1,
                None => {
                    next += 1;
                    next - 1
                }
            };
            out.push_str(args.get(idx).copied().unwrap_or(""));
        }
        out
    }
}
