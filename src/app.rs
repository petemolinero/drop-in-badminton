//! The main screen, styled after Microsoft 365 (Fluent 2): a title bar, a
//! command ribbon, and floating cards on a neutral canvas.
//!
//! ```text
//! ┌ title bar: app · game day ·························· round · here · waiting ┐
//! ├ ribbon: roster history settings │   ▶ 12:34  −  +  ⟲   │ next round  ⛶ ──┤
//! │ roster │        COURT 1          COURT 2          COURT 3      │  waiting   │
//! │ pane   │                                                       │  pane      │
//! │ (opt.) │        up next 1        up next 2        up next 3    │ (by wait)  │
//! └────────┴───────────────────────────────────────────────────────┴────────────┘
//! ```

use std::collections::HashMap;

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Key, Layout, Margin, Modal, Order,
    Panel, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::engine::Target;
use crate::model::*;
use crate::stats::Stats;
use crate::store::{DayDetail, DaySummary, Store};
use crate::hall::{self, Hall};
use crate::theme::{self, Palette, icon, icon_font, regular, semibold};
use crate::update::{self, Release, Status as UpdateStatus, Updater};
use crate::widgets::{self, ButtonKind, Help, clock, short_duration, unique};

mod analytics_view;
mod games_view;
mod players_view;

/// The dashboard sidebar's tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SideTab {
    Waiting,
    Roster,
    AddPlayer,
}

/// The app's top-level screens, picked from the title bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    /// Courts, Up next, the timer and the waiting list.
    Dashboard,
    /// Every player ever, as an editable table.
    Players,
    /// Every game ever played; read-only unless unlocked.
    Games,
    /// Headline numbers and charts across every session.
    Analytics,
}

struct Snackbar {
    text: String,
    undo: Option<Box<AppState>>,
    born: f64,
}

/// Something chosen from a player's right-click menu.
#[derive(Clone, Copy, PartialEq)]
enum PlayerAction {
    ToWaiting,
    CheckOut,
    Break,
    Skill(Skill),
    Rename,
    Wait,
    Delete,
}

#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    Delete(PlayerId),
}

/// The session date-and-time form: for a new session, or to move this one.
struct SessionForm {
    date: String,
    time: String,
    /// Starting a new session (true) or changing this one's start (false).
    new: bool,
    /// Opened because the last session sat idle for hours.
    idle: bool,
}

impl SessionForm {
    fn at(ms: i64, new: bool, idle: bool) -> Self {
        Self { date: fmt_date(ms, "%Y-%m-%d"), time: fmt_date(ms, "%-I:%M %p"), new, idle }
    }

    /// The typed date and time, if both make sense.
    fn parse(&self) -> Option<i64> {
        use chrono::TimeZone;
        let date = chrono::NaiveDate::parse_from_str(self.date.trim(), "%Y-%m-%d").ok()?;
        let t = self.time.trim().to_uppercase().replace('.', "");
        let time = ["%I:%M %p", "%I:%M%p", "%I %p", "%I%p", "%H:%M"]
            .iter()
            .find_map(|f| chrono::NaiveTime::parse_from_str(&t, f).ok())?;
        chrono::Local.from_local_datetime(&date.and_time(time)).earliest().map(|d| d.timestamp_millis())
    }
}

/// Animated on-screen rectangle of a player's chip, so players glide between
/// the pool, on-deck cards, and courts instead of teleporting.
struct ChipAnim {
    /// Where the chip is drawn right now.
    rect: Rect,
    /// The slot it is in (`None` while it is only a row in a list).
    spot: Option<Spot>,
    /// The glide into that slot: where it started, and progress in 0..=1.
    from: Rect,
    t: f32,
    seen: u64,
}

impl ChipAnim {
    fn resting(rect: Rect, spot: Option<Spot>, seen: u64) -> Self {
        Self { rect, spot, from: rect, t: 1.0, seen }
    }
}

/// How long a chip takes to glide to a new position.
const GLIDE_SECS: f32 = 0.45;

/// The History dialog's contents, loaded from the database when opened.
struct HistoryView {
    days: Vec<DaySummary>,
    selected: Option<i64>,
    detail: DayDetail,
}

/// A running session untouched this long (real time) was left on by mistake:
/// its clock is stopped where it was last used.
const IDLE_GAP_MS: i64 = 8 * 3600 * 1000;

/// Space between cards on the canvas.
const GAP: f32 = 10.0;
/// Text size for every status chip, so they all share one spec.
const CHIP_TEXT: f32 = 15.0;

pub struct DropInApp {
    state: AppState,
    hall: Hall,
    store: Store,
    history: Option<HistoryView>,
    dirty: bool,
    dirty_since: Option<f64>,
    side_tab: SideTab,
    settings_open: bool,
    /// Show a plain-language explanation at the top of each section.
    help: bool,
    new_name: String,
    new_skill: Skill,
    new_from: String,
    new_found_via: String,
    new_auto: bool,
    search: String,
    /// Filter for the waiting list.
    waiting_search: String,
    /// What's typed in the clock's right-click panel.
    time_edit: String,
    /// The session clock's edit box.
    session_edit: String,
    view: View,
    /// Bumped on every save; the Players and Analytics numbers are recomputed
    /// from the database when it moves.
    save_gen: u64,
    stats: Option<(u64, std::rc::Rc<Stats>)>,
    players_view: players_view::PlayersView,
    games_view: games_view::GamesView,
    /// Player being renamed, the edit buffer, and whether the box still needs focus.
    renaming: Option<(PlayerId, String, bool)>,
    snackbar: Option<Snackbar>,
    confirm: Option<Confirm>,
    /// The new-session / session-time form.
    session_form: Option<SessionForm>,
    /// A right-click menu choice, applied at the end of the frame.
    pending: Option<(PlayerId, PlayerAction)>,
    /// The rename dialog: player, edit buffer, and whether it still needs focus.
    rename_dialog: Option<(PlayerId, String, bool)>,
    /// The adjust-wait dialog: player, minutes typed, and whether it still needs focus.
    wait_dialog: Option<(PlayerId, String, bool)>,
    drag: Option<PlayerId>,
    drop_targets: Vec<(Rect, Target)>,
    anims: HashMap<PlayerId, ChipAnim>,
    frame: u64,
    applied_theme: Option<(bool, f32)>,
    updater: Updater,
    /// This frame's on-court name size; Up next names stay a step smaller.
    court_font: f32,
    /// Time's-up glow phase for the Next round button, this frame.
    next_glow: Option<f32>,
    /// Where toasts appear: the open stretch of the ribbon left of the timer.
    toast_gap: Option<Rect>,
    /// Last toast's width, to centre it.
    toast_w: f32,
    /// The "Update Drop-in Badminton" dialog is showing.
    update_open: bool,
    #[cfg(debug_assertions)]
    shot_requested: bool,
}

impl DropInApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_fonts(theme::fonts());
        widgets::load_skill_finishes(&cc.egui_ctx);
        let (store, startup_error) = match Store::open_default() {
            Ok(store) => (store, None),
            Err(err) => {
                eprintln!("drop-in-badminton: could not open database: {err}");
                (
                    Store::open_in_memory().expect("in-memory database"),
                    Some(format!("Couldn't open the database ({err}) — changes won't be saved")),
                )
            }
        };
        let state = store.load().unwrap_or_else(|err| {
            eprintln!("drop-in-badminton: could not load: {err}");
            AppState::default()
        });
        let mut app = Self {
            state,
            hall: Hall::load(&cc.egui_ctx),
            store,
            history: None,
            dirty: true,
            dirty_since: None,
            side_tab: SideTab::Waiting,
            settings_open: false,
            help: false,
            new_name: String::new(),
            new_skill: Skill::Intermediate,
            new_from: String::new(),
            new_found_via: String::new(),
            new_auto: false,
            search: String::new(),
            waiting_search: String::new(),
            time_edit: String::new(),
            session_edit: String::new(),
            view: View::Dashboard,
            save_gen: 0,
            stats: None,
            players_view: Default::default(),
            games_view: Default::default(),
            renaming: None,
            snackbar: None,
            confirm: None,
            session_form: None,
            pending: None,
            rename_dialog: None,
            wait_dialog: None,
            drag: None,
            drop_targets: Vec::new(),
            anims: HashMap::new(),
            frame: 0,
            applied_theme: None,
            updater: Updater::new(),
            court_font: 40.0,
            toast_gap: None,
            next_glow: None,
            toast_w: 420.0,
            update_open: false,
            #[cfg(debug_assertions)]
            shot_requested: false,
        };
        let wall = wall_ms();
        app.state.ensure_clock(wall);
        let now = app.state.now();
        app.state.sync_court_count(now);
        app.stop_if_idle(&cc.egui_ctx, wall);
        let previous = app.store.swap_app_version(update::VERSION);
        if let Some(err) = startup_error {
            app.toast(&cc.egui_ctx, err);
        } else if previous.is_some_and(|v| update::is_newer(update::VERSION, &v)) {
            app.toast(&cc.egui_ctx, format!("Updated to version {}", update::VERSION));
        }
        if app.state.session.present.is_empty() {
            app.side_tab = SideTab::Roster;
        }
        app
    }

    /// A session left running for hours with nobody touching it (say, until
    /// next week): stop its clock where it was last used, and offer a new one.
    fn stop_if_idle(&mut self, ctx: &egui::Context, wall: i64) {
        let s = &self.state.session;
        let started = s.day_id.is_some() || !s.present.is_empty();
        if !(s.clock.running() && started && wall - s.last_activity_ms > IDLE_GAP_MS) {
            return;
        }
        let at = s.last_activity_ms;
        self.state.session.clock.pause(at);
        self.dirty = true;
        self.session_form = Some(SessionForm::at(wall, true, true));
        let _ = ctx;
    }

    /// Pause or resume the session clock (and with it, everything timed).
    fn toggle_session_clock(&mut self) {
        let wall = wall_ms();
        let clock = &mut self.state.session.clock;
        if clock.running() {
            clock.pause(wall);
        } else {
            clock.resume(wall);
        }
        self.dirty = true;
    }

    fn save(&mut self) {
        if let Err(err) = self.store.save(&mut self.state) {
            eprintln!("drop-in-badminton: failed to save: {err}");
        }
        self.save_gen += 1;
    }

    /// Lifetime numbers for every player and session, from the database as of
    /// the last save.
    fn stats(&mut self) -> std::rc::Rc<Stats> {
        if let Some((seen, stats)) = &self.stats {
            if *seen == self.save_gen {
                return stats.clone();
            }
        }
        let stats = std::rc::Rc::new(match self.store.everything() {
            Ok(h) => Stats::compute(&h),
            Err(err) => {
                eprintln!("drop-in-badminton: couldn't read history: {err}");
                Stats::default()
            }
        });
        self.stats = Some((self.save_gen, stats.clone()));
        stats
    }

    /// Put the current session into History: games on court count as
    /// finished and everyone is checked out.
    fn close_session(&mut self) {
        let now = self.state.now();
        self.state.end_day(now);
        self.save();
    }

    /// Close the current session and start a fresh one whose clock reads
    /// `start_ms` and is running. The roster carries over; everyone starts
    /// checked out.
    fn start_session(&mut self, ctx: &egui::Context, start_ms: i64) {
        let before = Box::new(self.state.clone());
        self.close_session();
        self.state.new_day(start_ms, wall_ms());
        self.save();
        self.dirty = false;
        self.side_tab = SideTab::Roster;
        let when = fmt_date(start_ms, "%a, %b %-d, %-I:%M %p");
        let text = match self.state.session.present.len() {
            0 => format!("New session · {when} — check players in as they arrive"),
            1 => format!("New session · {when} — 1 regular checked in automatically"),
            n => format!("New session · {when} — {n} regulars checked in automatically"),
        };
        self.snackbar = Some(Snackbar {
            text,
            undo: Some(before),
            born: ctx.input(|i| i.time),
        });
    }

    /// Apply a change with an undo snapshot and a snackbar message.
    fn act(&mut self, ctx: &egui::Context, text: impl Into<String>, f: impl FnOnce(&mut AppState)) {
        let before = Box::new(self.state.clone());
        f(&mut self.state);
        self.snackbar = Some(Snackbar {
            text: text.into(),
            undo: Some(before),
            born: ctx.input(|i| i.time),
        });
        self.dirty = true;
    }

    fn next_round(&mut self, ctx: &egui::Context) {
        let now = self.state.now();
        let before = Box::new(self.state.clone());
        if self.state.next_round(now) {
            let n = self.state.session.round_number;
            self.snackbar = Some(Snackbar {
                text: {
                    const CHEERS: [&str; 6] = ["to the courts!", "rackets up!", "fresh legs!", "bring the smash!", "game on!", "let's rally!"];
                    format!("Round {n} — {}", CHEERS[(n as usize).saturating_sub(1) % CHEERS.len()])
                },
                undo: Some(before),
                born: ctx.input(|i| i.time),
            });
            self.dirty = true;
        } else {
            self.toast(ctx, "Nobody is on deck yet — check players in from the roster");
        }
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.snackbar = Some(Snackbar {
            text: text.into(),
            undo: None,
            born: ctx.input(|i| i.time),
        });
    }

    /// Ask where to save a table as a CSV file, then write it.
    fn export_csv(&mut self, ctx: &egui::Context, file_name: &str, header: &[&str], rows: &[Vec<String>]) {
        let Some(mut path) = rfd::FileDialog::new()
            .set_title("Export as CSV")
            .set_file_name(file_name)
            .add_filter("CSV spreadsheet", &["csv"])
            .save_file()
        else {
            return; // cancelled
        };
        if path.extension().is_none() {
            path.set_extension("csv");
        }
        let shown = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        match std::fs::write(&path, crate::csv::table(header, rows)) {
            Ok(()) => {
                let rows = if rows.len() == 1 { "1 row".to_owned() } else { format!("{} rows", rows.len()) };
                self.toast(ctx, format!("Exported {rows} to {shown}"));
            }
            Err(err) => self.toast(ctx, format!("Couldn't save {shown}: {err}")),
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.snackbar.take().and_then(|s| s.undo) {
            self.state = *prev;
            self.dirty = true;
        }
    }

    // ----- Chip animation -------------------------------------------------

    fn anim_rect(&mut self, id: PlayerId, spot: Spot, target: Rect, dt: f32) -> (Rect, bool) {
        let frame = self.frame;
        let e = self.anims.entry(id).or_insert(ChipAnim::resting(target, Some(spot), frame));
        if e.seen + 1 < frame {
            *e = ChipAnim::resting(target, Some(spot), frame);
        }
        e.seen = frame;
        if e.spot != Some(spot) {
            // Moved to another slot: glide there from wherever the chip is now.
            e.spot = Some(spot);
            e.from = e.rect;
            e.t = 0.0;
        }
        let moving = e.t < 1.0;
        if moving {
            e.t = (e.t + dt / GLIDE_SECS).min(1.0);
            let k = egui::emath::easing::cubic_in_out(e.t);
            e.rect = Rect::from_min_max(
                e.from.min + (target.min - e.from.min) * k,
                e.from.max + (target.max - e.from.max) * k,
            );
        } else {
            e.rect = target;
        }
        (e.rect, moving)
    }

    /// "40m waiting • 4 games": time since this player last played (or since
    /// they checked in, if they haven't played yet) and games tonight. The one
    /// format used everywhere a checked-in player is shown.
    fn since_played_label(&self, id: PlayerId) -> Option<String> {
        let ms = self.shown_wait_ms(id, self.state.now())?;
        let games = match self.state.session.present.get(&id)?.games_tonight {
            1 => "1 game".to_owned(),
            g => format!("{g} games"),
        };
        Some(format!("{} wait · {games}", short_duration(ms)))
    }

    /// Time since playing, as shown on screen: players on a court are playing,
    /// so they show zero. (Their real wait is kept underneath, so taking them
    /// off before the game is done picks it back up.)
    fn shown_wait_ms(&self, id: PlayerId, now: i64) -> Option<i64> {
        let p = self.state.session.present.get(&id)?;
        let on_court = matches!(self.state.session.locate(id), Some(Spot::Court(..)));
        Some(if on_court { 0 } else { p.waiting_ms(now) })
    }

    /// Record where a player is shown without a chip (e.g. a pool row), so a
    /// chip appearing elsewhere next frame flies from here.
    fn note_rect(&mut self, id: PlayerId, rect: Rect) {
        self.anims.insert(id, ChipAnim::resting(rect, None, self.frame));
    }
}

/// A floating pane card (Fluent "card" on the canvas).
fn pane(pal: &Palette) -> Frame {
    Frame::NONE
        .fill(pal.bg1)
        .stroke(Stroke::new(1.0, pal.card_stroke))
        .corner_radius(8)
        .inner_margin(Margin::same(16))
        .shadow(pal.shadow(4))
}

