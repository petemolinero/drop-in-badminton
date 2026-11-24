//! Hand-painted Fluent 2 components.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::model::Skill;
use crate::theme::{self, Palette, icon_font, lerp_color, regular, semibold};

/// Animated (hover, pressed) amounts for a response.
fn interaction(ui: &Ui, resp: &Response) -> (f32, f32) {
    let ctx = ui.ctx();
    let hover = ctx.animate_bool_responsive(resp.id.with("hover"), resp.hovered());
    let press = ctx.animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), 0.06);
    (hover, press)
}

/// Rest → hover → pressed color.
fn state_color(rest: Color32, hover: Color32, pressed: Color32, (h, p): (f32, f32)) -> Color32 {
    lerp_color(lerp_color(rest, hover, h), pressed, p)
}

/// Paint a Fluent two-layer elevation shadow.
pub fn elevate(painter: &egui::Painter, pal: &Palette, rect: Rect, radius: u8, level: u8) {
    for s in pal.shadows(level) {
        if s.color.a() > 0 {
            painter.add(s.as_shape(rect, CornerRadius::same(radius)));
        }
    }
}

/// A Fluent card: surface, hairline (dark mode), and elevation.
pub fn card(painter: &egui::Painter, pal: &Palette, rect: Rect, fill: Color32, radius: u8, elevation: u8) {
    elevate(painter, pal, rect, radius, elevation);
    painter.rect(
        rect,
        CornerRadius::same(radius),
        fill,
        Stroke::new(1.0, pal.card_stroke),
        StrokeKind::Inside,
    );
}

#[derive(Clone, Copy, PartialEq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Subtle,
    Danger,
    /// The warm "time's up" call to action.
    Alert,
}

