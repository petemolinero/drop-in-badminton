//! The Analytics view: headline numbers across every session, turnout over
//! time, the most active players, and the roster's skill mix.
//!
//! It sits on the same canvas and heading as the other pages; the blues live
//! in the cards: gradient icon tiles and gradient bars, painted with small
//! generated gradient textures so the fills keep their rounded corners.

use super::*;
use crate::stats::Turnout;
use egui::epaint::RectShape;
use egui::{Shadow, TextureId};

/// How many sessions the turnout chart shows (the most recent).
const TURNOUT_SESSIONS: usize = 24;
const GUTTER: f32 = 16.0;
const CARD_PAD: i8 = 22;
/// The same corners and shadow as every other card in the app.
const CARD_RADIUS: u8 = 8;

/// The page's colors: the app's neutrals, plus a family of blues.
struct Look {
    dark: bool,
    card: Color32,
    card_stroke: Color32,
    shadow: Shadow,
    ink: Color32,
    ink2: Color32,
    ink3: Color32,
    grid: Color32,
    track: Color32,
    hover: Color32,
    /// Returning players: a gradient from `blue_top` down to `blue`.
    blue: Color32,
    blue_top: Color32,
    /// First-timers: a pale tint of the same blue.
    sky: Color32,
    accent: Color32,
}

