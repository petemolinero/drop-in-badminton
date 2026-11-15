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
use crate::store::{DayDetail, DaySummary, Store};
use crate::surfaces::{self, Surfaces};
use crate::theme::{self, Palette, icon, icon_font, regular, semibold};
use crate::update::{self, Release, Status as UpdateStatus, Updater};
use crate::widgets::{self, ButtonKind, Help, clock, short_duration, unique};

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
    Delete,
}

#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    NewDay,
    Delete(PlayerId),
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

/// Gap after which the app assumes it's a new game day.
const NEW_DAY_GAP_MS: i64 = 8 * 3600 * 1000;

/// Space between cards on the canvas.
const GAP: f32 = 10.0;
/// Text size for every status chip, so they all share one spec.
const CHIP_TEXT: f32 = 15.0;

pub struct GoodmintonApp {
    state: AppState,
    surfaces: Surfaces,
    store: Store,
    history: Option<HistoryView>,
    dirty: bool,
    dirty_since: Option<f64>,
    roster_open: bool,
    settings_open: bool,
    /// Show a plain-language explanation at the top of each section.
    help: bool,
    new_name: String,
    new_skill: Skill,
    search: String,
    /// Player being renamed, the edit buffer, and whether the box still needs focus.
    renaming: Option<(PlayerId, String, bool)>,
    snackbar: Option<Snackbar>,
    confirm: Option<Confirm>,
    /// A right-click menu choice, applied at the end of the frame.
    pending: Option<(PlayerId, PlayerAction)>,
    /// The rename dialog: player, edit buffer, and whether it still needs focus.
    rename_dialog: Option<(PlayerId, String, bool)>,
    drag: Option<PlayerId>,
    drop_targets: Vec<(Rect, Target)>,
    anims: HashMap<PlayerId, ChipAnim>,
    frame: u64,
    applied_theme: Option<(bool, f32)>,
    updater: Updater,
    /// This frame's on-court name size; Up next names stay a step smaller.
    court_font: f32,
    /// Correction to the waiting list's height estimate, learned frame to frame.
    pool_bonus: f32,
    /// Where toasts appear: the open stretch of the ribbon left of the timer.
    toast_gap: Option<Rect>,
    /// Last toast's width, to centre it.
    toast_w: f32,
    /// The "Update Goodminton" dialog is showing.
    update_open: bool,
    #[cfg(debug_assertions)]
    shot_requested: bool,
}