/// A Fluent button with an optional leading icon.
pub fn button(
    ui: &mut Ui,
    pal: &Palette,
    kind: ButtonKind,
    icon: Option<&str>,
    label: &str,
    height: f32,
) -> Response {
    let font = semibold((height * 0.42).min(20.0));
    let icon_size = (height * 0.56).min(26.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let pad = height * 0.36;
    let gap = height * 0.18;
    let icon_w = if icon.is_some() { icon_size + gap } else { 0.0 };
    let mut width = (galley.size().x + icon_w + pad * 2.0).max(height * 2.0);
    // In a justified layout the button fills the row.
    if ui.layout().horizontal_justify() {
        width = width.max(ui.available_width());
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let enabled = ui.is_enabled();
    let st = interaction(ui, &resp);
    let (fill, stroke, fg, icon_fg) = if !enabled {
        // Visibly a button, just not ready yet.
        let fill = if kind == ButtonKind::Subtle { Color32::TRANSPARENT } else { pal.subtle_selected };
        let stroke = if kind == ButtonKind::Subtle { Color32::TRANSPARENT } else { pal.stroke2 };
        (fill, stroke, pal.fg3, pal.fg3)
    } else {
        match kind {
            ButtonKind::Primary => {
                let f = state_color(pal.brand, pal.brand_hover, pal.brand_pressed, st);
                (f, Color32::TRANSPARENT, pal.on_brand, pal.on_brand)
            }
            ButtonKind::Danger => {
                let f = state_color(pal.danger, pal.danger_hover, pal.danger_hover, st);
                (f, Color32::TRANSPARENT, Color32::WHITE, Color32::WHITE)
            }
            ButtonKind::Alert => {
                let f = state_color(theme::hex(0xC2410C), theme::hex(0xD9530F), theme::hex(0xA3360A), st);
                (f, Color32::TRANSPARENT, Color32::WHITE, Color32::WHITE)
            }
            ButtonKind::Secondary => {
                let f = state_color(pal.bg1, pal.bg1_hover, pal.bg1_pressed, st);
                let s = lerp_color(pal.stroke1, pal.stroke_accessible, st.0 * 0.5);
                (f, s, pal.fg1, pal.fg1)
            }
            ButtonKind::Subtle => {
                let f = state_color(Color32::TRANSPARENT, pal.subtle_hover, pal.subtle_pressed, st);
                (f, Color32::TRANSPARENT, pal.fg1, lerp_color(pal.fg2, pal.brand_fg, st.0))
            }
        }
    };
    let radius = CornerRadius::same(if height >= 48.0 { 6 } else { 4 });
    let p = ui.painter();
    p.rect(rect, radius, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    let content_w = galley.size().x + icon_w;
    let mut x = rect.center().x - content_w / 2.0;
    if let Some(icon) = icon {
        p.text(pos2(x + icon_size / 2.0, rect.center().y), Align2::CENTER_CENTER, icon, icon_font(icon_size), icon_fg);
        x += icon_w;
    }
    p.galley_with_override_text_color(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp
}

/// A square subtle icon button. `checked` shows the toggled-on state.
pub fn icon_button(ui: &mut Ui, pal: &Palette, icon: &str, size: f32, checked: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    paint_icon_button(ui, pal, rect, &resp, icon, checked, None);
    resp
}

/// Paint an icon button into `rect`. With `color = Some((bg, fg))` it's a
/// round filled button (e.g. the timer's play button).
/// The symbol on the timer's round button.
#[derive(Clone, Copy, PartialEq)]
pub enum Media {
    Play,
    Pause,
    Next,
}

/// A round button with a play / pause / next symbol drawn as shapes (not a
/// font glyph), so it sits exactly in the middle. Triangles are nudged right
/// a little so they look centred rather than measure centred.
pub fn paint_media_button(ui: &Ui, rect: Rect, resp: &Response, glyph: Media, bg: Color32) {
    let st = interaction(ui, resp);
    let p = ui.painter();
    let c = rect.center();
    let fill = lerp_color(bg, Color32::BLACK, st.0 * 0.12 + st.1 * 0.12);
    p.circle_filled(c, rect.width() / 2.0, fill);
    paint_media_glyph(p, c, rect.width() * 0.34, glyph, Color32::WHITE);
}

/// A play / pause / next symbol `s` tall, centred (optically) on `c`.
pub fn paint_media_glyph(p: &egui::Painter, c: Pos2, s: f32, glyph: Media, ink: Color32) {
    let bar = |x: f32, w: f32| {
        p.rect_filled(Rect::from_center_size(pos2(x, c.y), vec2(w, s)), CornerRadius::same((w * 0.3) as u8), ink);
    };
    let triangle = |left: f32, w: f32| {
        let pts = vec![pos2(left, c.y - s / 2.0), pos2(left + w, c.y), pos2(left, c.y + s / 2.0)];
        p.add(egui::Shape::convex_polygon(pts, ink, Stroke::new(1.5, ink)));
    };
    match glyph {
        Media::Pause => {
            let w = s * 0.3;
            let gap = s * 0.26;
            bar(c.x - (gap + w) / 2.0, w);
            bar(c.x + (gap + w) / 2.0, w);
        }
        Media::Play => {
            let w = s * 0.9;
            triangle(c.x - w / 2.0 + w * 0.1, w);
        }
        Media::Next => {
            let (w, bw, gap) = (s * 0.8, s * 0.22, s * 0.1);
            let total = w + gap + bw;
            let left = c.x - total / 2.0 + w * 0.06;
            triangle(left, w);
            bar(left + w + gap + bw / 2.0, bw);
        }
    }
}

pub fn paint_icon_button(
    ui: &Ui,
    pal: &Palette,
    rect: Rect,
    resp: &Response,
    icon: &str,
    checked: bool,
    color: Option<(Color32, Color32)>,
) {
    let st = interaction(ui, resp);
    let enabled = ui.is_enabled();
    let p = ui.painter();
    if let Some((bg, fg)) = color {
        let fill = lerp_color(bg, Color32::BLACK, st.0 * 0.12 + st.1 * 0.12);
        p.circle_filled(rect.center(), rect.width() / 2.0, fill);
        p.text(rect.center(), Align2::CENTER_CENTER, icon, icon_font(rect.width() * 0.5), fg);
        return;
    }
    let rest = if checked { pal.subtle_selected } else { Color32::TRANSPARENT };
    let fill = state_color(rest, pal.subtle_hover, pal.subtle_pressed, st);
    p.rect_filled(rect, CornerRadius::same(4), fill);
    let fg = if !enabled {
        pal.fg_disabled
    } else if checked {
        pal.brand_fg
    } else {
        lerp_color(pal.fg2, pal.brand_fg, st.0)
    };
    p.text(rect.center(), Align2::CENTER_CENTER, icon, icon_font(rect.width() * 0.6), fg);
}

/// An Office ribbon "large" button: icon above a label.
pub fn ribbon_button(ui: &mut Ui, pal: &Palette, icon: &str, label: &str, checked: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), regular(13.0), Color32::WHITE);
    let size = vec2((galley.size().x + 20.0).max(60.0), 60.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let st = interaction(ui, &resp);
    let rest = if checked { pal.subtle_selected } else { Color32::TRANSPARENT };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), state_color(rest, pal.subtle_hover, pal.subtle_pressed, st));
    let icon_fg = if checked { pal.brand_fg } else { lerp_color(pal.fg2, pal.brand_fg, st.0) };
    p.text(pos2(rect.center().x, rect.top() + 22.0), Align2::CENTER_CENTER, icon, icon_font(26.0), icon_fg);
    p.galley_with_override_text_color(
        pos2(rect.center().x - galley.size().x / 2.0, rect.bottom() - 10.0 - galley.size().y),
        galley,
        pal.fg1,
    );
    if checked {
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 2.0), vec2(16.0, 3.0));
        p.rect_filled(bar, CornerRadius::same(2), pal.brand_fg);
    }
    resp
}

/// A vertical divider line of the given height.
pub fn vdivider(ui: &mut Ui, pal: &Palette, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(17.0, height), Sense::hover());
    ui.painter().line_segment(
        [pos2(rect.center().x, rect.top()), pos2(rect.center().x, rect.bottom())],
        Stroke::new(1.0, pal.stroke2),
    );
}

/// A horizontal divider across the available width.
pub fn hdivider(ui: &mut Ui, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().line_segment(
        [pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)],
        Stroke::new(1.0, pal.stroke2),
    );
}

/// A switch with "On" / "Off" beside it (for settings rows).
pub fn switch_labeled(ui: &mut Ui, pal: &Palette, on: &mut bool) -> Response {
    // Called in right-to-left rows: the label goes first so it ends up to the
    // right of the switch, as Fluent places it.
    let r = switch(ui, pal, on);
    ui.label(egui::RichText::new(if *on { "On" } else { "Off" }).font(regular(14.0)).color(pal.fg2));
    r
}