impl Look {
    fn new(pal: &Palette) -> Look {
        let common = |grid, track, hover, sky, accent, shadow| Look {
            dark: pal.dark,
            card: pal.bg1,
            card_stroke: pal.card_stroke,
            shadow,
            ink: pal.fg1,
            ink2: pal.fg2,
            ink3: pal.fg3,
            grid,
            track,
            hover,
            blue: if pal.dark { theme::hex(0x3478E4) } else { theme::hex(0x3A80E6) },
            blue_top: theme::hex(0x74B8FA),
            sky,
            accent,
        };
        if pal.dark {
            common(
                theme::hex(0x3A3A3A),
                theme::hex(0x363636),
                pal.subtle_hover,
                theme::hex(0x4A7FC4),
                theme::hex(0x7DBBFF),
                pal.shadow(4),
            )
        } else {
            common(
                theme::hex(0xE8EDF4),
                theme::hex(0xEBF2FC),
                theme::hex(0xF3F7FD),
                theme::hex(0xB4D5FB),
                theme::hex(0x1F63D6),
                pal.shadow(4),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Gradient textures

fn mix(a: Color32, b: Color32, t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| x as f32 + (y as f32 - x as f32) * t;
    [l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b())]
}

fn to_color(c: [f32; 3]) -> Color32 {
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// A two-color gradient strip, vertical (top to bottom) or horizontal (left
/// to right), made once and kept.
fn ramp(ctx: &egui::Context, name: &str, from: Color32, to: Color32, vertical: bool) -> TextureId {
    const N: usize = 128;
    let id = unique(("analytics-ramp", name));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex.id();
    }
    let pixels = (0..N).map(|i| to_color(mix(from, to, i as f32 / (N - 1) as f32))).collect();
    let size = if vertical { [1, N] } else { [N, 1] };
    let tex = ctx.load_texture(name, egui::ColorImage::new(size, pixels), egui::TextureOptions::LINEAR);
    let tid = tex.id();
    ctx.data_mut(|d| d.insert_temp(id, tex));
    tid
}

/// Paint `rect` with part of a gradient texture, keeping rounded corners.
fn paint_gradient(p: &egui::Painter, rect: Rect, radius: impl Into<CornerRadius>, tex: TextureId, uv: Rect) {
    p.add(RectShape::filled(rect, radius, Color32::WHITE).with_texture(tex, uv));
}

const FULL_UV: Rect = Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 1.0, y: 1.0 } };

/// The diagonal-ish blue of the icon tiles and rank discs.
fn tile_ramp(ctx: &egui::Context, dark: bool) -> TextureId {
    if dark {
        ramp(ctx, "tile-dark", theme::hex(0x5FA8F8), theme::hex(0x2563D0), true)
    } else {
        ramp(ctx, "tile-light", theme::hex(0x5EA8F8), theme::hex(0x1F5FD0), true)
    }
}

/// Single-line text cut short with "…" to fit `width`.
fn fitted(p: &egui::Painter, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    p.layout_job(job)
}

// ---------------------------------------------------------------------------
// The page

impl DropInApp {
    pub(super) fn analytics_view(&mut self, ui: &mut Ui, pal: &Palette) {
        let look = Look::new(pal);
        let stats = self.stats();
        let roster = self.state.players.len();
        let everyone = stats.players.values().filter(|p| !p.name.is_empty()).count().max(roster);
        let sessions = stats.turnout.len();
        let per_session = if sessions > 0 { stats.games as f32 / sessions as f32 } else { 0.0 };
        let busiest = stats.busiest();

        let tiles: [(&str, &str, String, String); 4] = [
            (icon::GROUP_F, "Players", everyone.to_string(), format!("{roster} on the roster now")),
            (
                icon::CHART,
                "Average turnout",
                stats.average_turnout().map_or_else(|| "—".to_owned(), |a| format!("{a:.1}")),
                busiest.map_or_else(|| "Players a session".to_owned(), |b| format!("Busiest: {} on {}", b.players(), fmt_date(b.started_ms, "%b %-d"))),
            ),
            (
                icon::HISTORY,
                "Came back",
                stats.return_rate().map_or_else(|| "—".to_owned(), |r| format!("{:.0}%", r * 100.0)),
                "Played 2+ sessions".to_owned(),
            ),
            (
                icon::SHUTTLE_F,
                "Games played",
                stats.games.to_string(),
                if stats.court_ms > 0 {
                    format!("{per_session:.0} a session · {:.0}h on court", stats.court_ms as f32 / 3_600_000.0)
                } else {
                    format!("{per_session:.0} a session")
                },
            ),
        ];

        let top: Vec<(String, Skill, u32, u32)> = stats
            .most_games(10)
            .into_iter()
            .map(|(id, s)| {
                let skill = self.state.players.get(&id).map(|p| p.skill).or(s.skill).unwrap_or(Skill::Beginner2);
                let name = self.state.players.get(&id).map_or_else(|| s.name.clone(), |p| p.name.clone());
                (name, skill, s.games, s.sessions)
            })
            .collect();
        let mut mix = [0u32; 4];
        for p in self.state.players.values() {
            mix[p.skill.rank() as usize] += 1;
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // The same heading as the other pages.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Analytics").font(semibold(26.0)).color(pal.fg1));
                ui.add_space(8.0);
                let since = stats.turnout.first().map_or_else(
                    || "Numbers show up here after the first session".to_owned(),
                    |t| format!("Across {sessions} {} since {}", if sessions == 1 { "session" } else { "sessions" }, fmt_date(t.started_ms, "%B %-d, %Y")),
                );
                ui.label(RichText::new(since).font(regular(15.0)).color(pal.fg3));
            });
            ui.add_space(6.0);

            ui.spacing_mut().item_spacing = vec2(GUTTER, GUTTER);
            // Leave room for the cards' shadows at the edges.
            let width = ui.available_width() - 2.0;
            let tile_w = ((width - GUTTER * 3.0) / 4.0).floor();
            ui.horizontal(|ui| {
                for (glyph, label, value, note) in &tiles {
                    card(ui, &look, vec2(tile_w, 116.0), |ui| stat_tile(ui, &look, glyph, label, value, note));
                }
            });

