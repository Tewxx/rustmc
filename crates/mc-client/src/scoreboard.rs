//! The client `Scoreboard` (objectives, scores, display slots, teams) and the
//! `NetworkPlayerInfo` map, fed by S38/S3B-S3E, and the views `GuiIngame` and
//! `GuiPlayerTabOverlay` draw from them.
use mc_gui::hud::{fresh_layout_id, Sidebar, TabEntry, TabList};
use mc_protocol::Uuid;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub struct Objective {
    pub display_name: String,
    /// `EnumRenderType.HEARTS` (otherwise `INTEGER`).
    pub hearts: bool,
}

#[derive(Clone, Default)]
pub struct Team {
    pub display_name: String,
    pub prefix: String,
    pub suffix: String,
    pub friendly_flags: i8,
    /// `Team.EnumVisible`: always, never, hideForOtherTeams, hideForOwnTeam.
    pub name_tag_visibility: String,
    /// `EnumChatFormatting` color index, -1 for none.
    pub color: i8,
}

/// `NetworkPlayerInfo`
#[derive(Clone)]
pub struct PlayerInfo {
    pub name: String,
    pub game_mode: i32,
    pub ping: i32,
    pub display_name: Option<String>,
}

/// `playerInfoMap`. `net_handler` applies `PlayerListItem` in place through `DerefMut`, so a
/// mutable borrow bumps `revision`: that is what the tab-list cache invalidates on, without
/// re-hashing every entry each frame.
#[derive(Clone, Default)]
pub struct PlayerInfoMap {
    map: HashMap<Uuid, PlayerInfo>,
    revision: Cell<u64>,
}

impl Deref for PlayerInfoMap {
    type Target = HashMap<Uuid, PlayerInfo>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl DerefMut for PlayerInfoMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.revision.set(self.revision.get().wrapping_add(1));
        &mut self.map
    }
}

/// One built `sidebar()` value; `revision` and `me` are the inputs it was built from.
pub(crate) struct SidebarCache {
    revision: u64,
    me: String,
    value: Option<Sidebar>,
}

/// One built `tab_list()` value; the key is the data revision, the player-info map revision and
/// the header/footer (the only view inputs `net_handler` writes to `pub` fields directly).
pub(crate) struct TabCache {
    revision: u64,
    players: u64,
    header: Option<String>,
    footer: Option<String>,
    value: TabList,
}

#[derive(Default)]
pub struct Scoreboard {
    pub objectives: HashMap<String, Objective>,
    /// `entitiesScoreObjectives`: player name -> objective -> points.
    pub scores: HashMap<String, HashMap<String, i32>>,
    /// `objectiveDisplaySlots`: 0 list, 1 sidebar, 2 below name, 3-18 team-colored sidebars.
    pub display: [Option<String>; 19],
    pub teams: HashMap<String, Team>,
    /// `teamMemberships`: player name -> team name.
    pub membership: HashMap<String, String>,
    /// `playerInfoMap`
    pub players: PlayerInfoMap,
    pub header: Option<String>,
    pub footer: Option<String>,
    /// Bumped by every mutation here (`ScoreboardObjective`, `UpdateScore`,
    /// `DisplayScoreboard`, `Teams`); the view caches key on it.
    pub(crate) revision: u64,
    /// `GuiIngame.renderScoreboard`'s sorted, formatted lines, kept until the data changes.
    pub(crate) sidebar_cache: RefCell<Option<SidebarCache>>,
    /// `GuiPlayerTabOverlay.renderPlayerlist`'s sorted entries, likewise.
    pub(crate) tab_cache: RefCell<Option<TabCache>>,
}

impl Scoreboard {
    /// `handleScoreboardObjective`
    pub fn objective(&mut self, p: mc_protocol::packets::ScoreboardObjective) {
        let hearts = p.render_type == "hearts";
        match p.mode {
            0 => {
                // addScoreObjective throws on a duplicate name; the client then drops the packet.
                self.objectives.entry(p.name).or_insert(Objective { display_name: p.display_name, hearts });
            }
            1 => {
                // removeObjective: its display slots and every score on it go too.
                if self.objectives.remove(&p.name).is_some() {
                    for s in &mut self.display {
                        if s.as_deref() == Some(p.name.as_str()) {
                            *s = None;
                        }
                    }
                    for m in self.scores.values_mut() {
                        m.remove(&p.name);
                    }
                }
            }
            2 => {
                if let Some(o) = self.objectives.get_mut(&p.name) {
                    o.display_name = p.display_name;
                    o.hearts = hearts;
                }
            }
            _ => {}
        }
        self.bump();
    }

