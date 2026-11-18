//! The Analytics view: headline numbers across every session, turnout over
//! time, the most active players, and the roster's skill mix.

use super::*;
use crate::stats::Turnout;
use egui::Response;

/// Categorical series colors (validated pair: blue, then orange), stepped for
/// each mode's surface.
fn series(pal: &Palette, slot: usize) -> Color32 {
    match (slot, pal.dark) {
        (0, false) => theme::hex(0x2A78D6),
        (0, true) => theme::hex(0x3987E5),
        (_, false) => theme::hex(0xEB6834),
        (_, true) => theme::hex(0xD95926),
    }
}

/// How many sessions the turnout chart shows (the most recent).
const TURNOUT_SESSIONS: usize = 24;

impl DropInApp {
    pub(super) fn analytics_view(&mut self, ui: &mut Ui, pal: &Palette) {
        let stats = self.stats();
        let roster = self.state.players.len();
        let everyone = stats.players.values().filter(|p| !p.name.is_empty()).count().max(roster);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Analytics").font(semibold(26.0)).color(pal.fg1));
                ui.add_space(8.0);
                let since = stats.turnout.first().map_or_else(
                    || "No sessions yet".to_owned(),
                    |t| format!("Across {} sessions since {}", stats.turnout.len(), fmt_date(t.started_ms, "%B %-d, %Y")),
                );
                ui.label(RichText::new(since).font(regular(15.0)).color(pal.fg3));
            });
            ui.add_space(12.0);

            // Headline numbers.
            let sessions = stats.turnout.len();
            let per_session = if sessions > 0 { stats.games as f32 / sessions as f32 } else { 0.0 };
            let avg_game = if stats.games > 0 { stats.court_ms / stats.games as i64 } else { 0 };
            let busiest = stats.busiest();
            let tiles: [(&str, String, String); 6] = [
                ("Players", everyone.to_string(), format!("{roster} on the roster now")),
                (
                    "Sessions",
                    sessions.to_string(),
                    stats.turnout.first().map_or_else(|| "None yet".to_owned(), |t| format!("Since {}", fmt_date(t.started_ms, "%b %-d, %Y"))),
                ),
                ("Games played", stats.games.to_string(), format!("{per_session:.1} per session")),
                (
                    "Average turnout",
                    stats.average_turnout().map_or_else(|| "—".to_owned(), |a| format!("{a:.1}")),
                    busiest.map_or_else(|| "Players per session".to_owned(), |b| format!("Busiest: {} on {}", b.players(), fmt_date(b.started_ms, "%b %-d"))),
                ),
                (
                    "Came back",
                    stats.return_rate().map_or_else(|| "—".to_owned(), |r| format!("{:.0}%", r * 100.0)),
                    "Played 2 or more sessions".to_owned(),
                ),
                (
                    "Court time",
                    format!("{:.0}h", stats.court_ms as f32 / 3_600_000.0),
                    if avg_game > 0 { format!("Games average {} min", avg_game / 60_000) } else { "Time on court".to_owned() },
                ),
            ];
            let gap = GAP;
            let width = ui.available_width();
            let per_row = if width > 6.0 * 180.0 { 6 } else { 3 };
            let tile_w = ((width - gap * (per_row as f32 - 1.0)) / per_row as f32).floor();
            for chunk in tiles.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (label, value, note) in chunk {
                        stat_tile(ui, pal, tile_w, label, value, note);
                    }
                });
                ui.add_space(gap);
            }

            // Charts.
            // The charts take whatever height is left above the skill mix.
            let mix_h = 128.0;
            let chart_h = (ui.available_height() - mix_h - gap).clamp(320.0, 560.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let left = ((width - gap) * 0.6).floor();
                card(ui, pal, vec2(left, chart_h), |ui| turnout_chart(ui, pal, &stats.turnout));
                card(ui, pal, vec2(width - gap - left, chart_h), |ui| {
                    let top: Vec<(String, Skill, u32, u32)> = stats
                        .most_games(10)
                        .into_iter()
                        .map(|(id, s)| {
                            let skill = self.state.players.get(&id).map(|p| p.skill).or(s.skill).unwrap_or(Skill::Intermediate);
                            let name = self.state.players.get(&id).map_or_else(|| s.name.clone(), |p| p.name.clone());
                            (name, skill, s.games, s.sessions)
                        })
                        .collect();
                    most_games_chart(ui, pal, &top);
                });
            });
            ui.add_space(gap);
            let mut mix = [0u32; 4];
            for p in self.state.players.values() {
                mix[p.skill.rank() as usize] += 1;
            }
            card(ui, pal, vec2(width, mix_h), |ui| skill_mix(ui, pal, mix));
        });
    }
}

/// A plain white card filling `size`.
fn card(ui: &mut Ui, pal: &Palette, size: Vec2, add: impl FnOnce(&mut Ui)) {
    // The frame's margin and stroke sit outside the content.
    let inner = size - Vec2::splat(34.0);
    pane(pal).show(ui, |ui| {
        ui.set_min_size(inner);
        ui.set_max_size(inner);
        ui.vertical(|ui| {
            ui.set_min_size(inner);
            add(ui);
        });
    });
}

