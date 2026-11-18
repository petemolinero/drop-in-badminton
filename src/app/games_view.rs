//! The Games view: every game ever played, one row each, newest first.
//! It's a record of what happened, so it's read-only until unlocked (with a
//! warning); unlocked, every cell can be corrected and games deleted.

use std::rc::Rc;

use super::*;
use crate::store::LoggedGame;
use chrono::{NaiveDate, NaiveTime, TimeZone, Timelike};

const ROW_H: f32 = 40.0;
const HEADER_H: f32 = 38.0;
const CELL_PAD: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) enum Col {
    #[default]
    Date,
    Started,
    Length,
    Round,
    Court,
    Players,
    TeamA,
    TeamB,
    /// Delete, while unlocked.
    Remove,
}

impl Col {
    fn title(self) -> &'static str {
        match self {
            Col::Date => "Date",
            Col::Started => "Started",
            Col::Length => "Length",
            Col::Round => "Round",
            Col::Court => "Court",
            Col::Players => "Players",
            Col::TeamA => "Team A",
            Col::TeamB => "Team B",
            Col::Remove => "",
        }
    }

    /// Width at full size; the table scales every column to fit.
    fn width(self) -> f32 {
        match self {
            Col::Date => 180.0,
            Col::Started => 110.0,
            Col::Length => 100.0,
            Col::Round => 84.0,
            Col::Court => 90.0,
            Col::Players => 90.0,
            Col::TeamA | Col::TeamB => 270.0,
            Col::Remove => 56.0,
        }
    }

    fn numeric(self) -> bool {
        matches!(self, Col::Length | Col::Round | Col::Court | Col::Players)
    }

    fn sortable(self) -> bool {
        self != Col::Remove
    }

    /// Dates and numbers sort newest/biggest first on the first click.
    fn descending_first(self) -> bool {
        !matches!(self, Col::TeamA | Col::TeamB)
    }

    /// The typed-in columns, when unlocked.
    fn editable_text(self) -> bool {
        matches!(self, Col::Date | Col::Started | Col::Length | Col::Round | Col::Court)
    }
}

/// Where a game lives: the current session's games are in memory (and saved
/// with it); earlier sessions' are only in the database.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum GameKey {
    Current(usize),
    Past(i64),
}

struct Row {
    key: GameKey,
    day: Option<i64>,
    record: GameRecord,
}

pub(super) struct GamesView {
    search: String,
    sort: Col,
    descending: bool,
    pub(super) unlocked: bool,
    confirm_unlock: bool,
    /// Past sessions' games, as of save generation `.0`.
    log: Option<(u64, Rc<Vec<LoggedGame>>)>,
    /// The cell being typed in, and what's been typed so far.
    editing: Option<(GameKey, Col, String)>,
    /// What's typed in the player picker's search box.
    pick_search: String,
}

impl Default for GamesView {
    fn default() -> Self {
        Self {
            search: String::new(),
            sort: Col::Date,
            descending: true,
            unlocked: false,
            confirm_unlock: false,
            log: None,
            editing: None,
            pick_search: String::new(),
        }
    }
}

