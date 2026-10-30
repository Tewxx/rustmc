//! Account Manager 1.8 interaction state, separated from credentials and host-owned sessions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind { Offline, Premium, Microsoft, Token, Refresh, Cookie }

/// Only public metadata and an opaque host-owned identifier cross the guest boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BanStatus { Clear, Until(u64), Permanent }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub uuid: Option<String>,
    pub kind: AccountKind,
    pub ban: BanStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Activate(String),
    Remove(String),
    Add(AccountKind),
    RestoreLaunch,
    Move { id: String, before: Option<String> },
}

#[derive(Default, Debug)]
pub struct ScreenState {
    selected: Option<String>,
    pending: Option<u32>,
}
impl ScreenState {
    pub fn selected<'a>(&self, accounts: &'a [Account]) -> Option<&'a Account> {
        accounts.iter().find(|a| Some(a.id.as_str()) == self.selected.as_deref())
    }
    pub fn select(&mut self, accounts: &[Account], index: usize) {
        self.selected = accounts.get(index).map(|a| a.id.clone());
    }
    /// Up/down moves selection; with shift the host is asked to persist the reordered list.
    pub fn navigate(&mut self, accounts: &[Account], down: bool, shift: bool) -> Option<Action> {
        let current = accounts.iter().position(|a| Some(a.id.as_str()) == self.selected.as_deref())?;
        let next = if down { current.checked_add(1).filter(|&i| i < accounts.len())? } else { current.checked_sub(1)? };
        if shift {
            let before = if down { accounts.get(next + 1).map(|a| a.id.clone()) } else { Some(accounts[next].id.clone()) };
            Some(Action::Move { id: accounts[current].id.clone(), before })
        } else {
            self.selected = Some(accounts[next].id.clone()); None
        }
    }
    pub fn activate(&self, accounts: &[Account]) -> Option<Action> {
        if self.pending.is_some() { return None; }
        Some(Action::Activate(self.selected(accounts)?.id.clone()))
    }
    pub fn remove(&self, accounts: &[Account]) -> Option<Action> {
        Some(Action::Remove(self.selected(accounts)?.id.clone()))
    }
    pub fn restore_launch(&self, current: Option<&str>, launch: Option<&str>) -> Option<Action> {
        if self.pending.is_some() || launch.is_none() || current == launch { None } else { Some(Action::RestoreLaunch) }
    }
    pub fn start(&mut self, operation: u32) { self.pending = Some(operation); }
    pub fn finish(&mut self, operation: u32) {
        if self.pending == Some(operation) { self.pending = None; }
    }
    pub fn is_pending(&self) -> bool { self.pending.is_some() }
}

/// Interpret the ban messages recognized by AccountManager. `now_ms` is supplied by the host.
pub fn parse_ban(message: &str, now_ms: u64) -> Option<BanStatus> {
    let mut plain = String::new();
    let mut chars = message.chars();
    while let Some(c) = chars.next() {
        if c == '§' { chars.next(); } else { plain.push(c); }
    }
    let text = plain.split("\n\n").next().unwrap_or("").trim().to_lowercase();
    if text.contains("permanently banned from this server") || text.contains("your account has been blocked") {
        return Some(BanStatus::Permanent);
    }
    let start = ["temporarily banned for", "temporarily blocked for"].iter().find_map(|p| text.find(p).map(|i| i + p.len()))?;
    let rest = &text[start..];
    let duration = rest.split("from this server").next()?;
    if duration.len() == rest.len() { return None; }
    let b = duration.as_bytes();
    let mut i = 0;
    let mut total = 0u64;
    let mut found = false;
    while i < b.len() {
        if !b[i].is_ascii_digit() { i += 1; continue; }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() { i += 1; }
        if i == b.len() { break; }
        let multiplier = match b[i] { b'd' => 86_400_000, b'h' => 3_600_000, b'm' => 60_000, b's' => 1000, _ => continue };
        let n = duration[start..i].parse::<u64>().ok()?;
        total = total.checked_add(n.checked_mul(multiplier)?)?;
        found = true;
        i += 1;
    }
    found.then(|| now_ms.checked_add(total).map(BanStatus::Until)).flatten()
}

pub fn ban_label(ban: BanStatus, now_ms: u64) -> String {
    match ban {
        BanStatus::Permanent => "§4§l⚠".into(),
        BanStatus::Clear => "§2§l✔".into(),
        BanStatus::Until(t) if t <= now_ms => "§2§l✔".into(),
        BanStatus::Until(t) => {
            let seconds = (t - now_ms) / 1000;
            let mut parts = Vec::new();
            for (value, suffix) in [(seconds / 86400, "d"), (seconds / 3600 % 24, "h"), (seconds / 60 % 60, "m"), (seconds % 60, "s")] {
                if value != 0 { parts.push(format!("{value}{suffix}")); }
            }
            format!("{} §c§l⚠", parts.join(" "))
        }
    }
}

pub fn account_suffix(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Offline => "§7(Cracked)", AccountKind::Cookie => "§7(Cookie)",
        AccountKind::Refresh => "§7(Refresh)", AccountKind::Token => "§7(Token)",
        AccountKind::Premium | AccountKind::Microsoft => "§7(Premium)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str) -> Account { Account { id: id.into(), username: id.into(), uuid: None, kind: AccountKind::Offline, ban: BanStatus::Clear } }
    #[test]
    fn ban_parser_and_display_cover_permanent_temporary_and_clear() {
        assert_eq!(parse_ban("§r§cYou are permanently banned from this server!", 1000), Some(BanStatus::Permanent));
        assert_eq!(parse_ban("You are temporarily banned for §f1d 2h 3m 4s§c from this server!", 1000), Some(BanStatus::Until(93_785_000)));
        assert_eq!(parse_ban("ordinary disconnect", 1000), None);
        assert_eq!(ban_label(BanStatus::Until(93_785_000), 1000), "1d 2h 3m 4s §c§l⚠");
        assert_eq!(ban_label(BanStatus::Until(1000), 1000), "§2§l✔");
        assert_eq!(ban_label(BanStatus::Permanent, 1000), "§4§l⚠");
        assert_eq!(account_suffix(AccountKind::Offline), "§7(Cracked)");
    }
    #[test]
    fn selection_reorder_and_pending_actions() {
        let list = vec![row("a"), row("b"), row("c")];
        let mut state = ScreenState::default();
        assert_eq!(state.activate(&list), None);
        state.select(&list, 0);
        assert_eq!(state.activate(&list), Some(Action::Activate("a".into())));
        assert_eq!(state.navigate(&list, true, true), Some(Action::Move { id: "a".into(), before: Some("c".into()) }));
        state.navigate(&list, true, false);
        assert_eq!(state.selected(&list).unwrap().id, "b");
        state.start(7);
        assert_eq!(state.activate(&list), None);
        assert_eq!(state.restore_launch(Some("b"), Some("a")), None);
        state.finish(8); assert!(state.is_pending());
        state.finish(7); assert!(!state.is_pending());
        assert_eq!(state.restore_launch(Some("b"), Some("a")), Some(Action::RestoreLaunch));
        assert_eq!(state.restore_launch(Some("a"), Some("a")), None);
    }
}
