//! Core data model: players, settings, and the state of tonight's session.
//!
//! Every session timestamp (check-ins, games, the round timer) is in *session
//! time*: Unix milliseconds on the session's own clock ([`SessionClock`]),
//! which starts at whatever date and time the organiser chose and only moves
//! while the session is running. Pause it and every wait, game and timer
//! stops with it. Only `last_activity_ms` is real wall-clock time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub type PlayerId = u64;

/// The four slots of a doubles court. Slots 0–1 are one team, 2–3 the other.
pub type Slots = [Option<PlayerId>; 4];

pub const EMPTY_SLOTS: Slots = [None; 4];

/// Real (wall-clock) time, Unix ms. Session code wants [`AppState::now`].
pub fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Skill {
    // Saved under the names of the levels they replaced (v0.3 and earlier had
    // Beginner, Intermediate, Advanced, Expert), so old files still read right.
    #[serde(rename = "Beginner")]
    Beginner1,
    #[serde(rename = "Intermediate")]
    Beginner2,
    #[serde(rename = "Advanced")]
    Intermediate,
    #[serde(rename = "Expert")]
    Advanced,
}

impl Skill {
    pub const ALL: [Skill; 4] = [
        Skill::Beginner1,
        Skill::Beginner2,
        Skill::Intermediate,
        Skill::Advanced,
    ];

    pub fn rank(self) -> i32 {
        self as i32
    }

    /// Short tag shown on cards and badges.
    pub fn short(self) -> &'static str {
        match self {
            Skill::Beginner1 => "B1",
            Skill::Beginner2 => "B2",
            Skill::Intermediate => "INT",
            Skill::Advanced => "ADV",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Skill::Beginner1 => "Beginner 1",
            Skill::Beginner2 => "Beginner 2",
            Skill::Intermediate => "Intermediate",
            Skill::Advanced => "Advanced",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub skill: Skill,
    /// Games played across every night, for the roster's curiosity column.
    #[serde(default)]
    pub lifetime_games: u32,
    /// How they heard about the club (a friend, Instagram, …).
    #[serde(default)]
    pub found_via: String,
    /// Where they're from: a neighbourhood or town.
    #[serde(default)]
    pub from: String,
    /// Checked in automatically whenever a new session starts (regulars).
    #[serde(default)]
    pub auto_check_in: bool,
    /// Players who always play together (a parent and kids, a couple) share
    /// a link: whenever they're free at the same time they go on one court.
    #[serde(default)]
    pub link: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub courts: usize,
    pub round_minutes: u32,
    pub auto_start_timer: bool,
    pub dark_mode: bool,
    pub ui_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            courts: 3,
            round_minutes: 15,
            auto_start_timer: false,
            dark_mode: false,
            ui_scale: 1.0,
        }
    }
}

impl Settings {
    pub const MAX_COURTS: usize = 12;

    pub fn round_ms(&self) -> i64 {
        self.round_minutes.max(1) as i64 * 60_000
    }
}

/// A player's attendance record for tonight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Presence {
    /// When they (most recently) checked in.
    pub checked_in_ms: i64,
    /// When they last came off a court tonight.
    pub last_played_ms: Option<i64>,
    pub games_tonight: u32,
    /// Resting players stay checked in but are never auto-placed.
    #[serde(default)]
    pub resting: bool,
}

impl Presence {
    pub fn new(now: i64) -> Self {
        Self {
            checked_in_ms: now,
            last_played_ms: None,
            games_tonight: 0,
            resting: false,
        }
    }

    /// How long they've been off court: since the later of arriving or last playing.
    pub fn waiting_ms(&self, now: i64) -> i64 {
        let since = self
            .last_played_ms
            .map_or(self.checked_in_ms, |p| p.max(self.checked_in_ms));
        (now - since).max(0)
    }
}