            // The charts share whatever height is left (scrolling below a minimum).
            let row_h = (ui.available_height() - 8.0).max(500.0);
            let mix_h = 170.0;
            let left = ((width - GUTTER) * 0.6).floor();
            let right = width - GUTTER - left;
            ui.horizontal_top(|ui| {
                card(ui, &look, vec2(left, row_h), |ui| turnout_chart(ui, &look, &stats.turnout, stats.average_turnout()));
                ui.vertical(|ui| {
                    card(ui, &look, vec2(right, row_h - GUTTER - mix_h), |ui| most_games_chart(ui, pal, &look, &top));
                    card(ui, &look, vec2(right, mix_h), |ui| skill_mix(ui, &look, mix));
                });
            });
        });
    }
}

/// A white card filling `size`.
fn card(ui: &mut Ui, look: &Look, size: Vec2, add: impl FnOnce(&mut Ui)) {
    // The frame's margin and stroke sit outside the content.
    let inner = size - Vec2::splat(2.0 * CARD_PAD as f32 + 2.0);
    Frame::NONE
        .fill(look.card)
        .stroke(Stroke::new(1.0, look.card_stroke))
        .corner_radius(CARD_RADIUS)
        .inner_margin(Margin::same(CARD_PAD))
        .shadow(look.shadow)
        .show(ui, |ui| {
            ui.set_min_size(inner);
            ui.set_max_size(inner);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
                ui.set_min_size(inner);
                add(ui);
            });
        });
}

/// A headline number: a blue gradient icon tile, then the label, value and note.
fn stat_tile(ui: &mut Ui, look: &Look, glyph: &str, label: &str, value: &str, note: &str) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let p = ui.painter();
    let tile = Rect::from_center_size(pos2(area.left() + 22.0, area.center().y), Vec2::splat(44.0));
    paint_gradient(p, tile, 12, tile_ramp(ui.ctx(), look.dark), FULL_UV);
    p.text(tile.center(), Align2::CENTER_CENTER, glyph, icon_font(22.0), Color32::WHITE);
    let x = tile.right() + 16.0;
    let w = area.right() - x;
    let top = area.center().y - 37.0;
    p.galley(pos2(x, top), fitted(p, label, regular(14.0), look.ink2, w), look.ink2);
    p.galley(pos2(x - 1.0, top + 19.0), fitted(p, value, semibold(30.0), look.ink, w), look.ink);
    p.galley(pos2(x, top + 59.0), fitted(p, note, regular(13.0), look.ink3, w), look.ink3);
}

fn chart_title(ui: &mut Ui, look: &Look, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(18.0)).color(look.ink));
    ui.label(RichText::new(subtitle).font(regular(13.5)).color(look.ink3));
}

/// A round number of steps for an axis running up to `max`.
fn nice_step(max: f32) -> f32 {
    let raw = (max / 4.0).max(1.0);
    let mag = 10f32.powf(raw.log10().floor());
    let n = raw / mag;
    let step = if n <= 1.0 { 1.0 } else if n <= 2.0 { 2.0 } else if n <= 5.0 { 5.0 } else { 10.0 };
    step * mag
}

fn empty_note(ui: &mut Ui, look: &Look, text: &str) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, regular(15.0), look.ink3);
}