    /// Every in-place mutation here invalidates the sidebar/tab-list caches. The `players` map
    /// carries its own revision because `net_handler` mutates it directly.
    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// `handleUpdateScore`
    pub fn score(&mut self, p: mc_protocol::packets::UpdateScore) {
        use mc_protocol::packets::ScoreAction;
        match p.action {
            ScoreAction::Change => {
                if self.objectives.contains_key(&p.objective) {
                    self.scores.entry(p.name).or_default().insert(p.objective, p.value);
                }
            }
            ScoreAction::Remove => {
                if p.objective.is_empty() {
                    self.scores.remove(&p.name);
                } else if let Some(m) = self.scores.get_mut(&p.name) {
                    m.remove(&p.objective);
                    if m.is_empty() {
                        self.scores.remove(&p.name);
                    }
                }
            }
        }
        self.bump();
    }

    /// `handleDisplayScoreboard`
    pub fn display_slot(&mut self, slot: i8, name: String) {
        if let Some(s) = self.display.get_mut(slot as usize) {
            *s = (!name.is_empty() && self.objectives.contains_key(&name)).then_some(name);
        }
        self.bump();
    }

    /// `handleTeams`
    pub fn team(&mut self, p: mc_protocol::packets::Teams) {
        // Bumped up front: the unknown-team early return below mutates nothing but covers every
        // path that does.
        self.bump();
        if p.mode == 0 {
            self.teams.entry(p.name.clone()).or_default();
        }
        if !self.teams.contains_key(&p.name) {
            return;
        }
        if p.mode == 0 || p.mode == 2 {
            let t = self.teams.get_mut(&p.name).unwrap();
            t.display_name = p.display_name;
            t.prefix = p.prefix;
            t.suffix = p.suffix;
            t.color = p.color;
            t.friendly_flags = p.friendly_flags;
            if matches!(p.name_tag_visibility.as_str(), "always" | "never" | "hideForOtherTeams" | "hideForOwnTeam") {
                t.name_tag_visibility = p.name_tag_visibility;
            }
        }
        if p.mode == 0 || p.mode == 3 {
            for s in &p.players {
                self.membership.insert(s.clone(), p.name.clone());
            }
        }
        if p.mode == 4 {
            for s in &p.players {
                if self.membership.get(s) == Some(&p.name) {
                    self.membership.remove(s);
                }
            }
        }
        if p.mode == 1 {
            self.teams.remove(&p.name);
            self.membership.retain(|_, t| *t != p.name);
        }
    }

    pub fn team_of(&self, player: &str) -> Option<&Team> {
        self.membership.get(player).and_then(|t| self.teams.get(t))
    }

    /// `ScorePlayerTeam.formatPlayerName`
    pub fn format_name(&self, player: &str) -> String {
        match self.team_of(player) {
            Some(t) => format!("{}{player}{}", t.prefix, t.suffix),
            None => player.to_string(),
        }
    }

    /// `RendererLivingEntity.canRenderName` for another player: the team's name tag rule.
    pub fn name_tag_visible(&self, player: &str, me: &str) -> Option<bool> {
        let t = self.membership.get(player)?;
        let team = self.teams.get(t)?;
        let mine = self.membership.get(me);
        Some(match team.name_tag_visibility.as_str() {
            "never" => false,
            "hideForOtherTeams" => mine.is_none_or(|m| m == t),
            "hideForOwnTeam" => mine.is_none_or(|m| m != t),
            _ => true,
        })
    }

    fn points(&self, player: &str, objective: &str) -> i32 {
        self.scores.get(player).and_then(|m| m.get(objective)).copied().unwrap_or(0)
    }

    /// `GuiIngame.renderScoreboard`'s input: the team-colored sidebar for our team, else slot 1.
    /// The sorted, formatted lines are cached and only rebuilt when the data revision or `me`
    /// changes (`me` picks the team-colored display slot).
    pub fn sidebar(&self, me: &str) -> Option<Sidebar> {
        let cached = {
            let cache = self.sidebar_cache.borrow();
            cache
                .as_ref()
                .filter(|c| c.revision == self.revision && c.me == me)
                .map(|c| c.value.clone())
        };
        if let Some(value) = cached {
            return value;
        }
        let value = self.build_sidebar(me);
        *self.sidebar_cache.borrow_mut() = Some(SidebarCache { revision: self.revision, me: me.to_string(), value: value.clone() });
        value
    }