/// Fluent switch (toggle).
pub fn switch(ui: &mut Ui, pal: &Palette, on: &mut bool) -> Response {
    let size = vec2(44.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let (h, _) = interaction(ui, &resp);
    let p = ui.painter();
    let radius = CornerRadius::same(11);
    let off_stroke = lerp_color(pal.stroke_accessible, pal.fg1, h);
    let on_fill = lerp_color(pal.brand, pal.brand_hover, h);
    p.rect(
        rect,
        radius,
        on_fill.gamma_multiply(t),
        Stroke::new(1.0, off_stroke.gamma_multiply(1.0 - t)),
        StrokeKind::Inside,
    );
    let cx = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    let knob = lerp_color(off_stroke, pal.on_brand, t);
    p.circle_filled(pos2(cx, rect.center().y), 7.0, knob);
    resp
}

/// Fluent spin button: `[ value     −  + ]`. Returns true when the value changed.
pub fn stepper(ui: &mut Ui, pal: &Palette, value: &mut u32, min: u32, max: u32, step: u32, unit: &str) -> bool {
    let mut changed = false;
    let (outer, outer_resp) = ui.allocate_exact_size(vec2(168.0, 36.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect(outer, CornerRadius::same(4), pal.bg1, Stroke::new(1.0, pal.stroke1), StrokeKind::Inside);
    p.line_segment(
        [outer.left_bottom() + vec2(3.0, -0.5), outer.right_bottom() + vec2(-3.0, -0.5)],
        Stroke::new(1.0, pal.stroke_accessible),
    );
    let text = if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") };
    p.text(pos2(outer.left() + 12.0, outer.center().y), Align2::LEFT_CENTER, text, regular(16.0), pal.fg1);
    let plus = Rect::from_min_size(pos2(outer.right() - 34.0, outer.top() + 2.0), vec2(32.0, 32.0));
    let minus = plus.translate(vec2(-33.0, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(outer));
    for (r, glyph, delta) in [(minus, theme::icon::REMOVE, false), (plus, theme::icon::ADD, true)] {
        let allowed = if delta { *value < max } else { *value > min };
        child.add_enabled_ui(allowed, |ui| {
            let resp = ui
                .interact(r, outer_resp.id.with(glyph), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            paint_icon_button(ui, pal, r, &resp, glyph, false, None);
            if resp.clicked() {
                *value = if delta { (*value + step).min(max) } else { value.saturating_sub(step).max(min) };
                changed = true;
            }
        });
    }
    changed
}

/// A skill tag: the skill badge and the level's name. Selected tags get the
/// Fluent selected look. `compact` shows just the badge.
pub fn skill_pill(ui: &mut Ui, pal: &Palette, skill: Skill, selected: bool, compact: bool) -> Response {
    if compact {
        let (rect, resp) = ui.allocate_exact_size(vec2(badge_width(24.0), 24.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let (h, _) = interaction(ui, &resp);
        let p = ui.painter();
        paint_skill_badge(p, pal, rect, skill);
        if h > 0.0 {
            p.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha((h * 30.0) as u8));
        }
        return resp.on_hover_text(format!("{} — click to change", skill.label()));
    }
    let galley = ui.painter().layout_no_wrap(skill.label().to_owned(), semibold(13.0), Color32::WHITE);
    let badge = badge_width(20.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + badge + 22.0, 30.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let st = interaction(ui, &resp);
    let p = ui.painter();
    let fill = if selected { pal.subtle_selected } else { state_color(pal.bg1, pal.bg1_hover, pal.bg1_pressed, st) };
    let stroke = if selected { Stroke::new(1.5, pal.brand_fg) } else { Stroke::new(1.0, pal.stroke1) };
    p.rect(rect, CornerRadius::same(4), fill, stroke, StrokeKind::Inside);
    let b = Rect::from_min_size(pos2(rect.left() + 7.0, rect.center().y - 10.0), vec2(badge, 20.0));
    paint_skill_badge(p, pal, b, skill);
    p.galley_with_override_text_color(pos2(b.right() + 7.0, rect.center().y - galley.size().y / 2.0), galley, pal.fg1);
    resp
}

/// A fixed-width skill choice (for pickers laid out in a grid). The
/// selected one is filled with the brand tint and ticked.
pub fn skill_option(ui: &mut Ui, pal: &Palette, skill: Skill, selected: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let st = interaction(ui, &resp);
    let p = ui.painter();
    let fill = if selected { pal.brand_subtle } else { state_color(pal.bg1, pal.bg1_hover, pal.bg1_pressed, st) };
    let stroke = if selected { Stroke::new(1.5, pal.brand_fg) } else { Stroke::new(1.0, pal.stroke1) };
    p.rect(rect, CornerRadius::same(4), fill, stroke, StrokeKind::Inside);
    let b = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 10.0), vec2(badge_width(20.0), 20.0));
    paint_skill_badge(p, pal, b, skill);
    let fg = if selected { pal.on_brand_subtle } else { pal.fg1 };
    p.text(pos2(b.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, skill.label(), if selected { semibold(14.0) } else { regular(14.0) }, fg);
    if selected {
        p.text(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, theme::icon::CHECK, icon_font(16.0), pal.brand_fg);
    }
    resp
}

/// How wide a skill tag of height `h` is (it holds a three-letter level).
pub fn badge_width(h: f32) -> f32 {
    (h * 1.9).round()
}

/// The skill tag used everywhere outside the player cards: a small rounded
/// tag in the level's finish with its short name (B1, B2, INT, ADV).
pub fn paint_skill_badge(painter: &egui::Painter, pal: &Palette, rect: Rect, skill: Skill) {
    let radius = (rect.height() * 0.28).round() as u8;
    paint_skill_fill(painter, pal, rect, radius, skill);
    painter.text(rect.center(), Align2::CENTER_CENTER, skill.short(), theme::bold((rect.height() * 0.5).round()), theme::on_skill_color(skill));
}

/// Fit text into `max_width` by shrinking the font, returning the font size.
pub fn fit_font(ui: &Ui, text: &str, family: fn(f32) -> egui::FontId, size: f32, max_width: f32) -> f32 {
    fit(ui.painter(), text, family, size, max_width)
}

fn fit(painter: &egui::Painter, text: &str, family: fn(f32) -> egui::FontId, size: f32, max_width: f32) -> f32 {
    let w = painter.layout_no_wrap(text.to_owned(), family(size), Color32::WHITE).size().x;
    // Whole sizes only: every distinct size is rasterized into the font atlas,
    // so a width that changes every frame must not produce a new size each time.
    if w > max_width && w > 0.0 { (size * max_width / w).floor().max(8.0) } else { size }
}

fn chip_radius(rect: Rect) -> u8 {
    (rect.height() * 0.16).clamp(4.0, 10.0) as u8
}

/// Paint a player chip: a skill-colored card with the name.
/// The font shrinks to fit the name.
#[allow(clippy::too_many_arguments)]
pub fn paint_player_chip(
    painter: &egui::Painter,
    pal: &Palette,
    rect: Rect,
    name: &str,
    skill: Skill,
    font_size: f32,
    lifted: f32,
    highlight: bool,
    detail: Option<&str>,
    meta_cap: f32,
) {
    let fg = theme::on_skill_color(skill);
    let r = chip_radius(rect);
    elevate(painter, pal, rect, r, 2);
    if lifted > 0.0 {
        // Fade the lifted shadow in and out instead of popping between levels.
        for mut s in pal.shadows(16) {
            s.color = s.color.gamma_multiply(lifted);
            painter.add(s.as_shape(rect, CornerRadius::same(r)));
        }
    }
    paint_skill_fill(painter, pal, rect, r, skill);
    // A white edge keeps chips distinct on the court.
    let edge = if pal.dark { Color32::from_white_alpha(70) } else { Color32::from_white_alpha(225) };
    painter.rect_stroke(rect, CornerRadius::same(r), Stroke::new(2.0, edge), StrokeKind::Inside);
    if highlight {
        painter.rect_stroke(rect.expand(3.0), CornerRadius::same(r + 3), Stroke::new(3.0, pal.brand_fg), StrokeKind::Outside);
    }
    let h = rect.height();
    let stack = stacked(rect.size());
    let avail = text_width(rect.size());
    let detail = detail.filter(|_| h >= 40.0);
    let name_max = if detail.is_some() { font_size.min((h * 0.42).round()) } else { font_size.min((h * 0.5).round()) };
    let size = fit(painter, name, theme::semibold, name_max, avail);
    let sub_line = detail.map(|detail| {
        let small = (h * 0.16).round().clamp(12.0, 15.0).min(meta_cap);
        let small = fit(painter, detail, theme::regular, small, avail).max(9.0);
        elided(painter, detail, regular(small), fg, avail)
    });
    let sub_h = sub_line.as_ref().map_or(0.0, |g| g.size().y);
    let mut name_lines = vec![painter.layout_no_wrap(name.to_owned(), semibold(size), fg)];
    // A long name wraps onto two lines rather than shrinking a lot.
    if h >= 48.0 && size < (name_max * 0.9).floor() {
        if let Some((a, b)) = split_name(name) {
            let longer = if a.len() >= b.len() { a } else { b };
            let mut s2 = fit(painter, longer, theme::semibold, name_max, avail);
            while s2 > size {
                if la_lb_height(painter, a, b, s2, fg) + sub_h <= h * 0.98 {
                    name_lines = vec![
                        painter.layout_no_wrap(a.to_owned(), semibold(s2), fg),
                        painter.layout_no_wrap(b.to_owned(), semibold(s2), fg),
                    ];
                    break;
                }
                s2 -= 1.0;
            }
        }
    }
    let size = name_lines[0].size().y / 1.3;
    let tuck = -(size * 0.15).round();
    let line_gap = -(size * 0.04).round();
    let names_h: f32 = name_lines.iter().map(|g| g.size().y).sum::<f32>() + tuck * (name_lines.len() as f32 - 1.0);
    let total = names_h + if sub_h > 0.0 { line_gap + sub_h } else { 0.0 };
    let mut y = (rect.center().y - total / 2.0 - size * 0.06).round();
    // Tall cards centre their text; short ones line it up on the left.
    let x_of = |w: f32| if stack { rect.center().x - w / 2.0 } else { rect.left() + (h * 0.16).clamp(10.0, 18.0) };
    for g in name_lines {
        let pos = pos2(x_of(g.size().x), y);
        y += g.size().y + tuck;
        text_shadow(painter, fg, pos, &g);
        painter.galley(pos, g, fg);
    }
    if let Some(sub) = sub_line {
        let pos = pos2(x_of(sub.size().x), y - tuck + line_gap);
        text_shadow(painter, fg, pos, &sub);
        painter.galley(pos, sub, fg);
    }
}

fn la_lb_height(painter: &egui::Painter, a: &str, b: &str, size: f32, fg: Color32) -> f32 {
    painter.layout_no_wrap(a.to_owned(), semibold(size), fg).size().y + painter.layout_no_wrap(b.to_owned(), semibold(size), fg).size().y - size * 0.3
}

/// Width available for a card's name.
fn text_width(size: Vec2) -> f32 {
    let edge = (size.y * 0.16).clamp(10.0, 18.0);
    size.x - 2.0 * edge
}

/// A faint light edge under dark text on a colored chip, as if pressed into
/// the finish.
fn text_shadow(painter: &egui::Painter, fg: Color32, pos: Pos2, galley: &std::sync::Arc<egui::Galley>) {
    if fg.r() < 100 && fg.g() < 100 && fg.b() < 100 {
        painter.galley_with_override_text_color(pos + vec2(0.0, 1.0), galley.clone(), Color32::from_white_alpha(70));
    }
}




/// Cards tall enough to centre their text (name, then any small line).
fn stacked(size: Vec2) -> bool {
    size.y >= 40.0
}


/// The size a chip of `size` would draw `name` at (see `paint_player_chip`).
pub fn chip_name_size(painter: &egui::Painter, size: Vec2, name: &str, font_size: f32, two_lines: bool) -> f32 {
    let h = size.y;
    let width = text_width(size);
    let max = if two_lines && h >= 40.0 { font_size.min((h * 0.42).round()) } else { font_size };
    fit(painter, name, theme::semibold, max, width)
}

/// The sub-line size a chip of height `h` and text width would use for `detail`.
pub fn chip_meta_size(painter: &egui::Painter, size: Vec2, detail: &str) -> f32 {
    let h = size.y;
    let small = (h * 0.16).round().clamp(12.0, 15.0);
    fit(painter, detail, theme::regular, small, text_width(size)).max(9.0)
}

/// One name size for a group of chips, so a card doesn't mix huge and tiny
/// names. Very long names still shrink further on their own.
pub fn group_name_size(painter: &egui::Painter, size: Vec2, names: &[String], font_size: f32, two_lines: bool) -> f32 {
    // Everyone shares the smallest size among ordinary-length names; a really
    // long name (say "Bilbo Baggins of the Ford") wraps onto two lines at that
    // size instead of dragging everyone down.
    let ordinary = names
        .iter()
        .filter(|n| n.chars().count() <= 18)
        .map(|n| chip_name_size(painter, size, n, font_size, two_lines))
        .fold(f32::MAX, f32::min);
    if ordinary == f32::MAX { font_size } else { ordinary }
}

/// A shuttlecock seen from above: cork, feather skirt with its quills and two
/// bands, and a soft shadow. `angle` is the way the skirt points.
pub fn paint_shuttle(painter: &egui::Painter, c: Pos2, size: f32, angle: f32, dark: bool) {
    let d = Vec2::angled(angle);
    let n = d.rot90();
    let shade = if dark { 0.75 } else { 1.0 };
    let white = Color32::from_gray((250.0 * shade) as u8);
    let quill = Color32::from_gray((190.0 * shade) as u8);
    let at = |along: f32, across: f32| c + d * (along * size) + n * (across * size);
    // Shadow, offset as if lit from the top left.
    let sh = vec2(size * 0.12, size * 0.16);
    let skirt = |o: Vec2| vec![at(0.12, -0.2) + o, at(0.12, 0.2) + o, at(0.95, 0.46) + o, at(0.95, -0.46) + o];
    painter.add(egui::Shape::convex_polygon(skirt(sh), Color32::from_black_alpha(45), Stroke::NONE));
    painter.circle_filled(at(0.0, 0.0) + sh, size * 0.2, Color32::from_black_alpha(45));
    // Skirt and quills.
    painter.add(egui::Shape::convex_polygon(skirt(Vec2::ZERO), white, Stroke::new(1.0, quill)));
    for k in 0..7 {
        let t = k as f32 / 6.0 - 0.5;
        painter.line_segment([at(0.14, t * 0.38), at(0.95, t * 0.9)], Stroke::new(0.8, quill));
    }
    for along in [0.42, 0.62] {
        let w = 0.2 + (along - 0.12) / 0.83 * 0.26;
        painter.line_segment([at(along, -w), at(along, w)], Stroke::new(1.2, quill));
    }
    // Cork, with a little shading.
    painter.circle_filled(at(0.0, 0.0), size * 0.2, Color32::from_rgb((236.0 * shade) as u8, (226.0 * shade) as u8, (206.0 * shade) as u8));
    painter.circle_filled(at(-0.05, -0.05), size * 0.09, Color32::from_white_alpha(120));
    painter.circle_stroke(at(0.0, 0.0), size * 0.2, Stroke::new(1.0, quill));
}

/// A tilted rubber-stamp label, e.g. "TIME!" across a court.
pub fn paint_stamp(painter: &egui::Painter, c: Pos2, text: &str, size: f32, angle: f32, fill: Color32, fg: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), theme::bold(size), fg);
    let half = vec2(galley.size().x / 2.0 + size * 0.6, galley.size().y / 2.0 + size * 0.22);
    let rot = egui::emath::Rot2::from_angle(angle);
    let corners = [vec2(-half.x, -half.y), vec2(half.x, -half.y), vec2(half.x, half.y), vec2(-half.x, half.y)];
    let shadow: Vec<Pos2> = corners.iter().map(|v| c + rot * *v + vec2(0.0, 3.0)).collect();
    painter.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(60), Stroke::NONE));
    let pts: Vec<Pos2> = corners.iter().map(|v| c + rot * *v).collect();
    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(2.0, Color32::from_white_alpha(230))));
    // Text, rotated about its own centre.
    let offset = rot * (-galley.size() / 2.0);
    painter.add(egui::epaint::TextShape::new(c + offset, galley, fg).with_angle(angle));
}

