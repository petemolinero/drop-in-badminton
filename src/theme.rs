//! Fluent 2 styling (the Microsoft 365 look): color tokens, Segoe UI
//! typography, Fluent System Icons, elevation, and egui style setup.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke};

use crate::model::Skill;

/// Glyphs from the embedded Fluent System Icons subset
/// (`assets/fonts/FluentIcons-DropIn.ttf`, built by `tools/build_icons.py`).
/// `*_F` are the filled variants.
#[allow(dead_code)]
pub mod icon {
    pub const PLAY: &str = "\u{f606}";
    pub const PAUSE: &str = "\u{f5a2}";
    pub const REPLAY: &str = "\u{f140}";
    pub const NEXT: &str = "\u{f56a}";
    pub const SETTINGS: &str = "\u{f6aa}";
    pub const GROUP: &str = "\u{f5a9}";
    pub const PERSON_ADD: &str = "\u{f5c3}";
    pub const FULLSCREEN: &str = "\u{e685}";
    pub const FULLSCREEN_EXIT: &str = "\u{e688}";
    pub const CLOSE: &str = "\u{f36a}";
    pub const DELETE: &str = "\u{f34d}";
    pub const SHUTTLE: &str = "\u{eb67}";
    pub const SEARCH: &str = "\u{f690}";
    pub const HOURGLASS: &str = "\u{ef6c}";
    pub const COFFEE: &str = "\u{f3c5}";
    pub const LOGOUT: &str = "\u{f08e6}";
    pub const LOGIN: &str = "\u{e0c1}";
    pub const ADD: &str = "\u{f10a}";
    pub const REMOVE: &str = "\u{ebd1}";
    pub const UNDO: &str = "\u{f19a}";
    pub const UP: &str = "\u{f19c}";
    pub const NIGHT: &str = "\u{f87e}";
    pub const CHECK: &str = "\u{f295}";
    pub const HISTORY: &str = "\u{f47f}";
    pub const CALENDAR: &str = "\u{e24f}";
    pub const CLOCK: &str = "\u{f2de}";
    pub const CHEVRON_UP: &str = "\u{f2b7}";
    pub const CHEVRON_DOWN: &str = "\u{f2a4}";
    pub const INFO: &str = "\u{f4a4}";
    pub const WARNING: &str = "\u{f86a}";
    pub const CHECK_CIRCLE: &str = "\u{f299}";
    pub const TIMER: &str = "\u{f827}";
    pub const PERSON: &str = "\u{f5be}";
    pub const ARROW_SWAP: &str = "\u{f18e}";
    pub const QUESTION: &str = "\u{f63e}";
    pub const EDIT: &str = "\u{f3de}";
    pub const PERSON_STAR: &str = "\u{ef0d}";
    pub const DOWNLOAD: &str = "\u{f151}";
    pub const PLAY_F: &str = "\u{100000}";
    pub const PAUSE_F: &str = "\u{100001}";
    pub const NEXT_F: &str = "\u{100002}";
    pub const SHUTTLE_F: &str = "\u{100003}";
    pub const GROUP_F: &str = "\u{100004}";
    pub const CHECK_CIRCLE_F: &str = "\u{100005}";
    pub const INFO_F: &str = "\u{100006}";
    pub const WARNING_F: &str = "\u{100007}";
    pub const HOURGLASS_F: &str = "\u{100008}";
    pub const ARROW_UP_F: &str = "\u{100009}";
    pub const TIMER_F: &str = "\u{10000a}";
    pub const QUESTION_F: &str = "\u{10000b}";
    pub const DOWNLOAD_F: &str = "\u{10000c}";
}

pub const ICONS: &str = "icons";
pub const SEMIBOLD: &str = "semibold";
pub const BOLD: &str = "bold";

