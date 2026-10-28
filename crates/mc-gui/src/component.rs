//! `IChatComponent` as the GUI sees it: the component tree flattened in iteration order
//! (`createDeepCopyIterator`), each piece with its text and style.
//!
//! A piece keeps two formatting codes. `code` is `getChatStyle().getFormattingCode()` where the
//! piece sits now (parents included); `own_code` is the same for the piece's own fields only,
//! which is what survives `createShallowCopy` + `appendSibling` in `GuiUtilRenderComponents`.
//! Click, hover and insertion are stored as the piece's own fields: those are the only ones a
//! split chat line still has, so vanilla only reacts to events set on the clicked text itself.

use crate::font::FontRenderer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickAction {
    OpenUrl,
    OpenFile,
    RunCommand,
    TwitchUserInfo,
    SuggestCommand,
    ChangePage,
}

impl ClickAction {
    /// `ClickEvent.Action.getValueByCanonicalName`
    pub fn by_name(s: &str) -> Option<Self> {
        Some(match s {
            "open_url" => Self::OpenUrl,
            "open_file" => Self::OpenFile,
            "run_command" => Self::RunCommand,
            "twitch_user_info" => Self::TwitchUserInfo,
            "suggest_command" => Self::SuggestCommand,
            "change_page" => Self::ChangePage,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverAction {
    ShowText,
    ShowAchievement,
    ShowItem,
    ShowEntity,
}

impl HoverAction {
    pub fn by_name(s: &str) -> Option<Self> {
        Some(match s {
            "show_text" => Self::ShowText,
            "show_achievement" => Self::ShowAchievement,
            "show_item" => Self::ShowItem,
            "show_entity" => Self::ShowEntity,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClickEvent {
    pub action: ClickAction,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HoverEvent {
    pub action: HoverAction,
    pub value: Component,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piece {
    /// `getUnformattedTextForChat`
    pub text: String,
    pub code: String,
    pub own_code: String,
    pub click: Option<ClickEvent>,
    pub hover: Option<HoverEvent>,
    pub insertion: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Component(pub Vec<Piece>);

impl Component {
    /// `new ChatComponentText(text)`: legacy `§` codes stay inside the text.
    pub fn text(s: impl Into<String>) -> Self {
        Component(vec![Piece { text: s.into(), ..Default::default() }])
    }

    /// `getFormattedText`: `code + text + §r` for every non-empty piece.
    pub fn formatted(&self) -> String {
        let mut s = String::new();
        for p in &self.0 {
            if !p.text.is_empty() {
                s.push_str(&p.code);
                s.push_str(&p.text);
                s.push_str("\u{a7}r");
            }
        }
        s
    }

    /// `getUnformattedText`
    pub fn unformatted(&self) -> String {
        self.0.iter().map(|p| p.text.as_str()).collect()
    }
}

impl From<String> for Component {
    fn from(s: String) -> Self {
        Component::text(s)
    }
}

impl From<&str> for Component {
    fn from(s: &str) -> Self {
        Component::text(s)
    }
}

/// `EnumChatFormatting.getTextWithoutFormattingCodes`: drops `§` and the character after it.
pub fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\u{a7}' {
            // The regex is `(?i)§[0-9A-FK-OR]`; other pairs stay.
            match it.clone().next() {
                Some(n) if n.is_ascii_hexdigit() || matches!(n.to_ascii_lowercase(), 'k'..='o' | 'r') => {
                    it.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `GuiUtilRenderComponents.func_178909_a`
pub fn chat_colors(s: &str, force: bool, chat_colours: bool) -> String {
    if !force && !chat_colours {
        strip_codes(s)
    } else {
        s.to_string()
    }
}

fn substring(s: &[char], from: usize, to: usize) -> String {
    s[from..to].iter().collect()
}

/// `GuiUtilRenderComponents.splitText`
pub fn split_text(c: &Component, max: i32, font: &FontRenderer, trim_space: bool, force_color: bool, chat_colours: bool) -> Vec<Component> {
    let mut i = 0;
    let mut line = Component(vec![Piece::default()]);
    let mut out = Vec::new();
    let mut list: Vec<Piece> = c.0.clone();
    let mut j = 0;
    while j < list.len() {
        let piece = list[j].clone();
        let mut s: Vec<char> = piece.text.chars().collect();
        let mut flag = false;
        // A shallow style copy keeps the parent until `appendSibling` re-parents it to the
        // line, so queued remainders keep `code` and appended pieces only their own fields.
        let copy = |text: String| Piece { text, code: piece.own_code.clone(), ..piece.clone() };
        if let Some(k) = s.iter().position(|&ch| ch == '\n') {
            let rest = substring(&s, k + 1, s.len());
            s.truncate(k + 1);
            list.insert(j + 1, Piece { text: rest, ..piece.clone() });
            flag = true;
        }
        let s4: Vec<char> = chat_colors(&format!("{}{}", piece.code, s.iter().collect::<String>()), force_color, chat_colours).chars().collect();
        let s5: String = if s4.last() == Some(&'\n') { substring(&s4, 0, s4.len() - 1) } else { s4.iter().collect() };
        let mut i1 = font.width(&s5);
        let mut text1 = copy(s5);
        if i + i1 > max {
            let s4s: String = s4.iter().collect();
            let mut s2: Vec<char> = font.trim(&s4s, max - i, false).chars().collect();
            let s3 = (s2.len() < s4.len()).then(|| substring(&s4, s2.len(), s4.len()));
            if let Some(mut s3) = s3.filter(|t| !t.is_empty()) {
                let l = s2.iter().rposition(|&ch| ch == ' ');
                match l {
                    Some(mut l) if font.width(&substring(&s4, 0, l)) > 0 => {
                        s2 = s4[..l].to_vec();
                        if trim_space {
                            l += 1;
                        }
                        s3 = substring(&s4, l, s4.len());
                    }
                    _ if i > 0 && !s4.contains(&' ') => {
                        s2.clear();
                        s3 = s4s.clone();
                    }
                    _ => {}
                }
                list.insert(j + 1, Piece { text: s3, ..piece.clone() });
            }
            let s2s: String = s2.iter().collect();
            i1 = font.width(&s2s);
            text1 = copy(s2s);
            flag = true;
        }
        if i + i1 <= max {
            i += i1;
            line.0.push(text1);
        } else {
            flag = true;
        }
        if flag {
            out.push(std::mem::replace(&mut line, Component(vec![Piece::default()])));
            i = 0;
        }
        j += 1;
    }
    out.push(line);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_keeps_unknown_pairs() {
        assert_eq!(strip_codes("\u{a7}ca\u{a7}lb\u{a7}zc"), "ab\u{a7}zc");
    }

    #[test]
    fn formatted_skips_empty_pieces() {
        let c = Component(vec![
            Piece { text: String::new(), code: "\u{a7}c".into(), ..Default::default() },
            Piece { text: "x".into(), code: "\u{a7}e".into(), ..Default::default() },
        ]);
        assert_eq!(c.formatted(), "\u{a7}ex\u{a7}r");
    }
}
