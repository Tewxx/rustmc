//! `IChatComponent` JSON (`IChatComponent.Serializer`) to the `§`-coded string the chat GUI
//! draws (`getFormattedText`).
use mc_gui::component::{ClickAction, ClickEvent, Component, HoverAction, HoverEvent, Piece};
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

/// The JSON object's own click, hover and insertion fields (`ChatStyle.Serializer`).
fn events_of(o: &serde_json::Map<String, Value>, lang: &Lang) -> (Option<ClickEvent>, Option<HoverEvent>, Option<String>) {
    let click = o.get("clickEvent").and_then(Value::as_object).and_then(|c| {
        let action = ClickAction::by_name(c.get("action")?.as_str()?)?;
        let value = c.get("value")?.as_str()?.to_string();
        Some(ClickEvent { action, value })
    });
    let hover = o.get("hoverEvent").and_then(Value::as_object).and_then(|h| {
        let action = HoverAction::by_name(h.get("action")?.as_str()?)?;
        let value = parse_value(h.get("value")?, lang);
        Some(HoverEvent { action, value })
    });
    let insertion = o.get("insertion").and_then(Value::as_str).map(str::to_string);
    (click, hover, insertion)
}

/// One component and its parent's style while walking the tree.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    parent: Style,
    lang: &'a Lang,
}

fn push(out: &mut Vec<Piece>, text: String, own: Style, parent: Style, ev: (Option<ClickEvent>, Option<HoverEvent>, Option<String>)) {
    let code = own.inherit(parent).code();
    out.push(Piece { text, code, own_code: own.code(), click: ev.0, hover: ev.1, insertion: ev.2 });
}

/// `ChatComponentTranslation.initializeFromFormat`: literal runs and `%s`/`%N$s` arguments as
/// children of the translation.
fn translate(key: &str, args: &[Value], st: Style, ctx: Ctx, out: &mut Vec<Piece>) {
    let pattern = ctx.lang.get(key);
    let chars: Vec<char> = pattern.chars().collect();
    let mut literal = String::new();
    let mut next = 0usize;
    let mut i = 0;
    let flush = |lit: &mut String, out: &mut Vec<Piece>| {
        if !lit.is_empty() {
            push(out, std::mem::take(lit), Style::default(), st, (None, None, None));
        }
    };
    while i < chars.len() {
        if chars[i] != '%' {
            literal.push(chars[i]);
            i += 1;
            continue;
        }
        // %(?:(\d+)\$)?([A-Za-z%]|$)
        let mut j = i + 1;
        let digits_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        let index = if j > digits_start && j < chars.len() && chars[j] == '$' {
            let n: usize = chars[digits_start..j].iter().collect::<String>().parse().unwrap_or(1);
            j += 1;
            Some(n.saturating_sub(1))
        } else {
            j = i + 1;
            None
        };
        let conv = chars.get(j).copied();
        match conv {
            Some('%') => {
                literal.push('%');
                i = j + 1;
            }
            Some('s') | None => {
                flush(&mut literal, out);
                let idx = index.unwrap_or_else(|| {
                    next += 1;
                    next - 1
                });
                if let Some(a) = args.get(idx) {
                    // Arguments are components (strings become text) re-parented to this style.
                    walk(a, Ctx { parent: st, ..ctx }, out);
                }
                i = j + 1;
            }
            Some(_) => {
                // Not `%s`/`%%` (no match, or a conversion vanilla rejects): keep the `%`.
                literal.push('%');
                i += 1;
            }
        }
    }
    flush(&mut literal, out);
}

