//! The client `Scoreboard` (objectives, scores, display slots, teams) and the
//! `NetworkPlayerInfo` map, fed by S38/S3B-S3E, and the views `GuiIngame` and
//! `GuiPlayerTabOverlay` draw from them.
use mc_gui::hud::{Sidebar, TabEntry, TabList};
use mc_protocol::Uuid;
use std::collections::HashMap;

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
    pub players: HashMap<Uuid, PlayerInfo>,
    pub header: Option<String>,
    pub footer: Option<String>,
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
    }

    /// `handleDisplayScoreboard`
    pub fn display_slot(&mut self, slot: i8, name: String) {
        if let Some(s) = self.display.get_mut(slot as usize) {
            *s = (!name.is_empty() && self.objectives.contains_key(&name)).then_some(name);
        }
    }

    /// `handleTeams`
    pub fn team(&mut self, p: mc_protocol::packets::Teams) {
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
    pub fn sidebar(&self, me: &str) -> Option<Sidebar> {
        let team_slot = self.team_of(me).and_then(|t| (0..16).contains(&t.color).then_some(3 + t.color as usize));
        let name = team_slot.and_then(|s| self.display[s].as_ref()).or(self.display[1].as_ref())?;
        let o = self.objectives.get(name)?;
        // getSortedScores: ascending by points, ties by name descending (case-insensitive).
        let mut list: Vec<(&String, i32)> = self.scores.iter().filter_map(|(p, m)| m.get(name).map(|v| (p, *v))).filter(|(p, _)| !p.starts_with('#')).collect();
        list.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.to_lowercase().cmp(&a.0.to_lowercase())));
        if list.len() > 15 {
            list.drain(..list.len() - 15);
        }
        Some(Sidebar { title: o.display_name.clone(), lines: list.into_iter().map(|(p, v)| (self.format_name(p), v)).collect() })
    }

    /// `GuiPlayerTabOverlay.renderPlayerlist`'s input, sorted by `PlayerComparator`.
    pub fn tab_list(&self) -> TabList {
        let objective = self.display[0].as_ref().and_then(|n| self.objectives.get(n).map(|o| (n, o)));
        let mut list: Vec<&PlayerInfo> = self.players.values().collect();
        let team_name = |p: &PlayerInfo| self.membership.get(&p.name).cloned().unwrap_or_default();
        list.sort_by(|a, b| {
            (a.game_mode == 3).cmp(&(b.game_mode == 3)).then_with(|| team_name(a).cmp(&team_name(b))).then_with(|| a.name.cmp(&b.name))
        });
        TabList {
            entries: list
                .into_iter()
                .map(|p| TabEntry {
                    name: p.display_name.clone().unwrap_or_else(|| self.format_name(&p.name)),
                    ping: p.ping,
                    spectator: p.game_mode == 3,
                    score: objective.map(|(n, _)| self.points(&p.name, n)),
                })
                .collect(),
            hearts: objective.is_some_and(|(_, o)| o.hearts),
            header: self.header.clone(),
            footer: self.footer.clone(),
        }
    }

    /// `RenderPlayer.renderOffsetLivingLabel`: "points objective" under the name (slot 2).
    pub fn below_name(&self, player: &str) -> Option<String> {
        let name = self.display[2].as_ref()?;
        let o = self.objectives.get(name)?;
        Some(format!("{} {}", self.points(player, name), o.display_name))
    }

}