/// Find Segoe UI on this machine. It can't be redistributed, so it's loaded
/// at runtime when installed; otherwise the embedded Roboto is used.
fn system_segoe() -> Option<[Vec<u8>; 3]> {
    let wanted = ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"];
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/fonts".into(),
        "/usr/local/share/fonts".into(),
        "/Library/Fonts".into(),
        r"C:\Windows\Fonts".into(),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
        dirs.push(home.join("Library/Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
    }
    let mut found: HashMap<&str, PathBuf> = HashMap::new();
    fn walk(dir: &std::path::Path, depth: u32, wanted: &[&'static str], found: &mut HashMap<&'static str, PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                if depth < 5 {
                    walk(&path, depth + 1, wanted, found);
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_ascii_lowercase();
                if let Some(w) = wanted.iter().find(|w| **w == lower) {
                    found.entry(w).or_insert(path);
                }
            }
        }
    }
    for d in &dirs {
        if found.len() == wanted.len() {
            break;
        }
        walk(d, 0, &wanted, &mut found);
    }
    let read = |w: &str| found.get(w).and_then(|p| std::fs::read(p).ok());
    Some([read(wanted[0])?, read(wanted[1])?, read(wanted[2])?])
}

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, data: FontData| {
        fonts.font_data.insert(name.to_owned(), data.into());
    };
    match system_segoe() {
        Some([regular, semibold, bold]) => {
            add("ui", FontData::from_owned(regular));
            add(SEMIBOLD, FontData::from_owned(semibold));
            add(BOLD, FontData::from_owned(bold));
        }
        None => {
            add("ui", FontData::from_static(include_bytes!("../assets/fonts/Roboto-Regular.ttf")));
            add(SEMIBOLD, FontData::from_static(include_bytes!("../assets/fonts/Roboto-Medium.ttf")));
            add(BOLD, FontData::from_static(include_bytes!("../assets/fonts/Roboto-Bold.ttf")));
        }
    }
    add(ICONS, FontData::from_static(include_bytes!("../assets/fonts/FluentIcons-DropIn.ttf")));

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "ui".to_owned());
    prop.insert(1, ICONS.to_owned());
    for name in [SEMIBOLD, BOLD] {
        let mut fam = vec![name.to_owned()];
        fam.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), fam);
    }
    fonts
        .families
        .insert(FontFamily::Name(ICONS.into()), vec![ICONS.to_owned()]);
    fonts
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// Fluent 2 color tokens (web/Microsoft 365 values).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Window canvas behind the floating cards.
    pub canvas: Color32,
    pub bg1: Color32,
    pub bg1_hover: Color32,
    pub bg1_pressed: Color32,
    pub bg2: Color32,
    pub subtle_hover: Color32,
    pub subtle_pressed: Color32,
    pub subtle_selected: Color32,
    #[allow(dead_code)]
    pub bg_disabled: Color32,
    pub fg1: Color32,
    pub fg2: Color32,
    pub fg3: Color32,
    pub fg_disabled: Color32,
    pub stroke1: Color32,
    pub stroke2: Color32,
    pub stroke_accessible: Color32,
    pub card_stroke: Color32,
    pub brand: Color32,
    pub brand_hover: Color32,
    pub brand_pressed: Color32,
    pub on_brand: Color32,
    /// Brand color for text, icons and strokes on neutral backgrounds.
    pub brand_fg: Color32,
    pub brand_subtle: Color32,
    pub on_brand_subtle: Color32,
    pub danger: Color32,
    pub danger_hover: Color32,
    pub danger_fg: Color32,
    #[allow(dead_code)]
    pub danger_subtle: Color32,
    #[allow(dead_code)]
    pub success_fg: Color32,
    #[allow(dead_code)]
    pub success_subtle: Color32,
    pub warning_fg: Color32,
    pub warning_subtle: Color32,
    pub scrim: Color32,
}