impl DropInApp {
    pub(super) fn games_view(&mut self, ui: &mut Ui, pal: &Palette) {
        let rows = self.game_rows();
        let names = self.all_names();
        let sessions = rows.iter().map(|r| r.day).collect::<std::collections::HashSet<_>>().len();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Games").font(semibold(26.0)).color(pal.fg1));
            ui.add_space(8.0);
            let games = if rows.len() == 1 { "1 game".to_owned() } else { format!("{} games", rows.len()) };
            let sessions = if sessions == 1 { "1 session".to_owned() } else { format!("{sessions} sessions") };
            ui.label(RichText::new(format!("Every game played · {games} across {sessions}")).font(regular(15.0)).color(pal.fg3));
        });
        ui.add_space(10.0);
        let (mut export, mut lock_clicked) = (false, false);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.games_view.search)
                    .hint_text(format!("{}  Search by player or date", icon::SEARCH))
                    .font(regular(15.0))
                    .margin(Margin::symmetric(10, 7))
                    .desired_width(280.0),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let unlocked = self.games_view.unlocked;
                let (kind, glyph, label, tip) = if unlocked {
                    (ButtonKind::Primary, icon::LOCK, "Lock editing", "Make the games read-only again")
                } else {
                    (ButtonKind::Secondary, icon::LOCK_OPEN, "Unlock editing", "Correct or delete games that were logged wrongly")
                };
                lock_clicked = widgets::button(ui, pal, kind, Some(glyph), label, 34.0).on_hover_text(tip).clicked();
                export = widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::EXPORT), "Export CSV", 34.0)
                    .on_hover_text("Save the games shown below as a spreadsheet")
                    .clicked();
            });
        });
        if lock_clicked {
            if self.games_view.unlocked {
                self.games_view.unlocked = false;
                self.games_view.editing = None;
            } else {
                self.games_view.confirm_unlock = true;
            }
        }
        ui.add_space(12.0);
        if self.games_view.unlocked {
            Frame::NONE
                .fill(pal.warning_subtle)
                .stroke(Stroke::new(1.0, pal.warning_fg.gamma_multiply(0.4)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::WARNING).font(icon_font(18.0)).color(pal.warning_fg));
                        ui.label(
                            RichText::new("Editing history. Changes are saved straight away, and changes to past sessions can't be undone.")
                                .font(regular(14.0))
                                .color(pal.fg1),
                        );
                    });
                });
            ui.add_space(10.0);
        }

        // Which rows, in which order.
        let gv = &self.games_view;
        let query = gv.search.trim().to_lowercase();
        let mut shown: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                query.is_empty()
                    || r.record.players.iter().flatten().any(|id| names.get(id).is_some_and(|n| n.to_lowercase().contains(&query)))
                    || fmt_date(r.record.started_ms, "%a %A %b %B %-d %Y").to_lowercase().contains(&query)
            })
            .collect();
        if gv.editing.is_none() {
            shown.sort_by(|a, b| {
                let ord = compare(gv.sort, a, b, &names);
                let ord = if gv.descending { ord.reverse() } else { ord };
                // Ties: in the order they were played.
                ord.then(a.record.started_ms.cmp(&b.record.started_ms)).then(a.record.court.cmp(&b.record.court))
            });
        }
        if export {
            let lines: Vec<Vec<String>> = shown.iter().map(|r| csv_row(r, &names)).collect();
            let ctx = ui.ctx().clone();
            let file = format!("Games {}.csv", chrono::Local::now().format("%Y-%m-%d"));
            self.export_csv(&ctx, &file, &CSV_HEADER, &lines);
        }

        let cols: Vec<Col> = [Col::Date, Col::Started, Col::Length, Col::Round, Col::Court, Col::Players, Col::TeamA, Col::TeamB]
            .into_iter()
            .chain(self.games_view.unlocked.then_some(Col::Remove))
            .collect();
        pane(pal).inner_margin(Margin::same(0)).show(ui, |ui| {
            let avail = ui.available_size();
            ui.set_min_size(avail);
            let full: f32 = cols.iter().map(|c| c.width()).sum();
            let widths: Vec<f32> = cols.iter().map(|c| (c.width() * avail.x / full).floor()).collect();
            self.games_header(ui, pal, &cols, &widths);
            if shown.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    let text = if rows.is_empty() { "No games yet. They'll appear here as rounds finish." } else { "No games match." };
                    ui.label(RichText::new(text).font(regular(16.0)).color(pal.fg3));
                });
                return;
            }
            // Rows of one session share a shade, so sessions read as blocks.
            let mut shade = Vec::with_capacity(shown.len());
            let mut on = false;
            for (i, r) in shown.iter().enumerate() {
                if i > 0 && shown[i - 1].day != r.day {
                    on = !on;
                }
                shade.push(on);
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, shown.len(), |ui, range| {
                for i in range {
                    self.game_row(ui, pal, shown[i], shade[i], &cols, &widths, &names);
                }
            });
        });

        self.unlock_dialog(ui.ctx(), pal);
    }

    /// The current session's games from memory, then every earlier one.
    fn game_rows(&mut self) -> Vec<Row> {
        let generation = self.save_gen;
        let log = match &self.games_view.log {
            Some((seen, log)) if *seen == generation => log.clone(),
            _ => {
                let log = Rc::new(self.store.game_log().unwrap_or_else(|err| {
                    eprintln!("drop-in-badminton: couldn't read the games: {err}");
                    Vec::new()
                }));
                self.games_view.log = Some((generation, log.clone()));
                log
            }
        };
        let today = self.state.session.day_id;
        let mut rows: Vec<Row> = log
            .iter()
            .filter(|g| Some(g.day) != today)
            .map(|g| Row { key: GameKey::Past(g.id), day: Some(g.day), record: g.record.clone() })
            .collect();
        rows.extend(self.state.session.games.iter().enumerate().map(|(i, g)| Row { key: GameKey::Current(i), day: today, record: g.clone() }));
        rows
    }

    /// Every player's name, archived ones too.
    fn all_names(&mut self) -> HashMap<PlayerId, String> {
        let stats = self.stats();
        let mut names: HashMap<PlayerId, String> = stats.players.iter().map(|(id, s)| (*id, s.name.clone())).collect();
        names.extend(self.state.players.values().map(|p| (p.id, p.name.clone())));
        names
    }

    fn games_header(&mut self, ui: &mut Ui, pal: &Palette, cols: &[Col], widths: &[f32]) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
        let p = ui.painter();
        p.rect_filled(bar, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, pal.bg2);
        p.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, pal.stroke1));
        let mut x = bar.left();
        for (&col, w) in cols.iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, bar.top()), vec2(*w, HEADER_H));
            x += w;
            if !col.sortable() {
                continue;
            }
            let resp = ui.interact(cell, unique(("gv-head", col)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let sorted = self.games_view.sort == col;
            let ink = if sorted { pal.fg1 } else if resp.hovered() { pal.fg2 } else { pal.fg3 };
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(cell.shrink(2.0), CornerRadius::same(4), pal.subtle_hover);
            }
            let text = p.layout_no_wrap(col.title().to_owned(), semibold(13.0), ink);
            let arrow_w = if sorted { 18.0 } else { 0.0 };
            let tx = if col.numeric() { cell.right() - CELL_PAD - text.size().x - arrow_w } else { cell.left() + CELL_PAD };
            let tw = text.size().x;
            p.with_clip_rect(cell.shrink2(vec2(CELL_PAD - 2.0, 0.0))).galley(pos2(tx, cell.center().y - text.size().y / 2.0), text, ink);
            if sorted {
                let arrow = if self.games_view.descending { icon::DOWN } else { icon::UP };
                p.text(pos2(tx + tw + 4.0, cell.center().y + 1.0), Align2::LEFT_CENTER, arrow, icon_font(13.0), pal.brand_fg);
            }
            if cell.left() > bar.left() {
                p.vline(cell.left(), cell.y_range().shrink(10.0), Stroke::new(1.0, pal.stroke2));
            }
            if resp.clicked() {
                let gv = &mut self.games_view;
                if gv.sort == col {
                    gv.descending = !gv.descending;
                } else {
                    gv.sort = col;
                    gv.descending = col.descending_first();
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn game_row(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, shaded: bool, cols: &[Col], widths: &[f32], names: &HashMap<PlayerId, String>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let hovered = ui.rect_contains_pointer(rect);
        let p = ui.painter();
        let fill = if hovered { pal.subtle_hover } else if shaded { pal.bg2.gamma_multiply(0.5) } else { Color32::TRANSPARENT };
        p.rect_filled(rect, CornerRadius::ZERO, fill);
        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, pal.stroke2));
        let unlocked = self.games_view.unlocked;
        let mut x = rect.left();
        for (&col, w) in cols.iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, ROW_H));
            if x > rect.left() {
                ui.painter().vline(x, rect.y_range(), Stroke::new(1.0, pal.stroke2));
            }
            x += w;
            match col {
                Col::TeamA | Col::TeamB => {
                    let first = if col == Col::TeamA { 0 } else { 2 };
                    self.team_cell(ui, pal, row, cell, first, names);
                }
                Col::Remove => self.remove_cell(ui, pal, row, cell),
                _ if unlocked && col.editable_text() => self.game_text_cell(ui, pal, row, cell, col),
                _ => {
                    let p = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
                    let (pos, align) = if col.numeric() {
                        (pos2(cell.right() - CELL_PAD, cell.center().y), Align2::RIGHT_CENTER)
                    } else {
                        (pos2(cell.left() + CELL_PAD, cell.center().y), Align2::LEFT_CENTER)
                    };
                    p.text(pos, align, display(col, &row.record), regular(15.0), pal.fg1);
                }
            }
        }
    }

    /// A typed-in cell (date, time, length, round, court), while unlocked.
    fn game_text_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect, col: Col) {
        let mut buf = match &self.games_view.editing {
            Some((k, c, text)) if *k == row.key && *c == col => text.clone(),
            _ => edit_text(col, &row.record),
        };
        let edit = egui::TextEdit::singleline(&mut buf)
            .id(unique(("gv-text", row.key, col)))
            .frame(Frame::NONE.inner_margin(Margin { left: 6, right: 6, top: 4, bottom: 6 }))
            .font(regular(15.0))
            .text_color(pal.fg1)
            .align(if col.numeric() { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER })
            .desired_width(f32::INFINITY);
        let area = Rect::from_center_size(cell.center(), vec2(cell.width() - 12.0, 30.0));
        let resp = ui.put(area, edit).on_hover_text(match col {
            Col::Date => "A date, like Nov 12, 2026",
            Col::Started => "A time, like 7:30 PM",
            Col::Length => "Minutes, or minutes:seconds",
            _ => "A number",
        });
        let p = ui.painter();
        if resp.has_focus() {
            p.rect_stroke(area, CornerRadius::same(4), Stroke::new(2.0, pal.brand_fg), StrokeKind::Inside);
        } else {
            p.rect_stroke(area, CornerRadius::same(4), Stroke::new(1.0, if resp.hovered() { pal.stroke1 } else { pal.stroke2 }), StrokeKind::Inside);
        }
        if resp.has_focus() || resp.changed() {
            self.games_view.editing = Some((row.key, col, buf.clone()));
        }
        if !resp.lost_focus() {
            return;
        }
        self.games_view.editing = None;
        if buf.trim() == edit_text(col, &row.record) {
            return;
        }
        let ctx = ui.ctx().clone();
        match apply_text(col, &row.record, &buf) {
            Some(record) => self.replace_game(&ctx, row, record),
            None => self.toast(&ctx, format!("Couldn't read “{}” — {}", buf.trim(), resp_hint(col))),
        }
    }

    /// Two players' names; while unlocked each is a button to swap them.
    fn team_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect, first: usize, names: &HashMap<PlayerId, String>) {
        let name = |slot: usize| row.record.players[slot].map(|id| names.get(&id).cloned().unwrap_or_else(|| "Unknown player".into()));
        if !self.games_view.unlocked {
            let team: Vec<String> = (first..first + 2).filter_map(name).collect();
            let text = if team.is_empty() { "—".to_owned() } else { team.join(" & ") };
            let ink = if team.is_empty() { pal.fg3 } else { pal.fg1 };
            let p = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
            p.text(pos2(cell.left() + CELL_PAD, cell.center().y), Align2::LEFT_CENTER, text, regular(15.0), ink);
            return;
        }
        let half = (cell.width() - CELL_PAD * 2.0 + 8.0 - 6.0) / 2.0;
        for (i, slot) in (first..first + 2).enumerate() {
            let r = Rect::from_min_size(pos2(cell.left() + CELL_PAD - 4.0 + i as f32 * (half + 6.0), cell.center().y - 14.0), vec2(half, 28.0));
            let resp = ui
                .interact(r, unique(("gv-slot", row.key, slot)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Change who played");
            let who = name(slot);
            let p = ui.painter();
            let fill = if resp.hovered() { pal.subtle_hover } else { pal.bg1 };
            p.rect(r, CornerRadius::same(6), fill, Stroke::new(1.0, if resp.hovered() { pal.stroke1 } else { pal.stroke2 }), StrokeKind::Inside);
            let (text, ink) = match &who {
                Some(n) => (n.clone(), pal.fg1),
                None => ("Empty".to_owned(), pal.fg3),
            };
            let text = widgets::elided(p, &text, regular(14.0), ink, r.width() - 30.0);
            p.galley(pos2(r.left() + 8.0, r.center().y - text.size().y / 2.0 - 1.0), text, ink);
            p.text(pos2(r.right() - 11.0, r.center().y + 1.0), Align2::CENTER_CENTER, icon::CHEVRON_DOWN, icon_font(11.0), pal.fg3);
            if resp.clicked() {
                self.games_view.pick_search.clear();
            }
            let mut picked: Option<Option<PlayerId>> = None;
            egui::Popup::menu(&resp).id(unique(("gv-pick", row.key, slot))).show(|ui| {
                ui.set_min_width(240.0);
                ui.spacing_mut().button_padding = vec2(10.0, 6.0);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.games_view.pick_search)
                        .hint_text(format!("{}  Find a player", icon::SEARCH))
                        .font(regular(14.0))
                        .margin(Margin::symmetric(8, 6))
                        .desired_width(f32::INFINITY),
                );
                search.request_focus();
                ui.add_space(4.0);
                let query = self.games_view.pick_search.trim().to_lowercase();
                // Anyone not already in this game.
                let mut choices: Vec<(&PlayerId, &String)> = names
                    .iter()
                    .filter(|(id, n)| !row.record.players.contains(&Some(**id)) && (query.is_empty() || n.to_lowercase().contains(&query)))
                    .collect();
                choices.sort_by_key(|(id, n)| (n.to_lowercase(), **id));
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    if who.is_some() && query.is_empty() && ui.button(RichText::new("Leave empty").font(regular(14.0)).color(pal.fg2)).clicked() {
                        picked = Some(None);
                    }
                    for (id, n) in choices {
                        if ui.button(RichText::new(n).font(regular(14.0))).clicked() {
                            picked = Some(Some(*id));
                        }
                    }
                });
                let enter = search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if picked.is_some() || enter {
                    ui.close();
                }
            });
            if let Some(choice) = picked {
                let mut record = row.record.clone();
                record.players[slot] = choice;
                let ctx = ui.ctx().clone();
                self.replace_game(&ctx, row, record);
            }
        }
    }

    fn remove_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect) {
        let r = Rect::from_center_size(cell.center(), Vec2::splat(30.0));
        let resp = ui.interact(r, unique(("gv-remove", row.key)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Delete this game");
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(4), pal.danger_subtle);
        }
        p.text(r.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, icon::DELETE, icon_font(18.0), if resp.hovered() { pal.danger_fg } else { pal.fg3 });
        let mut delete = false;
        // A second click to confirm, since there's no undo.
        egui::Popup::menu(&resp).id(unique(("gv-remove-menu", row.key))).show(|ui| {
            ui.spacing_mut().button_padding = vec2(10.0, 7.0);
            let label = RichText::new(format!("{}  Delete this game", icon::DELETE)).font(regular(15.0)).color(pal.danger_fg);
            delete = ui.button(label).clicked();
        });
        if delete {
            let ctx = ui.ctx().clone();
            self.change_game(&ctx, row, None);
        }
    }

    fn replace_game(&mut self, ctx: &egui::Context, row: &Row, record: GameRecord) {
        self.change_game(ctx, row, Some(record));
    }

    /// Write a corrected game (or delete it, with `None`), keeping players'
    /// game counts in step with who's now recorded as playing.
    fn change_game(&mut self, ctx: &egui::Context, row: &Row, record: Option<GameRecord>) {
        let before: Vec<PlayerId> = row.record.players.iter().flatten().copied().collect();
        let after: Vec<PlayerId> = record.iter().flat_map(|g| g.players.iter().flatten().copied()).collect();
        let current = matches!(row.key, GameKey::Current(_));
        for id in before.iter().filter(|id| !after.contains(id)) {
            if let Some(p) = self.state.players.get_mut(id) {
                p.lifetime_games = p.lifetime_games.saturating_sub(1);
            }
            if let Some(p) = self.state.session.present.get_mut(id).filter(|_| current) {
                p.games_tonight = p.games_tonight.saturating_sub(1);
            }
        }
        for id in after.iter().filter(|id| !before.contains(id)) {
            if let Some(p) = self.state.players.get_mut(id) {
                p.lifetime_games += 1;
            }
            if let Some(p) = self.state.session.present.get_mut(id).filter(|_| current) {
                p.games_tonight += 1;
            }
        }
        let deleted = record.is_none();
        let result = match (row.key, record) {
            (GameKey::Current(i), Some(g)) => {
                if let Some(slot) = self.state.session.games.get_mut(i) {
                    *slot = g;
                }
                Ok(())
            }
            (GameKey::Current(i), None) => {
                if i < self.state.session.games.len() {
                    self.state.session.games.remove(i);
                }
                Ok(())
            }
            (GameKey::Past(id), Some(g)) => self.store.update_game(id, &g),
            (GameKey::Past(id), None) => self.store.delete_game(id),
        };
        // Counts (and the current session) are saved with the app state.
        self.save();
        self.dirty = false;
        match result {
            Err(err) => self.toast(ctx, format!("Couldn't save the change: {err}")),
            Ok(()) if deleted => self.toast(ctx, "Game deleted"),
            Ok(()) => {}
        }
    }

    fn unlock_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.games_view.confirm_unlock {
            return;
        }
        let mut unlock = false;
        let mut cancel = false;
        let modal = Modal::new(unique("games-unlock")).frame(Self::dialog_frame(pal)).backdrop_color(pal.scrim).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::WARNING_F).font(icon_font(22.0)).color(pal.warning_fg));
                ui.label(RichText::new("Edit game history?").font(semibold(20.0)));
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "These are the records of games already played. Changing or deleting them rewrites history: \
                     players' game counts, court time, usual partners and the Analytics charts all change with them, \
                     and changes to past sessions can't be undone.",
                )
                .font(regular(14.0))
                .color(pal.fg2),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Only do this if you know what you're doing — for example, to fix a game logged with the wrong players.")
                    .font(semibold(14.0))
                    .color(pal.fg1),
            );
            ui.add_space(18.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                unlock = widgets::button(ui, pal, ButtonKind::Danger, Some(icon::LOCK_OPEN), "Unlock editing", 34.0).clicked();
                cancel = widgets::button(ui, pal, ButtonKind::Secondary, None, "Cancel", 34.0).clicked();
            });
        });
        if unlock {
            self.games_view.unlocked = true;
        }
        if unlock || cancel || modal.should_close() {
            self.games_view.confirm_unlock = false;
        }
    }
}