/// One check-in → check-out span on a game day.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attendance {
    pub player: PlayerId,
    pub in_ms: i64,
    pub out_ms: Option<i64>,
}

/// A finished game: who played on which court, and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    pub round: u32,
    pub court: usize,
    pub started_ms: i64,
    pub ended_ms: i64,
    /// Slots 0–1 are one team, 2–3 the other.
    pub players: Slots,
}

/// The session's own clock, which every time in the session is read from.
/// It starts at a chosen date and time and counts up only while running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionClock {
    /// When the session began (any date and time; nothing checks it), Unix ms.
    pub start_ms: i64,
    /// Time banked on the clock before the current run.
    pub elapsed_ms: i64,
    /// Wall-clock time the current run began; `None` while paused.
    pub running_since: Option<i64>,
}

impl SessionClock {
    /// A clock reading `start_ms`, running from wall time `wall`.
    pub fn starting(start_ms: i64, wall: i64) -> Self {
        Self { start_ms, elapsed_ms: 0, running_since: Some(wall) }
    }

    /// Set up (sessions saved before the clock existed have none).
    pub fn is_set(&self) -> bool {
        self.start_ms != 0
    }

    pub fn running(&self) -> bool {
        self.running_since.is_some()
    }

    /// How long the session has run, as of wall time `wall`.
    pub fn elapsed(&self, wall: i64) -> i64 {
        self.elapsed_ms + self.running_since.map_or(0, |since| (wall - since).max(0))
    }

    /// Session time at wall time `wall`.
    pub fn now(&self, wall: i64) -> i64 {
        self.start_ms + self.elapsed(wall)
    }

    /// Stop the clock as it read at wall time `wall`.
    pub fn pause(&mut self, wall: i64) {
        self.elapsed_ms = self.elapsed(wall);
        self.running_since = None;
    }

    pub fn resume(&mut self, wall: i64) {
        if self.running_since.is_none() {
            self.running_since = Some(wall);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Timer {
    Idle,
    Running { end_ms: i64 },
    Paused { remaining_ms: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub present: BTreeMap<PlayerId, Presence>,
    pub courts: Vec<Slots>,
    /// When each court's current game began.
    pub court_started_ms: Vec<Option<i64>>,
    pub deck: Vec<Slots>,
    /// Players a person pulled off deck by hand: they won't be auto-placed again
    /// until the next rotation, so the slot doesn't bounce straight back to them.
    pub skip_round: BTreeSet<PlayerId>,
    /// When the timer last ran out (its end time), if the games on court then
    /// were ended there and then: logged, and their players' waits restarted.
    /// Games that began by then are over, even though their players are still
    /// on court until Next round.
    pub round_closed_ms: Option<i64>,
    /// Tonight's practice group (e.g. players preparing for a tournament):
    /// they only play each other, but share the courts fairly with everyone.
    pub practice: BTreeSet<PlayerId>,
    pub timer: Timer,
    pub round_number: u32,
    /// Wall-clock time of the last change (to notice a session left running
    /// for days). The only real-time field here.
    pub last_activity_ms: i64,
    /// The clock every other time here is read from.
    pub clock: SessionClock,

    // ----- History for this game day (mirrored into the database on save) -----
    /// Database id of this game day, assigned on first save.
    pub day_id: Option<i64>,
    pub day_ended_ms: Option<i64>,
    pub attendance: Vec<Attendance>,
    pub games: Vec<GameRecord>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            present: BTreeMap::new(),
            courts: Vec::new(),
            court_started_ms: Vec::new(),
            deck: Vec::new(),
            skip_round: BTreeSet::new(),
            practice: BTreeSet::new(),
            round_closed_ms: None,
            timer: Timer::Idle,
            round_number: 0,
            last_activity_ms: wall_ms(),
            clock: SessionClock::default(),
            day_id: None,
            day_ended_ms: None,
            attendance: Vec::new(),
            games: Vec::new(),
        }
    }
}

/// Where a player currently is tonight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Court(usize, usize),
    Deck(usize, usize),
}

impl Session {
    pub fn locate(&self, id: PlayerId) -> Option<Spot> {
        for (c, slots) in self.courts.iter().enumerate() {
            if let Some(s) = slots.iter().position(|p| *p == Some(id)) {
                return Some(Spot::Court(c, s));
            }
        }
        for (c, slots) in self.deck.iter().enumerate() {
            if let Some(s) = slots.iter().position(|p| *p == Some(id)) {
                return Some(Spot::Deck(c, s));
            }
        }
        None
    }