/// "Next on Court [3]": the Up next group's header, outlined so it can't be
/// mistaken for the court above it.
pub fn court_title_next(painter: &egui::Painter, pal: &Palette, left_center: Pos2, number: usize, size: f32) -> Rect {
    let side = (size * 1.35).round();
    let word = painter.text(left_center, Align2::LEFT_CENTER, "Next on Court", semibold(size), pal.fg2);
    let tile = Rect::from_min_size(pos2(word.right() + size * 0.4, left_center.y - side / 2.0), Vec2::splat(side));
    painter.rect(tile, CornerRadius::same((side * 0.22) as u8), pal.bg1, Stroke::new(2.0, pal.brand_fg), StrokeKind::Inside);
    painter.text(tile.center(), Align2::CENTER_CENTER, number.to_string(), theme::bold(size * 0.9), pal.brand_fg);
    tile.union(word)
}

/// "Court [3]" as the sign on the hall wall: white on a smoky plaque, with
/// the number on a tile in the brand color. Returns the plaque's rect.
pub fn court_sign(painter: &egui::Painter, pal: &Palette, left_center: Pos2, number: usize, size: f32) -> Rect {
    let side = (size * 1.3).round();
    let word = painter.layout_no_wrap("Court".to_owned(), semibold(size), Color32::WHITE);
    let pad = (size * 0.35).round();
    let plaque = Rect::from_min_size(
        pos2(left_center.x, left_center.y - side / 2.0 - 3.0),
        vec2(pad + word.size().x + size * 0.36 + side + 3.0, side + 6.0),
    );
    let radius = CornerRadius::same((side * 0.26) as u8);
    painter.rect_filled(plaque, radius, Color32::from_rgba_unmultiplied(6, 14, 24, 150));
    painter.rect_stroke(plaque, radius, Stroke::new(1.0, Color32::from_white_alpha(36)), StrokeKind::Inside);
    let tile = Rect::from_min_size(pos2(plaque.right() - side - 3.0, left_center.y - side / 2.0), Vec2::splat(side));
    painter.galley(pos2(plaque.left() + pad, left_center.y - word.size().y / 2.0), word, Color32::WHITE);
    painter.rect_filled(tile, CornerRadius::same((side * 0.22) as u8), pal.brand);
    painter.text(tile.center(), Align2::CENTER_CENTER, number.to_string(), theme::bold(size * 0.9), pal.on_brand);
    plaque
}

