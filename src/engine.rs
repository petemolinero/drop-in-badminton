//! Rotation and matchmaking rules.
//!
//! * The waiting list is ordered by time since each player last came off a
//!   court (or arrived), longest first; ties go to fewer games tonight.
//! * Up next always takes the next players in line: as many as there are open
//!   spots (4, 8, 12…). Skill only decides how those players are arranged:
//!   similar levels share a court, and teams are balanced.
//! * Linked players (who always play together) share a place in line and a
//!   wait, and go on one court together. If the cut-off for this round falls
//!   in the middle of them, they're held back for the next round rather than
//!   split up, and the next player in line goes instead.
//! * Tonight's practice group only plays each other. The courts are shared:
//!   each empty group goes to whichever side has played less tonight (games
//!   per hour, per person), so time on court evens out across everyone.
//! * Groups are filled for the courts that have been playing longest first, so
//!   when there aren't enough people for every court, the courts take turns.

use crate::model::*;

/// Who's waiting, in the order they'll go on.
pub struct WaitingLine {
    /// One round's worth each, first first.
    pub groups: Vec<Vec<PlayerId>>,
    /// Can't be placed as things stand (say, a practice group too small for a court).
    pub stuck: Vec<PlayerId>,
    /// On a break.
    pub resting: Vec<PlayerId>,
}

/// Players placed together: one person, or linked players free at once.
struct Unit {
    /// Best-placed first.
    ids: Vec<PlayerId>,
    practice: bool,
}

/// Where a dragged player was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Slot(Spot),
    /// Back to the waiting pool.
    Pool,
}

impl AppState {
    // ----- Queries ---------------------------------------------------------

    pub fn is_assigned(&self, id: PlayerId) -> bool {
        self.session.locate(id).is_some()
    }

    /// Checked-in players who are neither on court nor on deck, in the order
    /// they'll actually go on (see `waiting_line`).
    pub fn waiting_list(&self, now: i64) -> Vec<PlayerId> {
        let line = self.waiting_line(now);
        line.groups.into_iter().flatten().chain(line.stuck).chain(line.resting).collect()
    }

    /// The waiting players as they'll go on: the picking is run ahead, round
    /// after round, with the courts there are, so each group is one round's
    /// worth (4 a court). Linked players held back by a cut-off land in a
    /// later group than whoever goes ahead of them.
    pub fn waiting_line(&self, now: i64) -> WaitingLine {
        let mut ids: Vec<PlayerId> = self
            .session
            .present
            .keys()
            .copied()
            .filter(|id| !self.is_assigned(*id))
            .collect();
        self.sort_by_priority(&mut ids, now);
        let (mut left, resting): (Vec<PlayerId>, Vec<PlayerId>) = ids.into_iter().partition(|id| !self.session.present[id].resting);
        let courts = self.session.courts.len().max(1);
        let mut groups = Vec::new();
        while !left.is_empty() {
            let mut slots = vec![EMPTY_SLOTS; courts];
            self.fill_groups_from(&mut slots, &(0..courts).collect::<Vec<_>>(), &left, now);
            let picked: Vec<PlayerId> = left.iter().copied().filter(|id| slots.iter().flatten().any(|s| *s == Some(*id))).collect();
            if picked.is_empty() {
                break;
            }
            left.retain(|id| !picked.contains(id));
            groups.push(picked);
        }
        WaitingLine { groups, stuck: left, resting }
    }

    pub fn waiting_ms(&self, id: PlayerId, now: i64) -> i64 {
        self.session
            .present
            .get(&id)
            .map_or(0, |p| p.waiting_ms(now))
    }

    /// A player's own place-in-line key (smaller goes first).
    fn own_key(&self, id: PlayerId, now: i64) -> (std::cmp::Reverse<i64>, u32, std::cmp::Reverse<i64>, PlayerId) {
        let p = &self.session.present[&id];
        let wait = p.waiting_ms(now);
        // Waits within the same minute count as equal so that fewer games
        // tonight breaks the tie (e.g. a whole court that came off together).
        (std::cmp::Reverse(wait / 60_000), p.games_tonight, std::cmp::Reverse(wait), id)
    }

    /// Partners of `id` who are in `ids` too and can play alongside them now
    /// (not on a break, same side of the practice split).
    fn partners_among(&self, id: PlayerId, ids: &[PlayerId]) -> Vec<PlayerId> {
        let free = |x: &PlayerId| self.session.present.get(x).is_some_and(|p| !p.resting);
        if !free(&id) {
            return Vec::new();
        }
        self.linked(id).into_iter().filter(|p| ids.contains(p) && free(p) && self.in_practice(*p) == self.in_practice(id)).collect()
    }

    /// Linked players waiting together share a place in line: the unit takes
    /// its best member's place, and its members sit next to each other.
    fn sort_by_priority(&self, ids: &mut [PlayerId], now: i64) {
        let all: Vec<PlayerId> = ids.to_vec();
        let key = |id: &PlayerId| {
            let own = self.own_key(*id, now);
            let shared = self.partners_among(*id, &all).into_iter().map(|p| self.own_key(p, now)).fold(own, std::cmp::min);
            (shared, own)
        };
        ids.sort_by_cached_key(key);
    }

    /// Linked players waiting together with `id`, including them (just `[id]`
    /// if they're not waiting with anyone).
    pub fn waiting_unit(&self, id: PlayerId) -> Vec<PlayerId> {
        let waiting = |p: &PlayerId| self.session.present.contains_key(p) && !self.is_assigned(*p);
        let mut unit = vec![id];
        if waiting(&id) {
            let partners: Vec<PlayerId> = self.linked(id).into_iter().filter(|p| waiting(p)).collect();
            unit.extend(self.partners_among(id, &partners));
        }
        unit
    }

    /// How long `id` has waited, shared with linked players waiting with them
    /// (the longest of their waits).
    pub fn shared_wait_ms(&self, id: PlayerId, now: i64) -> i64 {
        self.waiting_unit(id).into_iter().map(|p| self.waiting_ms(p, now)).max().unwrap_or(0)
    }

    /// Players eligible for automatic placement, best candidate first.
    fn candidates(&self, now: i64) -> Vec<PlayerId> {
        let mut ids: Vec<PlayerId> = self
            .session
            .present
            .iter()
            .filter(|(id, p)| {
                !p.resting && !self.session.skip_round.contains(id) && !self.is_assigned(**id)
            })
            .map(|(id, _)| *id)
            .collect();
        self.sort_by_priority(&mut ids, now);
        ids
    }