    pub fn slot_mut(&mut self, spot: Spot) -> &mut Option<PlayerId> {
        match spot {
            Spot::Court(c, s) => &mut self.courts[c][s],
            Spot::Deck(c, s) => &mut self.deck[c][s],
        }
    }

    pub fn slot(&self, spot: Spot) -> Option<PlayerId> {
        match spot {
            Spot::Court(c, s) => self.courts.get(c).and_then(|x| x[s]),
            Spot::Deck(c, s) => self.deck.get(c).and_then(|x| x[s]),
        }
    }

    pub fn is_here(&self, id: PlayerId) -> bool {
        self.present.contains_key(&id)
    }

    /// Move the whole session in time by `delta` ms (a new start date or
    /// time): every timestamp shifts together, so durations don't change.
    /// Set how long the session has been running, without changing anyone's
    /// wait or the round timer: everything moves with the clock. The session's
    /// start date and time stay put.
    pub fn set_elapsed(&mut self, elapsed_ms: i64, wall: i64) {
        let delta = elapsed_ms.max(0) - self.clock.elapsed(wall);
        self.clock.elapsed_ms += delta;
        self.shift(delta);
        self.clock.start_ms -= delta;
    }

    pub fn shift(&mut self, delta: i64) {
        self.clock.start_ms += delta;
        for p in self.present.values_mut() {
            p.checked_in_ms += delta;
            if let Some(t) = &mut p.last_played_ms {
                *t += delta;
            }
        }
        for t in self.court_started_ms.iter_mut().flatten() {
            *t += delta;
        }
        if let Timer::Running { end_ms } = &mut self.timer {
            *end_ms += delta;
        }
        if let Some(t) = &mut self.day_ended_ms {
            *t += delta;
        }
        for a in &mut self.attendance {
            a.in_ms += delta;
            if let Some(t) = &mut a.out_ms {
                *t += delta;
            }
        }
        for g in &mut self.games {
            g.started_ms += delta;
            g.ended_ms += delta;
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub players: BTreeMap<PlayerId, Player>,
    pub next_id: PlayerId,
    pub settings: Settings,
    pub session: Session,
}

impl AppState {
    /// The current session time.
    pub fn now(&self) -> i64 {
        self.session.clock.now(wall_ms())
    }

    /// Sessions saved before the session clock existed: start it at the
    /// first check-in, already running (as wall-clock time always was).
    pub fn ensure_clock(&mut self, wall: i64) {
        let s = &mut self.session;
        if !s.clock.is_set() {
            let start = s.attendance.iter().map(|a| a.in_ms).min().unwrap_or(wall).min(wall);
            s.clock = SessionClock { start_ms: start, elapsed_ms: wall - start, running_since: Some(wall) };
        }
    }

    pub fn add_player(&mut self, name: &str, skill: Skill) -> PlayerId {
        self.next_id = self.next_id.max(self.players.keys().max().map_or(0, |m| m + 1));
        let id = self.next_id;
        self.next_id += 1;
        self.players.insert(
            id,
            Player {
                id,
                name: name.trim().to_string(),
                skill,
                lifetime_games: 0,
                found_via: String::new(),
                auto_check_in: false,
                link: None,
                from: String::new(),
            },
        );
        id
    }

    pub fn name(&self, id: PlayerId) -> &str {
        self.players.get(&id).map_or("?", |p| p.name.as_str())
    }

    pub fn skill(&self, id: PlayerId) -> Skill {
        self.players.get(&id).map_or(Skill::Beginner2, |p| p.skill)
    }

    /// The most players who can be linked to play together: one court's worth.
    pub const MAX_LINKED: usize = 4;

    /// Everyone on the roster linked with `id` (not including them).
    pub fn linked(&self, id: PlayerId) -> Vec<PlayerId> {
        let Some(link) = self.players.get(&id).and_then(|p| p.link) else { return Vec::new() };
        self.players.values().filter(|p| p.id != id && p.link == Some(link)).map(|p| p.id).collect()
    }

    /// Make `id` play together with exactly `partners` (none: unlink them).
    /// Partners leave any group they were in; a group left with one person
    /// dissolves.
    pub fn set_partners(&mut self, id: PlayerId, partners: &[PlayerId]) {
        let mut members: Vec<PlayerId> = std::iter::once(id).chain(partners.iter().copied()).filter(|p| self.players.contains_key(p)).collect();
        members.dedup();
        members.truncate(Self::MAX_LINKED);
        for &m in &members {
            self.unlink(m);
        }
        if members.len() >= 2 {
            // The group is known by its lowest id (never reused, so never shared).
            let link = Some(*members.iter().min().expect("two or more") as u64);
            for m in &members {
                if let Some(p) = self.players.get_mut(m) {
                    p.link = link;
                }
            }
        }
    }

    /// Take `id` out of their group.
    pub fn unlink(&mut self, id: PlayerId) {
        let rest = self.linked(id);
        if let Some(p) = self.players.get_mut(&id) {
            p.link = None;
        }
        if rest.len() == 1 {
            if let Some(p) = self.players.get_mut(&rest[0]) {
                p.link = None;
            }
        }
    }

    pub fn in_practice(&self, id: PlayerId) -> bool {
        self.session.practice.contains(&id)
    }
}

// ---------------------------------------------------------------------------
// Storage location

/// Name of the data folder kept next to the app.
pub const PORTABLE_DIR: &str = "Drop-in Badminton data";

/// Where everything is saved: a `Drop-in Badminton data` folder next to the app,
/// so copying the app's folder takes the data along. Falls back to the per-user
/// folder when that isn't writable, and for builds run from a cargo `target`
/// folder (development: `cargo clean` shouldn't wipe the data).
pub fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| portable_dir().unwrap_or_else(per_user_data_dir)).clone()
}

/// The folder earlier versions saved to (and the fallback).
pub fn per_user_data_dir() -> PathBuf {
    directories::ProjectDirs::from("app", "drop-in-badminton", "drop-in-badminton")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The folder next to the app, if it can be used.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let parent = exe.parent()?;
    let in_cargo_target = parent
        .ancestors()
        .any(|a| a.file_name().is_some_and(|n| n == "target") && a.parent().is_some_and(|p| p.join("Cargo.toml").exists()));
    if in_cargo_target {
        return None;
    }
    let dir = parent.join(PORTABLE_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    // Can we actually write there (not, say, Program Files)?
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"").ok()?;
    let _ = std::fs::remove_file(probe);
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_elapsed_keeps_the_round_timer_and_start() {
        let wall = 1_000_000_000;
        let mut s = Session { clock: SessionClock::starting(5_000_000, wall - 600_000), ..Default::default() };
        let now = s.clock.now(wall);
        s.timer = Timer::Running { end_ms: now + 60_000 };
        s.set_elapsed(3_600_000, wall);
        assert_eq!(s.clock.elapsed(wall), 3_600_000);
        assert_eq!(s.clock.start_ms, 5_000_000);
        let Timer::Running { end_ms } = s.timer else { panic!("timer stopped") };
        assert_eq!(end_ms - s.clock.now(wall), 60_000);
    }
}