    /// The uncached `renderScoreboard` input.
    fn build_sidebar(&self, me: &str) -> Option<Sidebar> {
        let team_slot = self.team_of(me).and_then(|t| (0..16).contains(&t.color).then_some(3 + t.color as usize));
        let name = team_slot.and_then(|s| self.display[s].as_ref()).or(self.display[1].as_ref())?;
        let o = self.objectives.get(name)?;
        // getSortedScores: ascending by points, ties by name descending (case-insensitive).
        let mut list: Vec<(&String, i32)> = self.scores.iter().filter_map(|(p, m)| m.get(name).map(|v| (p, *v))).filter(|(p, _)| !p.starts_with('#')).collect();
        // The comparator lower-cased both names on every comparison; key the sort once instead.
        list.sort_by_cached_key(|(p, v)| (*v, std::cmp::Reverse(p.to_lowercase())));
        if list.len() > 15 {
            list.drain(..list.len() - 15);
        }
        Some(Sidebar {
            title: Arc::from(o.display_name.as_str()),
            lines: list.into_iter().map(|(p, v)| (Arc::from(self.format_name(p)), v)).collect(),
            layout: fresh_layout_id(),
        })
    }

    /// `GuiPlayerTabOverlay.renderPlayerlist`'s input, sorted by `PlayerComparator`. Cached until
    /// the data revision, the player-info map revision or the header/footer changes.
    pub fn tab_list(&self) -> TabList {
        let players = self.players.revision.get();
        let cached = {
            let cache = self.tab_cache.borrow();
            cache
                .as_ref()
                .filter(|c| c.revision == self.revision && c.players == players && c.header == self.header && c.footer == self.footer)
                .map(|c| c.value.clone())
        };
        if let Some(value) = cached {
            return value;
        }
        let value = self.build_tab_list();
        *self.tab_cache.borrow_mut() =
            Some(TabCache { revision: self.revision, players, header: self.header.clone(), footer: self.footer.clone(), value: value.clone() });
        value
    }