/// A round "VS" medallion, for the line between two teams.
pub fn paint_vs(painter: &egui::Painter, pal: &Palette, c: Pos2, r: f32) {
    let (fill, ink) = if pal.dark { (pal.bg1, pal.fg1) } else { (Color32::WHITE, theme::hex(0x1C3D5A)) };
    painter.circle_filled(c + vec2(0.0, 1.0), r + 1.0, Color32::from_black_alpha(40));
    painter.circle_filled(c, r, fill);
    painter.circle_stroke(c, r, Stroke::new(1.5, ink));
    painter.text(c, Align2::CENTER_CENTER, "VS", theme::bold(r * 0.78), ink);
}

/// Split a name at the space nearest its middle.
fn split_name(name: &str) -> Option<(&str, &str)> {
    let mid = name.len() / 2;
    let at = name.match_indices(' ').map(|(i, _)| i).min_by_key(|&i| i.abs_diff(mid))?;
    Some((&name[..at], &name[at + 1..]))
}

/// Points around a rounded rectangle, clockwise from the top-left corner.
fn rounded_outline(rect: Rect, r: f32) -> Vec<Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.left() + r, rect.top() + r), PI),
        (pos2(rect.right() - r, rect.top() + r), PI + FRAC_PI_2),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
    ];
    let mut pts = Vec::new();
    for (c, start) in corners {
        for i in 0..=8 {
            pts.push(c + r * Vec2::angled(start + FRAC_PI_2 * i as f32 / 8.0));
        }
    }
    pts.push(pts[0]);
    pts
}