impl eframe::App for DropInApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        if self.frame == 3 {
            crate::launch::mark_launched();
        }
        let now = self.state.now();
        let settings = self.state.settings.clone();
        let pal = *Palette::get(settings.dark_mode);
        theme::set_dark(settings.dark_mode);
        if self.applied_theme != Some((settings.dark_mode, settings.ui_scale)) {
            theme::apply(&ctx, &pal);
            ctx.set_zoom_factor(settings.ui_scale);
            self.applied_theme = Some((settings.dark_mode, settings.ui_scale));
        }

        self.shortcuts(&ctx);
        self.updater.poll(&ctx);
        self.drop_targets.clear();
        #[cfg(debug_assertions)]
        self.debug_screenshot(&ctx);

        let g = GAP as i8;
        Panel::top("titlebar")
            .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: 16, right: 16, top: 0, bottom: 0 }))
            .exact_size(44.0)
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| self.title_bar(ui, &pal, now));

        if self.view != View::Dashboard {
            let view = self.view;
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: g, right: g, top: 4, bottom: g }))
                .show(ui, |ui| match view {
                    View::Players => self.players_view(ui, &pal),
                    View::Games => self.games_view(ui, &pal),
                    _ => self.analytics_view(ui, &pal),
                });
        }

        if self.view == View::Dashboard {
            Panel::top("ribbon")
                .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: g, right: g, top: 0, bottom: g }))
                .exact_size(128.0)
                .show_separator_line(false)
                .resizable(false)
                .show(ui, |ui| self.ribbon(ui, &pal, now));

            Panel::right("pool")
                .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: 0, right: g, top: 0, bottom: g }))
                .exact_size(440.0)
                .resizable(false)
                .show_separator_line(false)
                .show(ui, |ui| {
                    // The roster opens in place of the waiting list, so the courts
                    // and Up next (what the TV audience reads) never change size.
                    pane(&pal).show(ui, |ui| {
                        let avail = ui.available_size();
                        ui.set_min_size(avail);
                        ui.set_max_width(avail.x);
                        self.side_tabs(ui, &pal, now);
                        match self.side_tab {
                            SideTab::Waiting => self.pool(ui, &pal, now),
                            SideTab::Roster => {
                                // Players dragged off a court can still be dropped here.
                                self.drop_targets.push((ui.max_rect().expand(16.0), Target::Pool));
                                self.roster(ui, &pal, now);
                            }
                            SideTab::AddPlayer => self.add_player_form(ui, &pal, now),
                        }
                    });
                });

            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: g, right: g, top: 0, bottom: g }))
                .show(ui, |ui| self.arena(ui, &pal, now));
        }

        self.drag_overlay(&ctx, &pal);
        self.resolve_drop(&ctx);
        self.snackbar_ui(&ctx, &pal);
        self.settings_dialog(&ctx, &pal);
        self.update_dialog(&ctx, &pal);
        self.confirm_dialog(&ctx, &pal);
        self.history_dialog(&ctx, &pal);
        self.rename_dialog_ui(&ctx, &pal);
        self.wait_dialog_ui(&ctx, &pal);
        self.help_dialog(&ctx, &pal);
        if let Some((id, action)) = self.pending.take() {
            self.apply_player_action(&ctx, id, action);
        }
        self.stop_if_idle(&ctx, wall_ms());
        self.session_form_ui(&ctx, &pal);

        if self.dirty {
            self.state.session.last_activity_ms = wall_ms();
            // Save a second after the change rather than in the same frame, so
            // the write never lands on the first frames of an animation.
            let t = ctx.input(|i| i.time);
            let since = *self.dirty_since.get_or_insert(t);
            if t - since > 1.0 {
                self.save();
                self.dirty = false;
                self.dirty_since = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(1100));
            }
        } else {
            self.dirty_since = None;
        }
        let tick = match self.state.session.timer {
            Timer::Running { .. } => 50,
            _ => 1000,
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(tick));
    }

    fn on_exit(&mut self) {
        self.save();
    }

    /// Debug builds: `DROP_IN_BADMINTON_SHOW=menu` right-clicks the first court slot.
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !std::env::var("DROP_IN_BADMINTON_SHOW").is_ok_and(|s| s.contains("menu")) {
            // Screenshots shouldn't pick up hover from wherever the real mouse is.
            if std::env::var_os("DROP_IN_BADMINTON_SCREENSHOT").is_some() {
                raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_)));
                raw.events.push(egui::Event::PointerGone);
            }
            return;
        }
        let Some(pos) = self.drop_targets.iter().find_map(|(r, t)| matches!(t, Target::Slot(Spot::Court(0, 0))).then(|| r.center())) else {
            return;
        };
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        // Wait for the window to settle, then move, press, and release.
        use std::sync::atomic::{AtomicU8, Ordering};
        static STEP: AtomicU8 = AtomicU8::new(0);
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        if START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32() < 0.8 {
            return;
        }
        match STEP.fetch_add(1, Ordering::Relaxed) {
            0 => raw.events.push(egui::Event::PointerMoved(pos)),
            1 => raw.events.extend([egui::Event::PointerMoved(pos), button(true)]),
            2 => raw.events.extend([egui::Event::PointerMoved(pos), button(false)]),
            _ => {}
        }
    }
}

// ===========================================================================
// Development aid: `DROP_IN_BADMINTON_SCREENSHOT=out.ppm` captures one frame and exits.
// `DROP_IN_BADMINTON_SHOW=settings|roster|drag|next|timer|done|history` sets up the screen first.

#[cfg(debug_assertions)]
impl DropInApp {
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("DROP_IN_BADMINTON_SCREENSHOT") else { return };
        let show = std::env::var("DROP_IN_BADMINTON_SHOW").unwrap_or_default();
        if self.frame == 2 {
            self.settings_open = show.contains("settings");
            if show.contains("roster") {
                self.side_tab = SideTab::Roster;
            }
            if show.contains("addplayer") {
                self.side_tab = SideTab::AddPlayer;
            }
            self.help = show.contains("help");
            if show.contains("update") {
                self.updater.debug_set(UpdateStatus::Available(Release {
                    version: "0.2.0".into(),
                    url: String::new(),
                    notes: "• Courts can be renamed\n• Faster start-up on older laptops\n• Fixed the timer sound on Windows".into(),
                }));
                self.update_open = !show.contains("ribbon");
            }
            if show.contains("players") {
                self.view = View::Players;
            }
            if show.contains("analytics") {
                self.view = View::Analytics;
            }
            if show.contains("games") {
                self.view = View::Games;
                self.games_view.unlocked = show.contains("unlocked");
            }
            if show.contains("sessiontime") {
                self.session_edit = "0:30:00".into();
                egui::Popup::open_id(ctx, unique("session-time"));
            }
            if show.contains("light") {
                self.state.settings.dark_mode = false;
            }
            if show.contains("dark") {
                self.state.settings.dark_mode = true;
            }
            // The frozen review data was checked in hours ago; make the night
            // look like it's 20-odd minutes old, as it would be in real use.
            {
                let wall = wall_ms();
                self.state.session.clock = SessionClock { start_ms: wall - 30 * 60_000, elapsed_ms: 30 * 60_000, running_since: Some(wall) };
                self.state.session.last_activity_ms = wall;
                self.session_form = None;
                let now = self.state.now();
                for (k, p) in self.state.session.present.values_mut().enumerate() {
                    p.checked_in_ms = now - (14 + (k as i64 * 7) % 12) * 60_000;
                    p.last_played_ms = None;
                }
                for d in self.state.session.attendance.iter_mut() {
                    d.in_ms = d.in_ms.max(now - 30 * 60_000);
                }
            }
            if show.contains("all") {
                let now = self.state.now();
                let ids: Vec<PlayerId> = self.state.players.keys().copied().collect();
                for id in ids {
                    self.state.check_in(id, now - 20 * 60_000);
                }
                self.state.fill_deck(now);
            }
            if show.contains("next") {
                self.next_round(ctx);
            }
            if show.contains("timer") {
                self.state.session.timer = Timer::Running { end_ms: self.state.now() + 9 * 60_000 + 41_000 };
            }
            if show.contains("done") {
                self.state.session.timer = Timer::Running { end_ms: self.state.now() - 1000 };
            }
            if show.contains("history") {
                self.open_history();
            }
            if show.contains("paused") {
                self.state.session.clock.pause(wall_ms());
            }
            if show.contains("rename") {
                if let Some(id) = self.state.players.keys().next().copied() {
                    self.rename_dialog = Some((id, self.state.name(id).to_owned(), true));
                }
            }
            if show.contains("confirm") {
                self.session_form = Some(SessionForm::at(wall_ms(), true, false));
            }
            if show.contains("drag") {
                self.drag = self.state.session.deck.first().and_then(|g| g[0]);
            }
        }
        let t = ctx.input(|i| i.time);
        let gliding = self.anims.values().any(|a| a.t < 1.0);
        let settle = if show.contains("menu") { self.frame > 12 && !gliding } else { (self.snackbar.is_none() || t > 9.0) && !gliding };
        let settle = settle && self.frame > 5 && (self.view != View::Dashboard || self.hall.settled(t));
        if !self.shot_requested && t > 1.5 && settle {
            let ctx2 = ctx.clone();
            let cb = egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
                for px in &img.pixels {
                    out.extend_from_slice(&[px.r(), px.g(), px.b()]);
                }
                std::fs::write(&path, out).expect("write screenshot");
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx2.request_repaint();
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
            self.shot_requested = true;
        }
        ctx.request_repaint();
    }
}

// ===========================================================================
// Title bar, ribbon & timer