fn stat_tile(ui: &mut Ui, pal: &Palette, width: f32, label: &str, value: &str, note: &str) {
    pane(pal).show(ui, |ui| {
        ui.set_width(width - 34.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(label).font(semibold(14.0)).color(pal.fg2));
            ui.label(RichText::new(value).font(semibold(34.0)).color(pal.fg1));
            ui.add(egui::Label::new(RichText::new(note).font(regular(13.0)).color(pal.fg3)).truncate());
        });
    });
}

fn chart_title(ui: &mut Ui, pal: &Palette, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(17.0)).color(pal.fg1));
    ui.label(RichText::new(subtitle).font(regular(13.0)).color(pal.fg3));
}

/// A round number of steps for an axis running up to `max`.
fn nice_step(max: f32) -> f32 {
    let raw = (max / 4.0).max(1.0);
    let mag = 10f32.powf(raw.log10().floor());
    let n = raw / mag;
    let step = if n <= 1.0 { 1.0 } else if n <= 2.0 { 2.0 } else if n <= 5.0 { 5.0 } else { 10.0 };
    step * mag
}

fn empty_note(ui: &mut Ui, pal: &Palette, text: &str) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, regular(15.0), pal.fg3);
}

/// Players per session, split into returning players and first-timers.
fn turnout_chart(ui: &mut Ui, pal: &Palette, turnout: &[Turnout]) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| chart_title(ui, pal, "Turnout by session", "Players at each session, most recent on the right"));
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            legend_item(ui, pal, series(pal, 1), "First session");
            ui.add_space(14.0);
            legend_item(ui, pal, series(pal, 0), "Returning");
        });
    });
    ui.add_space(10.0);
    if turnout.is_empty() {
        empty_note(ui, pal, "Turnout shows up here after the first session.");
        return;
    }
    let shown = &turnout[turnout.len().saturating_sub(TURNOUT_SESSIONS)..];
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let max = shown.iter().map(|t| t.players()).max().unwrap_or(1).max(1) as f32;
    let step = nice_step(max);
    let top = (max / step).ceil() * step;
    let axis_w = 30.0;
    let plot = Rect::from_min_max(pos2(area.left() + axis_w, area.top() + 8.0), pos2(area.right(), area.bottom() - 24.0));
    let y_of = |v: f32| plot.bottom() - v / top * plot.height();
    let p = ui.painter();
    // Recessive grid and axis labels.
    let mut v = 0.0;
    while v <= top + 0.5 {
        let y = y_of(v);
        let stroke = if v == 0.0 { Stroke::new(1.0, pal.stroke1) } else { Stroke::new(1.0, pal.stroke2) };
        p.hline(plot.x_range(), y, stroke);
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{v:.0}"), regular(12.0), pal.fg3);
        v += step;
    }
    let slot = plot.width() / shown.len() as f32;
    let bar_w = (slot * 0.62).clamp(4.0, 34.0);
    // Date labels: as many as fit without touching.
    let label_every = ((64.0 / slot).ceil() as usize).max(1);
    let gap = 2.0; // surface gap between the stacked segments
    for (i, t) in shown.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let column = Rect::from_x_y_ranges(cx - slot / 2.0..=cx + slot / 2.0, plot.y_range());
        let resp = ui.interact(column, unique(("turnout", i)), Sense::hover());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(column, CornerRadius::same(4), pal.subtle_hover);
        }
        let x = cx - bar_w / 2.0..=cx + bar_w / 2.0;
        let returning_top = y_of(t.returning as f32);
        let total_top = y_of(t.players() as f32);
        // Rounded ends point away from the baseline; the segment that touches it stays square.
        let r = (bar_w / 2.0).min(4.0) as u8;
        if t.new > 0 {
            let seg = Rect::from_x_y_ranges(x.clone(), total_top..=returning_top - if t.returning > 0 { gap } else { 0.0 });
            p.rect_filled(seg, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, series(pal, 1));
        }
        if t.returning > 0 {
            let seg = Rect::from_x_y_ranges(x.clone(), returning_top..=plot.bottom());
            let corners = if t.new > 0 { CornerRadius::ZERO } else { CornerRadius { nw: r, ne: r, sw: 0, se: 0 } };
            p.rect_filled(seg, corners, series(pal, 0));
        }
        if i % label_every == (shown.len() - 1) % label_every {
            p.text(pos2(cx, plot.bottom() + 6.0), Align2::CENTER_TOP, fmt_date(t.started_ms, "%b %-d"), regular(12.0), pal.fg3);
        }
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(fmt_date(t.started_ms, "%A, %B %-d, %Y")).font(semibold(14.0)));
            ui.label(RichText::new(format!("{} players · {} games", t.players(), t.games)).font(regular(14.0)));
            tooltip_row(ui, pal, series(pal, 0), &format!("{} returning", t.returning));
            tooltip_row(ui, pal, series(pal, 1), &format!("{} at their first session", t.new));
        });
    }
}