/// An empty slot: a dashed rounded outline with a label. `glass` lays a
/// smoky pane behind it so court lines don't run through the label.
#[allow(clippy::too_many_arguments)]
pub fn paint_empty_slot(painter: &egui::Painter, pal: &Palette, rect: Rect, color: Color32, label: &str, font_size: f32, hot: bool, glass: bool) {
    let r = (rect.height() * 0.16).clamp(4.0, 10.0);
    let color = if hot { pal.brand_fg } else { color };
    if glass {
        painter.rect_filled(rect, CornerRadius::same(r as u8), Color32::from_rgba_unmultiplied(8, 22, 40, 150));
    }
    if hot {
        painter.rect_filled(rect, CornerRadius::same(r as u8), pal.brand_fg.gamma_multiply(0.16));
    }
    let stroke = Stroke::new(if hot { 2.0 } else { 1.0 }, color);
    painter.extend(egui::Shape::dashed_line(&rounded_outline(rect, r), stroke, 6.0, 4.0));
    let size = fit(painter, label, theme::regular, font_size, rect.width() - rect.height() * 0.5);
    painter.text(rect.center(), Align2::CENTER_CENTER, label, regular(size), color);
}

/// A small Fluent badge (tint appearance). Returns its rect.
pub fn paint_badge(
    painter: &egui::Painter,
    anchor: Pos2,
    align: Align2,
    icon: Option<&str>,
    text: &str,
    size: f32,
    bg: Color32,
    fg: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), semibold(size), fg);
    let h = size * 1.75;
    let icon_w = if icon.is_some() { size * 1.25 } else { 0.0 };
    let w = galley.size().x + icon_w + size * 1.1;
    let rect = align.anchor_size(anchor, vec2(w, h));
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    let mut x = rect.left() + size * 0.55;
    if let Some(icon) = icon {
        // Icon glyphs sit high in their line box and text sits low; meet in the middle.
        painter.text(pos2(x + size * 0.5, rect.center().y + size * 0.1), Align2::CENTER_CENTER, icon, icon_font(size * 1.1), fg);
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0 - size * 0.1), galley, fg);
    rect
}