impl DropInApp {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let now = self.state.now();
        let (space, n, f11, undo, esc, f1) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::Space),
                i.consume_key(egui::Modifiers::NONE, Key::N),
                i.consume_key(egui::Modifiers::NONE, Key::F11),
                i.consume_key(egui::Modifiers::COMMAND, Key::Z),
                i.key_pressed(Key::Escape),
                i.consume_key(egui::Modifiers::NONE, Key::F1),
            )
        });
        if f1 {
            self.help = !self.help;
        }
        let dashboard = self.view == View::Dashboard;
        if space && dashboard {
            self.state.timer_toggle(now);
            self.dirty = true;
        }
        if n && dashboard {
            self.next_round(ctx);
        }
        if f11 {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        if undo {
            self.undo();
        }
        // Esc closes an open menu first (egui handles that), not the panels behind it.
        let menu_open = egui::Popup::is_any_open(ctx);
        if esc && self.drag.is_none() && !menu_open {
            if self.settings_open {
                self.settings_open = false;
            } else if self.confirm.is_some() {
                self.confirm = None;
            } else if self.side_tab != SideTab::Waiting {
                self.side_tab = SideTab::Waiting;
            }
        }
    }

    fn title_bar(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let rect = ui.max_rect();
        let p = ui.painter();
        let y = rect.center().y;

        // App tile, name, and the game day — like an Office document title.
        let tile = Rect::from_center_size(pos2(rect.left() + 13.0, y), Vec2::splat(26.0));
        p.rect_filled(tile, CornerRadius::same(6), pal.brand);
        p.text(tile.center(), Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(17.0), pal.on_brand);
        let name = p.text(pos2(tile.right() + 10.0, y), Align2::LEFT_CENTER, "Drop-in Badminton", semibold(15.0), pal.fg1);
        p.line_segment(
            [pos2(name.right() + 12.0, y - 8.0), pos2(name.right() + 12.0, y + 8.0)],
            Stroke::new(1.0, pal.stroke1),
        );
        // The three views, as tabs.
        let mut x = name.right() + 20.0;
        for (view, glyph, label) in [
            (View::Dashboard, icon::BOARD, "Dashboard"),
            (View::Players, icon::TABLE, "Players"),
            (View::Games, icon::SHUTTLE, "Games"),
            (View::Analytics, icon::CHART, "Analytics"),
        ] {
            let text = ui.painter().layout_no_wrap(label.to_owned(), semibold(14.0), pal.fg1);
            let r = Rect::from_min_size(pos2(x, y - 15.0), vec2(text.size().x + 44.0, 30.0));
            let resp = ui.interact(r, unique(("view-tab", label)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let selected = self.view == view;
            let (fill, ink) = if selected {
                (pal.brand_subtle, pal.on_brand_subtle)
            } else if resp.hovered() {
                (pal.subtle_hover, pal.fg1)
            } else {
                (Color32::TRANSPARENT, pal.fg2)
            };
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(4), fill);
            p.text(pos2(r.left() + 20.0, y + 1.0), Align2::CENTER_CENTER, glyph, icon_font(17.0), if selected { pal.brand_fg } else { ink });
            p.galley_with_override_text_color(pos2(r.left() + 34.0, y - text.size().y / 2.0 - 1.0), text, ink);
            if resp.clicked() && !selected {
                self.view = view;
                // History is only ever unlocked on purpose, one visit at a time.
                self.games_view.unlocked = false;
                // Fresh numbers: write out anything still waiting to be saved.
                if self.dirty {
                    self.save();
                    self.dirty = false;
                }
            }
            x = r.right() + 4.0;
        }
        let p = ui.painter();
        p.line_segment([pos2(x + 8.0, y - 8.0), pos2(x + 8.0, y + 8.0)], Stroke::new(1.0, pal.stroke1));
        let start = self.state.session.clock.start_ms;
        // The session's date, when there's room before the clock on the right.
        let label = p.layout_no_wrap(format!("Session · {}", fmt_date(start, "%A, %B %-d · %-I:%M %p")), regular(14.0), pal.fg2);
        if x + 20.0 + label.size().x < rect.right() - 340.0 {
            p.galley(pos2(x + 20.0, y - label.size().y / 2.0 - 1.0), label, pal.fg2);
        }
        let _ = now;

        // Right: the session clock, which every wait and timer runs on. Laid
        // out by hand from the right edge, everything centred on one line.
        let clock = self.state.session.clock;
        let running = clock.running();
        let button = |ui: &mut Ui, right: f32, id: &str| {
            let r = Rect::from_center_size(pos2(right - 15.0, y), Vec2::splat(30.0));
            let resp = ui.interact(r, unique(id), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let fill = if resp.is_pointer_button_down_on() { pal.subtle_pressed } else if resp.hovered() { pal.subtle_hover } else { Color32::TRANSPARENT };
            ui.painter().rect_filled(r, CornerRadius::same(4), fill);
            resp
        };
        let menu_btn = button(ui, rect.right(), "session-edit").on_hover_text("Session date & time, or a new session");
        ui.painter().text(menu_btn.rect.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, icon::EDIT, icon_font(18.0), pal.fg2);
        egui::Popup::menu(&menu_btn).id(unique("session-menu")).show(|ui| {
            ui.set_min_width(220.0);
            ui.spacing_mut().button_padding = vec2(10.0, 7.0);
            if ui.button(RichText::new(format!("{}  Adjust session time…", icon::TIMER)).font(regular(15.0))).clicked() {
                self.session_edit = hms(clock.elapsed(wall_ms()));
                egui::Popup::open_id(ui.ctx(), unique("session-time"));
            }
            if ui.button(RichText::new(format!("{}  Change date & time…", icon::CALENDAR)).font(regular(15.0))).clicked() {
                self.session_form = Some(SessionForm::at(start, false, false));
            }
            if ui.button(RichText::new(format!("{}  New session…", icon::ADD)).font(regular(15.0))).clicked() {
                self.session_form = Some(SessionForm::at(wall_ms(), true, false));
            }
        });
        let (glyph, tip) = if running { (widgets::Media::Pause, "Pause the session: every clock stops") } else { (widgets::Media::Play, "Resume the session") };
        let toggle = button(ui, menu_btn.rect.left() - 2.0, "session-toggle").on_hover_text(tip);
        widgets::paint_media_glyph(ui.painter(), toggle.rect.center(), 13.0, glyph, if running { pal.fg2 } else { pal.brand_fg });
        if toggle.clicked() {
            self.toggle_session_clock();
        }
        let ink = if running { pal.fg1 } else { pal.warning_fg };
        // Text sits low in its line box and icons sit high; these nudges put
        // every piece's visual middle on the same line.
        let galley = ui.painter().layout_no_wrap(hms(clock.elapsed(wall_ms())), semibold(16.0), ink);
        let time = Rect::from_min_size(
            pos2(toggle.rect.left() - 6.0 - galley.size().x, y - galley.size().y / 2.0 - 1.5),
            galley.size(),
        );
        let time_resp = ui
            .interact(time.expand(4.0), unique("session-time-text"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Change the session time");
        if time_resp.hovered() {
            ui.painter().rect_filled(time.expand2(vec2(6.0, 4.0)), CornerRadius::same(4), pal.subtle_hover);
        }
        ui.painter().galley(time.min, galley, ink);
        if time_resp.clicked() || time_resp.secondary_clicked() {
            self.session_edit = hms(clock.elapsed(wall_ms()));
            egui::Popup::open_id(ui.ctx(), unique("session-time"));
        }
        self.session_time_menu(&time_resp, pal);
        let p = ui.painter();
        if running {
            p.text(pos2(time.left() - 10.0, y - 1.0), Align2::RIGHT_CENTER, "Session", regular(14.0), pal.fg3);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        } else {
            widgets::paint_badge(p, pos2(time.left() - 12.0, y), Align2::RIGHT_CENTER, Some(icon::PAUSE), "Session paused", 13.0, pal.warning_subtle, pal.warning_fg);
        }
    }

    /// The session clock's edit panel (click the time, or the pencil menu):
    /// type how long the session has been going.
    fn session_time_menu(&mut self, anchor: &egui::Response, pal: &Palette) {
        egui::Popup::from_response(anchor)
            .id(unique("session-time"))
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .show(|ui| {
                ui.set_width(220.0);
                ui.spacing_mut().button_padding = vec2(10.0, 7.0);
                ui.label(RichText::new("Session time").font(semibold(15.0)).color(pal.fg1));
                ui.label(RichText::new("How long it's been going: h:mm, or h:mm:ss. Waits and the round timer stay as they are.").font(regular(13.0)).color(pal.fg3));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.session_edit)
                        .font(regular(18.0))
                        .margin(Margin::symmetric(10, 6))
                        .desired_width(220.0),
                );
                if !edit.has_focus() && !edit.lost_focus() && ui.memory(|m| m.focused().is_none()) {
                    edit.request_focus();
                }
                let typed = parse_hms(&self.session_edit);
                let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(4.0);
                let set = ui.add_enabled_ui(typed.is_some(), |ui| widgets::button(ui, pal, ButtonKind::Primary, None, "Set time", 32.0)).inner;
                if let Some(ms) = typed.filter(|_| set.clicked() || entered) {
                    self.state.session.set_elapsed(ms, wall_ms());
                    self.dirty = true;
                    ui.close();
                }
            });
    }

    /// The session date-and-time form, for a new session or to move this one.
    fn session_form_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut form) = self.session_form.take() else { return };
        let mut open = true;
        let mut go = None;
        let parsed = form.parse();
        let modal = Modal::new(unique("session-form"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(440.0);
                let title = if form.new { "Start a new session" } else { "Session date & time" };
                ui.label(RichText::new(title).font(semibold(20.0)));
                ui.add_space(2.0);
                let body = if form.idle {
                    "The last session sat untouched for hours, so its clock is stopped where it was last used. Start a new one, or keep the last one (resume it from the clock at the top)."
                } else if form.new {
                    "This session goes into History (games still on court count as finished). Everyone starts checked out; your roster and settings are kept."
                } else {
                    "Moves the whole session: check-ins, games and the round timer shift together, so nobody's wait changes."
                };
                ui.label(RichText::new(body).font(regular(14.0)).color(pal.fg2));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Date").font(semibold(13.0)).color(pal.fg2));
                        ui.add(egui::TextEdit::singleline(&mut form.date).hint_text("2026-11-15").font(regular(16.0)).margin(Margin::symmetric(10, 6)).desired_width(170.0));
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Start time").font(semibold(13.0)).color(pal.fg2));
                        ui.add(egui::TextEdit::singleline(&mut form.time).hint_text("7:00 PM").font(regular(16.0)).margin(Margin::symmetric(10, 6)).desired_width(130.0));
                    });
                });
                let hint = match parsed {
                    Some(ms) => fmt_date(ms, "%A, %B %-d, %Y at %-I:%M %p"),
                    None => "Date as YYYY-MM-DD, time like 7:00 PM or 19:00".to_owned(),
                };
                ui.label(RichText::new(hint).font(regular(13.0)).color(if parsed.is_some() { pal.fg3 } else { pal.warning_fg }));
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let action = if form.new { "Start session" } else { "Save" };
                    let ok = ui.add_enabled_ui(parsed.is_some(), |ui| widgets::button(ui, pal, ButtonKind::Primary, None, action, 34.0)).inner;
                    if ok.clicked() {
                        go = parsed;
                    }
                    let cancel = if form.idle { "Keep the last session" } else { "Cancel" };
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, cancel, 34.0).clicked() {
                        open = false;
                    }
                });
            });
        if let Some(ms) = go {
            if form.new {
                self.start_session(ctx, ms);
            } else {
                let delta = ms - self.state.session.clock.start_ms;
                self.act(ctx, format!("Session moved to {}", fmt_date(ms, "%a, %b %-d, %-I:%M %p")), |s| s.session.shift(delta));
            }
            return;
        }
        if open && !modal.should_close() {
            self.session_form = Some(form);
        }
    }

    fn ribbon(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        let card = ui.max_rect();
        widgets::card(ui.painter(), pal, card, pal.bg1, 8, 4);
        let rect = card.shrink2(vec2(8.0, 0.0));

        // Left: panes and dialogs.
        let mut left = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
        {
            let ui = &mut left;
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::ribbon_button(ui, pal, if self.help { icon::QUESTION_F } else { icon::QUESTION }, "Help", self.help)
                .on_hover_text(if self.help { "Hide the explanations (F1)" } else { "Explain how each section works (F1)" })
                .clicked()
            {
                self.help = !self.help;
            }
            widgets::vdivider(ui, pal, 44.0);
            if widgets::ribbon_button(ui, pal, icon::HISTORY, "History", self.history.is_some())
                .on_hover_text("Past game days")
                .clicked()
            {
                self.open_history();
            }
            if widgets::ribbon_button(ui, pal, icon::SETTINGS, "Settings", self.settings_open).clicked() {
                self.settings_open = true;
            }
            widgets::vdivider(ui, pal, 44.0);
            self.update_button(ui, pal);
        }

        // Right: the main action.
        let mut right = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::right_to_left(Align::Center)));
        {
            let ui = &mut right;
            ui.spacing_mut().item_spacing.x = 2.0;
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if widgets::ribbon_button(ui, pal, if fs { icon::FULLSCREEN_EXIT } else { icon::FULLSCREEN }, "Full screen", fs)
                .on_hover_text("Full screen (F11)")
                .clicked()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
            }
            widgets::vdivider(ui, pal, 44.0);
            ui.add_space(6.0);
            self.next_round_button(ui, pal, now);
        }

        // Center: the timer.
        let left_edge = left.min_rect().right() + 8.0;
        let right_edge = right.min_rect().left() - 8.0;
        let width = 330.0_f32.min(right_edge - left_edge);
        let cx = card.center().x.clamp(left_edge + width / 2.0, right_edge - width / 2.0);
        let timer_rect = Rect::from_center_size(pos2(cx, card.center().y), vec2(width, card.height()));
        // Toasts sit in the open stretch between the menu buttons and the timer.
        self.toast_gap = Some(Rect::from_min_max(pos2(left_edge + 12.0, card.top()), pos2(timer_rect.left() - 12.0, card.bottom())));
        let mut tui = ui.new_child(UiBuilder::new().max_rect(timer_rect).layout(Layout::left_to_right(Align::Center)));
        self.timer_widget(&mut tui, pal, now);
    }

    fn next_round_button(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        let finished = self.state.timer_finished(now);
        let (outer, _) = ui.allocate_exact_size(vec2(200.0, 52.0), Sense::hover());
        if finished {
            // Time's up: a warm glowing ring says "this one, now".
            let t = ctx.input(|i| i.time) as f32;
            let glow = 0.5 + 0.5 * (t * std::f32::consts::TAU * 0.8).sin();
            self.next_glow = Some(glow);
            ctx.request_repaint();
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(outer).layout(Layout::left_to_right(Align::Center)));
        // Primary when it's the thing to press: time's up, or the courts are
        // empty and a group is waiting.
        let s = &self.state.session;
        let courts_idle = s.courts.iter().all(|c| c.iter().all(Option::is_none));
        let deck_ready = s.deck.iter().any(|g| g.iter().any(Option::is_some));
        // Primary when it's the thing to press: time's up, or the courts are
        // empty and a group is waiting. Mid-round, the play button is the move.
        // Primary when it's the move: time's up, or nothing is on court yet.
        // Mid-round (or before the clock starts) the play button is the move.
        let kind = if finished { ButtonKind::Alert } else if courts_idle && deck_ready { ButtonKind::Primary } else { ButtonKind::Secondary };
        child.set_width(outer.width());
        let label = if self.state.session.round_number == 0 { "Start round 1" } else { "Next round" };
        let resp = widgets::button(&mut child, pal, kind, Some(icon::NEXT_F), label, 52.0)
            .on_hover_text("Move everyone up next onto the courts (N)");
        if let Some(glow) = self.next_glow.take() {
            // Time's up: a soft warm halo breathing around the button.
            ui.painter().rect_filled(resp.rect.expand(3.0 + 4.0 * glow), CornerRadius::same(10), theme::hex(0xC2410C).gamma_multiply(0.18 * glow));
        }
        if resp.clicked() {
            self.next_round(&ctx);
        }
    }

    fn timer_widget(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        let total = self.state.settings.round_ms();
        let remaining = self.state.timer_remaining_ms(now);
        let finished = self.state.timer_finished(now);
        let timer = self.state.session.timer;
        let running = matches!(timer, Timer::Running { .. }) && !finished && self.state.session.clock.running();
        let frac = if total > 0 { remaining as f32 / total as f32 } else { 0.0 };

        // Play / pause inside a progress ring.
        let (ring, resp) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::click());
        let resp = resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(match (finished, timer) {
                (true, _) => "Next round (N)",
                (_, Timer::Running { .. }) => "Pause (Space)",
                _ => "Start the round (Space)",
            });
        let final_minute = running && remaining <= 120_000;
        let accent = if finished || final_minute { pal.warning_fg } else { pal.brand_fg };
        {
            let p = ui.painter();
            let c = ring.center();
            let r = 33.0;
            p.circle_stroke(c, r, Stroke::new(5.0, pal.stroke2));
            // Time's up shows a full ring in the warm "time" color.
            let shown = if finished { 1.0 } else { ctx.animate_value_with_time(resp.id.with("frac"), frac, 0.25) };
            if shown > 0.0 {
                let n = 72;
                let pts: Vec<Pos2> = (0..=n)
                    .map(|k| {
                        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * shown * k as f32 / n as f32;
                        c + r * Vec2::angled(a)
                    })
                    .collect();
                p.add(egui::Shape::line(pts, Stroke::new(5.0, accent)));
            }
        }
        let btn = Rect::from_center_size(ring.center(), Vec2::splat(50.0));
        // Players on court but the clock isn't running: the play button pulses.
        let courts_busy = self.state.session.courts.iter().any(|c| c.iter().any(Option::is_some));
        if matches!(timer, Timer::Idle) && courts_busy {
            let t = ctx.input(|i| i.time) as f32;
            let phase = (t * 0.9).fract();
            ui.painter().circle_stroke(btn.center(), 25.0 + phase * 12.0, Stroke::new(3.0, pal.brand_fg.gamma_multiply(0.7 * (1.0 - phase))));
            ctx.request_repaint();
        }
        // At 0:00 the button restarts the clock for another go.
        // The disc does the obvious next thing: start the first round, play,
        // pause, or (at time's up) move on to the next round.
        let courts_idle = self.state.session.courts.iter().all(|c| c.iter().all(Option::is_none));
        let deck_ready = self.state.session.deck.iter().any(|g| g.iter().any(Option::is_some));
        let advance = finished || (courts_idle && deck_ready);
        let glyph = if running { widgets::Media::Pause } else if advance { widgets::Media::Next } else { widgets::Media::Play };
        let bg = if finished { theme::hex(0xC2410C) } else { pal.brand };
        widgets::paint_media_button(ui, btn, &resp, glyph, bg);
        if resp.clicked() {
            if advance {
                self.next_round(&ctx);
            } else if !self.state.session.clock.running() {
                // Everything is stopped with the session: carry on.
                self.toggle_session_clock();
                if !matches!(timer, Timer::Running { .. }) {
                    self.state.timer_toggle(self.state.now());
                }
            } else {
                self.state.timer_toggle(now);
                self.dirty = true;
            }
        }
        ui.add_space(10.0);

        // Big digits, in fixed-width cells so they don't jiggle.
        let text = if finished { "0:00".to_owned() } else { clock(remaining) };
        // The timer is the most-glanced thing in the room: make it big.
        let size = 60.0;
        let cell = size * 0.58;
        let colon = size * 0.3;
        let width: f32 = text.chars().map(|ch| if ch == ':' { colon } else { cell }).sum();
        // Always room for "00:00", so the buttons beside it never shift.
        let (digits, digits_resp) = ui.allocate_exact_size(vec2(cell * 4.0 + colon + 8.0, 86.0), Sense::click());
        let digits_resp = digits_resp.on_hover_text("Right-click to change the time");
        let color = if finished {
            pal.warning_fg
        } else if final_minute {
            pal.warning_fg
        } else if matches!(timer, Timer::Paused { .. }) {
            pal.fg3
        } else {
            pal.fg1
        };
        let mut x = digits.center().x - width / 2.0;
        for ch in text.chars() {
            let w = if ch == ':' { colon } else { cell };
            ui.painter().text(pos2(x + w / 2.0, digits.center().y - 14.0), Align2::CENTER_CENTER, ch, semibold(size), color);
            x += w;
        }
        let session_paused = !self.state.session.clock.running();
        let (status, glyph, bg, fg) = match (finished, timer) {
            (true, _) => ("Time!", icon::SHUTTLE_F, pal.warning_subtle, pal.warning_fg),
            (_, Timer::Running { .. }) if session_paused => ("Session paused", icon::PAUSE, pal.warning_subtle, pal.warning_fg),
            (_, Timer::Running { .. }) => ("Playing", icon::SHUTTLE_F, pal.brand_subtle, pal.on_brand_subtle),
            (_, Timer::Paused { .. }) => ("Paused", icon::PAUSE, pal.subtle_selected, pal.fg2),
            (_, Timer::Idle) if self.state.session.round_number == 0 => ("Press Start round 1", icon::NEXT_F, pal.subtle_selected, pal.fg2),
            (_, Timer::Idle) => ("Not started", icon::CLOCK, pal.subtle_selected, pal.fg2),
        };
        let courts_busy = self.state.session.courts.iter().any(|c| c.iter().any(Option::is_some));
        let round = self.state.session.round_number;
        let status = if matches!(timer, Timer::Idle) && courts_busy {
            "Press play to start".to_owned()
        } else if round > 0 {
            format!("Round {round} · {status}")
        } else {
            status.to_owned()
        };
        widgets::paint_badge(ui.painter(), pos2(digits.center().x, digits.center().y + 33.0), Align2::CENTER_CENTER, Some(glyph), &status, 13.0, bg, fg);
        ui.add_space(6.0);
        let pencil = widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::EDIT), "Edit", 30.0).on_hover_text("Change the time");
        if pencil.clicked() || digits_resp.secondary_clicked() {
            self.time_edit = clock(remaining);
        }
        let menu = egui::Popup::menu(&pencil).id(unique("time-menu"));
        self.time_menu(menu, pal, remaining, now);
        let menu = egui::Popup::context_menu(&digits_resp).id(unique("time-menu-rc"));
        self.time_menu(menu, pal, remaining, now);
    }

    /// The clock's edit panel (from the pencil, or a right-click on the
    /// time): type a new time, or reset to a full round.
    fn time_menu(&mut self, popup: egui::Popup<'_>, pal: &Palette, remaining: i64, now: i64) {
        // Clicking into the box mustn't close it.
        popup.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_width(200.0);
            ui.spacing_mut().button_padding = vec2(10.0, 7.0);
            ui.label(RichText::new("Time left").font(semibold(15.0)).color(pal.fg1));
            ui.label(RichText::new("Type minutes, or m:ss").font(regular(13.0)).color(pal.fg3));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.time_edit)
                    .font(regular(18.0))
                    .margin(Margin::symmetric(10, 6))
                    .desired_width(200.0),
            );
            if !edit.has_focus() && !edit.lost_focus() && ui.memory(|m| m.focused().is_none()) {
                edit.request_focus();
            }
            let typed = parse_clock(&self.time_edit);
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let set = ui.add_enabled_ui(typed.is_some(), |ui| widgets::button(ui, pal, ButtonKind::Primary, None, "Set time", 32.0)).inner;
                if let Some(ms) = typed.filter(|_| set.clicked() || entered) {
                    self.state.timer_adjust(ms - remaining, now);
                    self.dirty = true;
                    ui.close();
                }
                if widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::REPLAY), "Reset", 32.0)
                    .on_hover_text("Put the clock back to the full round")
                    .clicked()
                {
                    self.state.timer_reset();
                    self.dirty = true;
                    ui.close();
                }
            });
        });
    }
}

// ===========================================================================
// Courts and on-deck cards