fn length_ms(g: &GameRecord) -> i64 {
    (g.ended_ms - g.started_ms).max(0)
}

fn players(g: &GameRecord) -> usize {
    g.players.iter().flatten().count()
}

/// "15:02": minutes and seconds.
fn fmt_length(ms: i64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// What a read-only cell shows.
fn display(col: Col, g: &GameRecord) -> String {
    match col {
        Col::Date => fmt_date(g.started_ms, "%a, %b %-d, %Y"),
        Col::Started => fmt_date(g.started_ms, "%-I:%M %p"),
        Col::Length => fmt_length(length_ms(g)),
        Col::Round => g.round.to_string(),
        Col::Court => g.court.saturating_add(1).to_string(),
        Col::Players => players(g).to_string(),
        _ => String::new(),
    }
}

/// What an editable cell starts out holding.
fn edit_text(col: Col, g: &GameRecord) -> String {
    match col {
        Col::Date => fmt_date(g.started_ms, "%b %-d, %Y"),
        _ => display(col, g),
    }
}

fn resp_hint(col: Col) -> &'static str {
    match col {
        Col::Date => "try a date like Nov 12, 2026",
        Col::Started => "try a time like 7:30 PM",
        Col::Length => "try minutes, like 15 or 14:30",
        _ => "try a whole number, 1 or more",
    }
}