    /// The uncached `renderPlayerlist` input. The sort keys (team name and profile name) are
    /// taken once per player instead of cloning team names inside the comparator.
    fn build_tab_list(&self) -> TabList {
        let objective = self.display[0].as_ref().and_then(|n| self.objectives.get(n).map(|o| (n, o)));
        let mut list: Vec<(bool, &str, &str, &PlayerInfo)> = self
            .players
            .values()
            .map(|p| (p.game_mode == 3, self.membership.get(&p.name).map_or("", String::as_str), p.name.as_str(), p))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)).then_with(|| a.2.cmp(b.2)));
        let hearts = objective.is_some_and(|(_, o)| o.hearts);
        TabList {
            entries: list
                .into_iter()
                .map(|(spectator, _, _, p)| {
                    let name = p.display_name.clone().unwrap_or_else(|| self.format_name(&p.name));
                    let score = objective.map(|(n, _)| self.points(&p.name, n));
                    TabEntry {
                        // `renderPlayerlist` prepends `EnumChatFormatting.ITALIC` to spectators.
                        name: if spectator { Arc::from(format!("\u{a7}o{name}")) } else { Arc::from(name) },
                        ping: p.ping,
                        spectator,
                        score,
                        score_text: score.filter(|_| !hearts).map(|v| Arc::from(format!("\u{a7}e{v}"))),
                    }
                })
                .collect(),
            hearts,
            header: self.header.as_deref().map(Arc::from),
            footer: self.footer.as_deref().map(Arc::from),
            layout: fresh_layout_id(),
        }
    }

    /// `RenderPlayer.renderOffsetLivingLabel`: "points objective" under the name (slot 2).
    pub fn below_name(&self, player: &str) -> Option<String> {
        let name = self.display[2].as_ref()?;
        let o = self.objectives.get(name)?;
        Some(format!("{} {}", self.points(player, name), o.display_name))
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_protocol::packets::{ScoreAction, ScoreboardObjective, Teams, UpdateScore};

    fn objective(name: &str, display: &str, render_type: &str) -> ScoreboardObjective {
        ScoreboardObjective { name: name.into(), mode: 0, display_name: display.into(), render_type: render_type.into() }
    }

    fn score(name: &str, value: i32) -> UpdateScore {
        UpdateScore { name: name.into(), action: ScoreAction::Change, objective: "obj".into(), value }
    }

    fn player(name: &str, ping: i32, game_mode: i32, display_name: Option<&str>) -> PlayerInfo {
        PlayerInfo { name: name.into(), game_mode, ping, display_name: display_name.map(Into::into) }
    }

    fn with_sidebar() -> Scoreboard {
        let mut sb = Scoreboard::default();
        sb.objective(objective("obj", "Kills", "integer"));
        sb.display_slot(1, "obj".into());
        sb
    }

    #[test]
    fn sidebar_cache_rebuilds_only_when_the_scoreboard_changes() {
        let mut sb = with_sidebar();
        sb.score(score("b", 1));
        sb.score(score("a", 1));
        let first = sb.sidebar("me").unwrap();
        // getSortedScores: points ascending, ties by name descending.
        assert_eq!(first.lines.iter().map(|(n, _)| &**n).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(first.layout, sb.sidebar("me").unwrap().layout, "an unchanged sidebar must be reused");
        // `UpdateScore` bumps the revision.
        sb.score(score("a", 2));
        let second = sb.sidebar("me").unwrap();
        assert_ne!(first.layout, second.layout);
        assert_eq!(second.lines.last().map(|(n, v)| (&**n, *v)), Some(("a", 2)));
        // `Teams` bumps it too, and the prefix is part of the formatted line.
        sb.team(Teams { name: "red".into(), mode: 0, prefix: "[R] ".into(), ..Default::default() });
        sb.team(Teams { name: "red".into(), mode: 3, players: vec!["a".into()], ..Default::default() });
        let third = sb.sidebar("me").unwrap();
        assert_ne!(second.layout, third.layout);
        assert!(third.lines.iter().any(|(n, _)| &**n == "[R] a"));
        // `DisplayScoreboard` bumps it as well.
        sb.display_slot(1, String::new());
        assert!(sb.sidebar("me").is_none());
    }

    #[test]
    fn sidebar_cache_keys_on_the_team_slot() {
        let mut sb = with_sidebar();
        sb.objective(objective("team", "Deaths", "integer"));
        sb.team(Teams { name: "red".into(), mode: 0, color: 1, ..Default::default() });
        sb.team(Teams { name: "red".into(), mode: 3, players: vec!["me".into()], ..Default::default() });
        // Slot 3 + color(1) is the team-colored sidebar.
        sb.display_slot(4, "team".into());
        let mine = sb.sidebar("me").unwrap();
        assert_eq!(mine.layout, sb.sidebar("me").unwrap().layout);
        let other = sb.sidebar("other").unwrap();
        assert_eq!(&*mine.title, "Deaths");
        assert_eq!(&*other.title, "Kills");
        assert_ne!(mine.layout, other.layout, "a different team slot is a different view");
    }

    #[test]
    fn tab_list_cache_tracks_players_header_and_objective() {
        let mut sb = Scoreboard::default();
        sb.objective(objective("obj", "Kills", "integer"));
        sb.display_slot(0, "obj".into());
        sb.players.insert(Uuid(1, 1), player("a", 20, 0, None));
        sb.players.insert(Uuid(2, 2), player("b", 900, 0, None));
        sb.players.insert(Uuid(3, 3), player("c", 20, 3, None));
        sb.score(score("a", 7));
        let first = sb.tab_list();
        // PlayerComparator: non-spectators first, then team name, then profile name.
        let names: Vec<&str> = first.entries.iter().map(|e| &*e.name).collect();
        assert_eq!(names, ["a", "b", "\u{a7}oc"]);
        assert_eq!(first.entries[0].score, Some(7));
        assert_eq!(first.entries[0].score_text.as_deref(), Some("\u{a7}e7"));
        assert_eq!(first.layout, sb.tab_list().layout, "an unchanged tab list must be reused");
        // `PlayerListItem` updates mutate the map through `DerefMut`.
        sb.players.get_mut(&Uuid(1, 1)).unwrap().ping = 999;
        let second = sb.tab_list();
        assert_ne!(first.layout, second.layout);
        assert_eq!(second.entries[0].ping, 999);
        // `PlayerListHeaderFooter` assigns the header/footer fields directly.
        sb.header = Some("hi".into());
        let third = sb.tab_list();
        assert_ne!(second.layout, third.layout);
        assert_eq!(third.header.as_deref(), Some("hi"));
        // The list objective is part of the view.
        sb.display_slot(0, String::new());
        let fourth = sb.tab_list();
        assert_ne!(third.layout, fourth.layout);
        assert_eq!(fourth.entries[0].score, None);
        assert_eq!(fourth.entries[0].score_text, None);
    }

    #[test]
    fn tab_list_formats_names_teams_and_hearts() {
        let mut sb = Scoreboard::default();
        sb.objective(objective("obj", "Health", "hearts"));
        sb.display_slot(0, "obj".into());
        sb.team(Teams { name: "red".into(), mode: 0, prefix: "<".into(), suffix: ">".into(), ..Default::default() });
        sb.team(Teams { name: "red".into(), mode: 3, players: vec!["a".into()], ..Default::default() });
        sb.players.insert(Uuid(1, 1), player("a", 20, 0, None));
        sb.players.insert(Uuid(2, 2), player("b", 20, 0, Some("\u{a7}6B")));
        let list = sb.tab_list();
        // "b" has no team (`""` sorts before "red"), and its display name wins over the profile.
        let names: Vec<&str> = list.entries.iter().map(|e| &*e.name).collect();
        assert_eq!(names, ["\u{a7}6B", "<a>"]);
        assert!(list.hearts);
        // Hearts are drawn from the number, not from a prebuilt string.
        assert!(list.entries.iter().all(|e| e.score_text.is_none()));
    }
}