impl DropInApp {
    fn arena(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let full = ui.available_rect_before_wrap();
        let n = self.state.session.courts.len().max(1);
        let cols = if n <= 4 { n } else { n.div_ceil(2) };
        let rows = n.div_ceil(cols);
        let gap = GAP;
        // Help paragraphs above the courts and under the "Up next" heading.
        let painter = ui.painter().clone();
        // Help is a dialog over the board now; the board itself never reflows.
        let courts_help: Option<widgets::HelpLayout> = None;
        let deck_help: Option<widgets::HelpLayout> = None;
        let courts_help_h = courts_help.as_ref().map_or(0.0, |g| g.height + gap);
        let deck_help_h = deck_help.as_ref().map_or(0.0, |g| g.height + gap);
        if let Some(g) = courts_help {
            let r = Rect::from_min_size(full.min, vec2(full.width(), courts_help_h - gap));
            widgets::paint_help(&painter, pal, r, g);
        }
        // A plain "Now playing" heading over the courts.
        let now_band = Rect::from_min_size(pos2(full.left(), full.top() + courts_help_h), vec2(full.width(), 40.0));
        {
            let p = ui.painter();
            let playing = self.state.session.courts.iter().any(|c| c.iter().any(Option::is_some));
            let over = self.state.timer_finished(now);
            let t = p.text(pos2(now_band.left() + 4.0, now_band.center().y), Align2::LEFT_CENTER, "Now playing", semibold(20.0), pal.fg1);
            let paused = !self.state.session.clock.running();
            let sub = match (playing, self.state.session.timer) {
                _ if paused => "Session paused — every clock is stopped until you resume it",
                _ if over && playing => "Time! Finish the point, shake hands, and come off",
                (false, _) => "Courts are open — round 1 hasn't started yet",
                (true, Timer::Idle) => "On court now — the clock starts when the organiser presses play",
                _ => "Games in progress — find your name on a court",
            };
            let sub_ink = if paused || (over && playing) { pal.warning_fg } else { pal.fg3 };
            p.text(pos2(t.right() + 14.0, now_band.center().y), Align2::LEFT_CENTER, sub, regular(16.0), sub_ink);
        }
        let area = Rect::from_min_max(pos2(full.left(), now_band.bottom() + gap), full.max);
        let label_h = 56.0 + deck_help_h;
        let usable = area.height() - label_h;
        let idle = self.state.session.courts.iter().all(|c| c.iter().all(Option::is_none));
        // Up next stays compact (the same height whether or not anyone is on
        // court yet); the courts get the rest.
        let deck_h = (usable * 0.3).min(260.0 * rows as f32);
        let courts_h = usable - deck_h;
        let col_w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let court_row_h = (courts_h - gap * (rows as f32 - 1.0)) / rows as f32;
        let deck_row_h = (deck_h - gap * (rows as f32 - 1.0)) / rows as f32;

        let cell = |top: f32, row_h: f32, i: usize| {
            let (r, c) = (i / cols, i % cols);
            Rect::from_min_size(
                pos2(area.left() + c as f32 * (col_w + gap), top + r as f32 * (row_h + gap)),
                vec2(col_w, row_h),
            )
        };

        // Clamped so a slow frame makes the glide pause rather than jump ahead.
        let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
        for i in 0..n {
            self.court_card(ui, pal, i, cell(area.top(), court_row_h, i), now, dt);
        }

        // "Up next" heading.
        let label_y = area.top() + courts_h + 28.0;
        if let Some(g) = deck_help {
            let r = Rect::from_min_size(pos2(area.left(), area.top() + courts_h + 44.0), vec2(area.width(), deck_help_h - gap));
            widgets::paint_help(&painter, pal, r, g);
        }
        let p = ui.painter();
        let has_deck = self.state.session.deck.iter().any(|g| g.iter().any(Option::is_some));
        let finished = self.state.timer_finished(now);
        let first = self.state.session.round_number == 0 && idle && has_deck;
        // A plain heading; it only turns brand blue when calling people to their courts.
        let cta = has_deck && (finished || first);
        let label = p.text(pos2(area.left() + 4.0, label_y), Align2::LEFT_CENTER, "Up next", semibold(20.0), pal.fg1);
        let (headline, hint) = if finished && has_deck {
            ("You're up! Grab your racket and head to your court", "")
        } else if first {
            ("First games are ready — find your name below", "")
        } else {
            (if matches!(self.state.session.timer, Timer::Idle) { "Next games — you're on after this round" } else { "Get ready — you're on when the timer ends" }, "")
        };
        let sub_font = if cta { semibold(16.0) } else { regular(16.0) };
        p.text(pos2(label.right() + 14.0, label_y), Align2::LEFT_CENTER, headline, sub_font, if cta { pal.brand_fg } else { pal.fg3 });
        if !hint.is_empty() {
            p.text(pos2(area.right() - 4.0, label_y), Align2::RIGHT_CENTER, hint, semibold(16.0), pal.fg1);
        }
        let deck_top = area.top() + courts_h + label_h;
        for i in 0..n {
            self.deck_card(ui, pal, i, cell(deck_top, deck_row_h, i), now, dt);
        }
        ui.allocate_rect(full, Sense::hover());
    }

    /// One draggable/droppable player position. All effects are recorded in
    /// `self.drag` and `self.drop_targets`.
    #[allow(clippy::too_many_arguments)]
    fn slot(
        &mut self,
        ui: &mut Ui,
        pal: &Palette,
        spot: Spot,
        rect: Rect,
        font: (f32, f32),
        empty_color: Color32,
        empty_label: &str,
        dt: f32,
    ) {
        self.drop_targets.push((rect, Target::Slot(spot)));
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let hot = self.drag.is_some() && pointer.is_some_and(|p| rect.contains(p));
        match self.state.session.slot(spot) {
            Some(pid) => {
                let id = unique(("slot", format!("{spot:?}")));
                let resp = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(CursorIcon::Grab);
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    self.drag = Some(pid);
                }
                self.player_menu(&resp, pal, pid);
                let name = self.state.name(pid).to_owned();
                let skill = self.state.skill(pid);
                if self.drag == Some(pid) {
                    // Ghost left behind while dragging.
                    widgets::paint_empty_slot(ui.painter(), pal, rect, empty_color, &name, font.0 * 0.8, hot, matches!(spot, Spot::Court(..)));
                    self.note_rect(pid, rect);
                    return;
                }
                let (shown, moving) = self.anim_rect(pid, spot, rect, dt);
                let painter = if moving {
                    ui.ctx().request_repaint();
                    ui.ctx().layer_painter(egui::LayerId::new(Order::Middle, unique("flying")))
                } else {
                    ui.painter().clone()
                };
                let lift = ui.ctx().animate_bool_responsive(id.with("lift"), resp.hovered() || moving);
                let fly = ui.ctx().animate_bool_responsive(id.with("fly"), moving);
                let shown = shown.expand(lift * 2.0);
                // On court the name is all that matters; the wait line is for
                // people who are waiting.
                let detail = if matches!(spot, Spot::Court(..)) { None } else { self.since_played_label(pid) };
                widgets::paint_player_chip(&painter, pal, shown, &name, skill, font.0, fly, hot, detail.as_deref(), font.1);
            }
            None => widgets::paint_empty_slot(ui.painter(), pal, rect, empty_color, empty_label, font.0 * 0.7, hot, matches!(spot, Spot::Court(..))),
        }
    }

    /// One name size for every chip on the courts (or every chip up next), so
    /// the board reads as one even type size from across the room.
    fn group_font(&self, painter: &egui::Painter, first: Spot, size: Vec2, font: f32) -> (f32, f32) {
        let n = self.state.session.courts.len();
        let ids: Vec<PlayerId> = (0..n)
            .flat_map(|c| (0..4).map(move |s| (c, s)))
            .filter_map(|(c, s)| {
                let spot = match first {
                    Spot::Court(..) => Spot::Court(c, s),
                    Spot::Deck(..) => Spot::Deck(c, s),
                };
                self.state.session.slot(spot)
            })
            .collect();
        let names: Vec<String> = ids.iter().map(|id| self.state.name(*id).to_owned()).collect();
        // The sub-lines share one size too.
        let meta = ids
            .iter()
            .filter_map(|id| self.since_played_label(*id))
            .map(|d| widgets::chip_meta_size(painter, size, &d))
            .fold(15.0_f32, f32::min);
        let two_lines = matches!(first, Spot::Deck(..));
        (widgets::group_name_size(painter, size, &names, font, two_lines), meta)
    }

    fn court_card(&mut self, ui: &mut Ui, pal: &Palette, i: usize, rect: Rect, now: i64, dt: f32) {
        let p = ui.painter().clone();
        // The whole card is a window onto the hall: the court in 3D, lit from
        // above, with the names standing on it. Snapped to whole pixels so
        // the picture is drawn texel for pixel.
        let ppp = ui.ctx().pixels_per_point();
        let snap = |v: f32| (v * ppp).round() / ppp;
        let rect = Rect::from_min_size(pos2(snap(rect.left()), snap(rect.top())), vec2(snap(rect.width()), snap(rect.height())));
        widgets::elevate(&p, pal, rect, 8, 4);
        let view = self.hall.paint(&p, rect, pal.dark, 8);
        let at = |r: Rect| r.translate(rect.min.to_vec2());
        let window = at(view.window);

        let occupied = (0..4).any(|s| self.state.session.slot(Spot::Court(i, s)).is_some());
        let time_up = occupied && self.state.timer_finished(now);
        // While a game is on, a shuttle is in play; otherwise it lies by the net.
        let rally = occupied && !time_up && !matches!(self.state.session.timer, Timer::Paused { .. }) && self.state.session.clock.running();
        self.hall.paint_live(&p, rect, &view, i, ui.input(|i| i.time), pal.dark, rally);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(if rally { 33 } else { 66 }));
        if !rally {
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            let (c, depth) = view.cam.project([side * (hall::HALF_W + 1.25), 0.0, -0.9 + 0.5 * (i % 3) as f32]);
            let size = (view.cam.scale(depth).x * 0.34).clamp(9.0, 18.0);
            widgets::paint_shuttle(&p, c + rect.min.to_vec2(), size, 2.3 + 1.9 * i as f32, pal.dark);
        }

        // The court's sign hangs on the back wall, beside the floodlights.
        let hy = rect.top() + view.top / 2.0 + 1.0;
        widgets::court_sign(&p, pal, pos2(rect.left() + 8.0, hy), i + 1, (view.top * 0.5).round());

        // Team A stands on the far half and team B on the near half, either
        // side of the net; every card is full size whatever its distance.
        let cards = view.cards.map(at);
        let chip = cards[0].size();
        let font = self.group_font(&p, Spot::Court(i, 0), chip, (chip.y * 0.5).min(56.0));
        self.court_font = font.0;
        if occupied && !time_up {
            widgets::paint_vs(&p, pal, window.center(), (window.height() * 0.3).clamp(12.0, 19.0));
        }
        if !occupied && self.drag.is_none() {
            // A free court gets one friendly sign instead of four empty slots
            // (they come back as drop targets while dragging). It hangs over
            // the far half, leaving the net and the near court in view.
            for (s, r) in cards.into_iter().enumerate() {
                self.drop_targets.push((r, Target::Slot(Spot::Court(i, s))));
            }
            let far = cards[0].union(cards[1]);
            let sign = Rect::from_center_size(far.center(), vec2((rect.width() * 0.6).max(200.0).min(rect.width() - 16.0), far.height().clamp(72.0, 104.0)));
            let sign = sign.translate(vec2(0.0, (far.top() - sign.top()).max(0.0)));
            p.rect_filled(sign.translate(vec2(0.0, 3.0)), CornerRadius::same(12), Color32::from_black_alpha(60));
            p.rect_filled(sign, CornerRadius::same(12), if pal.dark { theme::hex(0x1B3A5C) } else { theme::hex(0x2B5C8F) });
            p.rect_stroke(sign, CornerRadius::same(12), Stroke::new(2.0, Color32::from_white_alpha(200)), StrokeKind::Inside);
            p.rect_stroke(sign, CornerRadius::same(12), Stroke::new(1.0, Color32::from_white_alpha(70)), StrokeKind::Inside);
            let k = (sign.height() / 110.0).clamp(0.72, 1.4);
            p.text(sign.center() - vec2(0.0, 26.0 * k), Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(30.0 * k), Color32::from_white_alpha(230));
            p.text(sign.center() + vec2(0.0, 6.0 * k), Align2::CENTER_CENTER, "Court open", semibold(20.0 * k), Color32::WHITE);
            let hint = if self.state.session.deck[i].iter().any(Option::is_some) {
                "Up first: see Up next below"
            } else {
                "Drag players here to start a game"
            };
            let hint_size = widgets::fit_font(ui, hint, regular, 15.0 * k, sign.width() - 28.0);
            p.text(sign.center() + vec2(0.0, 30.0 * k), Align2::CENTER_CENTER, hint, regular(hint_size), Color32::from_white_alpha(210));
            return;
        }
        for (s, r) in cards.into_iter().enumerate() {
            self.slot(ui, pal, Spot::Court(i, s), r, font, Color32::from_white_alpha(140), "Open", dt);
        }
        if time_up {
            // Game over on this court: wash it out and stamp it.
            p.rect_filled(rect, CornerRadius::same(8), theme::hex(0xC2410C).gamma_multiply(if pal.dark { 0.18 } else { 0.14 }));
            let tilt = [-0.07, 0.05, -0.04, 0.06][i % 4];
            let size = (chip.y * 0.36).clamp(16.0, 30.0);
            widgets::paint_stamp(&p, window.center(), "TIME!", size, tilt, theme::hex(0xC2410C), Color32::WHITE);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn deck_card(&mut self, ui: &mut Ui, pal: &Palette, i: usize, rect: Rect, now: i64, dt: f32) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        if self.state.timer_finished(now) && self.state.session.deck[i].iter().any(Option::is_some) {
            // Time's up: these four are walking on — ring their group.
            let t = ui.input(|i| i.time) as f32;
            let a = 0.6 + 0.4 * (t * std::f32::consts::TAU * 0.9).sin();
            p.rect_stroke(rect.shrink(1.0), CornerRadius::same(8), Stroke::new(3.0, pal.brand_fg.gamma_multiply(a)), StrokeKind::Inside);
        }
        widgets::card(&p, pal, rect, pal.bg1, 8, 2);
        let group = self.state.session.deck[i];
        let filled = group.iter().flatten().count();

        let header_h = 46.0;
        let hfont = 21.0;
        let hy = rect.top() + header_h / 2.0 + 2.0;
        let title = widgets::court_title_next(&p, pal, pos2(rect.left() + 14.0, hy), i + 1, hfont);
        // Only a group that isn't complete needs a word.
        let pill = match filled {
            0 => Some(("Waiting for players".to_owned(), None, pal.subtle_selected, pal.fg2)),
            4 => None,
            k => Some((format!("{k} of 4"), Some(icon::PERSON), pal.warning_subtle, pal.warning_fg)),
        };
        if let Some((status, glyph, bg, fg)) = pill {
            widgets::paint_badge(&p, pos2(title.right() + 10.0, hy), Align2::LEFT_CENTER, glyph, &status, CHIP_TEXT, bg, fg);
        }
        let pointing = ui.ctx().pointer_hover_pos().is_some_and(|p| rect.contains(p));
        if filled > 0 && self.state.session.round_number > 0 && pointing {
            let b = Rect::from_min_max(pos2(rect.right() - 118.0, hy - 15.0), pos2(rect.right() - 10.0, hy + 15.0));
            let mut bui = ui.new_child(UiBuilder::new().max_rect(b).layout(Layout::right_to_left(Align::Center)));
            let resp = widgets::button(&mut bui, pal, ButtonKind::Secondary, Some(icon::ARROW_UP_F), "Play now", 34.0)
                .on_hover_text(format!("Send this group onto court {} now", i + 1));
            if resp.clicked() {
                self.act(&ctx, format!("Court {} started early", i + 1), |s| {
                    let now = s.now();
                    s.promote_deck(i, now);
                    s.fill_deck(now);
                });
            }
        }

        // Like the court: one team above the "net", the other below,
        // partners side by side.
        let body = Rect::from_min_max(
            pos2(rect.left() + 14.0, rect.top() + header_h + 2.0),
            pos2(rect.right() - 14.0, rect.bottom() - 14.0),
        );
        // Teams one above the other, partners side by side; no net.
        let net_h = 8.0;
        let col_gap = 8.0;
        let chip_h = ((body.height() - net_h) / 2.0).clamp(24.0, 150.0);
        let chip_w = (body.width() - col_gap) / 2.0;
        // Never louder than the courts: they're what matters most.
        let font = self.group_font(&p, Spot::Deck(i, 0), vec2(chip_w, chip_h), (chip_h * 0.38).min(32.0));
        let top = body.center().y - chip_h - net_h / 2.0;
        for s in 0..4 {
            let row = s / 2;
            let col = s % 2;
            let x = body.left() + col as f32 * (chip_w + col_gap);
            let y = top + row as f32 * (chip_h + net_h);
            let r = Rect::from_min_size(pos2(x, y), vec2(chip_w, chip_h));
            self.slot(ui, pal, Spot::Deck(i, s), r, font, pal.stroke_accessible, "Open", dt);
        }
    }

    // ----- Drag and drop --------------------------------------------------

    fn drag_overlay(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(pid) = self.drag else { return };
        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
        ctx.set_cursor_icon(CursorIcon::Grabbing);
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, unique("drag")));
        let name = self.state.name(pid).to_owned();
        let rect = Rect::from_center_size(pos, vec2(240.0, 56.0));
        widgets::paint_player_chip(&painter, pal, rect, &name, self.state.skill(pid), 22.0, 1.0, false, None, 14.0);
        ctx.request_repaint();
    }

    fn resolve_drop(&mut self, ctx: &egui::Context) {
        let Some(pid) = self.drag else { return };
        let (released, down, esc, pos) = ctx.input(|i| {
            (
                i.pointer.any_released(),
                i.pointer.primary_down(),
                i.key_pressed(Key::Escape),
                i.pointer.latest_pos(),
            )
        });
        if esc {
            self.drag = None;
            return;
        }
        if !(released || !down) {
            return;
        }
        self.drag = None;
        let Some(pos) = pos else { return };
        // Later targets (court/deck slots) take precedence over the pool area.
        let target = self
            .drop_targets
            .iter()
            .rev()
            .find(|(r, _)| r.contains(pos))
            .map(|(_, t)| *t);
        if let Some(target) = target {
            let now = self.state.now();
            self.state.move_player(pid, target, now);
            // Settle the chip from where it was dropped.
            self.note_rect(pid, Rect::from_center_size(pos, vec2(240.0, 56.0)));
            self.dirty = true;
        }
    }
}