/// The game with one typed-in cell changed, or `None` if it can't be read.
/// Moving the date or start time moves the whole game; the length moves its end.
fn apply_text(col: Col, g: &GameRecord, text: &str) -> Option<GameRecord> {
    let text = text.trim();
    let mut g = g.clone();
    let local = |ms: i64| chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.naive_local());
    let to_ms = |t: chrono::NaiveDateTime| chrono::Local.from_local_datetime(&t).earliest().map(|t| t.timestamp_millis());
    match col {
        Col::Date | Col::Started => {
            let start = local(g.started_ms)?;
            let moved = if col == Col::Date {
                parse_date(text)?.and_time(start.time())
            } else {
                // Keep the seconds, so an unchanged minute stays put.
                start.date().and_time(parse_time(text)?.with_second(start.second())?)
            };
            let delta = to_ms(moved)? - g.started_ms;
            g.started_ms += delta;
            g.ended_ms += delta;
        }
        Col::Length => g.ended_ms = g.started_ms + parse_length(text)?,
        Col::Round => g.round = text.parse::<u32>().ok().filter(|n| *n >= 1)?,
        Col::Court => g.court = text.parse::<usize>().ok().filter(|n| (1..=Settings::MAX_COURTS).contains(n))? - 1,
        _ => return None,
    }
    Some(g)
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.replace("  ", " ");
    ["%b %d, %Y", "%b %d %Y", "%B %d, %Y", "%B %d %Y", "%a, %b %d, %Y", "%Y-%m-%d", "%d %b %Y", "%m/%d/%Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(&text, f).ok())
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    let text = text.to_uppercase().replace('.', "");
    ["%I:%M %p", "%I:%M%p", "%I %p", "%I%p", "%H:%M"].iter().find_map(|f| NaiveTime::parse_from_str(&text, f).ok())
}