    // ----- Structure -------------------------------------------------------

    /// Keep the court and deck vectors matched to the configured court count.
    /// Anyone on a removed court goes back to the pool.
    pub fn sync_court_count(&mut self, now: i64) {
        let n = self.settings.courts.clamp(1, Settings::MAX_COURTS);
        self.settings.courts = n;
        let s = &mut self.session;
        while s.courts.len() > n {
            for id in s.courts.pop().unwrap().into_iter().flatten() {
                if let Some(p) = s.present.get_mut(&id) {
                    p.last_played_ms = Some(now);
                }
            }
        }
        s.courts.resize(n, EMPTY_SLOTS);
        s.deck.truncate(n);
        s.deck.resize(n, EMPTY_SLOTS);
        s.court_started_ms.resize(n, None);
        s.court_started_ms.truncate(n);
        for c in 0..n {
            if s.courts[c].iter().all(Option::is_none) {
                s.court_started_ms[c] = None;
            }
        }
        // Drop references to players who are no longer in the roster or present.
        let present: Vec<PlayerId> = s.present.keys().copied().collect();
        for slots in s.courts.iter_mut().chain(s.deck.iter_mut()) {
            for slot in slots.iter_mut() {
                if slot.is_some_and(|id| !present.contains(&id)) {
                    *slot = None;
                }
            }
        }
        self.fill_deck(now);
    }

    // ----- Attendance ------------------------------------------------------

    /// Make `id` count as having waited `wait_ms` as of `now` (the queue
    /// re-sorts by it).
    pub fn set_wait(&mut self, id: PlayerId, wait_ms: i64, now: i64) {
        let Some(p) = self.session.present.get_mut(&id) else { return };
        let since = now - wait_ms.max(0);
        match p.last_played_ms {
            Some(_) => {
                p.last_played_ms = Some(since);
                p.checked_in_ms = p.checked_in_ms.min(since);
            }
            None => p.checked_in_ms = since,
        }
        self.fill_deck(now);
    }

    pub fn check_in(&mut self, id: PlayerId, now: i64) {
        if !self.players.contains_key(&id) {
            return;
        }
        self.ensure_present(id, now);
        self.fill_deck(now);
    }

    fn ensure_present(&mut self, id: PlayerId, now: i64) {
        if self.session.present.contains_key(&id) {
            return;
        }
        self.session.present.insert(id, Presence::new(now));
        self.session.attendance.push(Attendance {
            player: id,
            in_ms: now,
            out_ms: None,
        });
    }

    fn close_attendance(&mut self, id: PlayerId, now: i64) {
        for a in &mut self.session.attendance {
            if a.player == id && a.out_ms.is_none() {
                a.out_ms = Some(now);
            }
        }
    }

    pub fn check_out(&mut self, id: PlayerId, now: i64) {
        if let Some(spot) = self.session.locate(id) {
            *self.session.slot_mut(spot) = None;
            self.clear_court_start_if_empty(spot);
        }
        if self.session.present.remove(&id).is_some() {
            self.close_attendance(id, now);
        }
        self.session.skip_round.remove(&id);
        self.fill_deck(now);
    }

    pub fn set_resting(&mut self, id: PlayerId, resting: bool, now: i64) {
        if resting {
            if let Some(spot) = self.session.locate(id) {
                self.leave_spot(id, spot, now);
            }
        }
        if let Some(p) = self.session.present.get_mut(&id) {
            p.resting = resting;
        }
        self.fill_deck(now);
    }

    pub fn delete_player(&mut self, id: PlayerId, now: i64) {
        self.check_out(id, now);
        self.unlink(id);
        self.session.practice.remove(&id);
        self.players.remove(&id);
    }

    /// Link `id` with `partners` (none: unlink them), then re-form any Up next
    /// group that splits the new link, so they're placed together right away.
    pub fn link_players(&mut self, id: PlayerId, partners: &[PlayerId], now: i64) {
        self.set_partners(id, partners);
        let members: Vec<PlayerId> = std::iter::once(id).chain(self.linked(id)).collect();
        let free: Vec<PlayerId> = self.candidates(now);
        let mut groups: Vec<usize> = members
            .iter()
            .filter_map(|m| match self.session.locate(*m) {
                Some(Spot::Deck(g, _)) => Some(g),
                _ => None,
            })
            .collect();
        groups.sort_unstable();
        groups.dedup();
        let split = groups.len() > 1 || (groups.len() == 1 && members.iter().any(|m| free.contains(m)));
        if split {
            for g in groups {
                self.session.deck[g] = EMPTY_SLOTS;
            }
            self.fill_deck(now);
        }
    }

    /// Set tonight's practice group. Up next groups that now mix the two
    /// sides are formed again.
    pub fn set_practice(&mut self, ids: std::collections::BTreeSet<PlayerId>, now: i64) {
        self.session.practice = ids;
        let practice = &self.session.practice;
        for g in &mut self.session.deck {
            let inside = g.iter().flatten().filter(|id| practice.contains(id)).count();
            if inside > 0 && inside < g.iter().flatten().count() {
                *g = EMPTY_SLOTS;
            }
        }
        self.fill_deck(now);
    }

    /// Wrap up the current game day: games in progress count as finished
    /// and everyone is checked out. Call before saving and `new_day`.
    pub fn end_day(&mut self, now: i64) {
        for c in 0..self.session.courts.len() {
            self.record_game(c, now);
        }
        for a in &mut self.session.attendance {
            if a.out_ms.is_none() {
                a.out_ms = Some(now);
            }
        }
        if !self.session.attendance.is_empty() {
            self.session.day_ended_ms = Some(now);
        }
    }

    /// Start a fresh game day: the courts are empty and only players set to
    /// check in automatically are here. The roster carries over. Its clock
    /// reads `start_ms` and starts running at wall time `wall`.
    pub fn new_day(&mut self, start_ms: i64, wall: i64) {
        self.session = Session::default();
        self.session.clock = SessionClock::starting(start_ms, wall);
        self.sync_court_count(start_ms);
        let regulars: Vec<PlayerId> = self.players.values().filter(|p| p.auto_check_in).map(|p| p.id).collect();
        for id in regulars {
            self.check_in(id, start_ms);
        }
    }