// ===========================================================================
// Waiting pool

impl DropInApp {
    /// The right panel's tabs: the waiting list, or the roster for check-in.
    fn side_tabs(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let waiting = self.state.waiting_list(now).iter().filter(|id| !self.state.session.present[id].resting).count();
        let tabs = [
            (SideTab::Waiting, "Waiting", Some(waiting)),
            (SideTab::Roster, "Roster", Some(self.state.session.present.len())),
            (SideTab::AddPlayer, "Add player", None),
        ];
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, pal.stroke2));
        let mut x = bar.left();
        for (tab, label, count) in tabs {
            let p = ui.painter();
            let selected = self.side_tab == tab;
            let text = p.layout_no_wrap(label.to_owned(), semibold(20.0), pal.fg1);
            // "Add player" leads with a + instead of trailing a count.
            let lead = if count.is_none() { 24.0 } else { 0.0 };
            let badge_w = count.map_or(0.0, |n| p.layout_no_wrap(n.to_string(), semibold(13.0), pal.fg1).size().x + 13.0 * 1.1 + 8.0);
            let w = lead + text.size().x + badge_w + 20.0;
            let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
            let resp = ui.interact(rect, unique(("side-tab", label)), Sense::click());
            let ink = if selected { pal.fg1 } else if resp.hovered() { pal.fg2 } else { pal.fg3 };
            let p = ui.painter();
            if lead > 0.0 {
                p.text(pos2(rect.left() + 20.0, rect.center().y - 1.0), Align2::CENTER_CENTER, icon::PERSON_ADD, icon_font(20.0), if selected { pal.brand_fg } else { ink });
            }
            let text_pos = pos2(rect.left() + 10.0 + lead, rect.center().y - text.size().y / 2.0 - 2.0);
            let text_right = text_pos.x + text.size().x;
            p.galley_with_override_text_color(text_pos, text, ink);
            let (bg, fg) = if selected { (pal.brand_subtle, pal.on_brand_subtle) } else { (pal.subtle_selected, pal.fg3) };
            if let Some(n) = count {
                widgets::paint_badge(p, pos2(text_right + 8.0, rect.center().y - 2.0), Align2::LEFT_CENTER, None, &n.to_string(), 13.0, bg, fg);
            }
            if selected {
                let line = Rect::from_min_max(pos2(rect.left() + 8.0, bar.bottom() - 3.0), pos2(rect.right() - 8.0, bar.bottom()));
                p.rect_filled(line, CornerRadius::same(2), pal.brand_fg);
            }
            if resp.clicked() {
                self.side_tab = tab;
            }
            x = rect.right() + 4.0;
        }
        ui.add_space(8.0);
    }

    fn pool(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        let panel = ui.max_rect().expand(16.0);
        self.drop_targets.push((panel, Target::Pool));
        let dragging_from_slot = self.drag.is_some_and(|id| self.state.is_assigned(id));
        if dragging_from_slot {
            let hot = ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| panel.contains(p));
            let zone = panel.shrink(4.0);
            ui.painter().rect_filled(zone, CornerRadius::same(6), pal.brand_fg.gamma_multiply(if hot { 0.14 } else { 0.05 }));
            ui.painter().rect_stroke(
                zone,
                CornerRadius::same(6),
                Stroke::new(if hot { 2.0 } else { 1.0 }, pal.brand_fg.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        }

        let waiting = self.state.waiting_list(now);
        let (active, resting): (Vec<PlayerId>, Vec<PlayerId>) = waiting
            .into_iter()
            .partition(|id| !self.state.session.present[id].resting);

        ui.label(RichText::new("Longest since last game first").font(regular(14.0)).color(pal.fg3));
        ui.add_space(6.0);

        if self.state.session.present.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(icon::GROUP).font(icon_font(56.0)).color(pal.fg3));
                ui.label(RichText::new("No one is checked in").font(semibold(17.0)));
                ui.label(
                    RichText::new("Open the roster to check players in or add new ones.")
                        .font(regular(14.0))
                        .color(pal.fg3),
                );
                ui.add_space(8.0);
                if widgets::button(ui, pal, ButtonKind::Primary, Some(icon::GROUP), "Open roster", 40.0).clicked() {
                    self.side_tab = SideTab::Roster;
                }
            });
            return;
        }

        // A departure board: one row style for everyone, big enough to read
        // from the bench, longest wait at the top. One scrollable list.
        let all: Vec<(PlayerId, Option<usize>)> = active
            .iter()
            .enumerate()
            .map(|(k, id)| (*id, Some(k + 1)))
            .chain(resting.iter().map(|id| (*id, None)))
            .collect();
        // Search: narrows the list, but everyone keeps their real place in line.
        ui.add(
            egui::TextEdit::singleline(&mut self.waiting_search)
                .hint_text(format!("{}  Search waiting players", icon::SEARCH))
                .font(regular(15.0))
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        let query = self.waiting_search.trim().to_lowercase();
        let filtering = !query.is_empty();
        let all: Vec<(PlayerId, Option<usize>)> = all
            .into_iter()
            .filter(|(id, _)| !filtering || self.state.name(*id).to_lowercase().contains(&query))
            .collect();
        // Rows get as tall as the pane allows (up to 50) and shrink, down to a
        // readable 30, so the whole queue fits on screen before it has to scroll.
        let footer = 32.0;
        let room = ui.available_height() - footer;
        const ROW_MAX: f32 = 50.0;
        const ROW_MIN: f32 = 30.0;
        let row = (room / all.len().max(1) as f32).floor().clamp(ROW_MIN, ROW_MAX);
        let fits_all = row * all.len() as f32 <= room;
        let footer = if fits_all { 0.0 } else { footer };
        let list_h = if fits_all { room + 30.0 } else { ((room / row).floor() * row).max(row) };
        #[allow(non_snake_case)]
        let ROW = row;
        let (list, _) = ui.allocate_exact_size(vec2(ui.available_width(), list_h), Sense::hover());
        let mut outer = ui.new_child(UiBuilder::new().max_rect(list).layout(Layout::top_down(Align::Min)));
        outer.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
        let scrolled = egui::ScrollArea::vertical().id_salt("waiting").auto_shrink([false, false]).show(&mut outer, |lui| {
        lui.spacing_mut().item_spacing.y = 0.0;
        if filtering && all.is_empty() {
            lui.add_space(24.0);
            lui.vertical_centered(|ui| {
                ui.label(RichText::new(icon::SEARCH).font(icon_font(34.0)).color(pal.fg3));
                ui.label(RichText::new("No one waiting matches").font(semibold(16.0)).color(pal.fg1));
                ui.label(RichText::new("Check the Roster tab, or they may be on a court.").font(regular(14.0)).color(pal.fg3));
            });
        } else if active.is_empty() {
            lui.add_space(24.0);
            lui.vertical_centered(|ui| {
                ui.label(RichText::new(icon::SHUTTLE_F).font(icon_font(40.0)).color(pal.brand_fg.gamma_multiply(0.6)));
                ui.label(RichText::new("Nobody's waiting").font(semibold(16.0)).color(pal.fg1));
                ui.label(RichText::new("Everyone is on a court or up next.").font(regular(14.0)).color(pal.fg3));
            });
        }
        for (k, (id, rank)) in all.iter().enumerate() {
            if k % 2 == 1 {
                let r = Rect::from_min_size(lui.cursor().min, vec2(lui.available_width(), ROW));
                lui.painter().rect_filled(r, CornerRadius::same(6), pal.bg2.gamma_multiply(0.7));
            }
            self.pool_row(lui, pal, *id, *rank, now, ROW);
        }
        if !filtering && (1..=6).contains(&all.len()) {
            lui.add_space(28.0);
            lui.vertical_centered(|ui| {
                ui.label(RichText::new(icon::SHUTTLE_F).font(icon_font(34.0)).color(pal.brand_fg.gamma_multiply(0.45)));
                ui.label(RichText::new("Short bench tonight").font(semibold(15.0)).color(pal.fg2));
                let note = if self.state.session.round_number == 0 { "Everyone else is in a first game below." } else { "Everyone else is playing or up next." };
                ui.label(RichText::new(note).font(regular(13.0)).color(pal.fg3));
            });
        }
        });
        let more = scrolled.content_size.y - scrolled.inner_rect.height() - scrolled.state.clamped_offset().y;
        // Footer: how many more are below (only when the list has to scroll).
        if footer <= 0.0 {
            return;
        }
        let (foot, _) = ui.allocate_exact_size(vec2(ui.available_width(), footer), Sense::hover());
        let p = ui.painter();
        // Under the list: how many more are below (it scrolls).
        p.line_segment([pos2(foot.left(), foot.top() + 1.0), pos2(foot.right(), foot.top() + 1.0)], Stroke::new(1.0, pal.stroke2));
        if more > 4.0 {
            let hidden = (more / ROW).ceil() as usize;
            p.text(pos2(foot.left() + 4.0, foot.top() + 16.0), Align2::LEFT_CENTER, format!("{} {hidden} more below — scroll the list", icon::CHEVRON_DOWN), semibold(14.0), pal.fg2);
        } else {
            p.text(pos2(foot.left() + 4.0, foot.top() + 16.0), Align2::LEFT_CENTER, "Everyone waiting is shown", regular(14.0), pal.fg3);
        }
    }

    fn pool_row(&mut self, ui: &mut Ui, pal: &Palette, id: PlayerId, rank: Option<usize>, now: i64, row_h: f32) {
        let ctx = ui.ctx().clone();
        let compact = true;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click_and_drag());
        let resp = resp.on_hover_cursor(CursorIcon::Grab);
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.drag = Some(id);
        }
        self.player_menu(&resp, pal, id);
        self.note_rect(id, rect);
        let presence = self.state.session.present[&id].clone();
        let skill = self.state.skill(id);
        let name = self.state.name(id).to_owned();
        let skipping = self.state.session.skip_round.contains(&id);
        let dimmed = presence.resting || self.drag == Some(id);
        let hover = ctx.animate_bool_responsive(resp.id.with("h"), resp.hovered());
        // Before the night's first round nobody can be "overdue" yet.
        let overdue = self.state.session.round_number > 0 && !presence.resting && presence.waiting_ms(now) >= self.state.settings.max_wait_ms();

        let p = ui.painter();
        let fill = theme::lerp_color(Color32::TRANSPARENT, pal.subtle_hover, hover);
        p.rect_filled(rect, CornerRadius::same(4), fill);
        let alpha = if dimmed { 0.45 } else { 1.0 };
        let cy = rect.center().y;
        let mut x = rect.left() + 4.0;
        // Position in a solid block, timing-tower style; the next three in brand blue.
        let k = (row_h / 50.0).clamp(0.6, 1.0);
        let block = Rect::from_center_size(pos2(x + 16.0, cy), vec2(34.0, (row_h - 8.0).min(36.0)));
        match rank {
            Some(rank) => {
                let (bg, fg) = (pal.subtle_selected, pal.fg1);
                p.rect_filled(block, CornerRadius::same(6), bg.gamma_multiply(alpha));
                p.text(block.center(), Align2::CENTER_CENTER, rank.to_string(), theme::bold((18.0 * k).max(14.0).round()), fg);
            }
            None => {
                p.text(block.center(), Align2::CENTER_CENTER, icon::COFFEE, icon_font(18.0), pal.fg3);
            }
        }
        x = block.right() + 10.0;
        let badge = (24.0 * k).max(18.0).round();
        let b = Rect::from_min_size(pos2(x, cy - badge / 2.0), vec2(widgets::badge_width(badge), badge));
        widgets::paint_skill_badge(p, pal, b, skill);
        if dimmed {
            p.rect_filled(b, CornerRadius::same(6), pal.bg1.gamma_multiply(0.55));
        }
        x = b.right() + 10.0;
        let mut detail = self.since_played_label(id).unwrap_or_default();
        if skipping && !presence.resting {
            detail.push_str(" · sitting out");
        }
        let right = rect.right() - 6.0 - if overdue { 22.0 } else { 0.0 };
        if compact {
            // One line: name on the left; on the right the wait (big) over the
            // games (small). The hover actions take that spot, so it fades for them.
            let fade = alpha * (1.0 - if presence.resting { 1.0 } else { hover });
            let detail_color = if overdue { pal.warning_fg } else { pal.fg2 };
            let wait = self.shown_wait_ms(id, now).map(short_duration).unwrap_or_default();
            let games = self.state.session.present.get(&id).map_or(0, |p| p.games_tonight);
            let games = if games == 1 { "1 game".to_owned() } else { format!("{games} games") };
            let (top, bottom) = if presence.resting {
                ("On a break".to_owned(), games)
            } else {
                (wait, if skipping { "sitting out".to_owned() } else { games })
            };
            let name_size = (21.0 * k).max(17.0).round();
            if row_h >= 40.0 {
                let tg = p.layout_no_wrap(top, semibold(18.0), detail_color.gamma_multiply(fade));
                let bg = p.layout_no_wrap(bottom, regular(13.0), pal.fg2.gamma_multiply(fade));
                let col = tg.size().x.max(bg.size().x);
                let name_g = widgets::elided(p, &name, semibold(name_size), pal.fg1.gamma_multiply(alpha), right - x - col - 12.0);
                let p = ui.painter();
                p.galley(pos2(x, cy - name_g.size().y / 2.0), name_g, pal.fg1);
                p.galley(pos2(right - tg.size().x, cy - tg.size().y + 3.0), tg, detail_color);
                p.galley(pos2(right - bg.size().x, cy + 1.0), bg, pal.fg3);
            } else {
                // Shorter rows: wait and games on one line.
                let mut job = egui::text::LayoutJob::default();
                job.append(&top, 0.0, egui::TextFormat { font_id: semibold(16.0), color: detail_color.gamma_multiply(fade), valign: Align::Center, ..Default::default() });
                job.append(&format!(" · {bottom}"), 0.0, egui::TextFormat { font_id: regular(13.0), color: pal.fg2.gamma_multiply(fade), valign: Align::Center, ..Default::default() });
                let dg = p.layout_job(job);
                let name_g = widgets::elided(p, &name, semibold(name_size), pal.fg1.gamma_multiply(alpha), right - x - dg.size().x - 12.0);
                let p = ui.painter();
                p.galley(pos2(x, cy - name_g.size().y / 2.0), name_g, pal.fg1);
                p.galley(pos2(right - dg.size().x, cy - dg.size().y / 2.0), dg, detail_color);
            }
        } else {
            let detail_color = if overdue { pal.warning_fg } else { pal.fg3 };
            let p = ui.painter();
            let g = widgets::elided(p, &name, semibold(18.0), pal.fg1.gamma_multiply(alpha), right - x);
            p.galley(pos2(x, cy - 9.0 - g.size().y / 2.0), g, pal.fg1);
            p.text(pos2(x, cy + 13.0), Align2::LEFT_CENTER, detail, regular(13.0), detail_color.gamma_multiply(alpha));
        }
        // Overdue players get an hourglass.
        if overdue {
            ui.painter().text(pos2(rect.right() - 6.0, cy), Align2::RIGHT_CENTER, icon::HOURGLASS_F, icon_font(17.0), pal.warning_fg.gamma_multiply(1.0 - hover));
        }

        // Hover actions.
        if hover > 0.01 || presence.resting {
            let b2 = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(32.0));
            let b1 = b2.translate(vec2(-34.0, 0.0));
            let r1 = ui
                .interact(b1, unique(("rest", id)), Sense::click())
                .on_hover_text(if presence.resting { "Back in the queue" } else { "Take a break" });
            let r2 = ui.interact(b2, unique(("out", id)), Sense::click()).on_hover_text("Check out (leaving)");
            // Background so the hourglass doesn't show through.
            let behind = if hover > 0.01 { pal.subtle_hover } else { pal.bg1 };
            ui.painter().rect_filled(b1.union(b2).expand(2.0), CornerRadius::same(4), behind);
            widgets::paint_icon_button(ui, pal, b1, &r1, if presence.resting { icon::LOGIN } else { icon::COFFEE }, false, None);
            widgets::paint_icon_button(ui, pal, b2, &r2, icon::LOGOUT, false, None);
            if r1.clicked() {
                self.state.set_resting(id, !presence.resting, now);
                self.dirty = true;
            }
            if r2.clicked() {
                self.act(&ctx, format!("{name} checked out"), |s| s.check_out(id, now));
            }
        }
    }
}

// ===========================================================================
// Roster pane