/// "15" (minutes), "14:30" (minutes:seconds) or "1:05:00".
fn parse_length(text: &str) -> Option<i64> {
    let parts: Vec<i64> = text.split(':').map(|p| p.trim().parse::<i64>().ok().filter(|n| *n >= 0)).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [m] => m * 60,
        [m, s] if *s < 60 => m * 60 + s,
        [h, m, s] if *m < 60 && *s < 60 => h * 3600 + m * 60 + s,
        _ => return None,
    };
    (secs <= 12 * 3600).then_some(secs * 1000)
}

fn compare(col: Col, a: &Row, b: &Row, names: &HashMap<PlayerId, String>) -> std::cmp::Ordering {
    let (x, y) = (&a.record, &b.record);
    let team = |g: &GameRecord, first: usize| {
        (first..first + 2).filter_map(|s| g.players[s]).filter_map(|id| names.get(&id)).map(|n| n.to_lowercase()).min()
    };
    match col {
        Col::Date => x.started_ms.cmp(&y.started_ms),
        // Time of day, whatever the date.
        Col::Started => fmt_date(x.started_ms, "%H%M%S").cmp(&fmt_date(y.started_ms, "%H%M%S")),
        Col::Length => length_ms(x).cmp(&length_ms(y)),
        Col::Round => x.round.cmp(&y.round),
        Col::Court => x.court.cmp(&y.court),
        Col::Players => players(x).cmp(&players(y)),
        Col::TeamA => team(x, 0).cmp(&team(y, 0)),
        Col::TeamB => team(x, 2).cmp(&team(y, 2)),
        Col::Remove => std::cmp::Ordering::Equal,
    }
}