impl GoodmintonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_fonts(theme::fonts());
        widgets::load_skill_finishes(&cc.egui_ctx);
        let (store, startup_error) = match Store::open_default() {
            Ok(store) => (store, None),
            Err(err) => {
                eprintln!("goodminton: could not open database: {err}");
                (
                    Store::open_in_memory().expect("in-memory database"),
                    Some(format!("Couldn't open the database ({err}) — changes won't be saved")),
                )
            }
        };
        let state = store.load().unwrap_or_else(|err| {
            eprintln!("goodminton: could not load: {err}");
            AppState::default()
        });
        let mut app = Self {
            state,
            surfaces: Surfaces::load(&cc.egui_ctx),
            store,
            history: None,
            dirty: true,
            dirty_since: None,
            roster_open: false,
            settings_open: false,
            help: false,
            new_name: String::new(),
            new_skill: Skill::Intermediate,
            search: String::new(),
            renaming: None,
            snackbar: None,
            confirm: None,
            pending: None,
            rename_dialog: None,
            drag: None,
            drop_targets: Vec::new(),
            anims: HashMap::new(),
            frame: 0,
            applied_theme: None,
            updater: Updater::new(),
            court_font: 40.0,
            pool_bonus: 0.0,
            toast_gap: None,
            toast_w: 420.0,
            update_open: false,
            #[cfg(debug_assertions)]
            shot_requested: false,
        };
        let now = now_ms();
        app.state.sync_court_count(now);
        // A long gap since the last activity means it's a new game day.
        if app.is_stale(now) {
            app.start_new_day(&cc.egui_ctx, false);
        }
        let previous = app.store.swap_app_version(update::VERSION);
        if let Some(err) = startup_error {
            app.toast(&cc.egui_ctx, err);
        } else if previous.is_some_and(|v| update::is_newer(update::VERSION, &v)) {
            app.toast(&cc.egui_ctx, format!("Updated to version {}", update::VERSION));
        }
        app.roster_open = app.state.session.present.is_empty();
        app
    }

    fn is_stale(&self, now: i64) -> bool {
        let s = &self.state.session;
        (s.day_id.is_some() || !s.present.is_empty()) && now - s.last_activity_ms > NEW_DAY_GAP_MS
    }

    fn save(&mut self) {
        if let Err(err) = self.store.save(&mut self.state) {
            eprintln!("goodminton: failed to save: {err}");
        }
    }

    /// Close out the current game day into history and start a fresh one.
    /// The roster carries over; everyone starts checked out.
    fn start_new_day(&mut self, ctx: &egui::Context, undoable: bool) {
        let before = Box::new(self.state.clone());
        let had_day = !self.state.session.attendance.is_empty();
        self.state.end_day(now_ms());
        self.save();
        self.state.new_day();
        self.save();
        self.dirty = false;
        self.roster_open = true;
        if had_day || undoable {
            self.snackbar = Some(Snackbar {
                text: "New game day — check players in as they arrive".into(),
                undo: undoable.then_some(before),
                born: ctx.input(|i| i.time),
            });
        }
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
        let now = now_ms();
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
        let ms = self.shown_wait_ms(id, now_ms())?;
        let games = match self.state.session.present.get(&id)?.games_tonight {
            1 => "1 game".to_owned(),
            g => format!("{g} games"),
        };
        Some(format!("{} waiting • {games}", short_duration(ms)))
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

impl eframe::App for GoodmintonApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        let now = now_ms();
        let settings = self.state.settings.clone();
        let pal = *Palette::get(settings.dark_mode);
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
                    if self.roster_open {
                        self.drop_targets.push((ui.max_rect().expand(16.0), Target::Pool));
                        self.roster(ui, &pal, now);
                    } else {
                        self.pool(ui, &pal, now);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(pal.canvas).inner_margin(Margin { left: g, right: g, top: 0, bottom: g }))
            .show(ui, |ui| self.arena(ui, &pal, now));

        self.drag_overlay(&ctx, &pal);
        self.resolve_drop(&ctx);
        self.snackbar_ui(&ctx, &pal);
        self.settings_dialog(&ctx, &pal);
        self.update_dialog(&ctx, &pal);
        self.confirm_dialog(&ctx, &pal);
        self.history_dialog(&ctx, &pal);
        self.rename_dialog_ui(&ctx, &pal);
        if let Some((id, action)) = self.pending.take() {
            self.apply_player_action(&ctx, id, action);
        }
        if self.is_stale(now) && !matches!(self.state.session.timer, Timer::Running { .. }) {
            self.start_new_day(&ctx, false);
        }

        if self.dirty {
            self.state.session.last_activity_ms = now;
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

    /// Debug builds: `GOODMINTON_SHOW=menu` right-clicks the first court slot.
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !std::env::var("GOODMINTON_SHOW").is_ok_and(|s| s.contains("menu")) {
            // Screenshots shouldn't pick up hover from wherever the real mouse is.
            if std::env::var_os("GOODMINTON_SCREENSHOT").is_some() {
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
// Development aid: `GOODMINTON_SCREENSHOT=out.ppm` captures one frame and exits.
// `GOODMINTON_SHOW=settings|roster|drag|next|timer|done|history` sets up the screen first.

#[cfg(debug_assertions)]
impl GoodmintonApp {
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("GOODMINTON_SCREENSHOT") else { return };
        let show = std::env::var("GOODMINTON_SHOW").unwrap_or_default();
        if self.frame == 2 {
            self.settings_open = show.contains("settings");
            self.roster_open = show.contains("roster");
            self.help = show.contains("help");
            if show.contains("update") {
                self.updater.debug_set(UpdateStatus::Available(Release {
                    version: "0.2.0".into(),
                    url: String::new(),
                    notes: "• Courts can be renamed\n• Faster start-up on older laptops\n• Fixed the timer sound on Windows".into(),
                }));
                self.update_open = !show.contains("ribbon");
            }
            if show.contains("light") {
                self.state.settings.dark_mode = false;
            }
            if show.contains("dark") {
                self.state.settings.dark_mode = true;
            }
            if show.contains("all") {
                let now = now_ms();
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
                self.state.session.timer = Timer::Running { end_ms: now_ms() + 9 * 60_000 + 41_000 };
            }
            if show.contains("done") {
                self.state.session.timer = Timer::Running { end_ms: now_ms() - 1000 };
            }
            if show.contains("history") {
                self.open_history();
            }
            if show.contains("rename") {
                if let Some(id) = self.state.players.keys().next().copied() {
                    self.rename_dialog = Some((id, self.state.name(id).to_owned(), true));
                }
            }
            if show.contains("confirm") {
                self.confirm = Some(Confirm::NewDay);
            }
            if show.contains("drag") {
                self.drag = self.state.session.deck.first().and_then(|g| g[0]);
            }
        }
        let t = ctx.input(|i| i.time);
        let gliding = self.anims.values().any(|a| a.t < 1.0);
        let settle = if show.contains("menu") { self.frame > 12 && !gliding } else { (self.snackbar.is_none() || t > 9.0) && !gliding };
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

impl GoodmintonApp {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let now = now_ms();
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
        if space {
            self.state.timer_toggle(now);
            self.dirty = true;
        }
        if n {
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
            } else if self.roster_open {
                self.roster_open = false;
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
        let name = p.text(pos2(tile.right() + 10.0, y), Align2::LEFT_CENTER, "Goodminton", semibold(15.0), pal.fg1);
        p.line_segment(
            [pos2(name.right() + 12.0, y - 8.0), pos2(name.right() + 12.0, y + 8.0)],
            Stroke::new(1.0, pal.stroke1),
        );
        let s = &self.state.session;
        let day_start = s.attendance.first().map_or(now, |a| a.in_ms);
        p.text(
            pos2(name.right() + 24.0, y),
            Align2::LEFT_CENTER,
            format!("Game day · {}", fmt_date(day_start, "%A, %B %-d")),
            regular(14.0),
            pal.fg2,
        );

        let _ = now;
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
            if widgets::ribbon_button(ui, pal, icon::GROUP, "Roster", self.roster_open)
                .on_hover_text("Roster & check-in")
                .clicked()
            {
                self.roster_open = !self.roster_open;
            }
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
            ui.add_space(6.0);
        }

        // Center: the timer.
        let left_edge = left.min_rect().right() + 8.0;
        let right_edge = right.min_rect().left() - 8.0;
        let width = 600.0_f32.min(right_edge - left_edge);
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
            ui.painter().rect_stroke(outer.expand(4.0), CornerRadius::same(9), Stroke::new(3.0, pal.warning_fg.gamma_multiply(0.6 + 0.4 * glow)), StrokeKind::Outside);
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
        let _ = (courts_idle, deck_ready);
        let kind = ButtonKind::Primary;
        child.set_width(outer.width());
        let label = if self.state.session.round_number == 0 { "Start round 1" } else { "Next round" };
        let resp = widgets::button(&mut child, pal, kind, Some(icon::NEXT_F), label, 52.0)
            .on_hover_text("Move everyone up next onto the courts (N)");
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
        let running = matches!(timer, Timer::Running { .. }) && !finished;
        let frac = if total > 0 { remaining as f32 / total as f32 } else { 0.0 };

        // Play / pause inside a progress ring.
        let (ring, resp) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::click());
        let resp = resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(match (finished, timer) {
                (true, _) => "Restart the timer (Space)",
                (_, Timer::Running { .. }) => "Pause (Space)",
                _ => "Start the round (Space)",
            });
        let final_minute = running && remaining <= 60_000;
        let accent = if finished || final_minute { pal.warning_fg } else { pal.brand_fg };
        {
            let p = ui.painter();
            let c = ring.center();
            let r = 44.0;
            p.circle_stroke(c, r, Stroke::new(7.0, pal.stroke2));
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
                let head = *pts.last().unwrap();
                p.add(egui::Shape::line(pts, Stroke::new(7.0, accent)));
                if running {
                    // A glowing head on the arc, like a shuttle in flight.
                    p.circle_filled(head, 7.0, accent.gamma_multiply(0.25));
                    p.circle_filled(head, 4.5, Color32::WHITE);
                    p.circle_stroke(head, 4.5, Stroke::new(1.5, accent));
                }
            }
        }
        let btn = Rect::from_center_size(ring.center(), Vec2::splat(66.0));
        // At 0:00 the button restarts the clock for another go.
        let glyph = if running { icon::PAUSE_F } else if finished { icon::SHUTTLE_F } else { icon::PLAY_F };
        let bg = if finished { pal.warning_fg } else { pal.brand };
        widgets::paint_icon_button(ui, pal, btn, &resp, glyph, false, Some((bg, Color32::WHITE)));
        if resp.clicked() {
            self.state.timer_toggle(now);
            self.dirty = true;
        }
        ui.add_space(12.0);

        // Big digits, in fixed-width cells so they don't jiggle.
        let text = if finished { "0:00".to_owned() } else { clock(remaining) };
        // The timer is the most-glanced thing in the room: make it big.
        let size = 80.0;
        let cell = size * 0.58;
        let colon = size * 0.3;
        let width: f32 = text.chars().map(|ch| if ch == ':' { colon } else { cell }).sum();
        // Always room for "00:00", so the buttons beside it never shift.
        let (digits, _) = ui.allocate_exact_size(vec2(cell * 4.0 + colon + 8.0, 114.0), Sense::hover());
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
        let (status, glyph, bg, fg) = match (finished, timer) {
            (true, _) => ("Time!", icon::SHUTTLE_F, pal.warning_subtle, pal.warning_fg),
            (_, Timer::Running { .. }) => ("Playing", icon::SHUTTLE_F, pal.brand_subtle, pal.on_brand_subtle),
            (_, Timer::Paused { .. }) => ("Paused", icon::PAUSE, pal.subtle_selected, pal.fg2),
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
        widgets::paint_badge(ui.painter(), pos2(digits.center().x, digits.center().y + 40.0), Align2::CENTER_CENTER, Some(glyph), &status, 15.0, bg, fg);
        ui.add_space(12.0);

        ui.spacing_mut().item_spacing.x = 2.0;
        if widgets::button(ui, pal, ButtonKind::Secondary, None, "−1 min", 36.0).on_hover_text("Take a minute off the clock").clicked() {
            self.state.timer_adjust(-60_000, now);
            self.dirty = true;
        }
        ui.add_space(4.0);
        if widgets::button(ui, pal, ButtonKind::Secondary, None, "+1 min", 36.0).on_hover_text("Add a minute to the clock").clicked() {
            self.state.timer_adjust(60_000, now);
            self.dirty = true;
        }
        ui.add_space(2.0);
        if widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::REPLAY), "Reset", 36.0).on_hover_text("Put the clock back to the full round").clicked() {
            self.state.timer_reset();
            self.dirty = true;
        }
    }
}

// ===========================================================================
// Courts and on-deck cards

impl GoodmintonApp {
    fn arena(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let full = ui.available_rect_before_wrap();
        let n = self.state.session.courts.len().max(1);
        let cols = if n <= 4 { n } else { n.div_ceil(2) };
        let rows = n.div_ceil(cols);
        let gap = GAP;
        // Help paragraphs above the courts and under the "Up next" heading.
        let painter = ui.painter().clone();
        let courts_help = self.help.then(|| widgets::help_layout(&painter, pal, &self.help_courts(), full.width()));
        let deck_help = self.help.then(|| widgets::help_layout(&painter, pal, &self.help_deck(), full.width()));
        let courts_help_h = courts_help.as_ref().map_or(0.0, |g| g.height + gap);
        let deck_help_h = deck_help.as_ref().map_or(0.0, |g| g.height + gap);
        if let Some(g) = courts_help {
            let r = Rect::from_min_size(full.min, vec2(full.width(), courts_help_h - gap));
            widgets::paint_help(&painter, pal, r, g);
        }
        let area = Rect::from_min_max(pos2(full.left(), full.top() + courts_help_h), full.max);
        let label_h = 44.0 + deck_help_h;
        let usable = area.height() - label_h;
        // Before anyone is on court, the people about to play matter most.
        let idle = self.state.session.courts.iter().all(|c| c.iter().all(Option::is_none));
        let courts_h = usable * 0.62;
        let deck_h = usable - courts_h;
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
        let label_y = area.top() + courts_h + 22.0 + 2.0;
        if let Some(g) = deck_help {
            let r = Rect::from_min_size(pos2(area.left(), area.top() + courts_h + 44.0), vec2(area.width(), deck_help_h - gap));
            widgets::paint_help(&painter, pal, r, g);
        }
        let p = ui.painter();
        let has_deck = self.state.session.deck.iter().any(|g| g.iter().any(Option::is_some));
        let finished = self.state.timer_finished(now);
        let first = self.state.session.round_number == 0 && idle && has_deck;
        // One header bar in every state: quiet white normally, brand blue when
        // it's calling people to their courts.
        let cta = has_deck && (finished || first);
        let banner = Rect::from_min_max(pos2(area.left(), label_y - 20.0), pos2(area.right(), label_y + 20.0));
        let (fill, ink, sub_ink) = if cta { (pal.brand, pal.on_brand, pal.on_brand) } else { (pal.bg1, pal.fg1, pal.fg2) };
        if cta && finished {
            let t = ui.input(|i| i.time) as f32;
            let glow = 0.5 + 0.5 * (t * std::f32::consts::TAU * 0.9).sin();
            p.rect_filled(banner.expand(1.0 + glow * 3.0), CornerRadius::same(12), pal.brand_fg.gamma_multiply(0.16 * glow));
            ui.ctx().request_repaint();
        }
        widgets::card(p, pal, banner, fill, 10, if cta { 4 } else { 2 });
        let label = p.text(pos2(banner.left() + 16.0, label_y), Align2::LEFT_CENTER, "Up next", semibold(20.0), ink);
        let c = pos2(label.right() + 26.0, banner.center().y);
        p.circle_filled(c, 13.0, if cta { pal.on_brand } else { pal.brand_subtle });
        p.text(c, Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(16.0), if cta { pal.brand } else { pal.brand_fg });
        let (headline, hint) = if finished && has_deck {
            ("You're up! Grab your racket and head to your court", "")
        } else if first {
            ("First games are ready — find your court below", "")
        } else {
            ("Get ready — you're on when the timer ends", "")
        };
        p.text(pos2(c.x + 22.0, c.y), Align2::LEFT_CENTER, headline, semibold(if cta { 19.0 } else { 16.0 }), if cta { ink } else { sub_ink });
        if !hint.is_empty() {
            p.text(pos2(banner.right() - 16.0, c.y), Align2::RIGHT_CENTER, hint, semibold(16.0), ink);
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
                let detail = self.since_played_label(pid);
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
            .fold(14.0_f32, f32::min);
        (widgets::group_name_size(painter, size, &names, font, true), meta)
    }

    fn court_card(&mut self, ui: &mut Ui, pal: &Palette, i: usize, rect: Rect, now: i64, dt: f32) {
        let p = ui.painter().clone();
        widgets::card(&p, pal, rect, pal.bg1, 8, 4);

        let header_h = (rect.height() * 0.12).clamp(36.0, 54.0);
        let hfont = (header_h * 0.46).min(24.0);
        let hy = rect.top() + header_h / 2.0 + 2.0;
        widgets::court_title(&p, pal, pos2(rect.left() + 14.0, hy), i + 1, hfont * 1.1, true);
        let badge_size = CHIP_TEXT;
        // The badge follows the round timer, so the board never contradicts itself.
        let occupied = (0..4).any(|s| self.state.session.slot(Spot::Court(i, s)).is_some());
        let remaining = self.state.timer_remaining_ms(now);
        // The round's state lives under the timer; a court only gets a pill
        // when there's something to say about that court.
        let pill = match self.state.session.timer {
            _ if !occupied => None,
            _ if self.state.timer_finished(now) => Some(("Time! Shake hands".to_owned(), Some(icon::SHUTTLE_F), theme::hex(0xC2410C), Color32::WHITE)),
            Timer::Running { .. } if remaining <= 120_000 => Some(("Last points".to_owned(), Some(icon::HOURGLASS_F), pal.warning_subtle, pal.warning_fg)),
            _ => None,
        };
        if let Some((text, glyph, bg, fg)) = pill {
            widgets::paint_badge(&p, pos2(rect.right() - 14.0, hy), Align2::RIGHT_CENTER, glyph, &text, badge_size, bg, fg);
        }

        // The court, seen from above with the net running left to right.
        let surface = Rect::from_min_max(
            pos2(rect.left() + 10.0, rect.top() + header_h + 2.0),
            pos2(rect.right() - 10.0, rect.bottom() - 10.0),
        );
        let lines = surfaces::paint_court(&p, &self.surfaces, pal.dark, i, surface);
        // A stray shuttle on the wood beside the court, lying differently by every court.
        let margin = lines.left() - surface.left();
        if margin > 36.0 {
            let right = surface.right() - margin / 2.0;
            let y = lines.top() + lines.height() * [0.44, 0.58, 0.4, 0.62, 0.46, 0.55][i % 6];
            let angle = [2.2, -0.6, 0.9, 3.6, -2.0, 1.4][i % 6];
            widgets::paint_shuttle(&p, pos2(right, y), (margin * 0.42).min(30.0), angle, pal.dark);
        }
        let (l, w) = (surfaces::COURT_L, surfaces::COURT_W);
        // `x` runs along the length (top to bottom), `y` across the width.
        let map = |x: f32, y: f32| pos2(lines.left() + lines.width() * y / w, lines.top() + lines.height() * x / l);

        // Player positions: team A in the top half, team B in the bottom half,
        // partners one behind the other so each name gets the court's width.
        // Cards are a little wider than the court so names read from across the gym.
        let chip_w = (lines.width() * 1.16).min(surface.width() - 24.0);
        let chip_h = (lines.height() * 0.185).clamp(28.0, 110.0);
        let font = self.group_font(&p, Spot::Court(i, 0), vec2(chip_w, chip_h), (chip_h * 0.5).min(56.0));
        self.court_font = font.0;
        let centers = [
            map(1.9, w / 2.0),
            map(4.75, w / 2.0),
            map(l - 4.75, w / 2.0),
            map(l - 1.9, w / 2.0),
        ];
        let time_up = occupied && self.state.timer_finished(now);
        if occupied && !time_up {
            widgets::paint_vs(&p, pal, map(l / 2.0, w / 2.0), (chip_h * 0.26).clamp(12.0, 19.0));
        }
        if !occupied && self.drag.is_none() {
            // A free court gets one friendly sign instead of four empty slots
            // (they come back as drop targets while dragging).
            for (s, c) in centers.into_iter().enumerate() {
                self.drop_targets.push((Rect::from_center_size(c, vec2(chip_w, chip_h)), Target::Slot(Spot::Court(i, s))));
            }
            let sign = Rect::from_center_size(map(l * 0.25, w / 2.0), vec2((lines.width() * 0.84).max(180.0), (chip_h * 1.5).max(96.0)));
            p.rect_filled(sign.translate(vec2(0.0, 3.0)), CornerRadius::same(12), Color32::from_black_alpha(50));
            p.rect_filled(sign, CornerRadius::same(12), Color32::from_rgb(14, 36, 60));
            p.rect_stroke(sign, CornerRadius::same(12), Stroke::new(1.0, Color32::from_white_alpha(70)), StrokeKind::Inside);
            let k = (sign.height() / 110.0).clamp(0.8, 1.4);
            p.text(sign.center() - vec2(0.0, 26.0 * k), Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(30.0 * k), Color32::from_white_alpha(230));
            p.text(sign.center() + vec2(0.0, 6.0 * k), Align2::CENTER_CENTER, "Court open", semibold(20.0 * k), Color32::WHITE);
            let hint = if self.state.session.deck[i].iter().any(Option::is_some) {
                "Next group plays here"
            } else {
                "Drag players here to start a game"
            };
            let hint_size = widgets::fit_font(ui, hint, regular, 15.0 * k, sign.width() - 28.0);
            p.text(sign.center() + vec2(0.0, 30.0 * k), Align2::CENTER_CENTER, hint, regular(hint_size), Color32::from_white_alpha(210));
            return;
        }
        for (s, c) in centers.into_iter().enumerate() {
            let r = Rect::from_center_size(c, vec2(chip_w, chip_h));
            self.slot(ui, pal, Spot::Court(i, s), r, font, Color32::from_white_alpha(140), "Open", dt);
        }
        if time_up {
            // Game over on this court: hush the cards and stamp it.
            let hush = Rect::from_min_max(pos2(surface.left(), lines.top()), pos2(surface.right(), lines.bottom()));
            // Game over on this court: wash it out and stamp it.
            let _ = hush;
            let tilt = [-0.07, 0.05, -0.04, 0.06][i % 4];
            let size = (chip_h * 0.28).clamp(14.0, 24.0);
            widgets::paint_stamp(&p, map(l / 2.0, w / 2.0), "TIME!", size, tilt, theme::hex(0xC2410C), Color32::WHITE);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn deck_card(&mut self, ui: &mut Ui, pal: &Palette, i: usize, rect: Rect, now: i64, dt: f32) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let finished = self.state.timer_finished(now);
        widgets::card(&p, pal, rect, pal.bg1, 8, 2);
        let group = self.state.session.deck[i];
        let filled = group.iter().flatten().count();

        let header_h = (rect.height() * 0.2).clamp(36.0, 50.0);
        let hfont = (header_h * 0.4).min(19.0);
        let hy = rect.top() + header_h / 2.0 + 2.0;
        let title = widgets::court_title(&p, pal, pos2(rect.left() + 14.0, hy), i + 1, hfont * 1.1, false);
        // Only a group that isn't complete needs a word.
        let pill = match filled {
            0 => Some(("Waiting for players".to_owned(), None, pal.subtle_selected, pal.fg2)),
            4 => None,
            k => Some((format!("{k} of 4"), Some(icon::PERSON), pal.warning_subtle, pal.warning_fg)),
        };
        if let Some((status, glyph, bg, fg)) = pill {
            widgets::paint_badge(&p, pos2(title.right() + 10.0, hy), Align2::LEFT_CENTER, glyph, &status, CHIP_TEXT, bg, fg);
        }
        if filled > 0 && self.state.session.round_number > 0 && !finished {
            let b = Rect::from_min_max(pos2(rect.right() - 118.0, hy - 15.0), pos2(rect.right() - 10.0, hy + 15.0));
            let mut bui = ui.new_child(UiBuilder::new().max_rect(b).layout(Layout::right_to_left(Align::Center)));
            let resp = widgets::button(&mut bui, pal, ButtonKind::Secondary, Some(icon::ARROW_UP_F), "Play now", 30.0)
                .on_hover_text(format!("Send this group onto court {} now", i + 1));
            if resp.clicked() {
                self.act(&ctx, format!("Court {} started early", i + 1), |s| {
                    s.promote_deck(i, now_ms());
                    s.fill_deck(now_ms());
                });
            }
        }

        // Like the court: one team above the "net", the other below,
        // partners side by side.
        let body = Rect::from_min_max(
            pos2(rect.left() + 14.0, rect.top() + header_h + 2.0),
            pos2(rect.right() - 14.0, rect.bottom() - 14.0),
        );
        let net_h = 40.0;
        let col_gap = 8.0;
        let chip_h = ((body.height() - net_h) / 2.0).clamp(24.0, 150.0);
        let chip_w = (body.width() - col_gap) / 2.0;
        // Never louder than the courts: they're what matters most.
        let font = self.group_font(&p, Spot::Deck(i, 0), vec2(chip_w, chip_h), (chip_h * 0.46).min(40.0).min(self.court_font * 0.92));
        let top = body.center().y - chip_h - net_h / 2.0;
        for s in 0..4 {
            let row = s / 2;
            let col = s % 2;
            let x = body.left() + col as f32 * (chip_w + col_gap);
            let y = top + row as f32 * (chip_h + net_h);
            let r = Rect::from_min_size(pos2(x, y), vec2(chip_w, chip_h));
            self.slot(ui, pal, Spot::Deck(i, s), r, font, pal.stroke_accessible, "Open", dt);
        }
        // The "net" between the teams.
        let c = body.center();
        // A little net between the teams, with the VS medallion on it.
        let net = Rect::from_center_size(c, vec2(body.width() - 8.0, 6.0));
        p.rect_filled(net, CornerRadius::same(2), pal.stroke2.gamma_multiply(0.6));
        for k in 0..((net.width() / 8.0) as i32) {
            let x = net.left() + 4.0 + k as f32 * 8.0;
            p.line_segment([pos2(x, net.top()), pos2(x, net.bottom())], Stroke::new(1.0, pal.bg1.gamma_multiply(0.7)));
        }
        widgets::paint_vs(&p, pal, c, 15.0);
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
            let now = now_ms();
            self.state.move_player(pid, target, now);
            // Settle the chip from where it was dropped.
            self.note_rect(pid, Rect::from_center_size(pos, vec2(240.0, 56.0)));
            self.dirty = true;
        }
    }
}

// ===========================================================================
// Waiting pool

impl GoodmintonApp {
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

        ui.horizontal(|ui| {
            ui.label(RichText::new("Waiting").font(semibold(20.0)).color(pal.fg1));
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 26.0), Sense::hover());
            widgets::paint_badge(ui.painter(), r.left_center(), Align2::LEFT_CENTER, None, &active.len().to_string(), 13.0, pal.brand_subtle, pal.on_brand_subtle);
        });
        ui.label(RichText::new("Longest since last game first").font(regular(14.0)).color(pal.fg3));
        ui.add_space(6.0);
        // The skill key, next to the lists that use it: two columns of two.
        {
            let bh = 22.0;
            let bw = widgets::badge_width(bh);
            let (key, _) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
            let p = ui.painter();
            p.rect(key, CornerRadius::same(6), pal.bg2, Stroke::new(1.0, pal.stroke2), StrokeKind::Inside);
            let col_w = (key.width() - 16.0) / 2.0;
            for (k, skill) in Skill::ALL.iter().enumerate() {
                let x = key.left() + 10.0 + (k / 2) as f32 * col_w;
                let y = key.top() + 15.0 + (k % 2) as f32 * 28.0;
                let b = Rect::from_min_size(pos2(x, y - bh / 2.0), vec2(bw, bh));
                widgets::paint_skill_badge(p, pal, b, *skill);
                p.text(pos2(b.right() + 8.0, y), Align2::LEFT_CENTER, skill.label(), semibold(15.0), pal.fg1);
            }
        }
        ui.add_space(6.0);
        if self.help {
            widgets::help_bar(ui, pal, &self.help_waiting());
            ui.add_space(4.0);
        }

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
                    self.roster_open = true;
                }
            });
            return;
        }

        // The front of the queue gets big rows (they're about to play); everyone
        // behind them goes in a compact two-column grid, so the whole queue
        // stays on screen for the TV.
        const BIG: f32 = 56.0;
        const SMALL: f32 = 34.0;
        const HEAD: f32 = 40.0;
        // `pool_bonus` corrects the estimate from what last frame actually used,
        // so the big rows grow to fill the pane without spilling over.
        let avail = ui.available_height() + self.pool_bonus;
        let n = active.len();
        let resting_h = if resting.is_empty() { 0.0 } else { HEAD + 12.0 + resting.len() as f32 * SMALL };
        let total = |k: usize| {
            let rest = n - k;
            k as f32 * BIG + if rest > 0 { HEAD + rest.div_ceil(2) as f32 * SMALL } else { 0.0 } + resting_h
        };
        // Always the same three "next in line" rows (fewer only if the pane is tiny).
        let avail = avail - HEAD - 16.0;
        let mut big = 3.min(n);
        while big > 1 && total(big) > avail {
            big -= 1;
        }
        // The grid rows shrink (to a floor) so everyone fits; only past that do
        // we show as many as fit and count the rest.
        let grid_rows = (n - big).div_ceil(2).max(1);
        let room = avail - big as f32 * BIG - HEAD - resting_h - 8.0;
        let cell_h = (room / grid_rows as f32 - 2.0).floor().clamp(24.0, SMALL - 2.0);
        let grid_rows_fit = ((room - 30.0) / (cell_h + 2.0)).floor().max(0.0) as usize;
        let shown = if grid_rows as f32 * (cell_h + 2.0) <= room { n } else { (big + grid_rows_fit * 2).min(n) };
        let out = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if active.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(icon::SHUTTLE_F).font(icon_font(40.0)).color(pal.brand_fg.gamma_multiply(0.6)));
                    ui.label(RichText::new("Nobody's waiting").font(semibold(16.0)).color(pal.fg1));
                    ui.label(RichText::new("Everyone is on a court or up next.").font(regular(14.0)).color(pal.fg3));
                });
            }
            if n > 0 {
                widgets::section_label(ui, pal, "Next in line");
                ui.add_space(2.0);
            }
            for (rank, id) in active.iter().take(big).enumerate() {
                self.pool_row(ui, pal, *id, Some(rank + 1), now, BIG - 2.0);
            }
            if big < n {
                ui.add_space(8.0);
                widgets::section_label(ui, pal, &format!("Also waiting ({})", n - big));
                ui.add_space(2.0);
                let w = ((ui.available_width() - 8.0) / 2.0).floor();
                // Read down the first column, then the second.
                let rest = &active[big..shown.max(big)];
                let rows = rest.len().div_ceil(2);
                for row in 0..rows {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for col in 0..2 {
                            if let Some(id) = rest.get(col * rows + row) {
                                self.pool_cell(ui, pal, *id, Some(big + col * rows + row + 1), w, cell_h, now);
                            }
                        }
                    });
                }
                if shown < n {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("+ {} more waiting", n - shown)).font(semibold(14.0)).color(pal.fg3));
                }
            }
            if !resting.is_empty() {
                ui.add_space(8.0);
                widgets::section_label(ui, pal, &format!("On a break ({})", resting.len()));
                ui.add_space(2.0);
                for id in &resting {
                    self.pool_row(ui, pal, *id, None, now, SMALL - 2.0);
                }
            }
            if n > 6 {
                // Only the queue counts toward fitting; leave the note out.
            } else if n > 0 {
                // A short bench gets a friendly note instead of a blank column.
                ui.add_space(28.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(icon::SHUTTLE_F).font(icon_font(34.0)).color(pal.brand_fg.gamma_multiply(0.45)));
                    ui.label(RichText::new("Short bench tonight").font(semibold(15.0)).color(pal.fg2));
                    ui.label(RichText::new("Everyone else is playing or up next.").font(regular(13.0)).color(pal.fg3));
                });
            }
        });
        if n > 6 {
            let spare = out.inner_rect.height() - out.content_size.y - 24.0;
            if spare.abs() > 6.0 {
                self.pool_bonus = (self.pool_bonus + spare * 0.7).clamp(-300.0, 600.0);
                ui.ctx().request_repaint();
            }
        }
    }

    /// A compact queue entry for the "Also waiting" grid: rank, badge, name.
    #[allow(clippy::too_many_arguments)]
    fn pool_cell(&mut self, ui: &mut Ui, pal: &Palette, id: PlayerId, rank: Option<usize>, width: f32, height: f32, now: i64) {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
        let resp = resp.on_hover_cursor(CursorIcon::Grab).on_hover_text(self.since_played_label(id).unwrap_or_default());
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.drag = Some(id);
        }
        self.player_menu(&resp, pal, id);
        self.note_rect(id, rect);
        let hover = ui.ctx().animate_bool_responsive(resp.id.with("h"), resp.hovered());
        let presence = &self.state.session.present[&id];
        let overdue = self.state.session.round_number > 0 && !presence.resting && presence.waiting_ms(now) >= self.state.settings.max_wait_ms();
        let alpha = if self.drag == Some(id) { 0.45 } else { 1.0 };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(4), theme::lerp_color(Color32::TRANSPARENT, pal.subtle_hover, hover));
        let cy = rect.center().y;
        let mut x = rect.left() + 4.0;
        if let Some(rank) = rank {
            p.text(pos2(x + 9.0, cy), Align2::CENTER_CENTER, rank.to_string(), regular(14.0), pal.fg3);
            x += 22.0;
        }
        let bh = (height * 0.6).clamp(16.0, 20.0).round();
        let b = Rect::from_min_size(pos2(x, cy - bh / 2.0), vec2(widgets::badge_width(bh), bh));
        widgets::paint_skill_badge(p, pal, b, self.state.skill(id));
        x = b.right() + 7.0;
        let name = self.state.name(id).to_owned();
        let color = if overdue { pal.warning_fg } else { pal.fg1 };
        // One size for the whole grid; a long name ends in "…" rather than shrinking.
        let g = widgets::elided(ui.painter(), &name, semibold((height * 0.5).clamp(13.0, 16.0).round()), color.gamma_multiply(alpha), rect.right() - x - 4.0);
        ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g, color);
    }

    fn pool_row(&mut self, ui: &mut Ui, pal: &Palette, id: PlayerId, rank: Option<usize>, now: i64, row_h: f32) {
        let ctx = ui.ctx().clone();
        let compact = row_h < 44.0;
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
        if let Some(rank) = rank {
            p.text(pos2(x + 11.0, cy), Align2::CENTER_CENTER, rank.to_string(), regular(13.0), pal.fg3.gamma_multiply(alpha));
            x += 28.0;
        }
        let badge = (row_h * 0.62).clamp(18.0, 28.0).round();
        let b = Rect::from_min_size(pos2(x, cy - badge / 2.0), vec2(widgets::badge_width(badge), badge));
        widgets::paint_skill_badge(p, pal, b, skill);
        if dimmed {
            p.rect_filled(b, CornerRadius::same(6), pal.bg1.gamma_multiply(0.55));
        }
        x = b.right() + 10.0;
        let mut detail = self.since_played_label(id).unwrap_or_default();
        if skipping && !presence.resting {
            detail.push_str(" • sitting out");
        }
        let right = rect.right() - 6.0 - if overdue { 22.0 } else { 0.0 };
        if compact {
            // One line: name on the left, wait and games on the right.
            // The hover actions take this spot, so the detail fades out for them.
            let fade = alpha * (1.0 - if presence.resting { 1.0 } else { hover });
            let detail_color = if overdue { pal.warning_fg } else { pal.fg3 };
            let dg = p.layout_no_wrap(detail, regular(12.5), detail_color.gamma_multiply(fade));
            let name_max = if row_h < 32.0 { 15.0 } else { 16.0 };
            let name_font = widgets::fit_font(ui, &name, semibold, name_max, right - x - dg.size().x - 10.0);
            let p = ui.painter();
            p.text(pos2(x, cy), Align2::LEFT_CENTER, &name, semibold(name_font), pal.fg1.gamma_multiply(alpha));
            p.galley(pos2(right - dg.size().x, cy - dg.size().y / 2.0), dg, detail_color.gamma_multiply(fade));
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

impl GoodmintonApp {
    fn roster(&mut self, ui: &mut Ui, pal: &Palette, now: i64) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Roster").font(semibold(20.0)));
            ui.label(
                RichText::new(format!("{} checked in · {} players", self.state.session.present.len(), self.state.players.len()))
                    .font(regular(14.0))
                    .color(pal.fg3),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, pal, icon::CLOSE, 32.0, false).on_hover_text("Close").clicked() {
                    self.roster_open = false;
                }
            });
        });
        ui.add_space(2.0);
        if self.help {
            widgets::help_bar(ui, pal, &help_roster());
            ui.add_space(2.0);
        }

        // --- Add a player ---
        Frame::NONE
            .fill(pal.bg2)
            .stroke(Stroke::new(1.0, pal.stroke2))
            .corner_radius(6)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_label(ui, pal, "Add a player");
                let edit = egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("Name")
                    .font(regular(16.0))
                    .margin(Margin::symmetric(10, 7))
                    .desired_width(f32::INFINITY);
                let resp = ui.add(edit);
                let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
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
                ui.add_space(2.0);
                let can_add = !self.new_name.trim().is_empty();
                // Always looks ready; with no name yet it just puts you in the name box.
                let add_clicked = ui
                    .with_layout(Layout::top_down_justified(Align::Min), |ui| {
                        widgets::button(ui, pal, ButtonKind::Primary, Some(icon::PERSON_ADD), "Add & check in", 36.0)
                    })
                    .inner
                    .clicked();
                if add_clicked && !can_add {
                    resp.request_focus();
                }
                if (entered || add_clicked) && can_add {
                    let name = self.new_name.trim().to_owned();
                    let exists = self
                        .state
                        .players
                        .values()
                        .find(|p| p.name.eq_ignore_ascii_case(&name))
                        .map(|p| p.id);
                    let id = exists.unwrap_or_else(|| self.state.add_player(&name, self.new_skill));
                    self.state.check_in(id, now);
                    self.dirty = true;
                    self.new_name.clear();
                    if exists.is_some() {
                        self.toast(&ctx, format!("{name} is already on the roster — checked in"));
                    }
                    resp.request_focus();
                }
            });
        ui.add_space(4.0);

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
                            "Your roster is empty. Add the first player above."
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
                if widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::WARNING), "New game day…", 36.0)
                    .on_hover_text("Check everyone out and clear the courts")
                    .clicked()
                {
                    self.confirm = Some(Confirm::NewDay);
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
                    (Some(ms), Some(p)) => format!("{} • {}", short_duration(ms), plural(p.games_tonight)),
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

impl GoodmintonApp {
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
        let now = now_ms();
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
                ui.label(RichText::new(format!("Goodminton {}", update::VERSION)).font(regular(15.0)).color(pal.fg1));
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
                        "The new version is installed. Goodminton will close and open again in a few seconds."
                    }
                    UpdateStatus::Failed(_) => "",
                    _ => {
                        "Goodminton will download the update, then restart. Your roster, courts, timer and history \
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
            let name = format!("goodminton-{}-{}.db", update::VERSION, chrono::Local::now().format("%Y%m%d-%H%M%S"));
            if let Err(err) = self.store.backup_to(&dir.join(name)) {
                eprintln!("goodminton: backup before update failed: {err}");
            }
            prune_backups(&dir, 10);
        }
        update::request_restart();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(which) = self.confirm else { return };
        let (title, body, action) = match which {
            Confirm::NewDay => (
                "Start a new game day?".to_owned(),
                "Today goes into History (games still on court count as finished). Everyone starts checked out; your roster and settings are kept.".to_owned(),
                "Start new day",
            ),
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
                Confirm::NewDay => self.start_new_day(ctx, true),
                Confirm::Delete(id) => {
                    let name = self.state.name(id).to_owned();
                    self.act(ctx, format!("{name} removed"), |s| s.delete_player(id, now_ms()));
                }
            }
        }
    }

    fn open_history(&mut self) {
        // Save first so today's games show up.
        self.save();
        self.dirty = false;
        let days = self.store.days().unwrap_or_else(|err| {
            eprintln!("goodminton: history: {err}");
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
        let size = vec2((screen.width() - 160.0).min(860.0), (screen.height() - 300.0).clamp(360.0, 500.0));
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
                let body = Rect::from_min_size(ui.cursor().min, vec2(size.x, ui.available_height().max(200.0)));
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
            widgets::paint_badge(p, pos2(title.right() + 8.0, title.center().y), Align2::LEFT_CENTER, None, "Today", 11.0, pal.brand_subtle, pal.on_brand_subtle);
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
        let span = day.ended_ms.unwrap_or_else(now_ms) - day.started_ms;
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
        let gap = self.toast_gap.filter(|g| g.width() >= self.toast_w);
        let (pivot, spot) = match gap {
            Some(g) => (Align2::CENTER_CENTER, g.center()),
            // Not enough room in the ribbon: the middle of the title bar.
            None => (Align2::CENTER_TOP, pos2(ctx.content_rect().center().x, 2.0)),
        };
        let area = egui::Area::new(unique("snackbar"))
            .order(Order::Foreground)
            .pivot(pivot)
            .fixed_pos(spot + vec2(0.0, (1.0 - k) * 12.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_opacity(k);
                Frame::NONE
                    .fill(if pal.dark { pal.subtle_selected } else { pal.bg1 })
                    .stroke(Stroke::new(1.0, pal.stroke2))
                    .corner_radius(8)
                    .inner_margin(Margin { left: 8, right: 4, top: 4, bottom: 4 })
                    .shadow(pal.shadow(8))
                    .show(ui, |ui| {
                        ui.set_min_height(40.0);
                        ui.set_max_height(44.0);
                        ui.horizontal_centered(|ui| {
                            if round_news {
                                // The app's shuttle tile: the round has turned over.
                                let (tile, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                                ui.painter().rect_filled(tile, CornerRadius::same(6), pal.brand);
                                ui.painter().text(tile.center(), Align2::CENTER_CENTER, icon::SHUTTLE_F, icon_font(17.0), pal.on_brand);
                            } else {
                                ui.add_space(4.0);
                                ui.label(RichText::new(icon::INFO_F).font(icon_font(20.0)).color(pal.brand_fg));
                            }
                            ui.label(RichText::new(&text).font(semibold(16.0)).color(pal.fg1));
                            ui.add_space(8.0);
                            if has_undo
                                && widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::UNDO), "Undo", 30.0).clicked()
                            {
                                undo = true;
                            }
                            if widgets::icon_button(ui, pal, icon::CLOSE, 30.0, false).clicked() {
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

impl GoodmintonApp {
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

impl GoodmintonApp {
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
        let now = now_ms();
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
            PlayerAction::Delete => self.confirm = Some(Confirm::Delete(id)),
        }
    }

    /// A small dialog for renaming from the right-click menu.
    /// Enter saves; Esc, Cancel, or clicking outside cancels.
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
