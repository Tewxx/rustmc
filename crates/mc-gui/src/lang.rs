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
        // Vanilla loads English first and overlays the selected language from all packs.
        let valid = !code.is_empty() && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_');
        for lang in ["en_US", if valid { code } else { "en_US" }] {
            for bytes in assets.read_all(&format!("lang/{lang}.lang")) {
                for line in String::from_utf8_lossy(&bytes).lines() {
                    if line.starts_with('#') { continue; }
                    if let Some((k, v)) = line.split_once('=') { map.insert(k.to_string(), v.to_string()); }
                }
            }
            if lang == code || !valid { break; }
        }
        Self { map }
    }

    /// Every translation, for the Forge runtime's `I18n`.
    pub fn table(&self) -> &HashMap<String, String> { &self.map }

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_language_overlays_english_and_pack_layers() {
        let root = std::env::temp_dir().join(format!("rustmc-lang-{}", std::process::id()));
        let base = root.join("base/assets/minecraft/lang");
        let pack = root.join("pack/assets/minecraft/lang");
        std::fs::create_dir_all(&base).unwrap(); std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(base.join("en_US.lang"), "fallback=English\nname=English\n").unwrap();
        std::fs::write(base.join("fr_FR.lang"), "name=Francais\npack=Base\n").unwrap();
        std::fs::write(pack.join("fr_FR.lang"), "pack=Override\n").unwrap();
        let mut assets = AssetSource::new(); assets.push(&root.join("base")).unwrap(); assets.push(&root.join("pack")).unwrap();
        let lang = Lang::load(&assets, "fr_FR");
        assert_eq!(lang.get("fallback"), "English"); assert_eq!(lang.get("name"), "Francais"); assert_eq!(lang.get("pack"), "Override");
        assert_eq!(Lang::load(&assets, "../../other").get("name"), "English");
        std::fs::remove_dir_all(root).unwrap();
    }
}