impl DropInApp {
    /// The sidebar's Add player tab: a new player's details, then add them
    /// (checked in, or just to the roster).
    fn add_player_form(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        ui.label(RichText::new("New to the club? Add them here.").font(regular(14.0)).color(pal.fg3));
        ui.add_space(10.0);
        let field = |ui: &mut Ui, label: &str| {
            ui.label(RichText::new(label).font(semibold(14.0)).color(pal.fg2));
            ui.add_space(2.0);
        };
        fn text<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
            egui::TextEdit::singleline(text)
                .hint_text(hint.to_owned())
                .font(regular(16.0))
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY)
        }
        let mut entered = false;
        field(ui, "Name");
        let name_box = ui.add(text(&mut self.new_name, "First and last name"));
        entered |= name_box.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(12.0);

        field(ui, "Skill level");
        // A 2 × 2 choice of equal-width options.
        let w = (ui.available_width() - 6.0) / 2.0;
        for pair in Skill::ALL.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for &skill in pair {
                    if widgets::skill_option(ui, pal, skill, self.new_skill == skill, w).clicked() {
                        self.new_skill = skill;
                    }
                }
            });
        }
        ui.add_space(12.0);

        field(ui, "Where are they from?");
        let from_box = ui.add(text(&mut self.new_from, "Neighbourhood or town"));
        entered |= from_box.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(12.0);

        field(ui, "How did they find us?");
        let via_box = ui.add(text(&mut self.new_found_via, "Type, or pick one below"));
        entered |= via_box.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(6.0);
        // The usual answers, one tap each.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for choice in ["A friend", "Instagram", "Facebook", "Heard about it at church", "Google", "Walked by"] {
                let picked = self.new_found_via.trim().eq_ignore_ascii_case(choice);
                let fill = if picked { pal.brand_subtle } else { pal.bg1 };
                let ink = if picked { pal.on_brand_subtle } else { pal.fg2 };
                let chip = egui::Button::new(RichText::new(choice).font(regular(14.0)).color(ink))
                    .fill(fill)
                    .stroke(Stroke::new(1.0, if picked { pal.brand_fg } else { pal.stroke1 }))
                    .corner_radius(14)
                    .min_size(vec2(0.0, 28.0));
                if ui.add(chip).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.new_found_via = if picked { String::new() } else { choice.to_owned() };
                }
            }
        });
        ui.add_space(14.0);

        // Regulars: checked in to every new session without asking.
        ui.horizontal(|ui| {
            widgets::switch(ui, pal, &mut self.new_auto);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Auto check-in").font(semibold(14.0)).color(pal.fg1));
                ui.label(RichText::new("Checked in at the start of every new session").font(regular(13.0)).color(pal.fg3));
            });
        });
        ui.add_space(16.0);

        let (check_in, roster_only) = ui
            .with_layout(Layout::top_down_justified(Align::Min), |ui| {
                let a = widgets::button(ui, pal, ButtonKind::Primary, Some(icon::PERSON_ADD), "Add & check in", 40.0).clicked();
                ui.add_space(2.0);
                let b = widgets::button(ui, pal, ButtonKind::Secondary, None, "Add to the roster only", 36.0).clicked();
                (a, b)
            })
            .inner;
        if !(entered || check_in || roster_only) {
            return;
        }
        let name = self.new_name.trim().to_owned();
        if name.is_empty() {
            // Always looks ready; with no name yet it just puts you in the name box.
            name_box.request_focus();
            return;
        }
        let (from, found_via) = (self.new_from.trim().to_owned(), self.new_found_via.trim().to_owned());
        let exists = self.state.players.values().find(|p| p.name.eq_ignore_ascii_case(&name)).map(|p| p.id);
        let id = exists.unwrap_or_else(|| self.state.add_player(&name, self.new_skill));
        if let Some(p) = self.state.players.get_mut(&id) {
            // Fill in what we didn't know about someone already on the roster.
            if p.from.is_empty() {
                p.from = from;
            }
            if p.found_via.is_empty() {
                p.found_via = found_via;
            }
            p.auto_check_in |= self.new_auto;
        }
        let checking_in = !roster_only;
        if checking_in {
            self.state.check_in(id, now);
        }
        self.dirty = true;
        self.new_name.clear();
        self.new_from.clear();
        self.new_found_via.clear();
        self.new_auto = false;
        let message = match (exists.is_some(), checking_in) {
            (true, true) => format!("{name} is already on the roster — checked in"),
            (true, false) => format!("{name} is already on the roster"),
            (false, true) => format!("{name} added and checked in"),
            (false, false) => format!("{name} added to the roster"),
        };
        self.toast(&ctx, message);
        name_box.request_focus();
    }

    fn roster(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let extra = format!("{} checked in · {} players", self.state.session.present.len(), self.state.players.len());
        ui.label(RichText::new(extra).font(regular(14.0)).color(pal.fg3));
        ui.add_space(6.0);

        // --- Search ---
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(format!("{}  Search players", icon::SEARCH))
                .font(regular(15.0))
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(2.0);

        // Column header, so the switch and badge explain themselves.
        ui.horizontal(|ui| {
            let cap = |t: &str| RichText::new(t).font(semibold(13.0)).color(pal.fg3);
            ui.add_space(12.0);
            ui.label(cap("Here"));
            ui.add_space(24.0);
            ui.label(cap("Player"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(12.0);
                ui.label(cap("Waited · games"));
            });
        });
        let footer_h = 52.0;
        // Whole rows only, so the list never ends on half a name.
        let list_h = ((ui.available_height() - footer_h) / 44.0).floor() * 44.0;
        let query = self.search.trim().to_lowercase();
        let mut players: Vec<Player> = self
            .state
            .players
            .values()
            .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
        players.sort_by_key(|p| (!self.state.session.is_here(p.id), p.name.to_lowercase()));

        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
        let out = egui::ScrollArea::vertical()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if players.is_empty() {
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new(if self.state.players.is_empty() {
                            "Your roster is empty. Add the first player from the Add player tab."
                        } else {
                            "No matches."
                        })
                        .color(pal.fg3),
                    );
                }
                for player in &players {
                    self.roster_row(ui, pal, player, now);
                }
            });
        let _ = out;

        // --- Footer ---
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::CALENDAR), "New session…", 36.0)
                    .on_hover_text("Put this session into History and start a new one")
                    .clicked()
                {
                    self.session_form = Some(SessionForm::at(wall_ms(), true, false));
                }
            });
            widgets::hdivider(ui, pal);
        });
    }

    fn roster_row(&mut self, ui: &mut Ui, pal: &Palette, player: &Player, now: i64) {
        let ctx = ui.ctx().clone();
        let id = player.id;
        let here = self.state.session.is_here(id);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
        let drag = ui
            .interact(rect, unique(("roster", id)), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);
        if drag.drag_started_by(egui::PointerButton::Primary) {
            self.drag = Some(id);
        }
        self.player_menu(&drag, pal, id);
        let hover = ctx.animate_bool_responsive(drag.id.with("h"), drag.hovered());
        // Plain rows: the switch says who's here, and absent names are muted.
        let fill = theme::lerp_color(Color32::TRANSPARENT, pal.subtle_hover, hover);
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);

        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(10.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 10.0;

        let mut on = here;
        if widgets::switch(ui, pal, &mut on)
            .on_hover_text(if here { "Check out" } else { "Check in" })
            .changed()
        {
            if on {
                self.state.check_in(id, now);
                self.dirty = true;
            } else {
                let name = player.name.clone();
                self.act(&ctx, format!("{name} checked out"), |s| s.check_out(id, now));
            }
        }

        let pill = widgets::skill_pill(ui, pal, player.skill, true, true);
        self.player_menu(&pill, pal, id);
        egui::Popup::menu(&pill).show(|ui| {
            ui.set_min_width(170.0);
            for skill in Skill::ALL {
                let selected = skill == player.skill;
                let text = widgets::skill_menu_text(pal, skill).font(semibold(15.0));
                if ui.selectable_label(selected, text).clicked() {
                    if let Some(p) = self.state.players.get_mut(&id) {
                        p.skill = skill;
                    }
                    self.dirty = true;
                }
            }
        });

        let renaming = self.renaming.as_ref().is_some_and(|(r, ..)| *r == id);
        if renaming {
            let (_, buf, needs_focus) = self.renaming.as_mut().unwrap();
            let resp = ui
                .add(
                    egui::TextEdit::singleline(buf)
                        .font(semibold(15.0))
                        .desired_width(ui.available_width() - 50.0),
                )
                .on_hover_text("Enter to save · Esc or click away to cancel");
            if *needs_focus {
                // Focus once; requesting it every frame would stop the box
                // from ever losing focus (so Enter/Esc/click-away did nothing).
                resp.request_focus();
                *needs_focus = false;
            } else if resp.lost_focus() || !resp.has_focus() {
                // Enter saves; Esc or clicking anywhere else cancels.
                let (_, name, _) = self.renaming.take().unwrap();
                let name = name.trim();
                let save = ui.input(|i| i.key_pressed(Key::Enter));
                if save && !name.is_empty() && name != player.name {
                    if let Some(p) = self.state.players.get_mut(&id) {
                        p.name = name.to_owned();
                        self.dirty = true;
                    }
                }
            }
        } else {
            let name_w = ui.available_width() - 96.0;
            let color = if here { pal.fg1 } else { pal.fg3 };
            // Same size for every name; a long one ends in "…".
            let name = ui
                .scope(|ui| {
                    ui.set_max_width(name_w.max(40.0));
                    ui.add(
                        egui::Label::new(RichText::new(&player.name).font(semibold(16.0)).color(color))
                            .truncate()
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                })
                .inner
                .on_hover_text("Double-click to rename · right-click for more");
            self.player_menu(&name, pal, id);
            if name.double_clicked() {
                self.renaming = Some((id, player.name.clone(), true));
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if hover > 0.01 {
                if widgets::icon_button(ui, pal, icon::DELETE, 32.0, false)
                    .on_hover_text("Remove from roster")
                    .clicked()
                {
                    self.confirm = Some(Confirm::Delete(id));
                }
            } else {
                // Checked in: the short form, "40m • 4 games" (tonight).
                // Not here: their all-time total.
                let plural = |g: u32| if g == 1 { "1 game".to_owned() } else { format!("{g} games") };
                let text = match (self.shown_wait_ms(id, now), self.state.session.present.get(&id)) {
                    (Some(ms), Some(p)) => format!("{} · {}", short_duration(ms), plural(p.games_tonight)),
                    _ => format!("{} total", plural(player.lifetime_games)),
                };
                ui.label(
                    RichText::new(text)
                        .font(regular(12.5))
                        .color(pal.fg3),
                );
            }
        });
    }
}

// ===========================================================================
// Dialogs & toast

impl DropInApp {
    fn dialog_frame(pal: &Palette) -> Frame {
        Frame::NONE
            .fill(pal.bg1)
            .stroke(Stroke::new(1.0, pal.card_stroke))
            .corner_radius(8)
            .inner_margin(Margin::same(24))
            .shadow(pal.shadow(64))
    }

