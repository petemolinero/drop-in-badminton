//! Numbers for the Players and Analytics views, computed from the whole
//! database (`Store::everything`). Pure functions, so they're easy to test.

use std::collections::HashMap;

use crate::model::{PlayerId, Skill};
use crate::store::RawHistory;

/// A game left running overnight still only lasted about a round.
const LONGEST_GAME_MS: i64 = 60 * 60_000;

/// One player's lifetime numbers.
#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub name: String,
    pub skill: Option<Skill>,
    pub found_via: String,
    pub from: String,
    pub auto_check_in: bool,
    /// Sessions they checked in to.
    pub sessions: u32,
    pub games: u32,
    /// Total time on court.
    pub court_ms: i64,
    /// Start of their first and latest session.
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// Who they've teamed up with most, and how often.
    pub partner: Option<(PlayerId, u32)>,
}

impl PlayerStats {
    pub fn games_per_session(&self) -> f32 {
        if self.sessions == 0 { 0.0 } else { self.games as f32 / self.sessions as f32 }
    }
}

/// One session's turnout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turnout {
    pub started_ms: i64,
    /// Players who'd been to an earlier session.
    pub returning: u32,
    /// Players at their first session.
    pub new: u32,
    pub games: u32,
}

impl Turnout {
    pub fn players(&self) -> u32 {
        self.returning + self.new
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub players: HashMap<PlayerId, PlayerStats>,
    /// Every session with anyone at it, oldest first.
    pub turnout: Vec<Turnout>,
    pub games: u32,
    pub court_ms: i64,
}

impl Stats {
    pub fn compute(h: &RawHistory) -> Stats {
        let mut players: HashMap<PlayerId, PlayerStats> = h
            .players
            .iter()
            .map(|p| {
                let stats = PlayerStats {
                    name: p.name.clone(),
                    skill: Some(p.skill),
                    found_via: p.found_via.clone(),
                    from: p.from.clone(),
                    auto_check_in: p.auto_check_in,
                    ..Default::default()
                };
                (p.id, stats)
            })
            .collect();
        let day_start: HashMap<i64, i64> = h.days.iter().copied().collect();

        // Attendance: one visit per player per session, however many check-ins.
        let mut visits: Vec<(i64, i64, PlayerId)> = h
            .attendance
            .iter()
            .map(|&(day, player, in_ms)| (day_start.get(&day).copied().unwrap_or(in_ms), day, player))
            .collect();
        visits.sort();
        visits.dedup_by_key(|v| (v.1, v.2));
        let mut seen: std::collections::HashSet<PlayerId> = Default::default();
        let mut by_day: Vec<(i64, Turnout)> = Vec::new();
        for &(start, day, player) in &visits {
            if by_day.last().is_none_or(|(d, _)| *d != day) {
                by_day.push((day, Turnout { started_ms: start, ..Default::default() }));
            }
            let t = &mut by_day.last_mut().expect("pushed above").1;
            if seen.insert(player) { t.new += 1 } else { t.returning += 1 }
            let s = players.entry(player).or_default();
            s.sessions += 1;
            s.first_ms = Some(s.first_ms.map_or(start, |f| f.min(start)));
            s.last_ms = Some(s.last_ms.map_or(start, |l| l.max(start)));
        }

        // Games: counts, court time, and who played on the same side.
        let mut games_by_day: HashMap<i64, u32> = HashMap::new();
        let mut together: HashMap<(PlayerId, PlayerId), u32> = HashMap::new();
        let mut stats = Stats::default();
        for g in &h.games {
            let length = (g.ended_ms - g.started_ms).clamp(0, LONGEST_GAME_MS);
            *games_by_day.entry(g.day).or_default() += 1;
            stats.games += 1;
            stats.court_ms += length;
            for &(p, team) in &g.players {
                let s = players.entry(p).or_default();
                s.games += 1;
                s.court_ms += length;
                for &(q, other) in &g.players {
                    if q != p && other == team {
                        *together.entry((p, q)).or_default() += 1;
                    }
                }
            }
        }
        for ((p, q), n) in together {
            let s = players.entry(p).or_default();
            // Most games together wins; ties go to the lower id so it's stable.
            if s.partner.is_none_or(|(best, m)| n > m || (n == m && q < best)) {
                s.partner = Some((q, n));
            }
        }
        // Games from before history was kept still count.
        for p in &h.players {
            if let Some(s) = players.get_mut(&p.id) {
                s.games = s.games.max(p.lifetime_games);
            }
        }
        stats.turnout = by_day
            .into_iter()
            .map(|(day, mut t)| {
                t.games = games_by_day.get(&day).copied().unwrap_or(0);
                t
            })
            .collect();
        stats.players = players;
        stats
    }