/// Appends the tree's components in iteration order (`createDeepCopyIterator`).
fn walk(v: &Value, ctx: Ctx, out: &mut Vec<Piece>) {
    // `{"translate":"%1$s%1$s%1$s...","with":[<same again>]}` nested 20 deep expands to 50^20
    // components: cap the work per parse so a hostile line cannot freeze or exhaust the client.
    if WORK.with(|w| {
        let n = w.get();
        w.set(n.saturating_sub(1));
        n == 0
    }) {
        return;
    }
    let none = || (None, None, None);
    match v {
        Value::String(s) => push(out, s.clone(), Style::default(), ctx.parent, none()),
        Value::Array(a) => {
            // The first element is the parent of the rest.
            if let Some((first, rest)) = a.split_first() {
                let st = match first {
                    Value::Object(o) => style_of(o).inherit(ctx.parent),
                    _ => ctx.parent,
                };
                walk(first, ctx, out);
                for r in rest {
                    walk(r, Ctx { parent: st, ..ctx }, out);
                }
            }
        }
        Value::Object(o) => {
            let own = style_of(o);
            let st = own.inherit(ctx.parent);
            let ev = events_of(o, ctx.lang);
            if let Some(key) = o.get("translate").and_then(Value::as_str) {
                let args = o.get("with").and_then(Value::as_array).cloned().unwrap_or_default();
                // The translation has no text of its own, so its events only reach its
                // children through the parent style, which wrapped chat lines drop.
                let _ = ev;
                translate(key, &args, st, ctx, out);
            } else {
                let text = if let Some(t) = o.get("text") {
                    match t {
                        Value::String(t) => t.clone(),
                        other => other.to_string(),
                    }
                } else if let Some(s) = o.get("score").and_then(Value::as_object) {
                    s.get("value").and_then(Value::as_str).unwrap_or("").to_string()
                } else if let Some(s) = o.get("selector").and_then(Value::as_str) {
                    s.to_string()
                } else {
                    String::new()
                };
                push(out, text, own, ctx.parent, ev);
            }
            if let Some(extra) = o.get("extra").and_then(Value::as_array) {
                for e in extra {
                    walk(e, Ctx { parent: st, ..ctx }, out);
                }
            }
        }
        Value::Number(n) => push(out, n.to_string(), Style::default(), ctx.parent, none()),
        Value::Bool(b) => push(out, b.to_string(), Style::default(), ctx.parent, none()),
        Value::Null => {}
    }
}

thread_local! {
    static WORK: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn parse_value(v: &Value, lang: &Lang) -> Component {
    WORK.with(|w| w.set(20_000));
    let mut out = Vec::new();
    walk(v, Ctx { parent: Style::default(), lang }, &mut out);
    Component(out)
}

/// `IChatComponent.Serializer.jsonToComponent`; unparseable JSON is kept as plain text
/// (lenient Gson accepts bare strings).
pub fn parse(json: &str, lang: &Lang) -> Component {
    match serde_json::from_str::<Value>(json) {
        Ok(v) => parse_value(&v, lang),
        Err(_) => Component::text(json),
    }
}

/// Formatted chat text for a component's JSON (`getFormattedText`).
pub fn format(json: &str, lang: &Lang) -> String {
    parse(json, lang).formatted()
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

    #[test]
    fn exponential_translation_trees_are_cut_off() {
        let lang = Lang::default();
        let pat = "%1$s".repeat(50);
        let mut json = r#""x""#.to_string();
        for _ in 0..30 {
            json = format!(r#"{{"translate":"{pat}","with":[{json}]}}"#);
        }
        let t = std::time::Instant::now();
        let c = parse(&json, &lang);
        assert!(c.0.len() <= 20_000);
        assert!(t.elapsed().as_secs() < 5);
    }

    #[test]
    fn click_events_stay_on_their_own_piece() {
        let lang = Lang::default();
        let c = parse(r#"{"text":"see ","extra":[{"text":"https://example.com","clickEvent":{"action":"open_url","value":"https://example.com"}}]}"#, &lang);
        assert_eq!(c.0.len(), 2);
        assert!(c.0[0].click.is_none());
        let click = c.0[1].click.as_ref().unwrap();
        assert_eq!(click.action, ClickAction::OpenUrl);
        assert_eq!(click.value, "https://example.com");
    }

    #[test]
    fn translation_arguments_keep_their_style() {
        let lang = Lang::default();
        // Without en_US loaded the key is its own pattern; `%s` slots take the arguments.
        let c = parse(r#"{"translate":"<%s> %s","with":[{"text":"Steve","color":"yellow"},"hi"]}"#, &lang);
        assert_eq!(c.formatted(), "<\u{a7}r\u{a7}eSteve\u{a7}r> \u{a7}rhi\u{a7}r");
    }
}