    /// Dialog title row with a close button. Returns true if closed.
    fn dialog_title(ui: &mut Ui, pal: &Palette, title: &str, extra: Option<String>) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            let glyph = match title {
                "Settings" => icon::SETTINGS,
                "History" => icon::HISTORY,
                "Rename player" => icon::EDIT,
                "Roster" => icon::GROUP,
                _ => icon::SHUTTLE_F,
            };
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            ui.painter().rect_filled(tile, CornerRadius::same(7), pal.brand);
            ui.painter().text(tile.center(), Align2::CENTER_CENTER, glyph, icon_font(19.0), pal.on_brand);
            ui.add_space(4.0);
            ui.label(RichText::new(title).font(semibold(20.0)));
            if let Some(extra) = extra {
                ui.label(RichText::new(extra).font(regular(14.0)).color(pal.fg3));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, pal, icon::CLOSE, 32.0, false).on_hover_text("Close").clicked() {
                    close = true;
                }
            });
        });
        close
    }

    fn settings_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.settings_open {
            return;
        }
        let now = self.state.now();
        let before = self.state.settings.clone();
        let modal = Modal::new(unique("settings"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(580.0);
                if Self::dialog_title(ui, pal, "Settings", None) {
                    self.settings_open = false;
                }
                if self.help {
                    widgets::help_bar(ui, pal, &help_settings());
                    ui.add_space(4.0);
                }
                ui.add_space(12.0);
                let s = &mut self.state.settings;

                let row = |ui: &mut Ui, title: &str, desc: &str, control: &mut dyn FnMut(&mut Ui)| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(46.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add_space(6.0);
                            ui.label(RichText::new(title).font(regular(15.0)).color(pal.fg1));
                            ui.label(RichText::new(desc).font(regular(13.0)).color(pal.fg3));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| control(ui));
                    });
                };

                widgets::section_label(ui, pal, "Session");
                let mut courts = s.courts as u32;
                row(ui, "Courts", "Doubles courts in play", &mut |ui| {
                    widgets::stepper(ui, pal, &mut courts, 1, Settings::MAX_COURTS as u32, 1, "");
                });
                s.courts = courts as usize;
                row(ui, "Round length", "Timer duration for each game", &mut |ui| {
                    widgets::stepper(ui, pal, &mut s.round_minutes, 1, 90, 1, "min");
                });
                row(ui, "Auto-start timer", "Start the clock right after “Next round”", &mut |ui| {
                    widgets::switch_labeled(ui, pal, &mut s.auto_start_timer);
                });

                widgets::hdivider(ui, pal);
                widgets::section_label(ui, pal, "Matchmaking");
                row(ui, "Match by skill", "Group players of similar level together", &mut |ui| {
                    widgets::switch_labeled(ui, pal, &mut s.skill_matching);
                });
                row(ui, "Maximum wait", "After this long, a player goes up next even without a skill match", &mut |ui| {
                    widgets::stepper(ui, pal, &mut s.max_wait_minutes, 5, 180, 5, "min");
                });

                widgets::hdivider(ui, pal);
                widgets::section_label(ui, pal, "Display");
                row(ui, "Dark theme", "Easier on the eyes on a big TV", &mut |ui| {
                    widgets::switch_labeled(ui, pal, &mut s.dark_mode);
                });
                row(ui, "Display scale", "Make everything larger or smaller", &mut |ui| {
                    let mut pct = (s.ui_scale * 100.0).round() as u32;
                    if widgets::stepper(ui, pal, &mut pct, 60, 250, 10, "%") {
                        s.ui_scale = pct as f32 / 100.0;
                    }
                });

                widgets::hdivider(ui, pal);
                self.about_row(ui, pal);
                ui.add_space(6.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, pal, ButtonKind::Primary, None, "Done", 34.0).clicked() {
                        self.settings_open = false;
                    }
                });
            });
        if modal.should_close() {
            self.settings_open = false;
        }
        if self.state.settings != before {
            if self.state.settings.courts != before.courts {
                self.state.sync_court_count(now);
            }
            if self.state.settings.round_minutes != before.round_minutes
                || self.state.settings.max_wait_minutes != before.max_wait_minutes
                || self.state.settings.skill_matching != before.skill_matching
            {
                self.state.fill_deck(now);
            }
            self.dirty = true;
        }
    }

    /// Settings footer: the version, update status, and a manual check.
    fn about_row(&mut self, ui: &mut Ui, pal: &Palette) {
        let ctx = ui.ctx().clone();
        let status = self.updater.status();
        let detail = if !Updater::enabled() {
            "Court rotation for badminton nights".to_owned()
        } else {
            match &status {
                UpdateStatus::Idle | UpdateStatus::Checking => "Checking for updates…".to_owned(),
                UpdateStatus::UpToDate => "You have the latest version".to_owned(),
                UpdateStatus::Available(r) => format!("Version {} is available", r.version),
                UpdateStatus::Installing(r, _) => format!("Downloading version {}…", r.version),
                UpdateStatus::Installed(r) => format!("Version {} is ready — restart to finish", r.version),
                UpdateStatus::Failed(err) => err.clone(),
            }
        };
        ui.horizontal(|ui| {
            ui.set_min_height(46.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Drop-in Badminton {}", update::VERSION)).font(regular(15.0)).color(pal.fg1));
                ui.label(RichText::new(detail).font(regular(13.0)).color(pal.fg3));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !Updater::enabled() {
                    return;
                }
                match status {
                    UpdateStatus::Available(_) | UpdateStatus::Installed(_) => {
                        if widgets::button(ui, pal, ButtonKind::Primary, Some(icon::DOWNLOAD), "Update…", 32.0).clicked() {
                            self.settings_open = false;
                            self.update_open = true;
                        }
                    }
                    UpdateStatus::Checking | UpdateStatus::Installing(..) => {
                        ui.add(egui::Spinner::new().size(18.0).color(pal.brand_fg));
                    }
                    _ => {
                        if widgets::button(ui, pal, ButtonKind::Secondary, None, "Check for updates", 32.0).clicked() {
                            self.updater.check(&ctx);
                        }
                    }
                }
            });
        });
    }

    /// Ribbon button that appears only when there's an update to install.
    fn update_button(&mut self, ui: &mut Ui, pal: &Palette) {
        let (label, tip) = match self.updater.status() {
            UpdateStatus::Available(r) => ("Update", format!("Version {} is available — click to install", r.version)),
            UpdateStatus::Installing(..) => ("Updating…", "Downloading the update".to_owned()),
            UpdateStatus::Installed(r) => ("Restart", format!("Restart to start using version {}", r.version)),
            _ => return,
        };
        let resp = widgets::ribbon_button(ui, pal, icon::DOWNLOAD_F, label, self.update_open);
        // A small dot so it catches the eye, like Office's "update available".
        let dot = pos2(resp.rect.center().x + 13.0, resp.rect.top() + 11.0);
        ui.painter().circle_filled(dot, 4.5, pal.bg1);
        ui.painter().circle_filled(dot, 3.5, pal.brand_fg);
        if resp.on_hover_text(tip).clicked() {
            self.update_open = true;
        }
    }

    fn update_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.update_open {
            return;
        }
        let status = self.updater.status();
        let release: Option<Release> = match &status {
            UpdateStatus::Available(r) | UpdateStatus::Installing(r, _) | UpdateStatus::Installed(r) => Some(r.clone()),
            _ => None,
        };
        let mut install = false;
        let mut restart = false;
        let modal = Modal::new(unique("update"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(480.0);
                let title = match (&status, &release) {
                    (UpdateStatus::Installed(_), _) => "Ready to restart".to_owned(),
                    (_, Some(r)) => format!("Update to version {}?", r.version),
                    _ => "Update".to_owned(),
                };
                if Self::dialog_title(ui, pal, &title, None) {
                    self.update_open = false;
                }
                let body = match &status {
                    UpdateStatus::Installed(_) => {
                        "The new version is installed. Drop-in Badminton will close and open again in a few seconds."
                    }
                    UpdateStatus::Failed(_) => "",
                    _ => {
                        "Drop-in Badminton will download the update, then restart. Your roster, courts, timer and history \
                         are kept, and a backup of your data is saved first."
                    }
                };
                if !body.is_empty() {
                    ui.label(RichText::new(body).font(regular(14.0)).color(pal.fg2));
                }
                if let UpdateStatus::Failed(err) = &status {
                    ui.label(RichText::new(err).font(regular(14.0)).color(pal.danger_fg));
                }
                if let Some(notes) = release.as_ref().map(|r| r.notes.as_str()).filter(|n| !n.is_empty()) {
                    ui.add_space(10.0);
                    widgets::section_label(ui, pal, "What's new");
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.label(RichText::new(notes).font(regular(14.0)).color(pal.fg1));
                    });
                }
                if let UpdateStatus::Installing(_, progress) = status {
                    ui.add_space(12.0);
                    let bar = egui::ProgressBar::new(progress.unwrap_or(0.0))
                        .desired_height(6.0)
                        .fill(pal.brand_fg)
                        .corner_radius(3);
                    ui.add(if progress.is_none() { bar.animate(true) } else { bar });
                }
                ui.add_space(18.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &status {
                    UpdateStatus::Available(_) => {
                        if widgets::button(ui, pal, ButtonKind::Primary, Some(icon::DOWNLOAD), "Update now", 34.0).clicked() {
                            install = true;
                        }
                        if widgets::button(ui, pal, ButtonKind::Secondary, None, "Not now", 34.0).clicked() {
                            self.update_open = false;
                        }
                    }
                    UpdateStatus::Installing(..) => {
                        ui.label(RichText::new("Downloading…").font(regular(14.0)).color(pal.fg3));
                    }
                    UpdateStatus::Installed(_) => {
                        if widgets::button(ui, pal, ButtonKind::Primary, None, "Restart now", 34.0).clicked() {
                            restart = true;
                        }
                        if widgets::button(ui, pal, ButtonKind::Secondary, None, "Later", 34.0).clicked() {
                            self.update_open = false;
                        }
                    }
                    _ => {
                        if widgets::button(ui, pal, ButtonKind::Secondary, None, "Close", 34.0).clicked() {
                            self.update_open = false;
                        }
                    }
                });
            });
        // Don't let a stray click dismiss it mid-download.
        if modal.should_close() && !matches!(status, UpdateStatus::Installing(..)) {
            self.update_open = false;
        }
        if install {
            self.updater.install(ctx);
        }
        // Once downloaded, restart straight away unless they chose "Later" earlier.
        if restart {
            self.restart_for_update(ctx);
        }
    }

    /// Save, back up the database, and close; `main` starts the new version.
    fn restart_for_update(&mut self, ctx: &egui::Context) {
        self.save();
        self.dirty = false;
        let dir = crate::model::data_dir().join("backups");
        if std::fs::create_dir_all(&dir).is_ok() {
            let name = format!("drop-in-badminton-{}-{}.db", update::VERSION, chrono::Local::now().format("%Y%m%d-%H%M%S"));
            if let Err(err) = self.store.backup_to(&dir.join(name)) {
                eprintln!("drop-in-badminton: backup before update failed: {err}");
            }
            prune_backups(&dir, 10);
        }
        update::request_restart();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(which) = self.confirm else { return };
        let (title, body, action) = match which {
            Confirm::Delete(id) => (
                format!("Remove {}?", self.state.name(id)),
                "They'll be taken off the roster. Their past games stay in History.".to_owned(),
                "Remove",
            ),
        };
        let mut go = false;
        let modal = Modal::new(unique("confirm"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(RichText::new(title).font(semibold(20.0)));
                ui.add_space(2.0);
                ui.label(RichText::new(body).font(regular(14.0)).color(pal.fg2));
                ui.add_space(18.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let kind = if matches!(which, Confirm::Delete(_)) { ButtonKind::Danger } else { ButtonKind::Primary };
                    if widgets::button(ui, pal, kind, None, action, 34.0).clicked() {
                        go = true;
                    }
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked() {
                        self.confirm = None;
                    }
                });
            });
        if modal.should_close() {
            self.confirm = None;
        }
        if go {
            self.confirm = None;
            match which {
                Confirm::Delete(id) => {
                    let name = self.state.name(id).to_owned();
                    self.act(ctx, format!("{name} removed"), |s| s.delete_player(id, s.now()));
                }
            }
        }
    }

    fn open_history(&mut self) {
        // Save first so today's games show up.
        self.save();
        self.dirty = false;
        let days = self.store.days().unwrap_or_else(|err| {
            eprintln!("drop-in-badminton: history: {err}");
            Vec::new()
        });
        let mut view = HistoryView {
            days,
            selected: None,
            detail: DayDetail::default(),
        };
        if let Some(first) = view.days.first().map(|d| d.id) {
            self.select_day(&mut view, first);
        }
        self.history = Some(view);
    }

    fn select_day(&self, view: &mut HistoryView, id: i64) {
        view.selected = Some(id);
        view.detail = self.store.day_detail(id).unwrap_or_default();
    }

    fn history_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(mut view) = self.history.take() else { return };
        let mut open = true;
        let mut pick = None;
        let screen = ctx.content_rect();
        let size = vec2((screen.width() - 160.0).min(860.0), (screen.height() - 240.0).clamp(420.0, 600.0));
        let modal = Modal::new(unique("history"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                let count = match view.days.len() {
                    1 => "1 game day".to_owned(),
                    n => format!("{n} game days"),
                };
                if Self::dialog_title(ui, pal, "History", Some(count)) {
                    open = false;
                }
                if self.help {
                    widgets::help_bar(ui, pal, &help_history());
                }
                ui.add_space(8.0);
                if view.days.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(icon::HISTORY).font(icon_font(56.0)).color(pal.fg3));
                        ui.label(RichText::new("No game days yet").font(semibold(17.0)));
                        ui.label(
                            RichText::new("Check players in and play a round — every game is recorded here.").color(pal.fg3),
                        );
                    });
                    return;
                }
                let body = Rect::from_min_size(ui.cursor().min, vec2(size.x, (ui.available_height() - 46.0).max(200.0)));
                ui.allocate_rect(body, Sense::hover());
                let list_rect = Rect::from_min_size(body.min, vec2(280.0, body.height()));
                let detail_rect = Rect::from_min_max(pos2(list_rect.right() + 25.0, body.top()), body.max);
                ui.painter().line_segment(
                    [pos2(list_rect.right() + 12.0, body.top()), pos2(list_rect.right() + 12.0, body.bottom())],
                    Stroke::new(1.0, pal.stroke2),
                );
                // Day list (a Fluent nav list).
                let mut list = ui.new_child(UiBuilder::new().max_rect(list_rect).layout(Layout::top_down(Align::Min)));
                egui::ScrollArea::vertical()
                    .id_salt("days")
                    .auto_shrink([false, false])
                    .show(&mut list, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for d in &view.days {
                            if self.day_row(ui, pal, d, view.selected == Some(d.id)).clicked() {
                                pick = Some(d.id);
                            }
                        }
                    });
                // Selected day.
                let mut detail = ui.new_child(UiBuilder::new().max_rect(detail_rect).layout(Layout::top_down(Align::Min)));
                egui::ScrollArea::vertical()
                    .id_salt("day")
                    .auto_shrink([false, false])
                    .show(&mut detail, |ui| self.day_detail_ui(ui, pal, &view));
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, pal, ButtonKind::Primary, None, "Done", 34.0).clicked() {
                        open = false;
                    }
                });

            });
        if let Some(id) = pick {
            self.select_day(&mut view, id);
        }

        if open && !modal.should_close() {
            self.history = Some(view);
        }
    }

    fn day_row(&self, ui: &mut Ui, pal: &Palette, d: &DaySummary, selected: bool) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
        let rest = if selected { pal.subtle_selected } else { Color32::TRANSPARENT };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(4), theme::lerp_color(rest, pal.subtle_hover, hover * if selected { 0.0 } else { 1.0 }));
        if selected {
            let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 16.0), vec2(3.0, rect.height() - 32.0));
            p.rect_filled(bar, CornerRadius::same(2), pal.brand_fg);
        }
        let live = d.ended_ms.is_none() && self.state.session.day_id == Some(d.id);
        let title = p.text(
            rect.left_top() + vec2(14.0, 20.0),
            Align2::LEFT_CENTER,
            format!("{} · {}", fmt_date(d.started_ms, "%a, %b %-d"), fmt_date(d.started_ms, "%-I:%M %p")),
            semibold(15.0),
            pal.fg1,
        );
        if live {
            widgets::paint_badge(p, pos2(title.right() + 8.0, title.center().y), Align2::LEFT_CENTER, None, "Current", 11.0, pal.brand_subtle, pal.on_brand_subtle);
        }
        let end = d.ended_ms.map_or("now".to_owned(), |e| fmt_date(e, "%-I:%M %p"));
        p.text(
            rect.left_top() + vec2(14.0, 40.0),
            Align2::LEFT_CENTER,
            format!("Until {end} · {} players · {} games", d.players, d.games),
            regular(13.0),
            pal.fg3,
        );
        resp
    }

    fn day_detail_ui(&self, ui: &mut Ui, pal: &Palette, view: &HistoryView) {
        let Some(day) = view.days.iter().find(|d| Some(d.id) == view.selected) else { return };
        let detail = &view.detail;
        let end = day.ended_ms.map_or("now".to_owned(), |e| fmt_date(e, "%-I:%M %p"));
        ui.label(RichText::new(fmt_date(day.started_ms, "%a, %b %-d")).font(semibold(20.0)));
        ui.label(
            RichText::new(format!("{} – {}", fmt_date(day.started_ms, "%-I:%M %p"), end))
                .font(regular(14.0))
                .color(pal.fg3),
        );
        ui.add_space(12.0);
        // The night at a glance.
        let span = day.ended_ms.unwrap_or_else(|| self.state.now()) - day.started_ms;
        let most = detail.attendees.first().filter(|(_, _, g)| *g > 0);
        let tile_w = ((ui.available_width() + 10.0) / 3.0 - 10.0).floor().max(120.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for (value, label, glyph) in [
                (detail.attendees.len().to_string(), "Players", icon::GROUP),
                (detail.games.len().to_string(), "Games", icon::SHUTTLE),
                (short_duration(span), if day.ended_ms.is_none() { "So far" } else { "Duration" }, icon::TIMER),
            ] {
                let (r, _) = ui.allocate_exact_size(vec2(tile_w, 64.0), Sense::hover());
                let p = ui.painter();
                p.rect(r, CornerRadius::same(8), pal.bg2, Stroke::new(1.0, pal.stroke2), StrokeKind::Inside);
                p.text(r.left_top() + vec2(14.0, 22.0), Align2::LEFT_CENTER, glyph, icon_font(18.0), pal.brand_fg);
                p.text(r.left_top() + vec2(38.0, 22.0), Align2::LEFT_CENTER, label, regular(13.0), pal.fg3);
                p.text(r.left_top() + vec2(14.0, 45.0), Align2::LEFT_CENTER, value, semibold(24.0), pal.fg1);
            }
        });
        if let Some((name, _, games)) = most {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{}  Most games: {name} · {games}", icon::PERSON_STAR))
                    .font(regular(14.0))
                    .color(pal.fg2),
            );
        }
        ui.add_space(14.0);
        widgets::section_label(ui, pal, &format!("Players ({})", detail.attendees.len()));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (name, skill, games) in &detail.attendees {
                // Tag: skill bar, name, games badge.
                let galley = ui.painter().layout_no_wrap(name.clone(), regular(14.0), pal.fg1);
                // No badge for nobody-played-yet; it would just be a row of zeros.
                let count = games.to_string();
                let cw = if *games > 0 { ui.painter().layout_no_wrap(count.clone(), semibold(12.0), pal.fg2).size().x + 12.0 } else { 0.0 };
                let bw = widgets::badge_width(18.0);
                let lead = bw + 14.0;
                let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + lead + 4.0 + cw + 8.0, 32.0), Sense::hover());
                let p = ui.painter();
                p.rect(r, CornerRadius::same(4), pal.bg2, Stroke::new(1.0, pal.stroke2), StrokeKind::Inside);
                let b = Rect::from_min_size(pos2(r.left() + 7.0, r.center().y - 9.0), vec2(bw, 18.0));
                widgets::paint_skill_badge(p, pal, b, *skill);
                p.galley(pos2(r.left() + lead, r.center().y - galley.size().y / 2.0), galley, pal.fg1);
                if *games > 0 {
                    widgets::paint_badge(p, pos2(r.right() - 5.0, r.center().y), Align2::RIGHT_CENTER, None, &count, 11.0, pal.subtle_selected, pal.fg2);
                }
            }
        });
        if detail.attendees.iter().any(|(_, _, g)| *g > 0) {
            ui.label(RichText::new("The number on each name is games played that day").font(regular(12.5)).color(pal.fg3));
        }
        ui.add_space(14.0);
        widgets::section_label(ui, pal, &if detail.games.is_empty() { "Games".to_owned() } else { format!("Games ({})", detail.games.len()) });
        if detail.games.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::SHUTTLE).font(icon_font(22.0)).color(pal.fg3));
                ui.label(RichText::new("No games finished yet — they're saved here as each round ends.").font(regular(14.0)).color(pal.fg3));
            });
            return;
        }
        // A simple Fluent data grid.
        let team = |t: &[(String, Skill)]| t.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(" & ");
        let cols = [0.0, 90.0, 160.0, 230.0, 290.0];
        let row = |ui: &mut Ui, header: bool, cells: [String; 5]| {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), if header { 32.0 } else { 40.0 }), Sense::hover());
            let p = ui.painter();
            let (font, color) = if header { (semibold(13.0), pal.fg2) } else { (regular(14.0), pal.fg1) };
            for (i, cell) in cells.into_iter().enumerate() {
                let x = r.left() + 8.0 + cols[i];
                let max_w = if i == 4 { r.right() - x - 8.0 } else { cols[i + 1] - cols[i] - 8.0 };
                let size = widgets::fit_font(ui, &cell, if header { semibold } else { regular }, font.size, max_w);
                let f = egui::FontId::new(size, font.family.clone());
                p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, cell, f, if i == 4 || header { color } else { pal.fg2 });
            }
            p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal.stroke2));
        };
        ui.spacing_mut().item_spacing.y = 0.0;
        row(ui, true, ["Time".into(), "Court".into(), "Length".into(), "Round".into(), "Match".into()]);
        for g in &detail.games {
            row(
                ui,
                false,
                [
                    fmt_date(g.started_ms, "%-I:%M %p"),
                    format!("{}", g.court + 1),
                    short_duration(g.ended_ms - g.started_ms),
                    g.round.to_string(),
                    format!("{}   vs   {}", team(&g.team_a), team(&g.team_b)),
                ],
            );
        }
    }

    fn snackbar_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(snack) = &self.snackbar else { return };
        let t = ctx.input(|i| i.time);
        let age = (t - snack.born) as f32;
        let life = 7.0;
        if age > life {
            self.snackbar = None;
            return;
        }
        let appear = (age / 0.22).min(1.0);
        let vanish = ((life - age) / 0.3).min(1.0);
        let k = egui::emath::easing::cubic_out(appear.min(vanish));
        ctx.request_repaint();

        let text = snack.text.clone();
        let has_undo = snack.undo.is_some();
        let mut undo = false;
        let mut close = false;
        // A Fluent toast, docked in the middle of the title bar so it never
        // covers the board. It slides down into place.
        let round_news = text.starts_with("Round");
        let gap = self.toast_gap.filter(|_| false);
        let (pivot, spot) = match gap {
            Some(g) => (Align2::CENTER_CENTER, g.center()),
            // Not enough room in the ribbon: the middle of the title bar.
            None => (Align2::CENTER_CENTER, pos2(ctx.content_rect().center().x, 22.0)),
        };
        let area = egui::Area::new(unique("snackbar"))
            .order(Order::Foreground)
            .pivot(pivot)
            .fixed_pos(spot + vec2(0.0, (1.0 - k) * 12.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_opacity(k);
                Frame::NONE
                    .fill(pal.brand_subtle)
                    .stroke(Stroke::new(1.5, pal.brand_fg))
                    .corner_radius(8)
                    .inner_margin(Margin { left: 6, right: 3, top: 2, bottom: 2 })
                    .shadow(pal.shadow(8))
                    .show(ui, |ui| {
                        ui.set_min_height(30.0);
                        ui.set_max_height(32.0);
                        ui.horizontal_centered(|ui| {
                            if round_news {
                                // The app's shuttle tile: the round has turned over.
                                let (tile, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                                ui.painter().rect_filled(tile, CornerRadius::same(5), pal.brand);
                                ui.painter().text(tile.center(), Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(15.0), pal.on_brand);
                            } else {
                                ui.add_space(4.0);
                                ui.label(RichText::new(icon::INFO_F).font(icon_font(20.0)).color(pal.brand_fg));
                            }
                            ui.label(RichText::new(&text).font(semibold(16.0)).color(pal.fg1));
                            ui.add_space(8.0);
                            if has_undo
                                && widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::UNDO), "Undo", 26.0).clicked()
                            {
                                undo = true;
                            }
                            if widgets::icon_button(ui, pal, icon::CLOSE, 26.0, false).clicked() {
                                close = true;
                            }
                        });
                    });
            });
        self.toast_w = area.response.rect.width();
        if undo {
            self.undo();
        } else if close {
            self.snackbar = None;
        }
    }
}

