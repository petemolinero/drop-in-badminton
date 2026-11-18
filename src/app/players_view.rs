//! The Players view: every player who has ever been on the roster, one row
//! each, like a spreadsheet. Names and skill levels are edited in place;
//! the rest are lifetime numbers from the database.

use super::*;
use crate::stats::PlayerStats;
use crate::theme::lerp_color;
use egui::Response;

const ROW_H: f32 = 40.0;
const HEADER_H: f32 = 38.0;
const CELL_PAD: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) enum Col {
    #[default]
    Name,
    Skill,
    Status,
    From,
    FoundVia,
    AutoCheckIn,
    Sessions,
    Games,
    PerSession,
    CourtTime,
    Partner,
    First,
    Last,
}

impl Col {
    const ALL: [Col; 13] = [
        Col::Name,
        Col::Skill,
        Col::Status,
        Col::From,
        Col::FoundVia,
        Col::AutoCheckIn,
        Col::Sessions,
        Col::Games,
        Col::PerSession,
        Col::CourtTime,
        Col::Partner,
        Col::First,
        Col::Last,
    ];

    fn title(self) -> &'static str {
        match self {
            Col::Name => "Name",
            Col::Skill => "Skill level",
            Col::Status => "Status",
            Col::From => "From",
            Col::FoundVia => "Found us via",
            Col::AutoCheckIn => "Auto check-in",
            Col::Sessions => "Sessions",
            Col::Games => "Games",
            Col::PerSession => "Games / session",
            Col::CourtTime => "Court time",
            Col::Partner => "Usual partner",
            Col::First => "First session",
            Col::Last => "Latest session",
        }
    }

    /// Width at full size; the table shrinks every column evenly to fit.
    fn width(self) -> f32 {
        match self {
            Col::Name => 220.0,
            Col::Skill => 160.0,
            Col::Status => 124.0,
            Col::From => 150.0,
            Col::FoundVia => 150.0,
            Col::AutoCheckIn => 124.0,
            Col::Sessions => 96.0,
            Col::Games => 80.0,
            Col::PerSession => 130.0,
            Col::CourtTime => 106.0,
            Col::Partner => 180.0,
            Col::First => 126.0,
            Col::Last => 126.0,
        }
    }

    fn numeric(self) -> bool {
        matches!(self, Col::Sessions | Col::Games | Col::PerSession | Col::CourtTime)
    }

    /// Numbers and dates sort biggest/latest first on the first click.
    fn descending_first(self) -> bool {
        self.numeric() || matches!(self, Col::First | Col::Last)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Filter {
    #[default]
    All,
    Active,
    Archived,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    /// On the roster, so they can be checked in to a session.
    Active,
    Archived,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Active => "Active",
            Status::Archived => "Archived",
        }
    }
}

#[derive(Default)]
pub(super) struct PlayersView {
    search: String,
    filter: Filter,
    sort: Col,
    descending: bool,
    /// Row order from the last frame, kept while a name is being typed so the
    /// row doesn't jump away mid-word.
    order: Vec<PlayerId>,
    /// The cell being edited and what it held before, to put a name back if
    /// it's left empty.
    editing: Option<(PlayerId, Field, String)>,
    /// An archived player as they're being edited (saved when the box is left).
    archived_edit: Option<Player>,
    /// A player just added from here: focus their name, and drop them again
    /// if it's left empty.
    added: Option<PlayerId>,
    focus: Option<PlayerId>,
}

/// The free-text columns.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    Name,
    From,
    FoundVia,
}

impl Field {
    fn get(self, p: &Player) -> &str {
        match self {
            Field::Name => &p.name,
            Field::From => &p.from,
            Field::FoundVia => &p.found_via,
        }
    }

    fn set(self, p: &mut Player, value: String) {
        match self {
            Field::Name => p.name = value,
            Field::From => p.from = value,
            Field::FoundVia => p.found_via = value,
        }
    }
}

struct Row {
    id: PlayerId,
    name: String,
    skill: Skill,
    status: Status,
    stats: PlayerStats,
    /// The player as stored (for the editable columns).
    player: Player,
}