    /// Whether the game on court `c` already ended when the timer ran out
    /// (its players are still there until Next round).
    pub fn game_over(&self, c: usize) -> bool {
        let started = self.session.court_started_ms.get(c).copied().flatten();
        matches!((started, self.session.round_closed_ms), (Some(s), Some(closed)) if s <= closed)
    }

    /// The timer has run out: every game on court ends now, as at Next round
    /// (logged, and their players' waits start again from the moment it hit
    /// zero), so Next round won't do it again. Once per run of the timer.
    /// Returns whether anything changed.
    pub fn close_round_if_time_up(&mut self, now: i64) -> bool {
        let Timer::Running { end_ms } = self.session.timer else { return false };
        if now < end_ms || self.session.round_closed_ms.is_some_and(|c| c >= end_ms) {
            return false;
        }
        for c in 0..self.session.courts.len() {
            if self.game_over(c) {
                continue;
            }
            self.record_game(c, end_ms);
            for id in self.session.courts[c].into_iter().flatten() {
                self.mark_off_court(id, end_ms);
            }
        }
        self.session.round_closed_ms = Some(end_ms);
        true
    }

    /// Log the game currently on court `c` as finished at `now`.
    fn record_game(&mut self, c: usize, now: i64) {
        let players = self.session.courts[c];
        if players.iter().all(Option::is_none) || self.game_over(c) {
            return;
        }
        let Some(started_ms) = self.session.court_started_ms[c] else { return };
        self.session.games.push(GameRecord {
            round: self.session.round_number,
            court: c,
            started_ms,
            ended_ms: now,
            players,
        });
    }

    // ----- Moving people ---------------------------------------------------

    fn mark_off_court(&mut self, id: PlayerId, now: i64) {
        if let Some(p) = self.session.present.get_mut(&id) {
            p.last_played_ms = Some(now);
        }
    }

    /// A player leaves court `c` before its game has ended (say, they never
    /// showed up and were replaced). A game ends at Next round or when the
    /// timer runs out, so it's as if they never went on: their wait keeps
    /// counting from before, so they're still near the front of the line, and
    /// the game isn't counted. (After the game ended, it stays counted.)
    fn leave_court(&mut self, id: PlayerId, c: usize, _now: i64) {
        if self.game_over(c) {
            return;
        }
        if let Some(p) = self.session.present.get_mut(&id) {
            p.games_tonight = p.games_tonight.saturating_sub(1);
        }
        if let Some(p) = self.players.get_mut(&id) {
            p.lifetime_games = p.lifetime_games.saturating_sub(1);
        }
    }

    fn mark_on_court(&mut self, id: PlayerId) {
        if let Some(p) = self.session.present.get_mut(&id) {
            p.games_tonight += 1;
        }
        if let Some(p) = self.players.get_mut(&id) {
            p.lifetime_games += 1;
        }
    }

    fn clear_court_start_if_empty(&mut self, spot: Spot) {
        if let Spot::Court(c, _) = spot {
            if self.session.courts[c].iter().all(Option::is_none) {
                self.session.court_started_ms[c] = None;
            }
        }
    }

    /// Remove a player from a slot back to the pool, recording why.
    fn leave_spot(&mut self, id: PlayerId, spot: Spot, now: i64) {
        *self.session.slot_mut(spot) = None;
        match spot {
            Spot::Court(c, _) => self.leave_court(id, c, now),
            Spot::Deck(..) => {
                self.session.skip_round.insert(id);
            }
        }
        self.clear_court_start_if_empty(spot);
    }

    /// Bookkeeping when a player arrives at `to` having come `from`.
    fn arrive(&mut self, id: PlayerId, from: Option<Spot>, to: Spot, now: i64) {
        let was_on_court = matches!(from, Some(Spot::Court(..)));
        match to {
            Spot::Court(c, _) => {
                // Joining a game that's already over doesn't count as one.
                if !was_on_court && !self.game_over(c) {
                    self.mark_on_court(id);
                }
                if self.session.court_started_ms[c].is_none() {
                    self.session.court_started_ms[c] = Some(now);
                }
            }
            Spot::Deck(..) => {
                if let Some(Spot::Court(c, _)) = from {
                    self.leave_court(id, c, now);
                }
            }
        }
        self.session.skip_round.remove(&id);
    }

    /// Drag-and-drop: move `id` to `target`, swapping with whoever is there.
    pub fn move_player(&mut self, id: PlayerId, target: Target, now: i64) {
        if !self.players.contains_key(&id) {
            return;
        }
        self.ensure_present(id, now);
        let from = self.session.locate(id);
        match target {
            Target::Pool => {
                if let Some(spot) = from {
                    self.leave_spot(id, spot, now);
                }
            }
            Target::Slot(to) => {
                if from == Some(to) {
                    return;
                }
                let occupant = self.session.slot(to);
                if let Some(f) = from {
                    *self.session.slot_mut(f) = None;
                }
                *self.session.slot_mut(to) = Some(id);
                self.arrive(id, from, to, now);

                if let Some(q) = occupant {
                    match from {
                        // Swap: the occupant takes the dragged player's old place.
                        Some(f) => {
                            *self.session.slot_mut(f) = Some(q);
                            self.arrive(q, Some(to), f, now);
                        }
                        // Bumped back to the pool.
                        None => match to {
                            Spot::Court(c, _) => self.leave_court(q, c, now),
                            Spot::Deck(..) => {
                                self.session.skip_round.insert(q);
                            }
                        },
                    }
                }
                if let Some(f) = from {
                    self.clear_court_start_if_empty(f);
                }
            }
        }
        self.fill_deck(now);
    }

    // ----- Automatic placement --------------------------------------------

