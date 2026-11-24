//! SQLite storage: the roster, settings, and week-to-week history.
//!
//! Tables:
//! * `players` — everyone who has ever come. Removing someone from the roster
//!   archives them, so their history stays intact.
//! * `game_days`, `attendance`, `games`, `game_players` — the history.
//! * `meta` — settings and the live session (courts, on-deck, timer) as JSON,
//!   so the app resumes exactly where it left off.
//!
//! The current game day's history lives in memory (so undo just works) and is
//! mirrored into its tables on every save. Past days are never modified.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::model::*;

pub struct Store {
    conn: Connection,
    /// Set when the data is password-protected: the database then lives in
    /// memory and every change is written out sealed with this key.
    vault: Option<crate::vault::Key>,
    /// The folder the data lives in (`None`: in memory only, for tests and
    /// when the file couldn't be opened).
    dir: Option<PathBuf>,
}

/// Why a password-protected data file wouldn't open.
pub enum UnlockError {
    WrongPassword,
    Other(String),
}

fn io_error(err: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR), Some(err.to_string()))
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS players (
    id             INTEGER PRIMARY KEY,
    name           TEXT    NOT NULL,
    skill          TEXT    NOT NULL,
    lifetime_games INTEGER NOT NULL DEFAULT 0,
    archived       INTEGER NOT NULL DEFAULT 0,
    found_via      TEXT    NOT NULL DEFAULT '',
    home           TEXT    NOT NULL DEFAULT '',
    auto_check_in  INTEGER NOT NULL DEFAULT 0,
    link_group     INTEGER
);
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS game_days (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER
);
CREATE TABLE IF NOT EXISTS attendance (
    id             INTEGER PRIMARY KEY,
    game_day_id    INTEGER NOT NULL REFERENCES game_days(id) ON DELETE CASCADE,
    player_id      INTEGER NOT NULL REFERENCES players(id),
    checked_in_at  INTEGER NOT NULL,
    checked_out_at INTEGER
);
CREATE TABLE IF NOT EXISTS games (
    id          INTEGER PRIMARY KEY,
    game_day_id INTEGER NOT NULL REFERENCES game_days(id) ON DELETE CASCADE,
    round       INTEGER NOT NULL,
    court       INTEGER NOT NULL,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS game_players (
    game_id   INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    player_id INTEGER NOT NULL REFERENCES players(id),
    team      INTEGER NOT NULL,
    slot      INTEGER NOT NULL,
    PRIMARY KEY (game_id, player_id)
);
CREATE INDEX IF NOT EXISTS attendance_day ON attendance(game_day_id);
CREATE INDEX IF NOT EXISTS attendance_player ON attendance(player_id);
CREATE INDEX IF NOT EXISTS games_day ON games(game_day_id);
CREATE INDEX IF NOT EXISTS game_players_player ON game_players(player_id);
PRAGMA user_version = 1;
";

/// A row in the history list.
#[derive(Clone, Debug)]
pub struct DaySummary {
    pub id: i64,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub players: u32,
    pub games: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DayDetail {
    /// (name, skill, games played that day), most games first.
    pub attendees: Vec<(String, Skill, u32)>,
    pub games: Vec<PastGame>,
}

#[derive(Clone, Debug)]
pub struct PastGame {
    pub round: u32,
    pub court: usize,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub team_a: Vec<(String, Skill)>,
    pub team_b: Vec<(String, Skill)>,
}

/// A player as stored, archived or not.
#[derive(Clone, Debug)]
pub struct RawPlayer {
    pub id: PlayerId,
    pub name: String,
    pub skill: Skill,
    pub lifetime_games: u32,
    pub found_via: String,
    pub from: String,
    pub auto_check_in: bool,
}

#[derive(Clone, Debug)]
pub struct RawGame {
    pub day: i64,
    pub started_ms: i64,
    pub ended_ms: i64,
    /// (player, team 0 or 1)
    pub players: Vec<(PlayerId, u8)>,
}

/// One game as logged, for the Games view.
#[derive(Clone, Debug, PartialEq)]
pub struct LoggedGame {
    /// Its database id (stable for past sessions; the current session's games
    /// are rewritten, and get new ids, on every save).
    pub id: i64,
    pub day: i64,
    /// Its place among its session's games, matching `Session::games`.
    pub index: usize,
    pub record: GameRecord,
}

/// The whole database, flattened, for statistics.
#[derive(Clone, Debug, Default)]
pub struct RawHistory {
    pub players: Vec<RawPlayer>,
    /// (day id, started at), oldest first.
    pub days: Vec<(i64, i64)>,
    /// (day id, player, checked in at)
    pub attendance: Vec<(i64, PlayerId, i64)>,
    pub games: Vec<RawGame>,
}

fn skill_to_str(s: Skill) -> &'static str {
    s.label()
}

fn skill_from_str(s: &str) -> Skill {
    Skill::ALL
        .into_iter()
        .find(|k| k.label() == s)
        .unwrap_or(Skill::Beginner2)
}

const DB_FILE: &str = "drop-in-badminton.db";
/// The data file when a password is set (encrypted; replaces the database).
const VAULT_FILE: &str = "drop-in-badminton.db.enc";

pub fn db_path() -> PathBuf {
    data_dir().join(DB_FILE)
}

pub fn vault_path() -> PathBuf {
    data_dir().join(VAULT_FILE)
}

/// Whether the data is password-protected (so the app must ask first).
pub fn is_locked() -> bool {
    vault_path().exists()
}

/// Earlier versions saved to the per-user folder (AppData on Windows). The first
/// time the data folder sits next to the app, bring everything over and rename
/// the old folder so it isn't picked up again (it stays as a safety copy).
pub fn move_from_per_user_folder() {
    let new = data_dir();
    let old = crate::model::per_user_data_dir();
    if new == old || !old.is_dir() {
        return;
    }
    let has_data = |dir: &Path| dir.join(DB_FILE).exists() || dir.join(VAULT_FILE).exists();
    if has_data(&new) || !has_data(&old) {
        return;
    }
    if let Err(err) = copy_data(&old, &new) {
        eprintln!("drop-in-badminton: could not move the data from {old:?}: {err}");
        // Leave nothing half-copied: the old folder is still the real data.
        for file in [DB_FILE, VAULT_FILE] {
            let _ = std::fs::remove_file(new.join(file));
        }
        return;
    }
    let mut aside = old.as_os_str().to_owned();
    aside.push(" (moved next to the app)");
    if let Err(err) = std::fs::rename(&old, &aside) {
        eprintln!("drop-in-badminton: moved the data, but could not rename {old:?}: {err}");
    }
}

/// Copy a data folder: the database as a consistent snapshot, everything else as is.
fn copy_data(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let (src, dst) = (entry.path(), to.join(&name));
        let name = name.to_string_lossy();
        if name == DB_FILE {
            let conn = Connection::open(&src).map_err(io_error_from)?;
            conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(io_error_from)?;
            conn.execute("VACUUM INTO ?1", [dst.to_string_lossy()]).map_err(io_error_from)?;
        } else if name.starts_with(DB_FILE) && (name.ends_with("-wal") || name.ends_with("-shm")) {
            // Part of the database, already in the snapshot.
        } else if entry.file_type()?.is_dir() {
            copy_data(&src, &dst)?;
        } else if !dst.exists() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn io_error_from(err: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

impl Store {
    pub fn open_default() -> rusqlite::Result<Self> {
        let path = db_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::adopt_old_name(&path);
        let mut store = Self::open(&path)?;
        store.import_legacy_json(&data_dir().join("state.json"));
        Ok(store)
    }

    /// The app used to be called Goodminton: on first run under the new name,
    /// bring that database over (a consistent copy; the old file is kept).
    fn adopt_old_name(path: &Path) {
        // Only on the very first run: not once there's protected data, or
        // backups (say, after starting over without the password).
        if path.exists() || vault_path().exists() || data_dir().join("backups").exists() {
            return;
        }
        let Some(old_dir) = directories::ProjectDirs::from("app", "goodminton", "goodminton") else { return };
        let old = old_dir.data_dir().join("goodminton.db");
        if !old.exists() {
            return;
        }
        let copied = Connection::open(&old).and_then(|c| c.execute("VACUUM INTO ?1", [path.to_string_lossy()]));
        if let Err(err) = copied {
            eprintln!("drop-in-badminton: could not copy {old:?}: {err}");
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let mut store = Self::init(conn)?;
        store.dir = path.parent().map(Path::to_path_buf);
        Ok(store)
    }

    fn dir(&self) -> rusqlite::Result<&Path> {
        self.dir.as_deref().ok_or_else(|| io_error("the data isn't saved to a file"))
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // Right after an update the old copy may still be finishing its last save.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL keeps writes cheap and crash-safe.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute_batch(SCHEMA)?;
        // Columns added since the first release.
        let columns: Vec<String> = {
            let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('players')")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for (column, ddl) in [
            ("found_via", "ALTER TABLE players ADD COLUMN found_via TEXT NOT NULL DEFAULT ''"),
            ("home", "ALTER TABLE players ADD COLUMN home TEXT NOT NULL DEFAULT ''"),
            ("auto_check_in", "ALTER TABLE players ADD COLUMN auto_check_in INTEGER NOT NULL DEFAULT 0"),
            ("link_group", "ALTER TABLE players ADD COLUMN link_group INTEGER"),
        ] {
            if !columns.iter().any(|c| c == column) {
                conn.execute_batch(ddl)?;
            }
        }
        // v0.3.1: the levels became Beginner 1, Beginner 2, Intermediate, Advanced,
        // with everyone moving down one name (Expert became Advanced, and so on).
        conn.execute_batch(
            "BEGIN;
             UPDATE players SET skill = CASE skill
                 WHEN 'Beginner' THEN 'Beginner 1'
                 WHEN 'Intermediate' THEN 'Beginner 2'
                 WHEN 'Advanced' THEN 'Intermediate'
                 WHEN 'Expert' THEN 'Advanced'
                 ELSE skill END
             WHERE NOT EXISTS (SELECT 1 FROM meta WHERE key = 'skill_levels');
             INSERT OR IGNORE INTO meta(key, value) VALUES ('skill_levels', 'beginner-1-2');
             COMMIT;",
        )?;
        Ok(Self { conn, vault: None, dir: None })
    }

    // ----- Password protection -----------------------------------------------

    /// Open the password-protected data file.
    pub fn unlock(password: &str) -> Result<Self, UnlockError> {
        Self::unlock_in(&data_dir(), password)
    }

    fn unlock_in(dir: &Path, password: &str) -> Result<Self, UnlockError> {
        let file = std::fs::read(dir.join(VAULT_FILE)).map_err(|e| UnlockError::Other(e.to_string()))?;
        let (key, plain) = match crate::vault::open(password, &file) {
            Ok(opened) => opened,
            Err(crate::vault::VaultError::WrongPassword) => return Err(UnlockError::WrongPassword),
            Err(err) => return Err(UnlockError::Other(err.to_string())),
        };
        let mut store = Self::from_bytes(&plain).map_err(|e| UnlockError::Other(e.to_string()))?;
        store.vault = Some(key);
        store.dir = Some(dir.to_path_buf());
        Ok(store)
    }

    /// A database held in memory, from a whole database file's bytes.
    fn from_bytes(bytes: &[u8]) -> rusqlite::Result<Self> {
        let mut bytes = bytes.to_vec();
        // A copy of a database in WAL mode says so in its header (bytes 18
        // and 19), and SQLite would go looking for the WAL file; in memory
        // it's an ordinary rollback-journal database.
        if bytes.len() > 19 {
            bytes[18] = 1;
            bytes[19] = 1;
        }
        let mut conn = Connection::open_in_memory()?;
        conn.deserialize_read_exact(rusqlite::MAIN_DB, &bytes[..], bytes.len(), false)?;
        Self::init(conn)
    }

    /// The whole database as a file's bytes.
    fn to_bytes(&mut self) -> rusqlite::Result<Vec<u8>> {
        Ok(self.conn.serialize(rusqlite::MAIN_DB)?.to_vec())
    }

    pub fn has_password(&self) -> bool {
        self.vault.is_some()
    }

    pub fn password_matches(&self, password: &str) -> bool {
        self.vault.as_ref().is_some_and(|k| k.matches(password))
    }

    /// Write the encrypted file (password-protected only; otherwise SQLite
    /// has already written the change).
    fn persist(&mut self) -> rusqlite::Result<()> {
        let Some(key) = self.vault.clone() else { return Ok(()) };
        let bytes = self.to_bytes()?;
        let path = self.dir()?.join(VAULT_FILE);
        crate::vault::write_atomic(&path, &crate::vault::seal(&key, &bytes)).map_err(io_error)
    }

    /// Protect the data with a password: from now on it's only ever written
    /// to disk encrypted, and the readable copies (the database, its backups)
    /// are removed. Also used to change the password.
    pub fn set_password(&mut self, password: &str) -> rusqlite::Result<()> {
        let dir = self.dir()?.to_path_buf();
        let old = self.vault.clone();
        let key = crate::vault::Key::new(password);
        let bytes = self.to_bytes()?;
        let sealed = crate::vault::seal(&key, &bytes);
        // Check it opens before anything readable is removed.
        if crate::vault::open_with(&key, &sealed).ok().as_deref() != Some(&bytes[..]) {
            return Err(io_error("couldn't verify the encrypted copy"));
        }
        crate::vault::write_atomic(&dir.join(VAULT_FILE), &sealed).map_err(io_error)?;
        if old.is_none() {
            // Switch to the copy in memory, then remove the readable files.
            *self = Self::from_bytes(&bytes)?;
            self.dir = Some(dir.clone());
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(dir.join(format!("{DB_FILE}{suffix}")));
            }
            // An old import, long since copied into the database.
            let _ = std::fs::remove_file(dir.join("state.json.imported"));
        }
        self.vault = Some(key.clone());
        Self::rekey_backups(&dir, old.as_ref(), Some(&key));
        Ok(())
    }

    /// Stop protecting the data: write it out readable again.
    pub fn remove_password(&mut self) -> rusqlite::Result<()> {
        let Some(old) = self.vault.clone() else { return Ok(()) };
        let dir = self.dir()?.to_path_buf();
        let bytes = self.to_bytes()?;
        crate::vault::write_atomic(&dir.join(DB_FILE), &bytes).map_err(io_error)?;
        *self = Self::open(&dir.join(DB_FILE))?;
        let _ = std::fs::remove_file(dir.join(VAULT_FILE));
        Self::rekey_backups(&dir, Some(&old), None);
        Ok(())
    }

    /// Re-seal every backup for a new key (`None`: write them readable).
    fn rekey_backups(dir: &Path, old: Option<&crate::vault::Key>, new: Option<&crate::vault::Key>) {
        let Ok(entries) = std::fs::read_dir(dir.join("backups")) else { return };
        for path in entries.flatten().map(|e| e.path()) {
            let name = path.to_string_lossy().into_owned();
            let (sealed, base) = match name.strip_suffix(".enc") {
                Some(base) if base.ends_with(".db") => (true, base.to_owned()),
                None if name.ends_with(".db") => (false, name.clone()),
                _ => continue,
            };
            let Ok(file) = std::fs::read(&path) else { continue };
            let plain = match (sealed, old) {
                (false, _) => file,
                (true, Some(old)) => match crate::vault::open_with(old, &file) {
                    Ok(plain) => plain,
                    Err(_) => continue, // sealed with some other key: leave it
                },
                (true, None) => continue,
            };
            let (target, bytes) = match new {
                Some(key) => (format!("{base}.enc"), crate::vault::seal(key, &plain)),
                None => (base, plain),
            };
            if crate::vault::write_atomic(Path::new(&target), &bytes).is_ok() && target != name {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    fn meta(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
    }

    /// Record which app version opened this database; returns the previous one.
    pub fn swap_app_version(&mut self, current: &str) -> Option<String> {
        let previous = self.meta("app_version").ok().flatten();
        let _ = self.conn.execute(
            "INSERT INTO meta(key, value) VALUES ('app_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [current],
        );
        if previous.as_deref() != Some(current) {
            let _ = self.persist();
        }
        previous
    }

    /// A consistent copy of the whole database, safe to take while it's open
    /// (encrypted too, with `.enc` added to the name, if the data is).
    pub fn backup_to(&mut self, path: &Path) -> rusqlite::Result<()> {
        match self.vault.clone() {
            Some(key) => {
                let bytes = self.to_bytes()?;
                let path = PathBuf::from(format!("{}.enc", path.display()));
                crate::vault::write_atomic(&path, &crate::vault::seal(&key, &bytes)).map_err(io_error)
            }
            None => {
                self.conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
                Ok(())
            }
        }
    }

    /// One-time import from the JSON file earlier versions used.
    fn import_legacy_json(&mut self, path: &Path) {
        if self.meta("settings").ok().flatten().is_some() {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let mut state: AppState = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("drop-in-badminton: could not import {path:?}: {err}");
                return;
            }
        };
        // The old format didn't log attendance; reconstruct today's.
        if state.session.attendance.is_empty() {
            state.session.attendance = state
                .session
                .present
                .iter()
                .map(|(id, p)| Attendance {
                    player: *id,
                    in_ms: p.checked_in_ms,
                    out_ms: None,
                })
                .collect();
        }
        match self.save(&mut state) {
            Ok(()) => {
                let _ = std::fs::rename(path, path.with_extension("json.imported"));
            }
            Err(err) => eprintln!("drop-in-badminton: import failed: {err}"),
        }
    }

    pub fn load(&self) -> rusqlite::Result<AppState> {
        let mut state = AppState::default();
        if let Some(json) = self.meta("settings")? {
            state.settings = serde_json::from_str(&json).unwrap_or_default();
        }
        if let Some(json) = self.meta("session")? {
            match serde_json::from_str(&json) {
                Ok(session) => state.session = session,
                Err(err) => eprintln!("drop-in-badminton: could not restore session: {err}"),
            }
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, name, skill, lifetime_games, found_via, home, auto_check_in, link_group FROM players WHERE archived = 0",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Player {
                id: r.get::<_, i64>(0)? as PlayerId,
                name: r.get(1)?,
                skill: skill_from_str(&r.get::<_, String>(2)?),
                lifetime_games: r.get(3)?,
                found_via: r.get(4)?,
                from: r.get(5)?,
                auto_check_in: r.get(6)?,
                link: r.get::<_, Option<i64>>(7)?.map(|l| l as u64),
            })
        })?;
        for p in rows {
            let p = p?;
            state.players.insert(p.id, p);
        }
        // Never reuse an id, even an archived one: history points at it.
        let max_id: Option<i64> = self
            .conn
            .query_row("SELECT MAX(id) FROM players", [], |r| r.get(0))?;
        state.next_id = max_id.map_or(0, |m| m as PlayerId + 1);
        // Drop session references to players that no longer exist.
        let players = &state.players;
        state.session.present.retain(|id, _| players.contains_key(id));
        Ok(state)
    }

    /// Write everything. May assign `state.session.day_id`.
    pub fn save(&mut self, state: &mut AppState) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::save_players(&tx, &state.players)?;
        Self::save_day(&tx, &mut state.session)?;
        let settings = serde_json::to_string(&state.settings).expect("settings serialize");
        let session = serde_json::to_string(&state.session).expect("session serialize");
        for (k, v) in [("settings", settings), ("session", session)] {
            tx.execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![k, v],
            )?;
        }
        tx.commit()?;
        self.persist()
    }

    fn save_players(tx: &Transaction, players: &BTreeMap<PlayerId, Player>) -> rusqlite::Result<()> {
        let mut upsert = tx.prepare_cached(
            "INSERT INTO players(id, name, skill, lifetime_games, archived, found_via, home, auto_check_in, link_group)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, skill = excluded.skill,
                 lifetime_games = excluded.lifetime_games, archived = 0,
                 found_via = excluded.found_via, home = excluded.home, auto_check_in = excluded.auto_check_in,
                 link_group = excluded.link_group",
        )?;
        // A player added from the Players view is only kept once they have a name.
        for p in players.values().filter(|p| !p.name.trim().is_empty()) {
            upsert.execute(params![p.id as i64, p.name, skill_to_str(p.skill), p.lifetime_games, p.found_via.trim(), p.from.trim(), p.auto_check_in, p.link.map(|l| l as i64)])?;
        }
        // Anyone no longer on the roster is archived, keeping their history.
        let ids: Vec<i64> = {
            let mut stmt = tx.prepare_cached("SELECT id FROM players WHERE archived = 0")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for id in ids {
            if !players.contains_key(&(id as PlayerId)) {
                tx.execute("UPDATE players SET archived = 1 WHERE id = ?1", [id])?;
            }
        }
        Ok(())
    }

    fn save_day(tx: &Transaction, session: &mut Session) -> rusqlite::Result<()> {
        if session.attendance.is_empty() && session.day_id.is_none() {
            return Ok(()); // Nothing has happened today yet.
        }
        // The session's chosen start (older sessions: the first check-in).
        let started = if session.clock.is_set() {
            session.clock.start_ms
        } else {
            session.attendance.iter().map(|a| a.in_ms).min().unwrap_or_else(wall_ms)
        };
        let day_id = match session.day_id {
            Some(id) => {
                tx.execute(
                    "INSERT INTO game_days(id, started_at, ended_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET started_at = excluded.started_at,
                         ended_at = excluded.ended_at",
                    params![id, started, session.day_ended_ms],
                )?;
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO game_days(started_at, ended_at) VALUES (?1, ?2)",
                    params![started, session.day_ended_ms],
                )?;
                tx.last_insert_rowid()
            }
        };
        session.day_id = Some(day_id);

        // Rewrite this day's rows from memory (a few dozen rows at most).
        tx.execute("DELETE FROM attendance WHERE game_day_id = ?1", [day_id])?;
        tx.execute("DELETE FROM games WHERE game_day_id = ?1", [day_id])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO attendance(game_day_id, player_id, checked_in_at, checked_out_at)
                 SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM players WHERE id = ?2)",
            )?;
            for a in &session.attendance {
                ins.execute(params![day_id, a.player as i64, a.in_ms, a.out_ms])?;
            }
        }
        let mut ins_game = tx.prepare_cached(
            "INSERT INTO games(game_day_id, round, court, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut ins_player = tx.prepare_cached(
            "INSERT OR IGNORE INTO game_players(game_id, player_id, team, slot)
             SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM players WHERE id = ?2)",
        )?;
        for g in &session.games {
            ins_game.execute(params![day_id, g.round, g.court as i64, g.started_ms, g.ended_ms])?;
            let game_id = tx.last_insert_rowid();
            for (slot, pid) in g.players.iter().enumerate() {
                if let Some(pid) = pid {
                    ins_player.execute(params![game_id, *pid as i64, (slot / 2) as i64, slot as i64])?;
                }
            }
        }
        // Tidy up days that ended up empty (e.g. after undoing "New game day").
        tx.execute(
            "DELETE FROM game_days WHERE id != ?1
               AND NOT EXISTS (SELECT 1 FROM attendance WHERE game_day_id = game_days.id)",
            [day_id],
        )?;
        Ok(())
    }

    // ----- History queries --------------------------------------------------

    /// Everything the Players and Analytics views are computed from: every
    /// player ever (archived ones too) and every session's attendance and games.
    pub fn everything(&self) -> rusqlite::Result<RawHistory> {
        let mut h = RawHistory::default();
        let mut stmt = self.conn.prepare("SELECT id, name, skill, lifetime_games, found_via, home, auto_check_in FROM players")?;
        h.players = stmt
            .query_map([], |r| {
                Ok(RawPlayer {
                    id: r.get::<_, i64>(0)? as PlayerId,
                    name: r.get(1)?,
                    skill: skill_from_str(&r.get::<_, String>(2)?),
                    lifetime_games: r.get(3)?,
                    found_via: r.get(4)?,
                    from: r.get(5)?,
                    auto_check_in: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare("SELECT id, started_at FROM game_days ORDER BY started_at")?;
        h.days = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare("SELECT game_day_id, player_id, checked_in_at FROM attendance")?;
        h.attendance = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as PlayerId, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT g.id, g.game_day_id, g.started_at, g.ended_at, gp.player_id, gp.team
             FROM games g LEFT JOIN game_players gp ON gp.game_id = g.id
             ORDER BY g.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })?;
        let mut last = None;
        for row in rows {
            let (id, day, start, end, player, team) = row?;
            if last != Some(id) {
                h.games.push(RawGame { day, started_ms: start, ended_ms: end, players: Vec::new() });
                last = Some(id);
            }
            if let (Some(p), Some(team)) = (player, team) {
                h.games.last_mut().expect("pushed above").players.push((p as PlayerId, team as u8));
            }
        }
        Ok(h)
    }

    /// Every game ever played, session by session, in the order played.
    pub fn game_log(&self) -> rusqlite::Result<Vec<LoggedGame>> {
        let mut stmt = self.conn.prepare(
            "SELECT g.id, g.game_day_id, g.round, g.court, g.started_at, g.ended_at, gp.player_id, gp.slot
             FROM games g LEFT JOIN game_players gp ON gp.game_id = g.id
             ORDER BY g.game_day_id, g.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })?;
        let mut log: Vec<LoggedGame> = Vec::new();
        for row in rows {
            let (id, day, round, court, start, end, player, slot) = row?;
            if log.last().is_none_or(|g| g.id != id) {
                let index = match log.last() {
                    Some(g) if g.day == day => g.index + 1,
                    _ => 0,
                };
                let record = GameRecord { round, court: court.max(0) as usize, started_ms: start, ended_ms: end, players: EMPTY_SLOTS };
                log.push(LoggedGame { id, day, index, record });
            }
            if let (Some(p), Some(slot)) = (player, slot) {
                if let Some(s) = log.last_mut().expect("pushed above").record.players.get_mut(slot as usize) {
                    *s = Some(p as PlayerId);
                }
            }
        }
        Ok(log)
    }

    /// Rewrite a game from a past session (the current session's games are
    /// edited in memory and saved with it instead).
    pub fn update_game(&mut self, id: i64, g: &GameRecord) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE games SET round = ?2, court = ?3, started_at = ?4, ended_at = ?5 WHERE id = ?1",
            params![id, g.round, g.court as i64, g.started_ms, g.ended_ms],
        )?;
        tx.execute("DELETE FROM game_players WHERE game_id = ?1", [id])?;
        for (slot, pid) in g.players.iter().enumerate() {
            if let Some(pid) = pid {
                tx.execute(
                    "INSERT OR IGNORE INTO game_players(game_id, player_id, team, slot)
                     SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM players WHERE id = ?2)",
                    params![id, *pid as i64, (slot / 2) as i64, slot as i64],
                )?;
            }
        }
        tx.commit()?;
        self.persist()
    }

    /// Remove a game from a past session.
    pub fn delete_game(&mut self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM games WHERE id = ?1", [id])?;
        self.persist()
    }

    /// Edit a player who's no longer on the roster (roster players are saved
    /// from the app state instead).
    pub fn update_archived_player(&mut self, p: &Player) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE players SET name = ?2, skill = ?3, found_via = ?4, home = ?5, auto_check_in = ?6
             WHERE id = ?1 AND archived = 1",
            params![p.id as i64, p.name, skill_to_str(p.skill), p.found_via.trim(), p.from.trim(), p.auto_check_in],
        )?;
        self.persist()
    }

    pub fn days(&self) -> rusqlite::Result<Vec<DaySummary>> {
        let mut stmt = self.conn.prepare(
            // The day runs from the first check-in to the last game (not to when
            // the app noticed the day was over, which can be the next morning).
            "SELECT d.id,
                    d.started_at,
                    CASE WHEN d.ended_at IS NULL THEN NULL
                         -- A game closed out the next morning still only lasted a round or so.
                         ELSE COALESCE((SELECT MAX(MIN(ended_at, started_at + 3600000)) FROM games g WHERE g.game_day_id = d.id), d.ended_at) END,
                    (SELECT COUNT(DISTINCT player_id) FROM attendance a WHERE a.game_day_id = d.id),
                    (SELECT COUNT(*) FROM games g WHERE g.game_day_id = d.id)
             FROM game_days d ORDER BY d.started_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DaySummary {
                id: r.get(0)?,
                started_ms: r.get(1)?,
                ended_ms: r.get(2)?,
                players: r.get(3)?,
                games: r.get(4)?,
            })
        })?;
        // Finished days where nobody played are just noise.
        rows.filter(|d| d.as_ref().map_or(true, |d| d.players > 0 && (d.games > 0 || d.ended_ms.is_none())))
            .collect()
    }

    pub fn day_detail(&self, day_id: i64) -> rusqlite::Result<DayDetail> {
        let mut detail = DayDetail::default();
        let mut stmt = self.conn.prepare(
            "SELECT p.name, p.skill,
                    (SELECT COUNT(*) FROM game_players gp JOIN games g ON g.id = gp.game_id
                      WHERE g.game_day_id = ?1 AND gp.player_id = p.id) AS n
             FROM players p
             WHERE p.id IN (SELECT player_id FROM attendance WHERE game_day_id = ?1)
             ORDER BY n DESC, p.name COLLATE NOCASE",
        )?;
        detail.attendees = stmt
            .query_map([day_id], |r| {
                Ok((r.get(0)?, skill_from_str(&r.get::<_, String>(1)?), r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT g.id, g.round, g.court, g.started_at, g.ended_at, p.name, p.skill, gp.team
             FROM games g
             JOIN game_players gp ON gp.game_id = g.id
             JOIN players p ON p.id = gp.player_id
             WHERE g.game_day_id = ?1
             ORDER BY g.started_at, g.court, gp.slot",
        )?;
        let mut by_game: Vec<(i64, PastGame)> = Vec::new();
        let mut index: HashMap<i64, usize> = HashMap::new();
        let rows = stmt.query_map([day_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                skill_from_str(&r.get::<_, String>(6)?),
                r.get::<_, i64>(7)?,
            ))
        })?;
        for row in rows {
            let (gid, round, court, start, end, name, skill, team) = row?;
            let i = *index.entry(gid).or_insert_with(|| {
                by_game.push((
                    gid,
                    PastGame {
                        round,
                        court: court as usize,
                        started_ms: start,
                        ended_ms: end,
                        team_a: Vec::new(),
                        team_b: Vec::new(),
                    },
                ));
                by_game.len() - 1
            });
            let g = &mut by_game[i].1;
            if team == 0 {
                g.team_a.push((name, skill));
            } else {
                g.team_b.push((name, skill));
            }
        }
        detail.games = by_game.into_iter().map(|(_, g)| g).collect();
        Ok(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Target;

    #[test]
    fn old_databases_get_the_new_player_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE players (id INTEGER PRIMARY KEY, name TEXT NOT NULL, skill TEXT NOT NULL,
                 lifetime_games INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
             INSERT INTO players(id, name, skill) VALUES (1, 'Ann', 'Advanced');",
        )
        .unwrap();
        let mut store = Store::init(conn).unwrap();
        let mut s = store.load().unwrap();
        assert_eq!((s.players[&1].from.as_str(), s.players[&1].found_via.as_str()), ("", ""));
        assert!(!s.players[&1].auto_check_in);
        let ann = s.players.get_mut(&1).unwrap();
        ann.from = "Leith ".into();
        ann.found_via = "A friend".into();
        ann.auto_check_in = true;
        ann.link = Some(1);
        store.save(&mut s).unwrap();
        let ann = &store.load().unwrap().players[&1];
        assert_eq!((ann.from.as_str(), ann.found_via.as_str(), ann.auto_check_in), ("Leith", "A friend", true));
        assert_eq!(ann.link, Some(1));
    }

    #[test]
    fn old_skill_levels_move_down_one_name_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE players (id INTEGER PRIMARY KEY, name TEXT NOT NULL, skill TEXT NOT NULL,
                 lifetime_games INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
             INSERT INTO players(id, name, skill) VALUES
                 (1, 'A', 'Beginner'), (2, 'B', 'Intermediate'), (3, 'C', 'Advanced'), (4, 'D', 'Expert');",
        )
        .unwrap();
        // Opening twice must not shift anyone a second time.
        let store = Store::init(Store::init(conn).unwrap().conn).unwrap();
        let s = store.load().unwrap();
        let skills: Vec<Skill> = (1..=4).map(|id| s.players[&id].skill).collect();
        assert_eq!(skills, Skill::ALL);
        assert_eq!(Skill::ALL.map(Skill::label), ["Beginner 1", "Beginner 2", "Intermediate", "Advanced"]);
    }

    #[test]
    fn a_password_encrypts_everything_and_can_be_changed_or_removed() {
        let dir = std::env::temp_dir().join(format!("dib-vault-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        let mut store = Store::open(&dir.join(DB_FILE)).unwrap();
        let mut s = AppState::default();
        s.add_player("Zebedee Quartermaine", Skill::Advanced);
        store.save(&mut s).unwrap();
        store.backup_to(&dir.join("backups/old.db")).unwrap();
        let readable = |path: &Path| std::fs::read(path).is_ok_and(|b| b.windows(9).any(|w| w == b"Zebedee Q"));
        assert!(readable(&dir.join(DB_FILE)) || readable(&dir.join(format!("{DB_FILE}-wal"))));

        store.set_password("hunter22").unwrap();
        assert!(!dir.join(DB_FILE).exists() && !dir.join(format!("{DB_FILE}-wal")).exists(), "readable copies removed");
        assert!(!readable(&dir.join(VAULT_FILE)));
        assert!(!dir.join("backups/old.db").exists() && !readable(&dir.join("backups/old.db.enc")), "backups encrypted");
        s.add_player("Second Person", Skill::Beginner1);
        store.save(&mut s).unwrap();
        drop(store);

        assert!(matches!(Store::unlock_in(&dir, "wrong"), Err(UnlockError::WrongPassword)));
        let Ok(mut store) = Store::unlock_in(&dir, "hunter22") else { panic!("didn't unlock") };
        assert_eq!(store.load().unwrap().players.len(), 2, "changes made while locked were saved");

        store.set_password("new one").unwrap();
        drop(store);
        assert!(Store::unlock_in(&dir, "hunter22").is_err());
        let Ok(mut store) = Store::unlock_in(&dir, "new one") else { panic!("didn't unlock") };
        assert!(store.password_matches("new one"));
        store.remove_password().unwrap();
        assert!(!dir.join(VAULT_FILE).exists() && dir.join("backups/old.db").exists());
        assert_eq!(Store::open(&dir.join(DB_FILE)).unwrap().load().unwrap().players.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn past_games_can_be_edited_and_removed() {
        let mut store = Store::open_in_memory().unwrap();
        let mut s = AppState::default();
        let ids: Vec<PlayerId> = ["Ann", "Bo", "Cy", "Di", "Ed"].iter().map(|n| s.add_player(n, Skill::Beginner2)).collect();
        s.session.clock = SessionClock::starting(1_000, 0);
        for &id in &ids {
            s.session.attendance.push(Attendance { player: id, in_ms: 1_000, out_ms: None });
        }
        let game = |court, players| GameRecord { round: 1, court, started_ms: 1_000, ended_ms: 901_000, players };
        s.session.games.push(game(0, [Some(ids[0]), Some(ids[1]), Some(ids[2]), Some(ids[3])]));
        s.session.games.push(game(1, [Some(ids[4]), None, None, None]));
        store.save(&mut s).unwrap();

        let log = store.game_log().unwrap();
        assert_eq!(log.iter().map(|g| g.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(log[0].record, s.session.games[0]);

        let mut edited = log[0].record.clone();
        edited.players[3] = Some(ids[4]);
        edited.ended_ms = 601_000;
        store.update_game(log[0].id, &edited).unwrap();
        store.delete_game(log[1].id).unwrap();
        let log = store.game_log().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].record, edited);
    }

    #[test]
    fn round_trip_and_history() {
        let mut store = Store::open_in_memory().unwrap();
        let mut s = AppState::default();
        s.settings.courts = 1;
        s.sync_court_count(0);
        let ids: Vec<_> = (0..5).map(|i| s.add_player(&format!("P{i}"), Skill::Intermediate)).collect();
        for id in &ids {
            s.check_in(*id, 0);
        }
        s.next_round(60_000);
        s.next_round(16 * 60_000);
        store.save(&mut s).unwrap();
        let day = s.session.day_id.expect("day created on save");

        // Reload: roster, session, and today's history all come back.
        let loaded = store.load().unwrap();
        assert_eq!(loaded.players.len(), 5);
        assert_eq!(loaded.session.games.len(), 1);
        assert_eq!(loaded.session.day_id, Some(day));

        // Saving again doesn't duplicate rows.
        store.save(&mut s).unwrap();
        let days = store.days().unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!((days[0].players, days[0].games), (5, 1));

        // End the day and start a new one: last week's history stays put.
        s.end_day(20 * 60_000);
        store.save(&mut s).unwrap();
        s.new_day(7 * 86_400_000, 0);
        // Removing a player archives them but keeps the history.
        s.delete_player(ids[0], 0);
        s.move_player(ids[1], Target::Slot(Spot::Deck(0, 0)), 7 * 86_400_000);
        store.save(&mut s).unwrap();

        let days = store.days().unwrap();
        assert_eq!(days.len(), 2);
        let last_week = store.day_detail(day).unwrap();
        assert_eq!(last_week.attendees.len(), 5);
        assert_eq!(last_week.games.len(), 2);
        assert_eq!(last_week.games[0].team_a.len() + last_week.games[0].team_b.len(), 4);
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.players.len(), 4);
        assert!(reloaded.next_id > ids[4], "archived ids are never reused");
    }

    #[test]
    fn copying_the_data_folder_takes_the_database_and_backups() {
        let root = std::env::temp_dir().join(format!("dib-move-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (old, new) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(old.join("backups")).unwrap();
        let mut store = Store::open(&old.join(DB_FILE)).unwrap();
        let mut state = AppState::default();
        state.add_player("Ana", Skill::Intermediate);
        store.save(&mut state).unwrap();
        std::fs::write(old.join("backups/before-update.db"), b"backup").unwrap();
        std::fs::write(old.join("graphics.txt"), b"dx12").unwrap();

        copy_data(&old, &new).unwrap();
        drop(store);
        let copied = Store::open(&new.join(DB_FILE)).unwrap().load().unwrap();
        assert_eq!(copied.players.len(), 1);
        assert_eq!(std::fs::read(new.join("backups/before-update.db")).unwrap(), b"backup");
        assert!(new.join("graphics.txt").exists());
        assert!(!new.join(format!("{DB_FILE}-wal")).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