/// Players per session, split into returning players and first-timers.
fn turnout_chart(ui: &mut Ui, look: &Look, turnout: &[Turnout], average: Option<f32>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| chart_title(ui, look, "Turnout", "Players at each session, most recent on the right"));
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.add_space(2.0);
            legend_item(ui, look, look.sky, "First session");
            ui.add_space(16.0);
            legend_item(ui, look, look.blue, "Returning");
        });
    });
    ui.add_space(18.0);
    if turnout.is_empty() {
        empty_note(ui, look, "Turnout shows up here after the first session.");
        return;
    }
    let shown = &turnout[turnout.len().saturating_sub(TURNOUT_SESSIONS)..];
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let max = shown.iter().map(|t| t.players()).max().unwrap_or(1).max(1) as f32;
    let step = nice_step(max);
    let top = ((max * 1.04) / step).ceil() * step;
    let axis_w = 34.0;
    // A gutter on the right for the average's label, so it never covers a bar.
    let average = average.filter(|_| shown.len() > 1);
    let gutter = if average.is_some() { 92.0 } else { 0.0 };
    let plot = Rect::from_min_max(pos2(area.left() + axis_w, area.top() + 6.0), pos2(area.right() - gutter, area.bottom() - 28.0));
    let y_of = |v: f32| plot.bottom() - v / top * plot.height();
    let p = ui.painter();
    // Recessive grid and axis labels.
    let mut v = 0.0;
    while v <= top + 0.5 {
        let y = y_of(v);
        if v == 0.0 {
            p.hline(plot.x_range(), y, Stroke::new(1.0, look.grid.gamma_multiply(1.6)));
        } else {
            p.add(egui::Shape::dashed_line(&[pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, look.grid), 4.0, 4.0));
        }
        p.text(pos2(plot.left() - 12.0, y), Align2::RIGHT_CENTER, format!("{v:.0}"), regular(12.5), look.ink3);
        v += step;
    }
    let slot = plot.width() / shown.len() as f32;
    let bar_w = (slot * 0.42).clamp(6.0, 56.0);
    // Date labels: as many as fit without touching. Two sessions on one day get a number.
    let label_every = ((70.0 / slot).ceil() as usize).max(1);
    let fill = ramp(ui.ctx(), if look.dark { "bar-v-dark" } else { "bar-v-light" }, look.blue_top, look.blue, true);
    let gap = 3.0; // surface gap between the stacked segments
    let mut previous_day = String::new();
    let mut same_day = 1;
    for (i, t) in shown.iter().enumerate() {
        let day = fmt_date(t.started_ms, "%b %-d");
        same_day = if day == previous_day { same_day + 1 } else { 1 };
        previous_day = day.clone();
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let column = Rect::from_x_y_ranges(cx - slot / 2.0 + 3.0..=cx + slot / 2.0 - 3.0, plot.top() - 4.0..=plot.bottom());
        let resp = ui.interact(column, unique(("turnout", i)), Sense::hover());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(column, CornerRadius::same(10), look.hover);
        }
        let x = cx - bar_w / 2.0..=cx + bar_w / 2.0;
        let returning_top = y_of(t.returning as f32);
        let total_top = y_of(t.players() as f32);
        // Rounded ends point away from the baseline; the segment that touches it stays square.
        let r = (bar_w / 2.0).min(8.0) as u8;
        let round_top = CornerRadius { nw: r, ne: r, sw: 0, se: 0 };
        if t.new > 0 {
            let seg = Rect::from_x_y_ranges(x.clone(), total_top..=returning_top - if t.returning > 0 { gap } else { 0.0 });
            let corners = if t.returning > 0 { round_top } else { round_top };
            p.rect_filled(seg, corners, look.sky);
        }
        if t.returning > 0 {
            let seg = Rect::from_x_y_ranges(x.clone(), returning_top..=plot.bottom());
            let corners = if t.new > 0 { CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 } } else { round_top };
            // The gradient runs over the whole plot height, so every bar shares one ramp.
            let uv = Rect::from_min_max(pos2(0.0, (seg.top() - plot.top()) / plot.height()), pos2(1.0, 1.0));
            paint_gradient(p, seg, corners, fill, uv);
        }
        p.text(pos2(cx, total_top - 7.0), Align2::CENTER_BOTTOM, t.players().to_string(), semibold(13.0), look.ink2);
        if i % label_every == (shown.len() - 1) % label_every {
            let label = if same_day > 1 { format!("{day} · {same_day}") } else { day };
            p.text(pos2(cx, plot.bottom() + 10.0), Align2::CENTER_TOP, label, regular(12.5), look.ink3);
        }
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(fmt_date(t.started_ms, "%A, %B %-d, %Y")).font(semibold(14.0)));
            ui.label(RichText::new(format!("{} players · {} games", t.players(), t.games)).font(regular(14.0)));
            tooltip_row(ui, look.blue, &format!("{} returning", t.returning));
            tooltip_row(ui, look.sky, &format!("{} at their first session", t.new));
        });
    }
    // The average, as a dashed reference line with its value.
    if let Some(avg) = average {
        let y = y_of(avg);
        let p = ui.painter();
        let text = p.layout_no_wrap(format!("Average {avg:.1}"), semibold(12.0), look.accent);
        let pill = Rect::from_min_size(pos2(plot.right() + 6.0, y - 11.0), vec2(text.size().x + 16.0, 22.0));
        p.add(egui::Shape::dashed_line(&[pos2(plot.left(), y), pos2(pill.left(), y)], Stroke::new(1.2, look.accent.gamma_multiply(0.5)), 5.0, 5.0));
        p.rect(pill, CornerRadius::same(11), look.card, Stroke::new(1.0, look.accent.gamma_multiply(0.5)), StrokeKind::Inside);
        p.galley(pos2(pill.left() + 8.0, pill.center().y - text.size().y / 2.0), text, look.accent);
    }
}

