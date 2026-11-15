//! Core data model: players, settings, and the state of tonight's session.
//!
//! Every timestamp is wall-clock Unix time in milliseconds, so "time since last
//! played" stays accurate across rounds, breaks between rounds, and even app
//! restarts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub type PlayerId = u64;

/// The four slots of a doubles court. Slots 0–1 are one team, 2–3 the other.
pub type Slots = [Option<PlayerId>; 4];

pub const EMPTY_SLOTS: Slots = [None; 4];

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Skill {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

impl Skill {
    pub const ALL: [Skill; 4] = [
        Skill::Beginner,
        Skill::Intermediate,
        Skill::Advanced,
        Skill::Expert,
    ];

    pub fn rank(self) -> i32 {
        self as i32
    }

    pub fn label(self) -> &'static str {
        match self {
            Skill::Beginner => "Beginner",
            Skill::Intermediate => "Intermediate",
            Skill::Advanced => "Advanced",
            Skill::Expert => "Expert",
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub courts: usize,
    pub round_minutes: u32,
    /// After waiting this long a player is placed on deck regardless of skill match.
    pub max_wait_minutes: u32,
    pub skill_matching: bool,
    pub auto_start_timer: bool,
    pub dark_mode: bool,
    pub ui_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            courts: 3,
            round_minutes: 15,
            max_wait_minutes: 30,
            skill_matching: true,
            auto_start_timer: false,
            dark_mode: true,
            ui_scale: 1.0,
        }
    }
}

impl Settings {
    pub const MAX_COURTS: usize = 12;

    pub fn round_ms(&self) -> i64 {
        self.round_minutes.max(1) as i64 * 60_000
    }

    pub fn max_wait_ms(&self) -> i64 {
        self.max_wait_minutes.max(1) as i64 * 60_000
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
    pub timer: Timer,
    pub round_number: u32,
    pub last_activity_ms: i64,

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
            timer: Timer::Idle,
            round_number: 0,
            last_activity_ms: now_ms(),
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
            },
        );
        id
    }

    pub fn name(&self, id: PlayerId) -> &str {
        self.players.get(&id).map_or("?", |p| p.name.as_str())
    }

    pub fn skill(&self, id: PlayerId) -> Skill {
        self.players.get(&id).map_or(Skill::Intermediate, |p| p.skill)
    }
}

// ---------------------------------------------------------------------------
// Storage location

pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("app", "goodminton", "goodminton")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