/// The ten players with the most games: horizontal bars, values at the ends.
fn most_games_chart(ui: &mut Ui, pal: &Palette, top: &[(String, Skill, u32, u32)]) {
    chart_title(ui, pal, "Most games played", "All sessions");
    ui.add_space(10.0);
    if top.is_empty() {
        empty_note(ui, pal, "Nobody has played a game yet.");
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let row_h = (area.height() / 10.0).min(30.0);
    let name_w = (area.width() * 0.38).min(170.0);
    let value_w = 34.0;
    let max = top.iter().map(|t| t.2).max().unwrap_or(1).max(1) as f32;
    let track = (area.left() + name_w + 8.0)..=(area.right() - value_w);
    for (i, (name, skill, games, sessions)) in top.iter().enumerate() {
        let y = area.top() + row_h * i as f32;
        let row = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), row_h));
        let resp = ui.interact(row, unique(("most-games", i)), Sense::hover());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(row, CornerRadius::same(4), pal.subtle_hover);
        }
        let badge = Rect::from_min_size(pos2(row.left() + 2.0, row.center().y - 8.0), vec2(widgets::badge_width(16.0), 16.0));
        widgets::paint_skill_badge(p, pal, badge, *skill);
        let name_clip = Rect::from_x_y_ranges(badge.right() + 6.0..=area.left() + name_w, row.y_range());
        p.with_clip_rect(name_clip).text(pos2(name_clip.left(), row.center().y), Align2::LEFT_CENTER, name, regular(14.0), pal.fg1);
        let len = (*games as f32 / max) * (track.end() - track.start());
        let bar = Rect::from_x_y_ranges(*track.start()..=track.start() + len.max(2.0), row.center().y - row_h * 0.28..=row.center().y + row_h * 0.28);
        p.rect_filled(bar, CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }, series(pal, 0));
        p.text(pos2(bar.right() + 6.0, row.center().y), Align2::LEFT_CENTER, games.to_string(), semibold(13.0), pal.fg2);
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(name).font(semibold(14.0)));
            let per = if *sessions > 0 { *games as f32 / *sessions as f32 } else { 0.0 };
            ui.label(RichText::new(format!("{games} games in {sessions} sessions ({per:.1} a session)")).font(regular(14.0)));
        });
    }
}

/// How the current roster splits across skill levels.
fn skill_mix(ui: &mut Ui, pal: &Palette, mix: [u32; 4]) {
    chart_title(ui, pal, "Roster by skill level", "Everyone on the roster now");
    ui.add_space(10.0);
    let total: u32 = mix.iter().sum();
    if total == 0 {
        empty_note(ui, pal, "The roster is empty.");
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 14.0));
    let gap = 2.0;
    let present: Vec<(Skill, u32)> = Skill::ALL.into_iter().zip(mix).filter(|(_, n)| *n > 0).collect();
    let usable = bar.width() - gap * (present.len() as f32 - 1.0);
    let mut x = bar.left();
    let mut label_x = bar.left();
    for (k, (skill, n)) in present.iter().enumerate() {
        let w = usable * *n as f32 / total as f32;
        let seg = Rect::from_x_y_ranges(x..=x + w, bar.y_range());
        let first = k == 0;
        let last = k + 1 == present.len();
        let corners = CornerRadius { nw: if first { 4 } else { 0 }, sw: if first { 4 } else { 0 }, ne: if last { 4 } else { 0 }, se: if last { 4 } else { 0 } };
        let resp = ui.interact(seg.expand2(vec2(0.0, 6.0)), unique(("skill-mix", k)), Sense::hover());
        ui.painter().rect_filled(seg, corners, theme::skill_color(*skill));
        resp.on_hover_text(format!("{}: {n} of {total}", skill.label()));
        x += w + gap;
        // Labels in a row underneath, each with its badge, count and share.
        let p = ui.painter();
        let y = bar.bottom() + 20.0;
        let badge = Rect::from_min_size(pos2(label_x, y - 9.0), vec2(widgets::badge_width(18.0), 18.0));
        widgets::paint_skill_badge(p, pal, badge, *skill);
        let r = p.text(pos2(badge.right() + 8.0, y), Align2::LEFT_CENTER, skill.label(), regular(14.0), pal.fg1);
        let r = p.text(
            pos2(r.right() + 6.0, y),
            Align2::LEFT_CENTER,
            format!("{n} · {:.0}%", *n as f32 / total as f32 * 100.0),
            semibold(14.0),
            pal.fg2,
        );
        label_x = r.right() + 28.0;
    }
}

fn legend_item(ui: &mut Ui, pal: &Palette, color: Color32, label: &str) -> Response {
    // Right-to-left layout: the label is added first so it sits right of the swatch.
    let resp = ui.label(RichText::new(label).font(regular(13.0)).color(pal.fg2));
    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(3), color);
    resp
}

fn tooltip_row(ui: &mut Ui, pal: &Palette, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(2), color);
        ui.label(RichText::new(text).font(regular(14.0)).color(pal.fg1));
    });
}