pub const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        dark: false,
        canvas: hex(0xF0F0F0),
        bg1: hex(0xFFFFFF),
        bg1_hover: hex(0xF5F5F5),
        bg1_pressed: hex(0xE0E0E0),
        bg2: hex(0xFAFAFA),
        subtle_hover: hex(0xF5F5F5),
        subtle_pressed: hex(0xE0E0E0),
        subtle_selected: hex(0xEBEBEB),
        bg_disabled: hex(0xF0F0F0),
        fg1: hex(0x242424),
        fg2: hex(0x424242),
        fg3: hex(0x616161),
        fg_disabled: hex(0xBDBDBD),
        stroke1: hex(0xD1D1D1),
        stroke2: hex(0xE0E0E0),
        stroke_accessible: hex(0x616161),
        card_stroke: Color32::TRANSPARENT,
        brand: hex(0x0F6CBD),
        brand_hover: hex(0x115EA3),
        brand_pressed: hex(0x0C3B5E),
        on_brand: hex(0xFFFFFF),
        brand_fg: hex(0x0F6CBD),
        brand_subtle: hex(0xEBF3FC),
        on_brand_subtle: hex(0x0F548C),
        danger: hex(0xC50F1F),
        danger_hover: hex(0xB10E1C),
        danger_fg: hex(0xB10E1C),
        danger_subtle: hex(0xFDF3F4),
        success_fg: hex(0x0E700E),
        success_subtle: hex(0xF1FAF1),
        warning_fg: hex(0xBC4B09),
        warning_subtle: hex(0xFFF9F5),
        scrim: Color32::from_rgba_premultiplied(0, 0, 0, 165),
    };

    pub const DARK: Palette = Palette {
        dark: true,
        canvas: hex(0x1A1A1A),
        bg1: hex(0x292929),
        bg1_hover: hex(0x3D3D3D),
        bg1_pressed: hex(0x1F1F1F),
        bg2: hex(0x1F1F1F),
        subtle_hover: hex(0x383838),
        subtle_pressed: hex(0x2E2E2E),
        subtle_selected: hex(0x333333),
        bg_disabled: hex(0x141414),
        fg1: hex(0xFFFFFF),
        fg2: hex(0xD6D6D6),
        fg3: hex(0xADADAD),
        fg_disabled: hex(0x5C5C5C),
        stroke1: hex(0x666666),
        stroke2: hex(0x525252),
        stroke_accessible: hex(0xADADAD),
        card_stroke: hex(0x333333),
        brand: hex(0x115EA3),
        brand_hover: hex(0x0F6CBD),
        brand_pressed: hex(0x0C3B5E),
        on_brand: hex(0xFFFFFF),
        brand_fg: hex(0x479EF5),
        brand_subtle: hex(0x0C3354),
        on_brand_subtle: hex(0x96C6FA),
        danger: hex(0xC50F1F),
        danger_hover: hex(0xD13438),
        danger_fg: hex(0xDC626D),
        danger_subtle: hex(0x5A1418),
        success_fg: hex(0x6CCB5F),
        success_subtle: hex(0x0E3A0E),
        warning_fg: hex(0xFAB37F),
        warning_subtle: hex(0x5A2A0A),
        scrim: Color32::from_rgba_premultiplied(0, 0, 0, 160),
    };

    pub fn get(dark: bool) -> &'static Palette {
        if dark { &Self::DARK } else { &Self::LIGHT }
    }

    /// Fluent elevation (`shadow2` … `shadow64`) as two layers: a tight
    /// ambient shadow and a softer key shadow below it.
    pub fn shadows(&self, level: u8) -> [Shadow; 2] {
        let (ambient, key) = if self.dark { (61, 71) } else { (31, 36) };
        let (ambient_blur, y, blur) = match level {
            0 => return [Shadow::NONE, Shadow::NONE],
            1 | 2 => (2, 1, 2),
            3 | 4 => (2, 2, 4),
            5..=8 => (2, 4, 8),
            9..=16 => (2, 8, 16),
            17..=28 => (8, 14, 28),
            _ => (8, 32, 64),
        };
        [
            Shadow {
                offset: [0, 0],
                blur: ambient_blur,
                spread: 0,
                color: Color32::from_black_alpha(ambient),
            },
            Shadow {
                offset: [0, y],
                blur,
                spread: 0,
                color: Color32::from_black_alpha(key),
            },
        ]
    }

    /// The key layer alone, for egui frames that take a single shadow.
    pub fn shadow(&self, level: u8) -> Shadow {
        self.shadows(level)[1]
    }
}

static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Which theme the skill colors are for; set once per frame.
pub fn set_dark(dark: bool) {
    DARK.store(dark, std::sync::atomic::Ordering::Relaxed);
}

