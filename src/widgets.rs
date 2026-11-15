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
    let width = (galley.size().x + icon_w + pad * 2.0).max(height * 2.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let enabled = ui.is_enabled();
    let st = interaction(ui, &resp);
    let (fill, stroke, fg, icon_fg) = if !enabled {
        let fill = if kind == ButtonKind::Subtle { Color32::TRANSPARENT } else { pal.bg_disabled };
        let stroke = if kind == ButtonKind::Subtle { Color32::TRANSPARENT } else { pal.stroke2 };
        (fill, stroke, pal.fg_disabled, pal.fg_disabled)
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

/// A skill tag. Selected tags are filled with the skill color; others are
/// outlined with a colored dot. `compact` shows just the initial.
pub fn skill_pill(ui: &mut Ui, pal: &Palette, skill: Skill, selected: bool, compact: bool) -> Response {
    let base = theme::skill_color(skill);
    let font = semibold(13.0);
    if compact {
        let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let (h, _) = interaction(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(4), lerp_color(base, Color32::BLACK, h * 0.15));
        p.text(rect.center(), Align2::CENTER_CENTER, &skill.label()[..1], font, theme::on_skill_color(skill));
        return resp.on_hover_text(format!("{} — click to change", skill.label()));
    }
    let galley = ui.painter().layout_no_wrap(skill.label().to_owned(), font, Color32::WHITE);
    let dot = if selected { 0.0 } else { 12.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 18.0 + dot, 30.0), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let st = interaction(ui, &resp);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(4), base);
        p.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, theme::on_skill_color(skill));
    } else {
        let fill = state_color(pal.bg1, pal.bg1_hover, pal.bg1_pressed, st);
        p.rect(rect, CornerRadius::same(4), fill, Stroke::new(1.0, pal.stroke1), StrokeKind::Inside);
        p.circle_filled(pos2(rect.left() + 13.0, rect.center().y), 4.5, base);
        p.galley_with_override_text_color(
            pos2(rect.left() + 9.0 + dot, rect.center().y - galley.size().y / 2.0),
            galley,
            pal.fg1,
        );
    }
    resp
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

/// A short vertical bar in a player's skill color, used to mark list rows.
pub fn paint_skill_bar(painter: &egui::Painter, rect: Rect, skill: Skill) {
    painter.rect_filled(rect, CornerRadius::same(2), theme::skill_color(skill));
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
) {
    let base = theme::skill_color(skill);
    let fg = theme::on_skill_color(skill);
    let r = (rect.height() * 0.16).clamp(4.0, 10.0) as u8;
    elevate(painter, pal, rect, r, 2);
    if lifted > 0.0 {
        // Fade the lifted shadow in and out instead of popping between levels.
        for mut s in pal.shadows(16) {
            s.color = s.color.gamma_multiply(lifted);
            painter.add(s.as_shape(rect, CornerRadius::same(r)));
        }
    }
    // A white edge keeps chips distinct on the green court (and green chips on it).
    painter.rect(rect, CornerRadius::same(r), base, Stroke::new(2.0, Color32::from_white_alpha(225)), StrokeKind::Inside);
    if highlight {
        painter.rect_stroke(rect.expand(3.0), CornerRadius::same(r + 3), Stroke::new(3.0, pal.brand_fg), StrokeKind::Outside);
    }
    let text_rect = rect.shrink2(vec2(rect.height() * 0.24, 0.0));
    let h = rect.height();
    match detail {
        // Two lines: the name, and a small line under it (e.g. time since playing).
        // Sizes are rounded so a chip in flight doesn't rasterize a new font size
        // every frame.
        Some(detail) if h >= 40.0 => {
            let name_max = font_size.min((h * 0.42).round());
            let size = fit(painter, name, theme::semibold, name_max, text_rect.width());
            let small = (h * 0.15).round().clamp(10.0, 14.0);
            let small = fit(painter, detail, theme::regular, small, text_rect.width());
            // Lay out both lines, then centre the pair vertically as one block.
            let top_line = painter.layout_no_wrap(name.to_owned(), semibold(size), fg);
            let sub_line = painter.layout_no_wrap(detail.to_owned(), regular(small), fg.gamma_multiply(0.85));
            // Tuck the second line up under the name (line boxes have slack below the baseline).
            let gap = -(size * 0.14).round();
            let total = top_line.size().y + gap + sub_line.size().y;
            // Line boxes carry space above the capitals and below the baseline;
            // nudge up so the visible ink is what's centred.
            let y = (rect.center().y - total / 2.0 - size * 0.1).round();
            let cx = text_rect.center().x;
            let sub_y = y + top_line.size().y + gap;
            painter.galley(pos2(cx - top_line.size().x / 2.0, y), top_line, fg);
            painter.galley(pos2(cx - sub_line.size().x / 2.0, sub_y), sub_line, fg);
        }
        _ => {
            let size = fit(painter, name, theme::semibold, font_size, text_rect.width());
            painter.text(text_rect.center(), Align2::CENTER_CENTER, name, semibold(size), fg);
        }
    }
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

/// An empty slot: a dashed rounded outline with a label.
pub fn paint_empty_slot(painter: &egui::Painter, pal: &Palette, rect: Rect, color: Color32, label: &str, font_size: f32, hot: bool) {
    let r = (rect.height() * 0.16).clamp(4.0, 10.0);
    let color = if hot { pal.brand_fg } else { color };
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
        painter.text(pos2(x + size * 0.5, rect.center().y), Align2::CENTER_CENTER, icon, icon_font(size * 1.1), fg);
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    rect
}

/// Lay out a help paragraph for a section at the given width.
pub fn help_galley(painter: &egui::Painter, pal: &Palette, text: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    let wrap = (width - HELP_PAD * 2.0 - HELP_ICON).max(60.0);
    painter.layout(text.to_owned(), regular(14.0), pal.fg1, wrap)
}

const HELP_PAD: f32 = 12.0;
const HELP_ICON: f32 = 28.0;

/// Height of a help bar holding `galley`.
pub fn help_height(galley: &egui::Galley) -> f32 {
    galley.size().y + HELP_PAD * 2.0 - 2.0
}

/// Paint a Fluent info message bar: tinted card, info icon, wrapped text.
pub fn paint_help(painter: &egui::Painter, pal: &Palette, rect: Rect, galley: std::sync::Arc<egui::Galley>) {
    painter.rect(rect, CornerRadius::same(6), pal.brand_subtle, Stroke::new(1.0, pal.brand_fg.gamma_multiply(0.35)), StrokeKind::Inside);
    painter.text(
        pos2(rect.left() + HELP_PAD + 9.0, rect.top() + HELP_PAD + 9.0),
        Align2::CENTER_CENTER,
        theme::icon::QUESTION_F,
        icon_font(20.0),
        pal.brand_fg,
    );
    painter.galley(pos2(rect.left() + HELP_PAD + HELP_ICON, rect.top() + HELP_PAD - 1.0), galley, pal.fg1);
}

/// A help bar in normal layout flow, across the available width.
pub fn help_bar(ui: &mut Ui, pal: &Palette, text: &str) {
    let galley = help_galley(ui.painter(), pal, text, ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), help_height(&galley)), Sense::hover());
    paint_help(ui.painter(), pal, rect, galley);
}

/// A Fluent subtitle-style group label.
pub fn section_label(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.label(egui::RichText::new(text).font(semibold(14.0)).color(pal.fg2));
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
    Id::unique(("goodminton", id))
}