/// Format a Unix-ms timestamp in local time.
fn fmt_date(ms: i64, fmt: &str) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |t| t.format(fmt).to_string())
}


// ===========================================================================
// Help text (shown when Help is toggled on in the ribbon)

fn help_roster() -> Help {
    Help::new(
        "Checking people in",
        &[
            "Flip someone's switch on when they arrive — their wait clock starts. Flip it off when they leave.",
            "The colored tag (BEG, INT, ADV, EXP) is their skill level; click it to change it.",
            "Double-click a name to fix its spelling. Right-click for more, including delete.",
            "New game day saves today to History and checks everyone out. It also happens by itself after 8 quiet hours.",
        ],
    )
}

fn help_settings() -> Help {
    Help::new(
        "Settings",
        &[
            "Changes apply straight away and save automatically.",
            "Round length is what the timer counts down from.",
            "Maximum wait: after this long, a player gets the next open spot even without a skill match.",
        ],
    )
}

fn help_history() -> Help {
    Help::new(
        "History",
        &[
            "Every game day is saved automatically: who came, and every game played.",
            "A game is recorded when it ends — on Next round, Play now, or a new game day.",
            "Pick a day on the left to see it.",
        ],
    )
}

impl DropInApp {
    fn help_courts(&self) -> Help {
        let s = &self.state.settings;
        let start = format!("Press the round play button to start the {}-minute timer.", s.round_minutes);
        let next = if s.auto_start_timer {
            "At 0:00 press Next round (N): Up next walks on, and the timer restarts by itself."
        } else {
            "At 0:00 press Next round (N): Up next walks on and players coming off go back to Waiting."
        };
        Help {
            title: "Courts — games in progress".into(),
            bullets: vec![
                start,
                next.into(),
                "Drag a name to swap players or move them; drop on Waiting to take someone off.".into(),
                "The time under a name is since they last played — 0m while they're on court.".into(),
                "Taken off before the round ends? They keep their earlier wait and the game doesn't count.".into(),
                "Right-click anyone, anywhere: send to waiting, break, check out, skill, rename, delete.".into(),
                "Keys: Space starts or pauses, N is Next round, Ctrl+Z undoes, F11 is full screen.".into(),
            ],
        }
    }

    fn help_deck(&self) -> Help {
        let s = &self.state.settings;
        let pick = if s.skill_matching {
            "Picked automatically: the longest wait first, then the closest skill levels.".to_owned()
        } else {
            "Picked automatically: the four longest waits (skill matching is off in Settings).".to_owned()
        };
        Help {
            title: "Up next — the following game on each court".into(),
            bullets: vec![
                pick,
                format!("Anyone waiting {} min or more gets the next spot whatever their level.", s.max_wait_minutes),
                "Teams are balanced: the strongest and weakest player against the middle two.".into(),
                "Drag to rearrange. Play now sends a group on straight away.".into(),
            ],
        }
    }

    fn help_waiting(&self) -> Help {
        Help {
            title: "Who plays next".into(),
            bullets: vec![
                "Longest since their last game at the top — or since check-in if they haven't played.".into(),
                "Ties go to whoever has played fewer games tonight.".into(),
                format!("An hourglass marks anyone waiting {}+ min: they're picked next regardless of skill.", self.state.settings.max_wait_minutes),
                "Point at a name for its buttons: take a break, or check out.".into(),
            ],
        }
    }
}

// ===========================================================================
// Right-click menu for any player, wherever they appear

impl DropInApp {
    /// Attach the player menu to `resp` (opens on right-click).
    fn player_menu(&mut self, resp: &egui::Response, pal: &Palette, id: PlayerId) {
        let Some(player) = self.state.players.get(&id) else { return };
        let (name, skill) = (player.name.clone(), player.skill);
        let presence = self.state.session.present.get(&id);
        let here = presence.is_some();
        let resting = presence.is_some_and(|p| p.resting);
        let assigned = self.state.is_assigned(id);
        let mut chosen = None;
        // Its own id, so it doesn't clash with a left-click menu on the same widget
        // (the roster's skill tag).
        egui::Popup::context_menu(resp).id(resp.id.with("player-menu")).show(|ui| {
            ui.set_min_width(240.0);
            ui.set_max_width(280.0);
            ui.spacing_mut().button_padding = vec2(10.0, 7.0);
            let status = if !here {
                "Not checked in"
            } else if resting {
                "On a break"
            } else if let Some(Spot::Court(c, _)) = self.state.session.locate(id) {
                return_court(c)
            } else if assigned {
                "Up next"
            } else {
                "Waiting"
            };
            // Header: skill badge beside the name, with where they are underneath.
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let bw = widgets::badge_width(28.0);
                let (b, _) = ui.allocate_exact_size(vec2(bw, 40.0), Sense::hover());
                widgets::paint_skill_badge(ui.painter(), pal, Rect::from_center_size(b.center(), vec2(bw, 28.0)), skill);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(&name).font(semibold(15.0)).color(pal.fg1));
                    ui.label(RichText::new(status).font(regular(13.0)).color(pal.fg3));
                });
            });
            ui.separator();
            let item = |ui: &mut Ui, glyph: &str, label: &str, enabled: bool, color: Color32| {
                let mut job = egui::text::LayoutJob::default();
                let icon_color = if color == pal.fg1 { pal.fg2 } else { color };
                job.append(glyph, 0.0, egui::TextFormat { font_id: icon_font(18.0), color: icon_color, valign: Align::Center, ..Default::default() });
                job.append(label, 12.0, egui::TextFormat { font_id: regular(15.0), color, valign: Align::Center, ..Default::default() });
                ui.add_enabled(enabled, egui::Button::new(job).frame_when_inactive(false)).clicked()
            };
            let (wait_label, wait_ok) = if !here {
                ("Check in", true)
            } else if resting {
                ("Back to waiting (end break)", true)
            } else {
                ("Send to waiting", assigned)
            };
            if item(ui, icon::LOGIN, wait_label, wait_ok, pal.fg1) {
                chosen = Some(PlayerAction::ToWaiting);
            }
            if item(ui, icon::COFFEE, "Take a break", here && !resting, pal.fg1) {
                chosen = Some(PlayerAction::Break);
            }
            if item(ui, icon::LOGOUT, "Check out", here, pal.fg1) {
                chosen = Some(PlayerAction::CheckOut);
            }
            ui.separator();
            let mut skill_job = egui::text::LayoutJob::default();
            skill_job.append(icon::PERSON_STAR, 0.0, egui::TextFormat { font_id: icon_font(18.0), color: pal.fg2, valign: Align::Center, ..Default::default() });
            skill_job.append(&format!("Skill level: {}", skill.label()), 12.0, egui::TextFormat { font_id: regular(15.0), color: pal.fg1, valign: Align::Center, ..Default::default() });
            ui.menu_button(
                skill_job,
                |ui| {
                    ui.set_min_width(170.0);
                    for s in Skill::ALL {
                        let text = widgets::skill_menu_text(pal, s).font(semibold(15.0));
                        if ui.selectable_label(s == skill, text).clicked() && s != skill {
                            chosen = Some(PlayerAction::Skill(s));
                        }
                    }
                },
            );
            if item(ui, icon::EDIT, "Rename…", true, pal.fg1) {
                chosen = Some(PlayerAction::Rename);
            }
            let on_court = matches!(self.state.session.locate(id), Some(Spot::Court(..)));
            if item(ui, icon::HOURGLASS, "Adjust wait time…", here && !on_court, pal.fg1) {
                chosen = Some(PlayerAction::Wait);
            }
            ui.separator();
            if item(ui, icon::DELETE, "Delete player…", true, pal.danger_fg) {
                chosen = Some(PlayerAction::Delete);
            }
        });
        if let Some(action) = chosen {
            self.pending = Some((id, action));
        }
    }

    fn apply_player_action(&mut self, ctx: &egui::Context, id: PlayerId, action: PlayerAction) {
        let Some(name) = self.state.players.get(&id).map(|p| p.name.clone()) else { return };
        let now = self.state.now();
        match action {
            PlayerAction::ToWaiting => {
                let presence = self.state.session.present.get(&id);
                if presence.is_none() {
                    self.act(ctx, format!("{name} checked in"), |s| s.check_in(id, now));
                } else if presence.is_some_and(|p| p.resting) {
                    self.act(ctx, format!("{name} is back in the queue"), |s| s.set_resting(id, false, now));
                } else {
                    self.act(ctx, format!("{name} sent to waiting"), |s| s.move_player(id, Target::Pool, now));
                }
            }
            PlayerAction::Break => {
                self.act(ctx, format!("{name} is taking a break"), |s| s.set_resting(id, true, now));
            }
            PlayerAction::CheckOut => {
                self.act(ctx, format!("{name} checked out"), |s| s.check_out(id, now));
            }
            PlayerAction::Skill(skill) => {
                self.act(ctx, format!("{name} is now {}", skill.label()), |s| {
                    if let Some(p) = s.players.get_mut(&id) {
                        p.skill = skill;
                    }
                });
            }
            PlayerAction::Rename => self.rename_dialog = Some((id, name, true)),
            PlayerAction::Wait => {
                let mins = self.shown_wait_ms(id, now).unwrap_or(0) / 60_000;
                self.wait_dialog = Some((id, mins.to_string(), true));
            }
            PlayerAction::Delete => self.confirm = Some(Confirm::Delete(id)),
        }
    }

    /// A small dialog for renaming from the right-click menu.
    /// Enter saves; Esc, Cancel, or clicking outside cancels.
    /// "How it works": one dialog with every section's explanation.
    fn help_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.help || self.settings_open || self.history.is_some() {
            return;
        }
        // Two balanced columns: courts + check-in on the left, Up next +
        // waiting on the right.
        let sections = [
            (icon::SHUTTLE_F, self.help_courts()),
            (icon::NEXT_F, self.help_deck()),
            (icon::GROUP_F, help_roster()),
            (icon::HOURGLASS_F, self.help_waiting()),
        ];
        let modal = Modal::new(unique("help"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(880.0);
                if Self::dialog_title(ui, pal, "How Drop-in Badminton works", None) {
                    self.help = false;
                }
                ui.add_space(8.0);
                egui::Grid::new(unique("help-grid")).num_columns(2).spacing(vec2(28.0, 18.0)).show(ui, |ui| {
                    for (k, (glyph, help)) in sections.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.set_width(410.0);
                            ui.horizontal(|ui| {
                                let (tile, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                                ui.painter().rect_filled(tile, CornerRadius::same(6), pal.brand_subtle);
                                ui.painter().text(tile.center(), Align2::CENTER_CENTER, *glyph, icon_font(16.0), pal.brand_fg);
                                ui.label(RichText::new(&help.title).font(semibold(16.0)).color(pal.fg1));
                            });
                            ui.add_space(4.0);
                            for b in &help.bullets {
                                ui.horizontal_top(|ui| {
                                    ui.add_space(10.0);
                                    ui.label(RichText::new("•").font(semibold(15.0)).color(pal.brand_fg));
                                    ui.add(egui::Label::new(RichText::new(b).font(regular(14.0)).color(pal.fg2)).wrap());
                                });
                            }
                        });
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, pal, ButtonKind::Primary, None, "Done", 34.0).clicked() {
                        self.help = false;
                    }
                });
            });
        if modal.should_close() {
            self.help = false;
        }
    }

    fn rename_dialog_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some((id, mut buf, needs_focus)) = self.rename_dialog.take() else { return };
        let mut save = false;
        let mut cancel = false;
        let modal = Modal::new(unique("rename"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("Rename player").font(semibold(20.0)));
                ui.add_space(4.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut buf)
                        .font(regular(17.0))
                        .margin(Margin::symmetric(10, 8))
                        .desired_width(f32::INFINITY),
                );
                if needs_focus {
                    resp.request_focus();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    save = true;
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    cancel = true;
                }
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, pal, ButtonKind::Primary, None, "Save", 34.0).clicked() {
                        save = true;
                    }
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            let name = buf.trim().to_owned();
            let old = self.state.name(id).to_owned();
            if !name.is_empty() && name != old {
                self.act(ctx, format!("{old} renamed to {name}"), |s| {
                    if let Some(p) = s.players.get_mut(&id) {
                        p.name = name.clone();
                    }
                });
            }
        } else if !cancel && !modal.should_close() {
            self.rename_dialog = Some((id, buf, false));
        }
    }
}

impl DropInApp {
    /// Set how long a player has been waiting, in minutes. Enter saves.
    fn wait_dialog_ui(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some((id, mut buf, needs_focus)) = self.wait_dialog.take() else { return };
        let name = self.state.name(id).to_owned();
        let mut save = false;
        let mut cancel = false;
        let modal = Modal::new(unique("wait"))
            .frame(Self::dialog_frame(pal))
            .backdrop_color(pal.scrim)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(format!("{name}'s wait")).font(semibold(20.0)));
                ui.label(
                    RichText::new("Minutes since they last played (or arrived). The waiting list re-sorts to match.")
                        .font(regular(14.0))
                        .color(pal.fg2),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let minutes = buf.trim().parse::<i64>().ok();
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "−5", 36.0).clicked() {
                        buf = (minutes.unwrap_or(0) - 5).max(0).to_string();
                    }
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut buf)
                            .font(regular(18.0))
                            .margin(Margin::symmetric(10, 7))
                            .desired_width(90.0)
                            .align(Align2::CENTER_CENTER),
                    );
                    if needs_focus {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        save = true;
                    }
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "+5", 36.0).clicked() {
                        buf = (minutes.unwrap_or(0) + 5).max(0).to_string();
                    }
                    ui.label(RichText::new("minutes").font(regular(15.0)).color(pal.fg2));
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    cancel = true;
                }
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ok = buf.trim().parse::<i64>().is_ok_and(|m| (0..=24 * 60).contains(&m));
                    if ui.add_enabled_ui(ok, |ui| widgets::button(ui, pal, ButtonKind::Primary, None, "Save", 34.0)).inner.clicked() {
                        save = true;
                    }
                    if widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            if let Some(mins) = buf.trim().parse::<i64>().ok().filter(|m| (0..=24 * 60).contains(m)) {
                let now = self.state.now();
                let label = if mins == 1 { "1 minute".to_owned() } else { format!("{mins} minutes") };
                self.act(ctx, format!("{name} has now waited {label}"), |s| s.set_wait(id, mins * 60_000, now));
                return;
            }
            self.wait_dialog = Some((id, buf, false));
        } else if !cancel && !modal.should_close() {
            self.wait_dialog = Some((id, buf, false));
        }
    }
}

fn return_court(c: usize) -> &'static str {
    const COURTS: [&str; 12] = [
        "On court 1", "On court 2", "On court 3", "On court 4", "On court 5", "On court 6",
        "On court 7", "On court 8", "On court 9", "On court 10", "On court 11", "On court 12",
    ];
    COURTS.get(c).copied().unwrap_or("On a court")
}

/// Keep only the newest `keep` database backups.
fn prune_backups(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".db"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

/// "7", "7:30" or "0:45" as milliseconds (at most 2 hours).
/// A session length as h:mm:ss.
fn hms(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// "90" (minutes), "1:30" (h:mm) or "1:30:15" (h:mm:ss), up to two days.
fn parse_hms(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.trim().split(':').map(str::trim).collect();
    let num = |s: &str| if s.is_empty() { Some(0) } else { s.parse::<i64>().ok() };
    let (h, m, s) = match parts.as_slice() {
        [m] => (0, num(m)?, 0),
        [h, m] => (num(h)?, num(m)?, 0),
        [h, m, s] => (num(h)?, num(m)?, num(s)?),
        _ => return None,
    };
    let ms = ((h * 60 + m) * 60 + s) * 1000;
    let in_range = (parts.len() == 1 || m < 60) && s < 60 && (0..=48 * 3_600_000).contains(&ms);
    in_range.then_some(ms)
}

fn parse_clock(text: &str) -> Option<i64> {
    let text = text.trim();
    let (m, s) = match text.split_once(':') {
        Some((m, s)) => (m.trim(), s.trim()),
        None => (text, "0"),
    };
    let m: i64 = if m.is_empty() { 0 } else { m.parse().ok()? };
    let s: i64 = s.parse().ok()?;
    let ms = (m * 60 + s) * 1000;
    (s < 60 && (0..=7_200_000).contains(&ms)).then_some(ms)
}