impl DropInApp {
    pub(super) fn players_view(&mut self, ui: &mut Ui, pal: &Palette) {
        let stats = self.stats();
        let rows = self.player_rows(&stats);
        let active = rows.iter().filter(|r| r.status == Status::Active).count();

        // Heading and controls.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Players").font(semibold(26.0)).color(pal.fg1));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Every player, past and present · {} players", rows.len()))
                    .font(regular(15.0))
                    .color(pal.fg3),
            );
        });
        ui.add_space(10.0);
        let mut add = false;
        let mut export = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.players_view.search)
                    .hint_text(format!("{}  Search players", icon::SEARCH))
                    .font(regular(15.0))
                    .margin(Margin::symmetric(10, 7))
                    .desired_width(280.0),
            );
            ui.add_space(12.0);
            for (filter, label, n) in [
                (Filter::All, "All", rows.len()),
                (Filter::Active, "Active", active),
                (Filter::Archived, "Archived", rows.len() - active),
            ] {
                if filter_chip(ui, pal, label, n, self.players_view.filter == filter).clicked() {
                    self.players_view.filter = filter;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                add = widgets::button(ui, pal, ButtonKind::Primary, Some(icon::PERSON_ADD), "Add player", 34.0).clicked();
                export = widgets::button(ui, pal, ButtonKind::Secondary, Some(icon::EXPORT), "Export CSV", 34.0)
                    .on_hover_text("Save the players shown below as a spreadsheet")
                    .clicked();
            });
        });
        ui.add_space(12.0);
        if add {
            let id = self.state.add_player("", Skill::Intermediate);
            self.dirty = true;
            let pv = &mut self.players_view;
            pv.added = Some(id);
            pv.focus = Some(id);
            pv.search.clear();
            if pv.filter == Filter::Archived {
                pv.filter = Filter::All;
            }
            // A blank name sorts first by name; show it at the top either way.
            pv.order.retain(|x| *x != id);
            pv.order.insert(0, id);
        }

        // Which rows, in which order.
        let pv = &self.players_view;
        let query = pv.search.trim().to_lowercase();
        let shown: HashMap<PlayerId, &Row> = rows
            .iter()
            .filter(|r| match pv.filter {
                Filter::All => true,
                Filter::Active => r.status == Status::Active,
                Filter::Archived => r.status == Status::Archived,
            })
            .filter(|r| {
                query.is_empty()
                    || pv.added == Some(r.id)
                    || [&r.name, &r.player.from, &r.player.found_via].iter().any(|s| s.to_lowercase().contains(&query))
            })
            .map(|r| (r.id, r))
            .collect();
        let order: Vec<PlayerId> = if pv.editing.is_some() || pv.focus.is_some() {
            // Keep rows where they are while a name is being typed.
            let mut order: Vec<PlayerId> = pv.order.iter().copied().filter(|id| shown.contains_key(id)).collect();
            let mut extra: Vec<PlayerId> = shown.keys().copied().filter(|id| !order.contains(id)).collect();
            extra.sort();
            order.extend(extra);
            order
        } else {
            let mut list: Vec<&Row> = shown.values().copied().collect();
            list.sort_by(|a, b| {
                let ord = compare(pv.sort, a, b);
                let ord = if pv.descending { ord.reverse() } else { ord };
                ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then(a.id.cmp(&b.id))
            });
            list.iter().map(|r| r.id).collect()
        };
        self.players_view.order = order.clone();
        let names: HashMap<PlayerId, String> = rows.iter().map(|r| (r.id, r.name.clone())).collect();
        if export {
            let lines: Vec<Vec<String>> = order.iter().map(|id| csv_row(shown[id], &names)).collect();
            let ctx = ui.ctx().clone();
            let file = format!("Players {}.csv", chrono::Local::now().format("%Y-%m-%d"));
            self.export_csv(&ctx, &file, &CSV_HEADER, &lines);
        }

        // The table: a card with a header row and a scrolling body.
        pane(pal).inner_margin(Margin::same(0)).show(ui, |ui| {
            let avail = ui.available_size();
            ui.set_min_size(avail);
            let full: f32 = Col::ALL.iter().map(|c| c.width()).sum();
            let scale = avail.x / full;
            let widths: Vec<f32> = Col::ALL.iter().map(|c| (c.width() * scale).floor()).collect();
            self.players_header(ui, pal, &widths);
            if order.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    let text = if rows.is_empty() { "No players yet. Add one to get started." } else { "Nobody matches." };
                    ui.label(RichText::new(text).font(regular(16.0)).color(pal.fg3));
                });
                return;
            }
            let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if let Some(i) = self.players_view.focus.and_then(|id| order.iter().position(|x| *x == id)) {
                let visible = ui.available_height();
                scroll = scroll.vertical_scroll_offset((i as f32 * ROW_H - visible / 3.0).max(0.0));
            }
            let out = scroll.show_rows(ui, ROW_H, order.len(), |ui, range| {
                for i in range {
                    let row = shown[&order[i]];
                    self.player_row(ui, pal, row, i, &widths, &names);
                }
            });
            widgets::fade_bottom(ui, &out, pal.bg1, 8);
        });
    }

    /// Everyone: the roster from the app (it may be ahead of the database),
    /// and archived players and lifetime numbers from the database.
    fn player_rows(&self, stats: &Stats) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .state
            .players
            .values()
            .map(|p| Row {
                id: p.id,
                name: p.name.clone(),
                skill: p.skill,
                status: Status::Active,
                stats: stats.players.get(&p.id).cloned().unwrap_or_default(),
                player: p.clone(),
            })
            .collect();
        for (id, s) in &stats.players {
            // Off the roster: archived, or just taken off and not saved yet.
            if !self.state.players.contains_key(id) && !s.name.is_empty() {
                let player = match &self.players_view.archived_edit {
                    Some(p) if p.id == *id => p.clone(),
                    _ => Player {
                        id: *id,
                        name: s.name.clone(),
                        skill: s.skill.unwrap_or(Skill::Intermediate),
                        lifetime_games: s.games,
                        found_via: s.found_via.clone(),
                        from: s.from.clone(),
                        auto_check_in: s.auto_check_in,
                    },
                };
                rows.push(Row {
                    id: *id,
                    name: player.name.clone(),
                    skill: player.skill,
                    status: Status::Archived,
                    stats: s.clone(),
                    player,
                });
            }
        }
        rows
    }

    fn players_header(&mut self, ui: &mut Ui, pal: &Palette, widths: &[f32]) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
        let p = ui.painter();
        p.rect_filled(bar, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, pal.bg2);
        p.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, pal.stroke1));
        let mut x = bar.left();
        for (col, w) in Col::ALL.into_iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, bar.top()), vec2(*w, HEADER_H));
            let resp = ui.interact(cell, unique(("pv-head", col)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let sorted = self.players_view.sort == col;
            let ink = if sorted { pal.fg1 } else if resp.hovered() { pal.fg2 } else { pal.fg3 };
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(cell.shrink(2.0), CornerRadius::same(4), pal.subtle_hover);
            }
            let text = p.layout_no_wrap(col.title().to_owned(), semibold(13.0), ink);
            let arrow = if self.players_view.descending { icon::DOWN } else { icon::UP };
            let arrow_w = if sorted { 18.0 } else { 0.0 };
            let max_w = (cell.width() - CELL_PAD * 2.0 - arrow_w).max(10.0);
            let text = if text.size().x > max_w { p.layout_no_wrap(col.title().to_owned(), semibold(12.0), ink) } else { text };
            let tx = if col.numeric() { cell.right() - CELL_PAD - text.size().x - arrow_w } else { cell.left() + CELL_PAD };
            let tw = text.size().x;
            p.with_clip_rect(cell.shrink2(vec2(CELL_PAD - 2.0, 0.0))).galley(pos2(tx, cell.center().y - text.size().y / 2.0), text, ink);
            if sorted {
                p.text(pos2(tx + tw + 4.0, cell.center().y + 1.0), Align2::LEFT_CENTER, arrow, icon_font(13.0), pal.brand_fg);
            }
            if x > bar.left() {
                p.vline(x, cell.y_range().shrink(10.0), Stroke::new(1.0, pal.stroke2));
            }
            if resp.clicked() {
                let pv = &mut self.players_view;
                if pv.sort == col {
                    pv.descending = !pv.descending;
                } else {
                    pv.sort = col;
                    pv.descending = col.descending_first();
                }
            }
            x += w;
        }
    }

    fn player_row(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, index: usize, widths: &[f32], names: &HashMap<PlayerId, String>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let hovered = ui.rect_contains_pointer(rect);
        let p = ui.painter();
        let fill = if hovered { pal.subtle_hover } else if index % 2 == 1 { pal.bg2.gamma_multiply(0.5) } else { Color32::TRANSPARENT };
        p.rect_filled(rect, CornerRadius::ZERO, fill);
        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, pal.stroke2));
        let archived = row.status == Status::Archived;
        let ink = if archived { pal.fg3 } else { pal.fg1 };
        let s = &row.stats;
        let mut x = rect.left();
        for (col, w) in Col::ALL.into_iter().zip(widths) {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, ROW_H));
            let text = |text: String, color: Color32| {
                let p = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
                let font = regular(15.0);
                if col.numeric() {
                    p.text(pos2(cell.right() - CELL_PAD, cell.center().y), Align2::RIGHT_CENTER, text, font, color);
                } else {
                    p.text(pos2(cell.left() + CELL_PAD, cell.center().y), Align2::LEFT_CENTER, text, font, color);
                }
            };
            let none = || "—".to_owned();
            match col {
                Col::Name => self.text_cell(ui, pal, row, cell, Field::Name),
                Col::Skill => self.skill_cell(ui, pal, row, cell),
                Col::Status => self.status_cell(ui, pal, row, cell),
                Col::From => self.text_cell(ui, pal, row, cell, Field::From),
                Col::FoundVia => self.text_cell(ui, pal, row, cell, Field::FoundVia),
                Col::AutoCheckIn => self.auto_cell(ui, pal, row, cell),
                Col::Sessions => text(if s.sessions > 0 { s.sessions.to_string() } else { none() }, ink),
                Col::Games => text(if s.games > 0 { s.games.to_string() } else { none() }, ink),
                Col::PerSession => text(if s.sessions > 0 { format!("{:.1}", s.games_per_session()) } else { none() }, ink),
                Col::CourtTime => text(if s.court_ms > 0 { short_duration(s.court_ms) } else { none() }, ink),
                Col::Partner => match s.partner.and_then(|(q, n)| names.get(&q).map(|name| (name, n))) {
                    Some((name, n)) => {
                        let p = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
                        let r = p.text(pos2(cell.left() + CELL_PAD, cell.center().y), Align2::LEFT_CENTER, name, regular(15.0), ink);
                        let games = if n == 1 { "1 game".to_owned() } else { format!("{n} games") };
                        p.text(pos2(r.right() + 6.0, cell.center().y + 1.0), Align2::LEFT_CENTER, games, regular(13.0), pal.fg3);
                    }
                    None => text(none(), pal.fg3),
                },
                Col::First => text(s.first_ms.map_or_else(none, |t| fmt_date(t, "%b %-d, %Y")), ink),
                Col::Last => text(s.last_ms.map_or_else(none, |t| fmt_date(t, "%b %-d, %Y")), ink),
            }
            if x > rect.left() {
                ui.painter().vline(x, rect.y_range(), Stroke::new(1.0, pal.stroke2));
            }
            x += w;
        }
    }

    /// A text column, edited in place like a spreadsheet cell.
    fn text_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect, field: Field) {
        let id = row.id;
        let mut buf = field.get(&row.player).to_owned();
        let edit = egui::TextEdit::singleline(&mut buf)
            .id(unique(("pv-text", id, field)))
            // The frame's margin is the text's padding; this centres it in the row.
            .frame(Frame::NONE.inner_margin(Margin { left: 6, right: 6, top: 4, bottom: 6 }))
            .font(regular(15.0))
            .text_color(if row.status == Status::Archived { pal.fg2 } else { pal.fg1 })
            .hint_text(if field == Field::Name { "Type a name" } else { "—" })
            .desired_width(f32::INFINITY);
        let area = Rect::from_center_size(cell.center(), vec2(cell.width() - 12.0, 30.0));
        let resp = ui.put(area, edit);
        if field == Field::Name && self.players_view.focus == Some(id) {
            resp.request_focus();
            self.players_view.focus = None;
        }
        let p = ui.painter();
        if resp.has_focus() {
            p.rect_stroke(area, CornerRadius::same(4), Stroke::new(2.0, pal.brand_fg), StrokeKind::Inside);
        } else if resp.hovered() {
            p.rect_stroke(area, CornerRadius::same(4), Stroke::new(1.0, pal.stroke1), StrokeKind::Inside);
        }
        if resp.gained_focus() {
            self.players_view.editing = Some((id, field, field.get(&row.player).to_owned()));
        }
        if resp.changed() {
            if row.status == Status::Archived {
                let mut edited = row.player.clone();
                field.set(&mut edited, buf.clone());
                self.players_view.archived_edit = Some(edited);
            } else if let Some(p) = self.state.players.get_mut(&id) {
                field.set(p, buf.clone());
                self.dirty = true;
            }
        }
        if resp.lost_focus() {
            self.finish_edit(id, field, &buf);
        }
    }

    /// Leaving a text box: tidy what was typed, put back a blanked name, or
    /// drop a player who was added but never named.
    fn finish_edit(&mut self, id: PlayerId, field: Field, typed: &str) {
        let pv = &mut self.players_view;
        let before = pv.editing.take().filter(|(x, f, _)| *x == id && *f == field).map(|(_, _, v)| v);
        let value = typed.trim().to_owned();
        let added = field == Field::Name && pv.added == Some(id);
        if field == Field::Name {
            pv.added = None;
        }
        if let Some(mut edited) = pv.archived_edit.take().filter(|p| p.id == id) {
            if field == Field::Name && value.is_empty() {
                return; // a name can't be blank: keep the stored one
            }
            field.set(&mut edited, value);
            if let Err(err) = self.store.update_archived_player(&edited) {
                eprintln!("drop-in-badminton: couldn't save {}: {err}", edited.name);
            }
            self.save_gen += 1;
            return;
        }
        let Some(p) = self.state.players.get_mut(&id) else { return };
        if field != Field::Name || !value.is_empty() {
            field.set(p, value);
        } else if added {
            self.state.players.remove(&id);
        } else if let Some(before) = before {
            p.name = before;
        }
        self.dirty = true;
    }

    fn skill_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect) {
        let compact = cell.width() < 150.0;
        let id = row.id;
        let resp = ui
            .scope_builder(egui::UiBuilder::new().max_rect(cell.shrink2(vec2(CELL_PAD - 4.0, 5.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                widgets::skill_pill(ui, pal, row.skill, false, compact)
            })
            .inner;
        let mut chosen = None;
        egui::Popup::menu(&resp).id(unique(("pv-skill", id))).show(|ui| {
            ui.set_min_width(190.0);
            for skill in Skill::ALL {
                if widgets::skill_option(ui, pal, skill, skill == row.skill, 190.0).clicked() {
                    chosen = Some(skill);
                    ui.close();
                }
            }
        });
        let Some(skill) = chosen.filter(|s| *s != row.skill) else { return };
        if row.status == Status::Archived {
            let edited = Player { skill, ..row.player.clone() };
            if let Err(err) = self.store.update_archived_player(&edited) {
                eprintln!("drop-in-badminton: couldn't change level: {err}");
            }
            self.save_gen += 1;
        } else if let Some(p) = self.state.players.get_mut(&id) {
            p.skill = skill;
            self.dirty = true;
        }
    }

    /// Regulars: checked in automatically when a new session starts.
    fn auto_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect) {
        let mut on = row.player.auto_check_in;
        let area = Rect::from_min_size(pos2(cell.left() + CELL_PAD, cell.center().y - 11.0), vec2(44.0, 22.0));
        let resp = ui.put(area, |ui: &mut Ui| widgets::switch(ui, pal, &mut on));
        let resp = resp.on_hover_text("Check them in at the start of every new session");
        if !resp.changed() {
            return;
        }
        if row.status == Status::Archived {
            let edited = Player { auto_check_in: on, ..row.player.clone() };
            if let Err(err) = self.store.update_archived_player(&edited) {
                eprintln!("drop-in-badminton: couldn't change auto check-in: {err}");
            }
            self.save_gen += 1;
        } else if let Some(p) = self.state.players.get_mut(&row.id) {
            p.auto_check_in = on;
            self.dirty = true;
        }
    }

    fn status_cell(&mut self, ui: &mut Ui, pal: &Palette, row: &Row, cell: Rect) {
        let (bg, fg, glyph) = match row.status {
            Status::Active => (pal.success_subtle, pal.success_fg, icon::CHECK_CIRCLE),
            Status::Archived => (pal.bg2, pal.fg3, icon::ARCHIVE),
        };
        let galley = ui.painter().layout_no_wrap(row.status.label().to_owned(), semibold(13.0), fg);
        let w = (galley.size().x + 54.0).min(cell.width() - CELL_PAD * 2.0 + 8.0);
        let pill = Rect::from_min_size(pos2(cell.left() + CELL_PAD - 4.0, cell.center().y - 13.0), vec2(w, 26.0));
        let resp = ui
            .interact(pill, unique(("pv-status", row.id)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Click to change");
        let p = ui.painter().with_clip_rect(cell);
        let fill = if resp.hovered() { lerp_color(bg, fg, 0.12) } else { bg };
        p.rect_filled(pill, CornerRadius::same(13), fill);
        p.text(pos2(pill.left() + 15.0, pill.center().y + 1.0), Align2::CENTER_CENTER, glyph, icon_font(14.0), fg);
        p.galley(pos2(pill.left() + 26.0, pill.center().y - galley.size().y / 2.0 - 1.0), galley, fg);
        p.text(pos2(pill.right() - 12.0, pill.center().y + 1.0), Align2::CENTER_CENTER, icon::CHEVRON_DOWN, icon_font(11.0), fg);

        let ctx = ui.ctx().clone();
        let mut action = None;
        egui::Popup::menu(&resp).id(unique(("pv-status-menu", row.id))).show(|ui| {
            ui.set_min_width(200.0);
            ui.spacing_mut().button_padding = vec2(10.0, 7.0);
            let item = |ui: &mut Ui, glyph: &str, label: &str| ui.button(RichText::new(format!("{glyph}  {label}")).font(regular(15.0))).clicked();
            match row.status {
                Status::Archived => {
                    if item(ui, icon::UNDO, "Make active again") {
                        action = Some("restore");
                    }
                }
                Status::Active => {
                    if item(ui, icon::ARCHIVE, "Archive (keeps their history)") {
                        action = Some("archive");
                    }
                }
            }
        });
        let (id, name) = (row.id, row.name.clone());
        let now = self.state.now();
        match action {
            Some("restore") => {
                let player = row.player.clone();
                self.act(&ctx, format!("{name} is active again"), |s| {
                    s.players.insert(id, player);
                });
            }
            Some("archive") => self.act(&ctx, format!("{name} archived — their history is kept"), |s| s.delete_player(id, now)),
            _ => {}
        }
    }
}

const CSV_HEADER: [&str; 15] = [
    "Name",
    "Skill level",
    "Status",
    "From",
    "Found us via",
    "Auto check-in",
    "Sessions",
    "Games",
    "Games per session",
    "Court time (minutes)",
    "Usual partner",
    "Games with partner",
    "First session",
    "Latest session",
    "Player ID",
];

/// One player as a spreadsheet row (blank, not "—", where there's nothing).
fn csv_row(r: &Row, names: &HashMap<PlayerId, String>) -> Vec<String> {
    let s = &r.stats;
    let count = |n: u32| if n > 0 { n.to_string() } else { String::new() };
    let date = |t: Option<i64>| t.map_or_else(String::new, |t| fmt_date(t, "%Y-%m-%d"));
    let partner = s.partner.and_then(|(q, n)| names.get(&q).map(|name| (name.clone(), n.to_string())));
    let (partner, together) = partner.unwrap_or_default();
    vec![
        r.name.clone(),
        r.skill.label().to_owned(),
        r.status.label().to_owned(),
        r.player.from.clone(),
        r.player.found_via.clone(),
        if r.player.auto_check_in { "Yes" } else { "No" }.to_owned(),
        count(s.sessions),
        count(s.games),
        if s.sessions > 0 { format!("{:.1}", s.games_per_session()) } else { String::new() },
        if s.court_ms > 0 { (s.court_ms / 60_000).to_string() } else { String::new() },
        partner,
        together,
        date(s.first_ms),
        date(s.last_ms),
        r.id.to_string(),
    ]
}

fn compare(col: Col, a: &Row, b: &Row) -> std::cmp::Ordering {
    let (x, y) = (&a.stats, &b.stats);
    match col {
        Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        Col::Skill => a.skill.rank().cmp(&b.skill.rank()),
        Col::Status => (a.status as u8).cmp(&(b.status as u8)),
        Col::From => a.player.from.to_lowercase().cmp(&b.player.from.to_lowercase()),
        Col::FoundVia => a.player.found_via.to_lowercase().cmp(&b.player.found_via.to_lowercase()),
        Col::AutoCheckIn => a.player.auto_check_in.cmp(&b.player.auto_check_in),
        Col::Sessions => x.sessions.cmp(&y.sessions),
        Col::Games => x.games.cmp(&y.games),
        Col::PerSession => x.games_per_session().total_cmp(&y.games_per_session()),
        Col::CourtTime => x.court_ms.cmp(&y.court_ms),
        Col::Partner => x.partner.map(|p| p.1).cmp(&y.partner.map(|p| p.1)),
        Col::First => x.first_ms.cmp(&y.first_ms),
        Col::Last => x.last_ms.cmp(&y.last_ms),
    }
}

/// A filter choice with its count, like the dashboard's side tabs.
fn filter_chip(ui: &mut Ui, pal: &Palette, label: &str, count: usize, selected: bool) -> Response {
    let text = ui.painter().layout_no_wrap(label.to_owned(), if selected { semibold(14.0) } else { regular(14.0) }, pal.fg1);
    let count_text = count.to_string();
    let count_w = ui.painter().layout_no_wrap(count_text.clone(), semibold(12.0), pal.fg1).size().x + 12.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(text.size().x + count_w + 30.0, 32.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let p = ui.painter();
    let (fill, stroke, ink) = if selected {
        (pal.brand_subtle, Stroke::new(1.5, pal.brand_fg), pal.on_brand_subtle)
    } else if resp.hovered() {
        (pal.bg1_hover, Stroke::new(1.0, pal.stroke1), pal.fg1)
    } else {
        (pal.bg1, Stroke::new(1.0, pal.stroke1), pal.fg2)
    };
    p.rect(rect, CornerRadius::same(16), fill, stroke, StrokeKind::Inside);
    p.galley_with_override_text_color(pos2(rect.left() + 12.0, rect.center().y - text.size().y / 2.0 - 1.0), text, ink);
    let badge = Rect::from_min_size(pos2(rect.right() - 10.0 - count_w, rect.center().y - 10.0), vec2(count_w, 20.0));
    p.rect_filled(badge, CornerRadius::same(10), if selected { pal.bg1 } else { pal.subtle_selected });
    p.text(badge.center() - vec2(0.0, 1.0), Align2::CENTER_CENTER, count_text, semibold(12.0), ink);
    resp
}