/// Lay out a help paragraph for a section at the given width.
/// Help content: a one-line title and a few short bullets.
pub struct Help {
    pub title: String,
    pub bullets: Vec<String>,
}

impl Help {
    pub fn new(title: impl Into<String>, bullets: &[&str]) -> Self {
        Self { title: title.into(), bullets: bullets.iter().map(|b| (*b).to_owned()).collect() }
    }
}

const HELP_PAD: f32 = 12.0;
const HELP_ICON: f32 = 30.0;
/// Columns never get wider than this, so lines stay easy to read.
const HELP_COL_MAX: f32 = 520.0;

/// A laid-out help callout, ready to paint.
pub struct HelpLayout {
    title: std::sync::Arc<egui::Galley>,
    /// Each column's bullets, laid out separately for a hanging indent.
    columns: Vec<Vec<std::sync::Arc<egui::Galley>>>,
    col_w: f32,
    pub height: f32,
}

const BULLET_INDENT: f32 = 14.0;
const BULLET_GAP: f32 = 5.0;

/// Lay out `help` for a callout `width` wide: bullets flow into as many
/// readable-width columns as fit (up to three).
pub fn help_layout(painter: &egui::Painter, pal: &Palette, help: &Help, width: f32) -> HelpLayout {
    let inner = (width - HELP_PAD * 2.0 - HELP_ICON).max(60.0);
    let gap = 28.0;
    // The column grid depends only on the width, so callouts side by side line up.
    let ncols = (((inner + gap) / (HELP_COL_MAX * 0.75 + gap)).floor() as usize).clamp(1, 3);
    let col_w = ((inner - gap * (ncols as f32 - 1.0)) / ncols as f32).min(HELP_COL_MAX);
    let n = help.bullets.len();
    let mut columns: Vec<Vec<_>> = Vec::new();
    let mut start = 0;
    for c in 0..ncols {
        // Spread bullets as evenly as possible, earlier columns first.
        let take = n / ncols + usize::from(c < n % ncols);
        columns.push(
            help.bullets[start..start + take]
                .iter()
                .map(|b| painter.layout(b.clone(), regular(14.0), pal.fg1, col_w - BULLET_INDENT))
                .collect(),
        );
        start += take;
    }
    let title = painter.layout_no_wrap(help.title.clone(), semibold(15.0), pal.fg1);
    let body_h = columns
        .iter()
        .map(|col| col.iter().map(|g| g.size().y + BULLET_GAP).sum::<f32>() - BULLET_GAP)
        .fold(0.0, f32::max);
    HelpLayout { height: HELP_PAD * 2.0 + title.size().y + 6.0 + body_h, title, columns, col_w }
}

/// Paint a Fluent info message bar: tinted card, help icon, title, bullets.
pub fn paint_help(painter: &egui::Painter, pal: &Palette, rect: Rect, layout: HelpLayout) {
    painter.rect(rect, CornerRadius::same(8), pal.brand_subtle, Stroke::new(1.0, pal.brand_fg.gamma_multiply(0.35)), StrokeKind::Inside);
    let x0 = rect.left() + HELP_PAD + HELP_ICON;
    let y0 = rect.top() + HELP_PAD;
    painter.text(
        pos2(rect.left() + HELP_PAD + 10.0, y0 + layout.title.size().y / 2.0),
        Align2::CENTER_CENTER,
        theme::icon::QUESTION_F,
        icon_font(20.0),
        pal.brand_fg,
    );
    let body_y = y0 + layout.title.size().y + 6.0;
    painter.galley(pos2(x0, y0), layout.title, pal.fg1);
    for (i, col) in layout.columns.into_iter().enumerate() {
        let x = x0 + i as f32 * (layout.col_w + 28.0);
        let mut y = body_y;
        for g in col {
            let row_h = g.rows.first().map_or(g.size().y, |r| r.size.y);
            painter.circle_filled(pos2(x + 4.0, y + row_h / 2.0), 2.5, pal.brand_fg);
            let h = g.size().y;
            painter.galley(pos2(x + BULLET_INDENT, y), g, pal.fg1);
            y += h + BULLET_GAP;
        }
    }
}