const CSV_HEADER: [&str; 11] = [
    "Date",
    "Started",
    "Ended",
    "Length (minutes)",
    "Round",
    "Court",
    "Players",
    "Team A player 1",
    "Team A player 2",
    "Team B player 1",
    "Team B player 2",
];

fn csv_row(r: &Row, names: &HashMap<PlayerId, String>) -> Vec<String> {
    let g = &r.record;
    let mut row = vec![
        fmt_date(g.started_ms, "%Y-%m-%d"),
        fmt_date(g.started_ms, "%H:%M:%S"),
        fmt_date(g.ended_ms, "%H:%M:%S"),
        format!("{:.1}", length_ms(g) as f64 / 60_000.0),
        g.round.to_string(),
        (g.court + 1).to_string(),
        players(g).to_string(),
    ];
    row.extend(g.players.iter().map(|p| p.and_then(|id| names.get(&id).cloned()).unwrap_or_default()));
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_cells_are_read_forgivingly() {
        assert_eq!(parse_length("15"), Some(15 * 60_000));
        assert_eq!(parse_length("14:30"), Some(870_000));
        assert_eq!(parse_length("14:75"), None);
        assert_eq!(parse_time("7:30 pm"), NaiveTime::from_hms_opt(19, 30, 0));
        assert_eq!(parse_time("19:05"), NaiveTime::from_hms_opt(19, 5, 0));
        assert_eq!(parse_date("Nov 12, 2026"), NaiveDate::from_ymd_opt(2026, 11, 12));
        assert_eq!(parse_date("2026-11-12"), NaiveDate::from_ymd_opt(2026, 11, 12));

        let start = chrono::Local.with_ymd_and_hms(2026, 11, 12, 19, 30, 15).unwrap().timestamp_millis();
        let g = GameRecord { round: 2, court: 0, started_ms: start, ended_ms: start + 900_000, players: EMPTY_SLOTS };
        let moved = apply_text(Col::Started, &g, "8:00 PM").unwrap();
        assert_eq!(moved.started_ms - g.started_ms, 30 * 60_000, "the seconds stay put");
        assert_eq!(length_ms(&moved), 900_000, "moving the start keeps the length");
        let moved = apply_text(Col::Date, &g, "Nov 13, 2026").unwrap();
        assert_eq!(fmt_date(moved.started_ms, "%Y-%m-%d %H:%M:%S"), "2026-11-13 19:30:15");
        assert_eq!(apply_text(Col::Court, &g, "3").unwrap().court, 2);
        assert!(apply_text(Col::Court, &g, "0").is_none());
        assert_eq!(length_ms(&apply_text(Col::Length, &g, "12:00").unwrap()), 720_000);
    }
}