    /// Everyone who has come to at least one session.
    pub fn attendees(&self) -> usize {
        self.players.values().filter(|p| p.sessions > 0).count()
    }

    /// Share of attendees who came back for a second session or more.
    pub fn return_rate(&self) -> Option<f32> {
        let came = self.attendees();
        let back = self.players.values().filter(|p| p.sessions >= 2).count();
        (came > 0).then(|| back as f32 / came as f32)
    }

    pub fn average_turnout(&self) -> Option<f32> {
        let n = self.turnout.len();
        (n > 0).then(|| self.turnout.iter().map(|t| t.players()).sum::<u32>() as f32 / n as f32)
    }

    pub fn busiest(&self) -> Option<Turnout> {
        self.turnout.iter().copied().max_by_key(|t| (t.players(), t.started_ms))
    }

    /// The `n` players with the most games, most first.
    pub fn most_games(&self, n: usize) -> Vec<(PlayerId, &PlayerStats)> {
        let mut all: Vec<_> = self.players.iter().filter(|(_, s)| s.games > 0).map(|(id, s)| (*id, s)).collect();
        all.sort_by(|a, b| b.1.games.cmp(&a.1.games).then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase())));
        all.truncate(n);
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{RawGame, RawPlayer};

    fn player(id: PlayerId, name: &str) -> RawPlayer {
        RawPlayer { id, name: name.into(), skill: Skill::Beginner2, lifetime_games: 0, found_via: String::new(), from: String::new(), auto_check_in: false }
    }

    #[test]
    fn turnout_partners_and_court_time() {
        let h = RawHistory {
            players: vec![player(1, "Ann"), player(2, "Bo"), player(3, "Cy"), player(4, "Di")],
            days: vec![(10, 1_000), (11, 2_000)],
            attendance: vec![(10, 1, 1_000), (10, 2, 1_100), (10, 2, 1_500), (11, 1, 2_000), (11, 3, 2_100)],
            games: vec![
                RawGame { day: 10, started_ms: 0, ended_ms: 15 * 60_000, players: vec![(1, 0), (2, 0), (3, 1), (4, 1)] },
                // Left running overnight: counts as an hour, not twelve.
                RawGame { day: 11, started_ms: 0, ended_ms: 12 * 3_600_000, players: vec![(1, 0), (3, 0)] },
            ],
        };
        let s = Stats::compute(&h);
        assert_eq!(s.turnout, vec![
            Turnout { started_ms: 1_000, returning: 0, new: 2, games: 1 },
            Turnout { started_ms: 2_000, returning: 1, new: 1, games: 1 },
        ]);
        let ann = &s.players[&1];
        assert_eq!((ann.sessions, ann.games, ann.court_ms), (2, 2, 75 * 60_000));
        assert_eq!((ann.first_ms, ann.last_ms), (Some(1_000), Some(2_000)));
        // Ann teamed with Bo once and Cy once: the tie goes to the lower id.
        assert_eq!(ann.partner, Some((2, 1)));
        assert_eq!(s.players[&2].sessions, 1, "two check-ins, one session");
        assert_eq!(s.attendees(), 3);
        assert_eq!(s.return_rate(), Some(1.0 / 3.0));
        assert_eq!(s.most_games(1)[0].0, 1);
    }
}
