//! Rotation and matchmaking rules.
//!
//! * The waiting pool is ordered by wall-clock time since each player last came
//!   off a court (or arrived), longest first.
//! * Empty on-deck slots are always auto-filled from that pool. Each on-deck
//!   group is seeded with the longest-waiting player, then completed with the
//!   players closest in skill (ties go to whoever has waited longer).
//! * Anyone who has waited past `max_wait` is placed next regardless of skill,
//!   so a lone expert never gets stuck on the bench.
//! * Groups are filled for the courts that have been playing longest first, so
//!   when there aren't enough people for every court, the courts take turns.

use crate::model::*;

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

    /// Checked-in players who are neither on court nor on deck, longest wait first.
    pub fn waiting_list(&self, now: i64) -> Vec<PlayerId> {
        let mut ids: Vec<PlayerId> = self
            .session
            .present
            .keys()
            .copied()
            .filter(|id| !self.is_assigned(*id))
            .collect();
        self.sort_by_priority(&mut ids, now);
        ids
    }

    pub fn waiting_ms(&self, id: PlayerId, now: i64) -> i64 {
        self.session
            .present
            .get(&id)
            .map_or(0, |p| p.waiting_ms(now))
    }

    fn sort_by_priority(&self, ids: &mut [PlayerId], now: i64) {
        let key = |id: &PlayerId| {
            let p = &self.session.present[id];
            let wait = p.waiting_ms(now);
            // Waits within the same minute count as equal so that fewer games
            // tonight breaks the tie (e.g. a whole court that came off together).
            (
                std::cmp::Reverse(wait / 60_000),
                p.games_tonight,
                std::cmp::Reverse(wait),
                *id,
            )
        };
        ids.sort_by_key(key);
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
        self.players.remove(&id);
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

    /// Start a fresh game day: nobody is present and the courts are empty.
    /// The roster carries over.
    pub fn new_day(&mut self) {
        self.session = Session::default();
        self.sync_court_count(now_ms());
    }

    /// Log the game currently on court `c` as finished at `now`.
    fn record_game(&mut self, c: usize, now: i64) {
        let players = self.session.courts[c];
        if players.iter().all(Option::is_none) {
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

    /// A player leaves court `c` other than by rotation. If the game there has
    /// run its full length it counts as played; otherwise it's as if they never
    /// went on: their wait keeps counting from before and the game isn't counted.
    fn leave_court(&mut self, id: PlayerId, c: usize, now: i64) {
        let full_game = self.session.court_started_ms[c].is_some_and(|start| now - start >= self.settings.round_ms());
        if full_game {
            self.mark_off_court(id, now);
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
                if !was_on_court {
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

    /// Fill the empty slots of `groups` (visited in `order`) from the candidates.
    fn fill_groups(&self, groups: &mut [Slots], order: &[usize], now: i64) {
        // Players in `groups` were temporarily taken out of the session, so
        // exclude them explicitly.
        let in_groups: Vec<PlayerId> = groups.iter().flatten().flatten().copied().collect();
        let mut pool: Vec<PlayerId> = self
            .candidates(now)
            .into_iter()
            .filter(|id| !in_groups.contains(id))
            .collect();
        let max_wait = self.settings.max_wait_ms();

        for &g in order {
            if pool.is_empty() {
                break;
            }
            let group = &mut groups[g];
            let mut added = false;
            while let Some(slot) = group.iter().position(Option::is_none) {
                if pool.is_empty() {
                    break;
                }
                let members: Vec<i32> = group
                    .iter()
                    .flatten()
                    .map(|id| self.skill(*id).rank())
                    .collect();
                let overdue = self.waiting_ms(pool[0], now) >= max_wait;
                let pick = if overdue || members.is_empty() || !self.settings.skill_matching {
                    0
                } else {
                    let target = members.iter().sum::<i32>() as f32 / members.len() as f32;
                    // `pool` is in priority order, so `min_by` keeps the
                    // longest-waiting player among equally good skill matches.
                    pool.iter()
                        .enumerate()
                        .min_by(|(ia, a), (ib, b)| {
                            let da = (self.skill(**a).rank() as f32 - target).abs();
                            let db = (self.skill(**b).rank() as f32 - target).abs();
                            da.total_cmp(&db).then(ia.cmp(ib))
                        })
                        .map_or(0, |(i, _)| i)
                };
                group[slot] = Some(pool.remove(pick));
                added = true;
            }
            if added {
                self.balance_teams(group);
            }
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
        if self.session.deck[c].iter().all(Option::is_none) {
            return false;
        }
        self.record_game(c, now);
        for id in self.session.courts[c].into_iter().flatten() {
            self.mark_off_court(id, now);
        }
        self.session.courts[c] = std::mem::replace(&mut self.session.deck[c], EMPTY_SLOTS);
        self.session.court_started_ms[c] = Some(now);

        // Top up an incomplete group from the pool (which now includes the
        // players who just came off).
        if self.session.courts[c].iter().any(Option::is_none) {
            let mut courts = std::mem::take(&mut self.session.courts);
            self.fill_groups(&mut courts, &[c], now);
            self.session.courts = courts;
        }
        for id in self.session.courts[c].into_iter().flatten() {
            self.mark_on_court(id);
        }
        true
    }

    /// The big button: everyone on deck takes the courts, the deck refills.
    /// Courts with nobody on deck keep playing.
    pub fn next_round(&mut self, now: i64) -> bool {
        // Promote in court-priority order so top-ups favour the right people.
        let order = self.court_priority();
        let mut any = false;
        for c in order {
            any |= self.promote_deck(c, now);
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
        let (s, ids) = setup(&[Skill::Intermediate; 14], 3);
        let deck: Vec<_> = s.session.deck.iter().flatten().flatten().copied().collect();
        assert_eq!(deck.len(), 12);
        // The last two arrivals are waiting.
        assert_eq!(s.waiting_list(20_000), vec![ids[12], ids[13]]);
    }

    #[test]
    fn rotation_moves_deck_to_courts_and_refills() {
        let (mut s, _) = setup(&[Skill::Intermediate; 20], 3);
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
        let (mut s, _) = setup(&[Skill::Intermediate; 20], 3);
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
        let (mut s, ids) = setup(&[Skill::Intermediate; 8], 1);
        s.next_round(MIN); // P0..P3 on court, P4..P7 on deck
        s.next_round(16 * MIN); // P4..P7 on court, P0..P3 come off and go on deck
        let deck: Vec<_> = s.session.deck[0].iter().flatten().copied().collect();
        for id in &ids[0..4] {
            assert!(deck.contains(id));
        }
    }

    #[test]
    fn skill_matching_groups_similar_players() {
        use Skill::*;
        // Arrival order alternates skills; matching should still group them.
        let (s, _) = setup(
            &[Beginner, Expert, Beginner, Expert, Beginner, Expert, Beginner, Expert],
            2,
        );
        for g in &s.session.deck {
            let skills: Vec<Skill> = g.iter().flatten().map(|id| s.skill(*id)).collect();
            assert!(skills.iter().all(|k| *k == skills[0]), "mixed group: {skills:?}");
        }
    }

    #[test]
    fn overdue_player_placed_regardless_of_skill() {
        use Skill::*;
        let run = |max_wait: u32| {
            let mut s = AppState::default();
            s.settings.courts = 1;
            s.settings.max_wait_minutes = max_wait;
            s.sync_court_count(0);
            let seed = s.add_player("Long wait", Beginner);
            let ace = s.add_player("Ace", Expert);
            let fresh: Vec<_> = (0..3).map(|i| s.add_player(&format!("B{i}"), Beginner)).collect();
            let now = 60 * MIN;
            s.session.present.insert(seed, Presence::new(now - 25 * MIN));
            s.session.present.insert(ace, Presence::new(now - 22 * MIN));
            for b in &fresh {
                s.session.present.insert(*b, Presence::new(now - MIN));
            }
            s.fill_deck(now);
            s.session.deck[0].contains(&Some(ace))
        };
        // Generous max wait: the beginner group stays pure and the expert waits.
        assert!(!run(60));
        // Expert is past the 20 minute max wait, so they go in anyway.
        assert!(run(20));
    }

    #[test]
    fn dragging_off_deck_does_not_bounce_back() {
        let (mut s, _) = setup(&[Skill::Intermediate; 6], 1);
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
        let (mut s, _) = setup(&[Skill::Intermediate; 8], 1);
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
    fn leaving_court_early_keeps_wait_but_full_game_resets_it() {
        let (mut s, _) = setup(&[Skill::Intermediate; 8], 1);
        let round = s.settings.round_ms();
        s.next_round(MIN);
        s.next_round(MIN + round); // second group on; first group came off at MIN + round
        let a = s.session.courts[0][0].unwrap();
        let b = s.session.courts[0][1].unwrap();
        let before = s.session.present[&a].clone();
        assert_eq!(before.games_tonight, 1);
        let lifetime = s.players[&a].lifetime_games;

        // Sent to waiting 5 minutes into a game: wait keeps counting from before.
        let t = MIN + round + 5 * MIN;
        s.move_player(a, Target::Pool, t);
        let p = &s.session.present[&a];
        assert_eq!(p.last_played_ms, before.last_played_ms, "wait not reset");
        assert_eq!(p.waiting_ms(t), before.waiting_ms(t));
        assert_eq!(p.games_tonight, 0, "unfinished game not counted");
        assert_eq!(s.players[&a].lifetime_games, lifetime - 1);

        // Taken off after the full round length: that game counts.
        let t2 = MIN + 2 * round;
        s.move_player(b, Target::Pool, t2);
        assert_eq!(s.session.present[&b].last_played_ms, Some(t2));
        assert_eq!(s.session.present[&b].games_tonight, 1);
    }

    #[test]
    fn incomplete_deck_group_is_topped_up_on_rotation() {
        let (mut s, _) = setup(&[Skill::Intermediate; 6], 1);
        s.next_round(MIN); // 4 on court, 2 on deck
        assert_eq!(s.session.deck[0].iter().flatten().count(), 2);
        s.next_round(16 * MIN);
        assert_eq!(s.session.courts[0].iter().flatten().count(), 4);
    }

    #[test]
    fn history_records_attendance_and_games() {
        let (mut s, ids) = setup(&[Skill::Intermediate; 5], 1);
        // `setup` bypasses check-in; check one more player in properly.
        let late = s.add_player("Late", Skill::Expert);
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
        s.new_day();
        assert!(s.session.present.is_empty() && s.session.games.is_empty());
        assert!(s.players.contains_key(&ids[0]), "roster carries over");
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