/// A help callout in normal layout flow, across the available width.
pub fn help_bar(ui: &mut Ui, pal: &Palette, help: &Help) {
    let layout = help_layout(ui.painter(), pal, help, ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), layout.height), Sense::hover());
    paint_help(ui.painter(), pal, rect, layout);
}

/// One line of text that ends in "…" if it would run past `max_width`.
pub fn elided(painter: &egui::Painter, text: &str, font: egui::FontId, color: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

/// A Fluent subtitle-style group label.
pub fn section_label(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.label(egui::RichText::new(text).font(semibold(15.0)).color(pal.fg1));
}

/// Format milliseconds as `m:ss`.
pub fn clock(ms: i64) -> String {
    let secs = (ms.max(0) + 999) / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Compact duration like `7m` or `1h 05m`.
pub fn short_duration(ms: i64) -> String {
    let mins = ms.max(0) / 60_000;
    if mins >= 60 {
        format!("{}h {:02}m", mins / 60, mins % 60)
    } else {
        format!("{mins}m")
    }
}

pub fn unique(id: impl std::hash::Hash + std::fmt::Debug) -> Id {
    Id::unique(("drop-in-badminton", id))
}

/// A soft fade over the bottom of a scrolled list while there's more below,
/// so rows ending neatly at the edge don't look like the end of the list.
/// `corner` is the radius of the card's bottom corners the list sits in.
pub fn fade_bottom<R>(ui: &Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, bg: Color32, corner: u8) {
    let view = out.inner_rect;
    let below = out.content_size.y - out.state.clamped_offset().y - view.height();
    // Fades away over the last stretch, so it's gone at the very end.
    let t = (below / 40.0).clamp(0.0, 1.0);
    if t <= 0.0 {
        return;
    }
    let inset = if corner > 0 { 1.0 } else { 0.0 };
    let bottom = view.bottom() - inset;
    let (left, right) = (view.left() + inset, view.right() - inset);
    let h = 64.0f32.min(view.height() / 3.0);
    let solid = bottom - corner as f32;
    let mut mesh = egui::Mesh::default();
    const BANDS: u32 = 10;
    for i in 0..=BANDS {
        let f = i as f32 / BANDS as f32;
        let y = egui::lerp((bottom - h)..=solid, f);
        // Ease in, so the top of the fade has no visible edge.
        let c = bg.gamma_multiply(t * f * f * (3.0 - 2.0 * f));
        mesh.colored_vertex(pos2(left, y), c);
        mesh.colored_vertex(pos2(right, y), c);
        if i > 0 {
            let k = 2 * i;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    let p = ui.painter();
    p.add(mesh);
    if corner > 0 {
        let r = Rect::from_min_max(pos2(left, solid), pos2(right, bottom));
        p.rect_filled(r, CornerRadius { nw: 0, ne: 0, sw: corner - 1, se: corner - 1 }, bg.gamma_multiply(t));
    }
}

// ===========================================================================
// Skill colors: each level's polished finish.

use egui::epaint::RectShape;

static GLOSS: std::sync::OnceLock<egui::TextureHandle> = std::sync::OnceLock::new();
static APP_ICON: std::sync::OnceLock<egui::TextureHandle> = std::sync::OnceLock::new();

/// The app's own icon (the same one as the window and desktop icon), for
/// drawing in the title bar. Mipmapped so it stays crisp drawn small.
pub fn app_icon(ctx: &egui::Context) -> egui::TextureId {
    APP_ICON
        .get_or_init(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied([256, 256], include_bytes!("../assets/icon/app.rgba"));
            let opts = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
            ctx.load_texture("app-icon", image, opts)
        })
        .id()
}

/// The card finish: one soft grey-scale gradient (a bright top lip fading to
/// a gently shaded base), tinted by each level's color when drawn.
pub fn load_skill_finishes(ctx: &egui::Context) {
    GLOSS.get_or_init(|| {
        const H: usize = 64;
        let pixels = (0..H)
            .flat_map(|y| {
                let v = (y as f32 + 0.5) / H as f32;
                let lip = if v < 0.1 { (1.0 - v / 0.1) * 0.06 } else { 0.0 };
                let k = (1.0 - 0.1 * v * v + lip).min(1.0);
                std::iter::repeat_n(Color32::from_gray((k * 255.0) as u8), 4)
            })
            .collect();
        let opts = egui::TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, ..Default::default() };
        ctx.load_texture("card-gloss", egui::ColorImage::new([4, H], pixels), opts)
    });
}

/// The background of a chip or tag: the level's color, softly lit from above.
pub fn paint_skill_fill(painter: &egui::Painter, pal: &Palette, rect: Rect, radius: u8, skill: Skill) {
    let [hi, body, lo] = theme::skill_tones(skill);
    let fill = if pal.dark { body } else { theme::lerp_color(body, hi, 0.25) };
    match GLOSS.get() {
        Some(tex) => {
            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
            painter.add(RectShape::filled(rect, CornerRadius::same(radius), fill).with_texture(tex.id(), uv));
        }
        None => {
            painter.rect_filled(rect, CornerRadius::same(radius), fill);
        }
    }
    painter.rect_stroke(rect, CornerRadius::same(radius), Stroke::new(1.0, lo.gamma_multiply(0.6)), StrokeKind::Inside);
}

/// Menu entry text for a level: a dot in its color, then its name.
pub fn skill_menu_text(_pal: &Palette, skill: Skill) -> egui::RichText {
    egui::RichText::new(format!("●  {}", skill.label())).color(theme::skill_color(skill))
}