    /// Order in which groups get filled: courts that have been on longest first.
    fn court_priority(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.session.courts.len()).collect();
        order.sort_by_key(|&c| (self.session.court_started_ms[c].unwrap_or(i64::MIN), c));
        order
    }

    /// Fill every empty on-deck slot from the waiting pool.
    pub fn fill_deck(&mut self, now: i64) {
        let order = self.court_priority();
        let mut groups = std::mem::take(&mut self.session.deck);
        self.fill_groups(&mut groups, &order, now);
        self.session.deck = groups;
    }

    /// The candidates as units to place: linked players who are free together
    /// (and on the same side of the practice split) form one unit. In line
    /// order, which keeps a unit's members together.
    fn units(&self, pool: &[PlayerId]) -> Vec<Unit> {
        let mut used = std::collections::HashSet::new();
        let mut units = Vec::new();
        for &id in pool {
            if !used.insert(id) {
                continue;
            }
            let mut ids = vec![id];
            for p in self.partners_among(id, pool) {
                if ids.len() < Self::MAX_LINKED && used.insert(p) {
                    ids.push(p);
                }
            }
            units.push(Unit { ids, practice: self.in_practice(id) });
        }
        units
    }

    fn unit_rank(&self, unit: &Unit) -> f32 {
        unit.ids.iter().map(|id| self.skill(*id).rank() as f32).sum::<f32>() / unit.ids.len() as f32
    }

    /// Games per hour since checking in, averaged over one side's players,
    /// counting `extra` places already promised to that side as played.
    fn play_rate(&self, practice: bool, extra: usize, now: i64) -> Option<f64> {
        let hours: Vec<(u32, f64)> = self
            .session
            .present
            .iter()
            .filter(|(id, p)| self.in_practice(**id) == practice && !p.resting)
            // Everyone counts as here at least a round, so a fresh arrival
            // doesn't look starved.
            .map(|(_, p)| (p.games_tonight, (now - p.checked_in_ms).max(self.settings.round_ms()) as f64 / 3_600_000.0))
            .collect();
        let n = hours.len() as f64;
        (n > 0.0).then(|| hours.iter().map(|(g, h)| (*g as f64 + extra as f64 / n) / h).sum::<f64>() / n)
    }

    /// Which side gets an empty group needing `need` players, given the places
    /// already promised to each side this time (`taken`, general then practice).
    ///
    /// Only a side that can fill it gets it (unless neither can). Players on
    /// court count towards that: at the next round they come off before any
    /// short group is topped up. Then the side that has played less tonight
    /// gets it, so time on court evens out across everyone; when that's close,
    /// whichever side's next players have waited longer. `None`: nobody's free.
    fn choose_side(&self, units: &[Unit], need: usize, taken: [usize; 2], now: i64) -> Option<bool> {
        struct Side {
            fills: bool,
            any: bool,
            rate: f64,
            wait: f64,
        }
        let side = |practice: bool| {
            let t = taken[practice as usize];
            let all: Vec<PlayerId> = units.iter().filter(|u| u.practice == practice).flat_map(|u| u.ids.iter().copied()).collect();
            // Places promised beyond who's free come from those coming off court.
            let borrowed = t.saturating_sub(all.len());
            let ids = &all[t.min(all.len())..];
            let next = &ids[..ids.len().min(need)];
            let wait = next.iter().map(|id| self.waiting_ms(*id, now)).sum::<i64>() as f64 / next.len().max(1) as f64;
            let on_court = self.session.courts.iter().flatten().flatten().filter(|id| self.in_practice(**id) == practice).count();
            let coming_off = on_court.saturating_sub(borrowed);
            Side { fills: ids.len() + coming_off >= need, any: !ids.is_empty(), rate: self.play_rate(practice, t, now).unwrap_or(0.0), wait }
        };
        let (general, practice) = (side(false), side(true));
        match (general.any, practice.any) {
            (false, false) => return None,
            (true, false) => return Some(false),
            (false, true) => return Some(true),
            _ => {}
        }
        if general.fills != practice.fills {
            return Some(practice.fills);
        }
        // Within a few hundredths of a game an hour it's a tie, settled by
        // who's waited longer (a wider margin lets the smaller side drift
        // behind, since the bigger side nearly always has someone who's
        // waited longer).
        if (general.rate - practice.rate).abs() > 0.02 {
            return Some(practice.rate < general.rate);
        }
        Some(practice.wait > general.wait)
    }

    /// A group belongs to the practice side if everyone in it is practising.
    pub fn is_practice_group(&self, group: &Slots) -> bool {
        let mut members = group.iter().flatten().peekable();
        members.peek().is_some() && members.all(|id| self.in_practice(*id))
    }

    /// Fill the empty slots of `groups` (visited in `order`): the next players
    /// in line, as many as there are open spots, arranged by skill.
    fn fill_groups(&self, groups: &mut [Slots], order: &[usize], now: i64) {
        // Players in `groups` were temporarily taken out of the session, so
        // exclude them explicitly.
        let in_groups: Vec<PlayerId> = groups.iter().flatten().flatten().copied().collect();
        let pool: Vec<PlayerId> = self.candidates(now).into_iter().filter(|id| !in_groups.contains(id)).collect();
        self.fill_groups_from(groups, order, &pool, now);
    }

    /// `fill_groups` from a given line of players (best first).
    fn fill_groups_from(&self, groups: &mut [Slots], order: &[usize], pool: &[PlayerId], now: i64) {
        let units = self.units(pool);

        // Which side each group to fill belongs to, and how many it needs.
        struct Target {
            g: usize,
            practice: bool,
            /// Empty to start with (so free to arrange by skill).
            empty: bool,
            free: usize,
            placed: Vec<usize>,
        }
        let mut targets: Vec<Target> = Vec::new();
        let mut taken = [0usize; 2];
        for &g in order {
            let free = groups[g].iter().filter(|s| s.is_none()).count();
            if free == 0 {
                continue;
            }
            let empty = free == 4;
            let practice = if empty {
                match self.choose_side(&units, free, taken, now) {
                    Some(side) => side,
                    None => break,
                }
            } else {
                self.is_practice_group(&groups[g])
            };
            taken[practice as usize] += free;
            targets.push(Target { g, practice, empty, free, placed: Vec::new() });
        }

        // The next units in line take the open spots, each where it fits. A
        // unit that doesn't fit (linked players the cut-off would split) is
        // held back for next time, and whoever's next goes instead.
        for (i, unit) in units.iter().enumerate() {
            let size = unit.ids.len();
            if let Some(t) = targets.iter_mut().find(|t| t.practice == unit.practice && t.free >= size) {
                t.free -= size;
                t.placed.push(i);
            }
        }

        // Arrange the groups that started empty by skill: on each side, the
        // chosen players sorted by level fill those courts in turn, so similar
        // levels play together. (If the linked units won't pack that way, the
        // order they were chosen in is kept.)
        for practice in [false, true] {
            let slots: Vec<usize> = (0..targets.len()).filter(|&t| targets[t].practice == practice && targets[t].empty).collect();
            let mut chosen: Vec<usize> = slots.iter().flat_map(|&t| targets[t].placed.iter().copied()).collect();
            chosen.sort_by(|&a, &b| self.unit_rank(&units[b]).total_cmp(&self.unit_rank(&units[a])).then(a.cmp(&b)));
            let mut room: Vec<usize> = slots.iter().map(|&t| 4 - targets[t].free).collect();
            let mut packed: Vec<Vec<usize>> = vec![Vec::new(); slots.len()];
            let mut ok = true;
            for &u in &chosen {
                let size = units[u].ids.len();
                // The first group with room: in level order, that's the one
                // filling up now, or a later one for a unit too big for it.
                match (0..slots.len()).find(|&k| room[k] >= size) {
                    Some(k) => {
                        room[k] -= size;
                        packed[k].push(u);
                    }
                    None => ok = false,
                }
            }
            if ok {
                for (k, &t) in slots.iter().enumerate() {
                    targets[t].placed = std::mem::take(&mut packed[k]);
                }
            }
        }

        for t in &targets {
            if t.placed.is_empty() {
                continue;
            }
            let group = &mut groups[t.g];
            for &u in &t.placed {
                for &id in &units[u].ids {
                    if let Some(slot) = group.iter().position(Option::is_none) {
                        group[slot] = Some(id);
                    }
                }
            }
            self.balance_teams(group);
        }
    }

    /// Arrange a full group so the strongest and weakest partner up against the
    /// middle two, for an even game.
    fn balance_teams(&self, group: &mut Slots) {
        if group.iter().any(Option::is_none) {
            return;
        }
        let mut ids: Vec<PlayerId> = group.iter().flatten().copied().collect();
        ids.sort_by_key(|id| std::cmp::Reverse(self.skill(*id).rank()));
        *group = [Some(ids[0]), Some(ids[3]), Some(ids[1]), Some(ids[2])];
    }

    // ----- Rounds ----------------------------------------------------------

    /// Send on-deck group `c` onto court `c`, topping it up if incomplete.
    /// Returns false if there was nobody on deck for that court.
    pub fn promote_deck(&mut self, c: usize, now: i64) -> bool {
        let moved = self.swap_in_deck(c, now);
        if moved {
            self.top_up(c, now);
        }
        moved
    }

    /// Group `c` walks on and the players on court `c` come off.
    fn swap_in_deck(&mut self, c: usize, now: i64) -> bool {
        if self.session.deck[c].iter().all(Option::is_none) {
            return false;
        }
        // Unless the timer already ended this game (and restarted their waits).
        if !self.game_over(c) {
            self.record_game(c, now);
            for id in self.session.courts[c].into_iter().flatten() {
                self.mark_off_court(id, now);
            }
        }
        self.session.courts[c] = std::mem::replace(&mut self.session.deck[c], EMPTY_SLOTS);
        self.session.court_started_ms[c] = Some(now);
        true
    }

    /// Complete a short group that just walked on, from the pool (which now
    /// includes the players who just came off), and count their games.
    fn top_up(&mut self, c: usize, now: i64) {
        if self.session.courts[c].iter().any(Option::is_none) {
            let mut courts = std::mem::take(&mut self.session.courts);
            self.fill_groups(&mut courts, &[c], now);
            self.session.courts = courts;
        }
        for id in self.session.courts[c].into_iter().flatten() {
            self.mark_on_court(id);
        }
    }

    /// The big button: everyone on deck takes the courts, the deck refills.
    /// Courts with nobody on deck keep playing.
    pub fn next_round(&mut self, now: i64) -> bool {
        // Everyone due off comes off first, then short groups are topped up
        // in court-priority order, so top-ups favour the right people.
        let order = self.court_priority();
        let moved: Vec<usize> = order.iter().copied().filter(|&c| self.swap_in_deck(c, now)).collect();
        let any = !moved.is_empty();
        for &c in &moved {
            self.top_up(c, now);
        }
        // A court with nobody up next keeps playing: if the timer ended its
        // game, this is a new one.
        for c in 0..self.session.courts.len() {
            if !moved.contains(&c) && self.game_over(c) && self.session.courts[c].iter().any(Option::is_some) {
                self.session.court_started_ms[c] = Some(now);
                for id in self.session.courts[c].into_iter().flatten() {
                    self.mark_on_court(id);
                }
            }
        }
        if any {
            self.session.round_number += 1;
        }
        self.session.skip_round.clear();
        self.fill_deck(now);
        self.session.timer = if self.settings.auto_start_timer && any {
            Timer::Running {
                end_ms: now + self.settings.round_ms(),
            }
        } else {
            Timer::Idle
        };
        any
    }

    // ----- Timer -----------------------------------------------------------

    pub fn timer_remaining_ms(&self, now: i64) -> i64 {
        match self.session.timer {
            Timer::Idle => self.settings.round_ms(),
            Timer::Running { end_ms } => (end_ms - now).max(0),
            Timer::Paused { remaining_ms } => remaining_ms,
        }
    }

    pub fn timer_finished(&self, now: i64) -> bool {
        matches!(self.session.timer, Timer::Running { end_ms } if now >= end_ms)
    }

    pub fn timer_toggle(&mut self, now: i64) {
        self.session.timer = match self.session.timer {
            Timer::Idle => Timer::Running {
                end_ms: now + self.settings.round_ms(),
            },
            Timer::Running { end_ms } if now >= end_ms => Timer::Running {
                end_ms: now + self.settings.round_ms(),
            },
            Timer::Running { end_ms } => Timer::Paused {
                remaining_ms: end_ms - now,
            },
            Timer::Paused { remaining_ms } => Timer::Running {
                end_ms: now + remaining_ms,
            },
        };
    }

    pub fn timer_reset(&mut self) {
        self.session.timer = Timer::Idle;
    }

    /// Nudge the running/paused clock by `delta_ms` (e.g. +1 minute).
    pub fn timer_adjust(&mut self, delta_ms: i64, now: i64) {
        self.session.timer = match self.session.timer {
            Timer::Idle => Timer::Paused {
                remaining_ms: (self.settings.round_ms() + delta_ms).max(0),
            },
            Timer::Running { end_ms } => Timer::Running {
                end_ms: (end_ms + delta_ms).max(now),
            },
            Timer::Paused { remaining_ms } => Timer::Paused {
                remaining_ms: (remaining_ms + delta_ms).max(0),
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn setup(skills: &[Skill], courts: usize) -> (AppState, Vec<PlayerId>) {
        let mut s = AppState::default();
        s.settings.courts = courts;
        s.sync_court_count(0);
        let ids: Vec<PlayerId> = skills
            .iter()
            .enumerate()
            .map(|(i, sk)| s.add_player(&format!("P{i}"), *sk))
            .collect();
        for (i, id) in ids.iter().enumerate() {
            // Everyone arrives one second apart, then the deck fills once.
            s.session.present.insert(*id, Presence::new(i as i64 * 1000));
        }
        s.fill_deck(20_000);
        (s, ids)
    }

    fn all_assigned(s: &AppState) -> Vec<PlayerId> {
        s.session
            .courts
            .iter()
            .chain(s.session.deck.iter())
            .flatten()
            .flatten()
            .copied()
            .collect()
    }

    #[test]
    fn deck_autofills_in_arrival_order() {
        let (s, ids) = setup(&[Skill::Beginner2; 14], 3);
        let deck: Vec<_> = s.session.deck.iter().flatten().flatten().copied().collect();
        assert_eq!(deck.len(), 12);
        // The last two arrivals are waiting.
        assert_eq!(s.waiting_list(20_000), vec![ids[12], ids[13]]);
    }

    #[test]
    fn rotation_moves_deck_to_courts_and_refills() {
        let (mut s, _) = setup(&[Skill::Beginner2; 20], 3);
        assert!(s.next_round(MIN));
        assert_eq!(s.session.courts.iter().flatten().flatten().count(), 12);
        assert_eq!(s.session.deck.iter().flatten().flatten().count(), 8);
        let mut a = all_assigned(&s);
        a.sort();
        a.dedup();
        assert_eq!(a.len(), 20, "no one is in two places");
    }

    #[test]
    fn short_handed_courts_take_turns() {
        // 20 players, 3 courts: one court must keep playing each round, but it
        // should never be the same court twice in a row.
        let (mut s, _) = setup(&[Skill::Beginner2; 20], 3);
        s.next_round(MIN); // everyone from deck onto courts, 8 now on deck
        let mut kept = Vec::new();
        for round in 2..8 {
            let before = s.session.courts.clone();
            s.next_round(round * 15 * MIN);
            let k: Vec<usize> = (0..3).filter(|c| s.session.courts[*c] == before[*c]).collect();
            assert_eq!(k.len(), 1);
            kept.push(k[0]);
        }
        for w in kept.windows(2) {
            assert_ne!(w[0], w[1], "a court stayed on twice in a row: {kept:?}");
        }
    }

    #[test]
    fn longest_waiting_go_first() {
        let (mut s, ids) = setup(&[Skill::Beginner2; 8], 1);
        s.next_round(MIN); // P0..P3 on court, P4..P7 on deck
        s.next_round(16 * MIN); // P4..P7 on court, P0..P3 come off and go on deck
        let deck: Vec<_> = s.session.deck[0].iter().flatten().copied().collect();
        for id in &ids[0..4] {
            assert!(deck.contains(id));
        }
    }

    #[test]
    fn similar_levels_share_a_court() {
        use Skill::*;
        // The next eight in line alternate levels; arranged, each court is even.
        let (s, _) = setup(&[Beginner1, Advanced, Beginner1, Advanced, Beginner1, Advanced, Beginner1, Advanced], 2);
        for g in &s.session.deck {
            let skills: Vec<Skill> = g.iter().flatten().map(|id| s.skill(*id)).collect();
            assert!(skills.iter().all(|k| *k == skills[0]), "mixed group: {skills:?}");
        }
    }

    #[test]
    fn next_in_line_play_whatever_their_level() {
        use Skill::*;
        let mut s = AppState::default();
        s.settings.courts = 1;
        s.sync_court_count(0);
        let now = 60 * MIN;
        // Four have waited longest; a beginner who arrived later is a closer
        // match for three of them but isn't next, so doesn't play.
        let first: Vec<_> = [Beginner1, Advanced, Beginner1, Beginner1].iter().enumerate().map(|(i, k)| s.add_player(&format!("W{i}"), *k)).collect();
        let late = s.add_player("Late", Beginner1);
        for (i, id) in first.iter().enumerate() {
            s.session.present.insert(*id, Presence::new(now - (30 - i as i64) * MIN));
        }
        s.session.present.insert(late, Presence::new(now - MIN));
        s.fill_deck(now);
        let mut deck: Vec<_> = s.session.deck[0].iter().flatten().copied().collect();
        deck.sort();
        assert_eq!(deck, first);
    }

    #[test]
    fn linked_players_split_by_the_cut_off_wait_a_round() {
        let mut s = AppState::default();
        s.settings.courts = 1;
        s.sync_court_count(0);
        let ids: Vec<_> = (0..6).map(|i| s.add_player(&format!("P{i}"), Skill::Beginner2)).collect();
        for (i, id) in ids.iter().enumerate() {
            s.session.present.insert(*id, Presence::new(i as i64 * MIN));
        }
        // P3 and P4 are linked: they share P3's place, 4th and 5th in line, so
        // one court's cut-off would split them. They wait; P5 goes instead.
        s.link_players(ids[3], &[ids[4]], 10 * MIN);
        s.fill_deck(10 * MIN);
        assert_eq!(s.waiting_list(10 * MIN), vec![ids[3], ids[4]], "held back, together");
        assert_eq!(s.shared_wait_ms(ids[4], 10 * MIN), s.waiting_ms(ids[3], 10 * MIN), "they share the longer wait");
        let mut deck: Vec<_> = s.session.deck[0].iter().flatten().copied().collect();
        deck.sort();
        assert_eq!(deck, vec![ids[0], ids[1], ids[2], ids[5]]);
        // Next round they're at the front, and go on together.
        s.next_round(11 * MIN);
        assert!(s.session.deck[0].contains(&Some(ids[3])) && s.session.deck[0].contains(&Some(ids[4])));
    }

    #[test]
    fn dragging_off_deck_does_not_bounce_back() {
        let (mut s, _) = setup(&[Skill::Beginner2; 6], 1);
        let first = s.session.deck[0][0].unwrap();
        s.move_player(first, Target::Pool, 2 * MIN);
        assert!(!s.session.deck[0].contains(&Some(first)));
        assert_eq!(s.session.deck[0].iter().flatten().count(), 4);
        // After the next rotation they're eligible again.
        s.next_round(3 * MIN);
        assert!(s.session.deck[0].contains(&Some(first)));
    }

    #[test]
    fn swap_between_court_and_deck() {
        let (mut s, _) = setup(&[Skill::Beginner2; 8], 1);
        s.next_round(MIN);
        let on = s.session.courts[0][0].unwrap();
        let dk = s.session.deck[0][2].unwrap();
        s.move_player(dk, Target::Slot(Spot::Court(0, 0)), 2 * MIN);
        assert_eq!(s.session.courts[0][0], Some(dk));
        assert_eq!(s.session.deck[0][2], Some(on));
        // Taken off a minute into the game: it doesn't count, and their wait isn't reset.
        assert_eq!(s.session.present[&on].last_played_ms, None);
        assert_eq!(s.session.present[&on].games_tonight, 0);
        assert_eq!(s.session.present[&dk].games_tonight, 1);
    }

    #[test]
    fn only_next_round_resets_a_wait() {
        let (mut s, _) = setup(&[Skill::Beginner2; 8], 1);
        let round = s.settings.round_ms();
        s.next_round(MIN);
        s.next_round(MIN + round); // second group on; first group came off at MIN + round
        let a = s.session.courts[0][0].unwrap();
        let b = s.session.courts[0][1].unwrap();
        let before = s.session.present[&a].clone();
        assert_eq!(before.games_tonight, 1);
        let lifetime = s.players[&a].lifetime_games;

        // Sent to waiting mid-game: wait keeps counting from before, game not counted.
        let t = MIN + round + 5 * MIN;
        s.move_player(a, Target::Pool, t);
        let p = &s.session.present[&a];
        assert_eq!(p.last_played_ms, before.last_played_ms, "wait not reset");
        assert_eq!(p.games_tonight, 0, "unfinished game not counted");
        assert_eq!(s.players[&a].lifetime_games, lifetime - 1);

        // Even long after the round's time is up, until Next round.
        let before = s.session.present[&b].clone();
        s.move_player(b, Target::Pool, MIN + 3 * round);
        assert_eq!(s.session.present[&b].last_played_ms, before.last_played_ms);

        // Whoever's on court at Next round has played.
        let c = s.session.courts[0][2].unwrap();
        s.next_round(MIN + 4 * round);
        assert_eq!(s.session.present[&c].last_played_ms, Some(MIN + 4 * round));
    }

    #[test]
    fn incomplete_deck_group_is_topped_up_on_rotation() {
        let (mut s, _) = setup(&[Skill::Beginner2; 6], 1);
        s.next_round(MIN); // 4 on court, 2 on deck
        assert_eq!(s.session.deck[0].iter().flatten().count(), 2);
        s.next_round(16 * MIN);
        assert_eq!(s.session.courts[0].iter().flatten().count(), 4);
    }

    #[test]
    fn history_records_attendance_and_games() {
        let (mut s, ids) = setup(&[Skill::Beginner2; 5], 1);
        // `setup` bypasses check-in; check one more player in properly.
        let late = s.add_player("Late", Skill::Advanced);
        s.check_in(late, MIN);
        assert_eq!(s.session.attendance.len(), 1);
        s.next_round(2 * MIN); // first game starts
        assert!(s.session.games.is_empty());
        s.next_round(17 * MIN); // first game ends
        assert_eq!(s.session.games.len(), 1);
        let g = &s.session.games[0];
        assert_eq!((g.started_ms, g.ended_ms, g.court), (2 * MIN, 17 * MIN, 0));
        s.check_out(late, 20 * MIN);
        assert_eq!(s.session.attendance[0].out_ms, Some(20 * MIN));
        s.end_day(30 * MIN);
        assert_eq!(s.session.games.len(), 2, "game in progress is logged at day end");
        assert_eq!(s.session.day_ended_ms, Some(30 * MIN));
        s.new_day(7 * 86_400_000, 0);
        assert!(s.session.present.is_empty() && s.session.games.is_empty());
        assert!(s.players.contains_key(&ids[0]), "roster carries over");
        // Regulars are checked in to every new session from its start.
        s.players.get_mut(&late).unwrap().auto_check_in = true;
        s.new_day(8 * 86_400_000, 0);
        assert_eq!(s.session.present.keys().copied().collect::<Vec<_>>(), vec![late]);
        assert_eq!(s.session.attendance[0].in_ms, 8 * 86_400_000);
    }

    fn group_of(s: &AppState, id: PlayerId) -> Option<(bool, usize)> {
        match s.session.locate(id)? {
            Spot::Court(c, _) => Some((true, c)),
            Spot::Deck(c, _) => Some((false, c)),
        }
    }

    #[test]
    fn linked_players_share_a_court() {
        // P0 arrived first and P9 last; linked, they still go on together.
        let (mut s, ids) = setup(&[Skill::Beginner2; 10], 2);
        s.link_players(ids[0], &[ids[9]], 20_000);
        assert!(group_of(&s, ids[0]).is_some());
        assert_eq!(group_of(&s, ids[0]), group_of(&s, ids[9]));
        // Through several rounds, whenever they're both placed it's together.
        for round in 1..8 {
            s.next_round(round * 15 * MIN);
            if let (Some(a), Some(b)) = (group_of(&s, ids[0]), group_of(&s, ids[9])) {
                assert_eq!(a, b, "split up in round {round}");
            }
        }
        // Unlinked, they're on their own again.
        s.unlink(ids[0]);
        assert!(s.linked(ids[9]).is_empty());
    }

    #[test]
    fn linked_trio_never_starves() {
        let (mut s, ids) = setup(&[Skill::Beginner2; 11], 2);
        s.set_partners(ids[8], &[ids[9], ids[10]]);
        let mut played = 0;
        for round in 1..10 {
            s.next_round(round * 15 * MIN);
            let on: Vec<_> = (8..11).map(|i| group_of(&s, ids[i])).collect();
            if on[0].is_some_and(|(court, _)| court) {
                assert!(on.iter().all(|g| *g == on[0]));
                played += 1;
            }
        }
        assert!(played >= 3, "the trio played {played} of 9 rounds");
    }

    /// `n` players on `courts` courts for `rounds` rounds, `k` of them practising.
    /// Returns the games played by the practice side and everyone else.
    fn practice_night(n: usize, k: usize, courts: usize, rounds: i64) -> (Vec<u32>, Vec<u32>) {
        let skills: Vec<Skill> = (0..n).map(|i| Skill::ALL[i % 4]).collect();
        let (mut s, ids) = setup(&skills, courts);
        let practice: std::collections::BTreeSet<_> = ids.iter().copied().step_by(n / k).take(k).collect();
        s.set_practice(practice.clone(), 20_000);
        for round in 1..=rounds {
            s.next_round(round * 15 * MIN);
            if std::env::var("TRACE").is_ok() {
                let on = |gs: &Vec<Slots>| gs.iter().map(|g| g.iter().flatten().filter(|id| practice.contains(id)).count()).collect::<Vec<_>>();
                eprintln!("round {round}: practice on courts {:?}, up next {:?}", on(&s.session.courts), on(&s.session.deck));
            }
            for g in s.session.courts.iter().chain(&s.session.deck) {
                let sides: Vec<bool> = g.iter().flatten().map(|id| practice.contains(id)).collect();
                assert!(sides.iter().all(|x| *x == sides[0]), "mixed group in round {round}: {g:?}");
            }
        }
        let games = |inside: bool| ids.iter().filter(|id| practice.contains(id) == inside).map(|id| s.session.present[id].games_tonight).collect();
        (games(true), games(false))
    }

    #[test]
    fn practice_group_plays_only_each_other_and_gets_fair_time() {
        let mean = |v: &[u32]| v.iter().sum::<u32>() as f32 / v.len() as f32;
        // (players, practising, courts): a small night, and the 17-of-70 one.
        for (n, k, courts) in [(26, 7, 3), (70, 17, 6), (40, 9, 4), (30, 5, 3)] {
            let (inside, outside) = practice_night(n, k, courts, 24);
            let (a, b) = (mean(&inside), mean(&outside));
            eprintln!("{n} players, {k} practising, {courts} courts: practice {a:.2} games each, others {b:.2} · practice {inside:?}");
            assert!((a - b).abs() <= 0.6, "{n}/{k}/{courts}: practice averaged {a:.1} games, everyone else {b:.1}");
            let all: Vec<u32> = inside.iter().chain(&outside).copied().collect();
            assert!(all.iter().max().unwrap() - all.iter().min().unwrap() <= 2, "{n}/{k}/{courts} uneven: {all:?}");
        }
    }

    #[test]
    fn waiting_list_shows_the_real_picking_order() {
        let (mut s, ids) = setup(&[Skill::Beginner2; 10], 1);
        // P0–P3 are up next. Next in line: P4, P5, P6, then P7 and P8 linked —
        // but only one place is left for them, so they wait and P9 goes first.
        s.link_players(ids[7], &[ids[8]], 20_000);
        assert_eq!(s.waiting_list(20_000), vec![ids[4], ids[5], ids[6], ids[9], ids[7], ids[8]]);
        // And that's what happens.
        s.next_round(MIN);
        let mut deck: Vec<_> = s.session.deck[0].iter().flatten().copied().collect();
        deck.sort();
        assert_eq!(deck, vec![ids[4], ids[5], ids[6], ids[9]]);
    }

    #[test]
    fn no_show_taken_off_court_keeps_their_wait() {
        let (mut s, _) = setup(&[Skill::Beginner2; 14], 2);
        for r in 1..=3 {
            s.next_round(r * 15 * MIN);
        }
        let start = 3 * 15 * MIN;
        // Walked on with their group; two minutes in, they haven't shown.
        let absent = s.session.courts[0][0].unwrap();
        let waited = s.session.present[&absent].waiting_ms(start);
        s.move_player(absent, Target::Pool, start + 2 * MIN);
        assert_eq!(s.waiting_ms(absent, start + 2 * MIN), waited + 2 * MIN);
        // Their replacement, from Up next, plays to Next round: that counts.
        let sub = s.session.deck[1][0].unwrap();
        s.move_player(sub, Target::Slot(Spot::Court(0, 0)), start + 3 * MIN);
        s.next_round(start + 15 * MIN);
        assert_eq!(s.session.present[&sub].last_played_ms, Some(start + 15 * MIN));
    }

    #[test]
    fn time_up_ends_the_games_and_next_round_does_not_again() {
        let (mut s, ids) = setup(&[Skill::Beginner2; 10], 2);
        s.settings.auto_start_timer = true;
        let round = s.settings.round_ms();
        s.next_round(MIN);
        let on: Vec<PlayerId> = s.session.courts.iter().flatten().flatten().copied().collect();
        let end = MIN + round;
        assert!(!s.close_round_if_time_up(end - 1000));
        // Noticed a little after zero: the games end at zero.
        assert!(s.close_round_if_time_up(end + 2000));
        assert!(!s.close_round_if_time_up(end + 3000), "only once");
        assert_eq!(s.session.games.len(), 2);
        assert!(s.session.games.iter().all(|g| g.ended_ms == end));
        for id in &on {
            assert_eq!(s.session.present[id].last_played_ms, Some(end));
            assert_eq!(s.session.present[id].games_tonight, 1);
        }
        // Taken off after time's up: the game still counts.
        s.move_player(on[0], Target::Pool, end + MIN);
        assert_eq!(s.session.present[&on[0]].games_tonight, 1);

        // Next round three minutes later: nothing logged again, waits from zero.
        s.next_round(end + 3 * MIN);
        assert_eq!(s.session.games.len(), 2);
        for id in &on {
            assert_eq!(s.session.present[id].last_played_ms, Some(end), "{id}");
        }
        // Ten players, two courts: one court kept playing; that's a new game.
        let kept: Vec<PlayerId> = s.session.courts.iter().flatten().flatten().copied().filter(|id| on.contains(id)).collect();
        for id in &kept {
            assert_eq!(s.session.present[id].games_tonight, 2);
        }
        let _ = ids;

        // Next round before zero: it ends the games that change over, then
        // (a court kept on plays on, and is logged when it changes over).
        let t = end + 5 * MIN;
        s.next_round(t);
        assert!(s.session.games.len() > 2);
        assert!(s.session.games[2..].iter().all(|g| g.ended_ms == t));
    }

    #[test]
    fn timer_cycle() {
        let mut s = AppState::default();
        s.settings.round_minutes = 15;
        s.timer_toggle(0);
        assert_eq!(s.timer_remaining_ms(5 * MIN), 10 * MIN);
        s.timer_toggle(5 * MIN); // pause
        assert_eq!(s.timer_remaining_ms(9 * MIN), 10 * MIN);
        s.timer_toggle(9 * MIN); // resume
        assert!(s.timer_finished(19 * MIN));
        s.next_round(20 * MIN);
        assert_eq!(s.session.timer, Timer::Idle);
    }
}