fn dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Skill colors: Beginner yellow, Intermediate red, Advanced green, Expert
/// blue. Light theme: bright, light fills for dark text. Dark theme: deep
/// tinted fills for light text, so the cards glow rather than glare.
pub fn chip_color(skill: Skill) -> Color32 {
    match (skill, dark()) {
        (Skill::Beginner, false) => hex(0xFFDF5E),
        (Skill::Intermediate, false) => hex(0xFF9A94),
        (Skill::Advanced, false) => hex(0x86DC8E),
        (Skill::Expert, false) => hex(0x8EC3FF),
        (Skill::Beginner, true) => hex(0xC9A227),
        (Skill::Intermediate, true) => hex(0x9A2A2A),
        (Skill::Advanced, true) => hex(0x1E6B2C),
        (Skill::Expert, true) => hex(0x1F5AA6),
    }
}

/// The level's chip color as (highlight, body, shadow).
pub fn skill_tones(skill: Skill) -> [Color32; 3] {
    let body = chip_color(skill);
    [lerp_color(body, Color32::WHITE, 0.35), body, lerp_color(body, Color32::BLACK, 0.25)]
}

/// A strong version of each level's color, for dots and small marks.
pub fn skill_color(skill: Skill) -> Color32 {
    match skill {
        Skill::Beginner => hex(0xD9A800),
        Skill::Intermediate => hex(0xD13438),
        Skill::Advanced => hex(0x1E9E3A),
        Skill::Expert => hex(0x1C6FD6),
    }
}

/// Readable text on a skill-colored chip.
pub fn on_skill_color(skill: Skill) -> Color32 {
    if dark() {
        return match skill {
            Skill::Beginner => hex(0x2A1F00),
            Skill::Intermediate => hex(0xFFE5E3),
            Skill::Advanced => hex(0xE3FAE6),
            Skill::Expert => hex(0xE3EFFF),
        };
    }
    match skill {
        Skill::Beginner => hex(0x3D2E00),
        Skill::Intermediate => hex(0x4A0D0D),
        Skill::Advanced => hex(0x0A3314),
        Skill::Expert => hex(0x0A2148),
    }
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn apply(ctx: &egui::Context, pal: &Palette) {
    let mut visuals = if pal.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = pal.canvas;
    visuals.window_fill = pal.bg1;
    visuals.window_stroke = Stroke::new(1.0, pal.card_stroke);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.window_shadow = pal.shadow(28);
    visuals.popup_shadow = pal.shadow(16);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.extreme_bg_color = pal.bg1;
    visuals.text_edit_bg_color = Some(pal.bg1);
    visuals.faint_bg_color = pal.bg2;
    visuals.override_text_color = Some(pal.fg1);
    visuals.hyperlink_color = pal.brand_fg;
    visuals.selection.bg_fill = pal.brand_fg.gamma_multiply(0.3);
    visuals.selection.stroke = Stroke::new(2.0, pal.brand_fg);
    visuals.text_cursor.stroke = Stroke::new(1.5, pal.fg1);
    visuals.slider_trailing_fill = true;

    let radius = CornerRadius::same(4);
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = pal.bg1;
    w.noninteractive.weak_bg_fill = pal.bg1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, pal.stroke2);
    w.noninteractive.fg_stroke = Stroke::new(1.0, pal.fg1);
    w.noninteractive.corner_radius = radius;
    for (state, bg, weak) in [
        (&mut w.inactive, pal.bg1, pal.bg1),
        (&mut w.hovered, pal.bg1_hover, pal.subtle_hover),
        (&mut w.active, pal.bg1_pressed, pal.subtle_pressed),
        (&mut w.open, pal.bg1_hover, pal.subtle_selected),
    ] {
        state.bg_fill = bg;
        state.weak_bg_fill = weak;
        state.bg_stroke = Stroke::new(1.0, pal.stroke1);
        state.fg_stroke = Stroke::new(1.5, pal.fg1);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, pal.stroke_accessible);

    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Heading, semibold(24.0)),
            (Body, regular(16.0)),
            (Button, semibold(15.0)),
            (Small, regular(13.0)),
            (Monospace, FontId::monospace(14.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.menu_margin = egui::Margin::same(4);
        style.spacing.slider_width = 220.0;
        style.animation_time = 0.15;
    });
}