/// The ten players with the most games, as a leaderboard.
fn most_games_chart(ui: &mut Ui, pal: &Palette, look: &Look, top: &[(String, Skill, u32, u32)]) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| chart_title(ui, look, "Most games played", "Across every session"));
    });
    ui.add_space(14.0);
    if top.is_empty() {
        empty_note(ui, look, "Nobody has played a game yet.");
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    // As many rows as fit comfortably, up to ten.
    let rows = ((area.height() / 30.0).floor() as usize).clamp(1, top.len());
    let top = &top[..rows];
    let row_h = (area.height() / rows.max(5) as f32).min(42.0);
    let name_w = (area.width() * 0.5).min(220.0);
    let value_w = 40.0;
    let max = top.iter().map(|t| t.2).max().unwrap_or(1).max(1) as f32;
    let track = (area.left() + name_w + 12.0)..=(area.right() - value_w);
    let bar_fill = ramp(ui.ctx(), if look.dark { "bar-h-dark" } else { "bar-h-light" }, look.blue_top, look.blue, false);
    let medal = tile_ramp(ui.ctx(), look.dark);
    for (i, (name, skill, games, sessions)) in top.iter().enumerate() {
        let y = area.top() + row_h * i as f32;
        let row = Rect::from_min_size(pos2(area.left() - 8.0, y), vec2(area.width() + 16.0, row_h));
        let resp = ui.interact(row, unique(("most-games", i)), Sense::hover());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(row.shrink2(vec2(0.0, 1.0)), CornerRadius::same(8), look.hover);
        }
        let cy = row.center().y;
        // Rank: the top three in a blue disc.
        let rank = Rect::from_center_size(pos2(area.left() + 11.0, cy), Vec2::splat(22.0));
        if i < 3 {
            paint_gradient(p, rank, 11, medal, FULL_UV);
            p.text(rank.center(), Align2::CENTER_CENTER, (i + 1).to_string(), semibold(12.0), Color32::WHITE);
        } else {
            p.text(rank.center(), Align2::CENTER_CENTER, (i + 1).to_string(), semibold(13.0), look.ink3);
        }
        let badge = Rect::from_min_size(pos2(rank.right() + 12.0, cy - 8.0), vec2(widgets::badge_width(16.0), 16.0));
        widgets::paint_skill_badge(p, pal, badge, *skill);
        let name_clip = Rect::from_x_y_ranges(badge.right() + 8.0..=area.left() + name_w, row.y_range());
        let galley = fitted(p, name, regular(14.5), look.ink, name_clip.width());
        p.galley(pos2(name_clip.left(), cy - galley.size().y / 2.0), galley, look.ink);
        // A slim rounded bar on a pale track.
        let bar_h = 6.0;
        let full = Rect::from_x_y_ranges(track.clone(), cy - bar_h / 2.0..=cy + bar_h / 2.0);
        p.rect_filled(full, CornerRadius::same(3), look.track);
        let frac = *games as f32 / max;
        let bar = Rect::from_x_y_ranges(*track.start()..=track.start() + (frac * full.width()).max(bar_h), full.y_range());
        paint_gradient(p, bar, 3, bar_fill, Rect::from_min_max(pos2(0.0, 0.0), pos2(frac.max(0.05), 1.0)));
        p.text(pos2(area.right(), cy), Align2::RIGHT_CENTER, games.to_string(), semibold(14.0), look.ink);
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(name).font(semibold(14.0)));
            let per = if *sessions > 0 { *games as f32 / *sessions as f32 } else { 0.0 };
            ui.label(RichText::new(format!("{games} games in {sessions} sessions ({per:.1} a session)")).font(regular(14.0)));
        });
    }
}

