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
    pub fn select_id(&mut self, id: Option<String>) { self.selected = id; }
    pub fn pending(&self) -> Option<u32> { self.pending }
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

/// The component requires the reserved interactive and host-owned account interfaces.
/// Current hosts cannot instantiate it until those capabilities are implemented and granted.
#[cfg(target_arch = "wasm32")]
mod guest {
    wit_bindgen::generate!({ path: "wit", world: "interactive-mod" });
    use rustmc::plugin::{accounts, screens, types};
    use screens::{Widget, WidgetKind, Value};
    use std::cell::RefCell;

    const SCREEN: &str = "accountmanager";
    struct State {
        screen: super::ScreenState,
        rows: Vec<super::Account>,
        notice: String,
        add_menu: bool,
        active: bool,
        scroll: usize,
        visible: usize,
    }
    impl Default for State {
        fn default() -> Self { Self { screen: Default::default(), rows: vec![], notice: String::new(), add_menu: false, active: false, scroll: 0, visible: 0 } }
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
    struct AccountManager;

    fn rows() -> Vec<super::Account> {
        accounts::list().into_iter().map(|a| super::Account {
            id: a.id, username: a.username, uuid: a.uuid,
            kind: match a.kind {
                accounts::AccountKind::Offline => super::AccountKind::Offline,
                accounts::AccountKind::Microsoft => super::AccountKind::Microsoft,
                accounts::AccountKind::LegacyPremium => super::AccountKind::Premium,
                accounts::AccountKind::AccessToken => super::AccountKind::Token,
                accounts::AccountKind::RefreshToken => super::AccountKind::Refresh,
                accounts::AccountKind::Cookie => super::AccountKind::Cookie,
            },
            ban: match a.ban { accounts::BanStatus::None => super::BanStatus::Clear, accounts::BanStatus::Temporary(t) => super::BanStatus::Until(t), accounts::BanStatus::Permanent => super::BanStatus::Permanent },
        }).collect()
    }
    fn button(id: u32, x: i32, y: i32, w: i32, label: String, enabled: bool) -> Widget {
        Widget { id, x, y, w, h: 20, kind: WidgetKind::Button(enabled), label }
    }
    fn refresh() { screens::refresh(SCREEN); }
    fn record_operation(s: &mut State, result: Result<u32, String>) {
        match result { Ok(id) => { s.screen.start(id); s.notice = "Working...".into(); }, Err(e) => s.notice = e }
    }
    impl exports::rustmc::plugin::lifecycle::Guest for AccountManager {
        fn on_load(_api: String, _features: Vec<String>) -> Result<(), String> {
            screens::register(SCREEN, "Lumiere Account Manager");
            screens::add_entry(screens::ParentScreen::Title, SCREEN, "Accounts");
            screens::add_entry(screens::ParentScreen::Multiplayer, SCREEN, "Accounts");
            Ok(())
        }
        fn on_unload() {}
        fn on_tick() {
            let changed = STATE.with(|state| {
                let mut changed = false;
                let mut s = state.borrow_mut();
                if let Some(id) = s.screen.pending() {
                    match accounts::operation(id) {
                        Some(accounts::OperationState::Pending) => {},
                        Some(accounts::OperationState::Complete) => { s.screen.finish(id); s.notice = "Account updated".into(); changed = s.active; },
                        Some(accounts::OperationState::Failed(error)) => { s.screen.finish(id); s.notice = error; changed = s.active; },
                        None => {},
                    }
                }
                changed
            });
            if changed { refresh(); }
        }
        fn on_frame(_ctx: types::FrameCtx) {}
    }
    impl exports::rustmc::plugin::render::Guest for AccountManager {
        fn render_hud(_ctx: types::FrameCtx) {}
    }
    impl exports::rustmc::plugin::events::Guest for AccountManager {
        fn on_key(event: exports::rustmc::plugin::events::KeyEvent) -> bool {
            if !event.down { return false; }
            let (active, action, close) = STATE.with(|state| {
                let mut s = state.borrow_mut();
                if !s.active { return (false, None, false); }
                if s.add_menu {
                    if event.code == 1 { s.add_menu = false; return (true, None, false); }
                    return (false, None, false);
                }
                match event.code {
                    1 => (true, None, true),
                    28 => (true, s.screen.activate(&s.rows), false),
                    211 => (true, s.screen.remove(&s.rows), false),
                    200 | 208 => {
                        let rows = s.rows.clone();
                        let action = s.screen.navigate(&rows, event.code == 208, event.modifiers & 1 != 0);
                        if let Some(index) = rows.iter().position(|a| s.screen.selected(&rows).is_some_and(|selected| selected.id == a.id)) {
                            if index < s.scroll { s.scroll = index; }
                            if s.visible > 0 && index >= s.scroll + s.visible { s.scroll = index + 1 - s.visible; }
                        }
                        (true, action, false)
                    }
                    _ => (false, None, false),
                }
            });
            if close { screens::close(); return true; }
            match action {
                Some(super::Action::Move { id, before }) => {
                    if let Err(error) = accounts::move_before(&id, before.as_deref()) { STATE.with(|s| s.borrow_mut().notice = error); }
                }
                Some(super::Action::Activate(id)) => {
                    let result = accounts::activate(&id);
                    STATE.with(|s| record_operation(&mut s.borrow_mut(), result));
                }
                Some(super::Action::Remove(id)) => {
                    let result = accounts::remove(&id);
                    STATE.with(|s| s.borrow_mut().notice = match result { Ok(()) => "Account deleted".into(), Err(e) => e });
                }
                _ => {},
            }
            if active { refresh(); }
            active
        }
        fn on_mouse(event: exports::rustmc::plugin::events::MouseEvent) -> bool {
            if event.wheel == 0 { return false; }
            let handled = STATE.with(|state| {
                let mut s = state.borrow_mut();
                if !s.active || s.add_menu { return false; }
                if event.wheel > 0 { s.scroll = s.scroll.saturating_sub(1); }
                else { s.scroll = (s.scroll + 1).min(s.rows.len().saturating_sub(s.visible)); }
                true
            });
            if handled { refresh(); }
            handled
        }
        fn on_screen_open(_kind: exports::rustmc::plugin::events::ScreenKind, _title: String) {}
        fn on_screen_close(_kind: exports::rustmc::plugin::events::ScreenKind) {}
    }
    impl exports::rustmc::plugin::screen_handlers::Guest for AccountManager {
        fn init(id: String, w: i32, h: i32) -> Vec<Widget> {
            if id != SCREEN { return vec![]; }
            STATE.with(|state| {
                let mut s = state.borrow_mut();
                s.active = true;
                s.rows = rows();
                if s.add_menu {
                    return vec![button(5, w/2-100, h/2-75, 200, "Offline account".into(), true),
                                button(6, w/2-100, h/2-50, 200, "Microsoft account".into(), true),
                                button(8, w/2-100, h/2-25, 200, "Access token".into(), true),
                                button(9, w/2-100, h/2, 200, "Refresh token".into(), true),
                                button(10, w/2-100, h/2+25, 200, "Cookie import".into(), true),
                                button(7, w/2-100, h/2+50, 200, "Back".into(), true)];
                }
                let current = accounts::current();
                let launch = accounts::launch();
                let selected = s.screen.selected(&s.rows).is_some();
                let mut widgets = Vec::new();
                let available = (((h - 100).max(0) / 22) as usize).min(1024);
                s.visible = available;
                s.scroll = s.scroll.min(s.rows.len().saturating_sub(available));
                for (slot, (i, row)) in s.rows.iter().enumerate().skip(s.scroll).take(available).enumerate() {
                    let active = if current.as_deref() == Some(row.id.as_str()) { "§a§l" } else { "§r" };
                    let ban = match row.ban {
                        super::BanStatus::Clear => "§2§l✔",
                        super::BanStatus::Permanent => "§4§l⚠",
                        super::BanStatus::Until(_) => "§c§l⚠",
                    };
                    let timed = match row.ban {
                        super::BanStatus::Until(_) => accounts::ban_remaining_ms(&row.id).map(|remaining| super::ban_label(super::BanStatus::Until(remaining), 0)),
                        _ => None,
                    };
                    let label = format!("{active}{} {} {}", row.username, super::account_suffix(row.kind), timed.as_deref().unwrap_or(ban));
                    widgets.push(button(100+i as u32, w/2-150, 32+slot as i32*22, 300, label, true));
                }
                widgets.push(button(0, w/2-154, h-52, 150, "Login".into(), selected && !s.screen.is_pending()));
                widgets.push(button(1, w/2+4, h-52, 150, "Add".into(), true));
                widgets.push(button(2, w/2-154, h-28, 150, "Delete".into(), selected));
                widgets.push(button(3, w/2+4, h-28, 150, "Cancel".into(), true));
                if launch.is_some() { widgets.push(button(4, w-156, 6, 150, "Restore launch account".into(), s.screen.restore_launch(current.as_deref(), launch.as_deref()).is_some())); }
                widgets
            })
        }
        fn on_widget(id: String, widget: u32, value: Value) {
            if id != SCREEN || !matches!(value, Value::Click) { return; }
            let close = STATE.with(|state| {
                let mut s = state.borrow_mut();
                match widget {
                    0 => if let Some(super::Action::Activate(id)) = s.screen.activate(&s.rows) {
                        let result = accounts::activate(&id); record_operation(&mut s, result);
                    },
                    1 => s.add_menu = true,
                    2 => if let Some(super::Action::Remove(id)) = s.screen.remove(&s.rows) {
                        s.notice = match accounts::remove(&id) { Ok(()) => "Account deleted".into(), Err(e) => e };
                    },
                    3 => return true,
                    4 => { let result = accounts::restore_launch(); record_operation(&mut s, result); },
                    5 => { let result = accounts::initiate_add(accounts::AccountKind::Offline); s.add_menu = false; record_operation(&mut s, result); },
                    6 => { let result = accounts::initiate_add(accounts::AccountKind::Microsoft); s.add_menu = false; record_operation(&mut s, result); },
                    7 => s.add_menu = false,
                    8 => { let result = accounts::initiate_add(accounts::AccountKind::AccessToken); s.add_menu = false; record_operation(&mut s, result); },
                    9 => { let result = accounts::initiate_add(accounts::AccountKind::RefreshToken); s.add_menu = false; record_operation(&mut s, result); },
                    10 => { let result = accounts::initiate_add(accounts::AccountKind::Cookie); s.add_menu = false; record_operation(&mut s, result); },
                    n if n >= 100 => {
                        let index = (n-100) as usize;
                        let selected = s.rows.get(index).map(|r| r.id.clone());
                        s.screen.select_id(selected);
                    },
                    _ => return false,
                }
                false
            });
            if close { screens::close(); } else { refresh(); }
        }
        fn draw(id: String, ctx: types::FrameCtx) {
            if id != SCREEN { return; }
            STATE.with(|state| {
                let s = state.borrow();
                let mut out = b"RMD1".to_vec();
                for (x, y, text) in [(3i32, 3i32, format!("Accounts: {}", s.rows.len())), (3, ctx.gui_height as i32-76, s.notice.clone())] {
                    if text.is_empty() { continue; }
                    out.push(3); out.extend(x.to_le_bytes()); out.extend(y.to_le_bytes());
                    out.extend(0xffff_ffffu32.to_le_bytes()); out.push(1);
                    let mut len = text.len().min(u16::MAX as usize);
                    while !text.is_char_boundary(len) { len -= 1; }
                    out.extend((len as u16).to_le_bytes()); out.extend(&text.as_bytes()[..len]);
                }
                rustmc::plugin::ui_draw::submit(&out);
            });
        }
        fn on_close(id: String) { if id == SCREEN { STATE.with(|s| { let mut s = s.borrow_mut(); s.active = false; s.add_menu = false; }); } }
    }
    export!(AccountManager);
}
