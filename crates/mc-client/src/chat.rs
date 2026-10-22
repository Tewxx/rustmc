//! `IChatComponent` JSON (`IChatComponent.Serializer`) to the `§`-coded string the chat GUI
//! draws (`getFormattedText`).
use mc_gui::lang::Lang;
use serde_json::Value;

#[derive(Clone, Copy, Default)]
struct Style {
    color: Option<char>,
    bold: Option<bool>,
    italic: Option<bool>,
    underlined: Option<bool>,
    strikethrough: Option<bool>,
    obfuscated: Option<bool>,
}

impl Style {
    /// Child style inheriting unset fields from the parent (`ChatStyle.setParentStyle`).
    fn inherit(self, parent: Style) -> Style {
        Style {
            color: self.color.or(parent.color),
            bold: self.bold.or(parent.bold),
            italic: self.italic.or(parent.italic),
            underlined: self.underlined.or(parent.underlined),
            strikethrough: self.strikethrough.or(parent.strikethrough),
            obfuscated: self.obfuscated.or(parent.obfuscated),
        }
    }
    fn is_empty(&self) -> bool {
        self.color.is_none() && self.bold.is_none() && self.italic.is_none() && self.underlined.is_none() && self.strikethrough.is_none() && self.obfuscated.is_none()
    }
    /// `ChatStyle.getFormattingCode`
    fn code(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut s = String::new();
        if let Some(c) = self.color {
            s.push('\u{a7}');
            s.push(c);
        }
        for (on, c) in [(self.bold, 'l'), (self.italic, 'o'), (self.underlined, 'n'), (self.obfuscated, 'k'), (self.strikethrough, 'm')] {
            if on == Some(true) {
                s.push('\u{a7}');
                s.push(c);
            }
        }
        s
    }
}

fn color_code(name: &str) -> Option<char> {
    Some(match name {
        "black" => '0',
        "dark_blue" => '1',
        "dark_green" => '2',
        "dark_aqua" => '3',
        "dark_red" => '4',
        "dark_purple" => '5',
        "gold" => '6',
        "gray" => '7',
        "dark_gray" => '8',
        "blue" => '9',
        "green" => 'a',
        "aqua" => 'b',
        "red" => 'c',
        "light_purple" => 'd',
        "yellow" => 'e',
        "white" => 'f',
        "reset" => 'r',
        _ => return None,
    })
}

fn style_of(o: &serde_json::Map<String, Value>) -> Style {
    let b = |k: &str| o.get(k).and_then(Value::as_bool);
    Style {
        color: o.get("color").and_then(Value::as_str).and_then(color_code),
        bold: b("bold"),
        italic: b("italic"),
        underlined: b("underlined"),
        strikethrough: b("strikethrough"),
        obfuscated: b("obfuscated"),
    }
}

/// Appends each component of the tree as `code + text + §r`, like `getFormattedText`.
fn walk(v: &Value, parent: Style, lang: &Lang, out: &mut String) {
    match v {
        Value::String(s) => {
            out.push_str(&parent.code());
            out.push_str(s);
            out.push_str("\u{a7}r");
        }
        Value::Array(a) => {
            // The first element is the parent of the rest.
            if let Some((first, rest)) = a.split_first() {
                let st = match first {
                    Value::Object(o) => style_of(o).inherit(parent),
                    _ => parent,
                };
                walk(first, parent, lang, out);
                for r in rest {
                    walk(r, st, lang, out);
                }
            }
        }
        Value::Object(o) => {
            let st = style_of(o).inherit(parent);
            let text = if let Some(t) = o.get("text").and_then(Value::as_str) {
                t.to_string()
            } else if let Some(key) = o.get("translate").and_then(Value::as_str) {
                let args: Vec<String> = o
                    .get("with")
                    .and_then(Value::as_array)
                    .map(|w| {
                        w.iter()
                            .map(|a| {
                                // Arguments render with their own formatting, then return to this style.
                                let mut s = String::new();
                                walk(a, st, lang, &mut s);
                                s.push_str(&st.code());
                                s
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                lang.format(key, &refs)
            } else if let Some(s) = o.get("score").and_then(Value::as_object) {
                s.get("value").and_then(Value::as_str).unwrap_or("").to_string()
            } else if let Some(s) = o.get("selector").and_then(Value::as_str) {
                s.to_string()
            } else {
                String::new()
            };
            out.push_str(&st.code());
            out.push_str(&text);
            out.push_str("\u{a7}r");
            if let Some(extra) = o.get("extra").and_then(Value::as_array) {
                for e in extra {
                    walk(e, st, lang, out);
                }
            }
        }
        Value::Number(n) => {
            out.push_str(&parent.code());
            out.push_str(&n.to_string());
            out.push_str("\u{a7}r");
        }
        Value::Bool(b) => {
            out.push_str(&parent.code());
            out.push_str(&b.to_string());
            out.push_str("\u{a7}r");
        }
        Value::Null => {}
    }
}

/// Formatted chat text for a component's JSON; unparseable JSON is shown verbatim (lenient
/// Gson accepts bare strings).
pub fn format(json: &str, lang: &Lang) -> String {
    match serde_json::from_str::<Value>(json) {
        Ok(v) => {
            let mut s = String::new();
            walk(&v, Style::default(), lang, &mut s);
            s
        }
        Err(_) => json.to_string(),
    }
}

#[allow(dead_code)]
/// Plain text without codes (`getUnformattedText`), for the window title of disconnect screens.
pub fn unformatted(json: &str, lang: &Lang) -> String {
    let s = format(json, lang);
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\u{a7}' {
            it.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_flatten_like_get_formatted_text() {
        let lang = Lang::default();
        assert_eq!(format(r#"{"text":"hi"}"#, &lang), "hi\u{a7}r");
        assert_eq!(format(r#"{"text":"a","color":"red","extra":[{"text":"b","bold":true}]}"#, &lang), "\u{a7}ca\u{a7}r\u{a7}c\u{a7}lb\u{a7}r");
        assert_eq!(format(r#""plain""#, &lang), "plain\u{a7}r");
        assert_eq!(unformatted(r#"{"text":"x","extra":["y"]}"#, &lang), "xy");
    }
}