/// How the current roster splits across skill levels.
fn skill_mix(ui: &mut Ui, look: &Look, mix: [u32; 4]) {
    let total: u32 = mix.iter().sum();
    let subtitle = format!("{total} {} on the roster now", if total == 1 { "player" } else { "players" });
    chart_title(ui, look, "Roster by level", &subtitle);
    ui.add_space(16.0);
    if total == 0 {
        empty_note(ui, look, "The roster is empty.");
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let tone = |skill: Skill| theme::lerp_color(theme::skill_color(skill), look.card, if look.dark { 0.25 } else { 0.32 });
    // A strip of rounded segments, one per level present.
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 10.0));
    let gap = 4.0;
    let present: Vec<(Skill, u32)> = Skill::ALL.into_iter().zip(mix).filter(|(_, n)| *n > 0).collect();
    let usable = bar.width() - gap * (present.len() as f32 - 1.0);
    let mut x = bar.left();
    for (k, (skill, n)) in present.iter().enumerate() {
        let w = (usable * *n as f32 / total as f32).max(10.0);
        let seg = Rect::from_x_y_ranges(x..=x + w, bar.y_range());
        let resp = ui.interact(seg.expand2(vec2(0.0, 6.0)), unique(("skill-mix", k)), Sense::hover());
        ui.painter().rect_filled(seg, CornerRadius::same(5), tone(*skill));
        resp.on_hover_text(format!("{}: {n} of {total}", skill.label()));
        x += w + gap;
    }
    // Underneath, a column per level: its name, then the count and share.
    let col_w = area.width() / 4.0;
    let p = ui.painter();
    for (k, (skill, n)) in Skill::ALL.into_iter().zip(mix).enumerate() {
        let x = area.left() + col_w * k as f32;
        let y = bar.bottom() + 22.0;
        p.circle_filled(pos2(x + 4.0, y), 4.0, tone(skill));
        p.with_clip_rect(Rect::from_x_y_ranges(x..=x + col_w - 6.0, area.y_range()))
            .text(pos2(x + 14.0, y), Align2::LEFT_CENTER, skill.label(), regular(13.5), look.ink2);
        let count = p.text(pos2(x, y + 14.0), Align2::LEFT_TOP, n.to_string(), semibold(24.0), look.ink);
        p.text(
            pos2(count.right() + 6.0, count.bottom() - 5.0),
            Align2::LEFT_BOTTOM,
            format!("{:.0}%", n as f32 / total as f32 * 100.0),
            regular(13.5),
            look.ink3,
        );
    }
}

fn legend_item(ui: &mut Ui, look: &Look, color: Color32, label: &str) {
    // Right-to-left layout: the label is added first so it sits right of the dot.
    ui.label(RichText::new(label).font(regular(13.5)).color(look.ink2));
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.5, color);
}

fn tooltip_row(ui: &mut Ui, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.5, color);
        ui.label(RichText::new(text).font(regular(14.0)));
    });
}
