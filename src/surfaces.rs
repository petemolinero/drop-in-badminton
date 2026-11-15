//! Procedural textures for the courts: a hardwood gym floor and the blue
//! court mat. Generated once at startup and tiled with `Repeat` wrapping, so
//! each court costs just a couple of textured rectangles per frame.

use egui::{Color32, ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};
use std::f32::consts::TAU;

/// Texture pixels per on-screen point (textures are drawn at half size for crispness).
pub const TEXELS_PER_POINT: f32 = 2.0;

pub struct Surfaces {
    /// Maple strip flooring (tiles).
    pub wood: TextureHandle,
    /// Blue court mat (tiles).
    pub mat: TextureHandle,
    /// Net mesh: light threads over a faint shade (tiles).
    pub net: TextureHandle,
    /// Streaks of window light, added over the floor (tiles; mapped in screen
    /// space so the light falls across all the courts as one room).
    pub light: TextureHandle,
    /// Darkening toward the edges plus a broad glow, stretched over a court.
    pub vignette: TextureHandle,
}

/// On-screen size (points) of one tile of the window-light texture.
pub const LIGHT_TILE: egui::Vec2 = egui::vec2(470.0, 1034.0);

impl Surfaces {
    pub fn load(ctx: &egui::Context) -> Self {
        let options = TextureOptions {
            magnification: TextureFilter::Linear,
            minification: TextureFilter::Linear,
            wrap_mode: TextureWrapMode::Repeat,
            mipmap_mode: Some(TextureFilter::Linear),
        };
        let stretched = TextureOptions { wrap_mode: TextureWrapMode::ClampToEdge, mipmap_mode: None, ..options };
        Self {
            wood: ctx.load_texture("hardwood", hardwood(1024, 504), options),
            mat: ctx.load_texture("court-mat", court_mat(256, 256), options),
            net: ctx.load_texture("net-mesh", net_mesh(64), options),
            light: ctx.load_texture("window-light", window_light(256, 256), options),
            vignette: ctx.load_texture("vignette", vignette(128, 128), stretched),
        }
    }
}

/// Deterministic hash → [0, 1).
fn hash(a: u32, b: u32, c: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

fn rgb(r: f32, g: f32, b: f32) -> Color32 {
    let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(c(r), c(g), c(b))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth value noise in [0, 1) that repeats every `px` × `py` lattice cells.
fn vnoise(x: f32, y: f32, px: u32, py: u32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |dx: i64, dy: i64| {
        let xi = (x0 as i64 + dx).rem_euclid(px as i64) as u32;
        let yi = (y0 as i64 + dy).rem_euclid(py as i64) as u32;
        hash(xi, yi, seed)
    };
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Maple strip flooring as laid in a sports hall: narrow boards running left
/// to right with staggered end joints, a little tone variation from board to
/// board (the odd one browner), and long, quiet grain. Tiles seamlessly.
pub fn hardwood(w: usize, h: usize) -> ColorImage {
    const BOARD: usize = 14; // texels per board (h must be a multiple)
    debug_assert_eq!(h % BOARD, 0);
    let rows = h / BOARD;
    let mut pixels = Vec::with_capacity(w * h);
    let mut board_at = vec![0u32; w];
    let mut joint_at = vec![false; w];
    for y in 0..h {
        let row = (y / BOARD) as u32;
        let in_board = y % BOARD;
        if in_board == 0 {
            // Lay this row: random board lengths, staggered against the rows above.
            joint_at.fill(false);
            let start = (hash(row, 1, 7) * 400.0) as usize % w;
            let (mut x, mut board, mut next) = (0, 0u32, hash(row, 0, 3));
            while x < w {
                let len = 190 + (next * 330.0) as usize;
                // Don't leave a stub of a board before the tile wraps.
                let end = if x + len + 150 > w { w } else { x + len };
                for k in x..end {
                    board_at[(start + k) % w] = board;
                }
                joint_at[(start + x) % w] = true;
                x = end;
                board += 1;
                next = hash(row, board, 3);
            }
        }
        for x in 0..w {
            let board = board_at[x];
            let seed = row * 131 + board * 7 + 1000;
            let tone = hash(row, board, 11) - 0.5;
            let warm = hash(row, board, 13) - 0.5;
            let heart = hash(row, board, 19) > 0.88;
            // Grain: long streaks along the board, at two scales.
            let (xf, yf) = (x as f32 / w as f32, y as f32);
            let shift = hash(row, board, 17) * 7.0;
            let long = vnoise(xf * 10.0 + shift, yf / 2.6, 10, (rows * BOARD) as u32, seed) - 0.5;
            let fine = vnoise(xf * 40.0 + shift, yf / 1.1, 40, (rows * BOARD) as u32, seed + 1) - 0.5;
            let speck = hash(x as u32, y as u32, 23) - 0.5;
            let mut shade = 1.0 + tone * 0.075 + long * 0.075 + fine * 0.04 + speck * 0.012;
            // Seams: a fine dark line between rows and at board ends.
            if in_board == 0 {
                shade *= 0.83;
            } else if in_board == 1 {
                shade *= 1.015;
            }
            if joint_at[x] {
                shade *= 0.84;
            }
            // Golden-orange finished maple (the amber of a varnished gym floor);
            // heartwood boards run browner.
            let (mut r, mut g, mut b) = (236.0 + warm * 8.0, 176.0 + warm * 8.0, 92.0 - warm * 4.0);
            if heart {
                r *= 0.955;
                g *= 0.92;
                b *= 0.87;
            }
            pixels.push(rgb(r * shade, g * shade, b * shade));
        }
    }
    ColorImage::new([w, h], pixels)
}

/// The blue court mat: matte PVC with a fine pebbled finish and faint mottling.
pub fn court_mat(w: usize, h: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f32 / w as f32, y as f32 / h as f32);
            let mottle = vnoise(xf * 6.0, yf * 6.0, 6, 6, 41) - 0.5;
            let pebble = vnoise(xf * 86.0, yf * 86.0, 86, 86, 43) - 0.5;
            let grain = hash(x as u32, y as u32, 5) - 0.5;
            let shade = 1.0 + mottle * 0.022 + pebble * 0.05 + grain * 0.03;
            // The deep court blue (#1C4E78).
            pixels.push(rgb(28.0 * shade, 78.0 * shade, 120.0 * shade));
        }
    }
    ColorImage::new([w, h], pixels)
}

/// Net mesh: pale cord in a square mesh over a faint shade, mostly see-through.
pub fn net_mesh(size: usize) -> ColorImage {
    const CELL: usize = 8;
    debug_assert_eq!(size % CELL, 0);
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let cord = x % CELL == 0 || y % CELL == 0;
            pixels.push(if cord {
                Color32::from_rgba_premultiplied(150, 150, 146, 165)
            } else {
                Color32::from_black_alpha(46)
            });
        }
    }
    ColorImage::new([size, size], pixels)
}

/// Light from a row of high windows, falling across the floor as slanted,
/// soft-edged streaks. Purely additive (zero alpha), and periodic so it tiles.
pub fn window_light(w: usize, h: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            // Slanted coordinate across the streaks (period 1 in both u and v).
            let s = (u + v).rem_euclid(1.0);
            let soft = 0.03;
            let mut light = 0.0_f32;
            for (a, b, gain) in [(0.06, 0.20, 1.0), (0.27, 0.41, 0.85), (0.48, 0.62, 0.6)] {
                light += gain * smoothstep(a - soft, a + soft, s) * (1.0 - smoothstep(b - soft, b + soft, s));
            }
            // The streaks swell and fade along their length.
            light *= 0.62 + 0.38 * (TAU * (v - 0.15)).cos();
            let dither = (hash(x as u32, y as u32, 31) - 0.5) * 1.2;
            let c = |k: f32| (light * k + dither).round().clamp(0.0, 255.0) as u8;
            pixels.push(Color32::from_rgba_premultiplied(c(46.0), c(42.0), c(32.0), 0));
        }
    }
    ColorImage::new([w, h], pixels)
}

/// Lighting stretched over one court's patch of floor: it falls off toward
/// the edges (black with alpha) and glows softly up the middle (additive).
pub fn vignette(w: usize, h: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let centre = (16.0 * u * (1.0 - u) * v * (1.0 - v)).powf(0.3);
            let dark = (1.0 - centre) * 0.4;
            let (dx, dy) = (u - 0.5, v - 0.36);
            let glow = (-(dx * dx / 0.16 + dy * dy / 0.20)).exp() * 15.0;
            let dither = hash(x as u32, y as u32, 37) - 0.5;
            let c = |k: f32| (glow * k + dither).round().clamp(0.0, 255.0) as u8;
            let a = (dark * 255.0 + dither).round().clamp(0.0, 255.0) as u8;
            pixels.push(Color32::from_rgba_premultiplied(c(1.0), c(0.96), c(0.86), a));
        }
    }
    ColorImage::new([w, h], pixels)
}

/// Court length and width in metres (the net crosses the width at half length).
pub const COURT_L: f32 = 13.4;
pub const COURT_W: f32 = 6.1;

/// Paint one badminton court, seen from above, filling `surface`: the gym's
/// maple floor with a stray game line or two from other sports, a blue mat
/// laid on it, the court markings, the room's light, and the net.
///
/// Returns the rectangle of the boundary lines: the length runs top to bottom
/// and the net crosses the middle. Everything textured is a pre-built tile, so
/// a court costs a few dozen plain rectangles per frame.
pub fn paint_court(p: &egui::Painter, tex: &Surfaces, dark: bool, index: usize, surface: egui::Rect) -> egui::Rect {
    use egui::epaint::{RectShape, Shadow};
    use egui::{CornerRadius, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

    let ppp = p.ctx().pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp;
    let clipped = p.with_clip_rect(surface.intersect(p.clip_rect()));
    // Dark mode turns the hall lights down.
    let lit = |full: u8, dimmed: u8| Color32::from_gray(if dark { dimmed } else { full });
    let paint = |c: Color32| {
        let k = if dark { 0.7 } else { 1.0 };
        rgb(c.r() as f32 * k, c.g() as f32 * k, c.b() as f32 * k)
    };
    let tiled = |t: &TextureHandle, r: Rect, origin: Vec2| {
        let [tw, th] = t.size();
        let uv_size = r.size() * TEXELS_PER_POINT / vec2(tw as f32, th as f32);
        (t.id(), Rect::from_min_size(origin.to_pos2(), uv_size))
    };
    let corners = CornerRadius::same(6);

    // Hardwood floor (each court sits on a different patch of it).
    let (wood_id, wood_uv) = tiled(&tex.wood, surface, vec2(0.37 * index as f32, 0.29 * index as f32));
    p.add(RectShape::filled(surface, corners, lit(255, 172)).with_texture(wood_id, wood_uv));

    // Game lines of the other sports painted on the gym floor, running on
    // under the mat. Each court gets a different piece of them.
    let navy = paint(Color32::from_rgb(30, 66, 132));
    let red = paint(Color32::from_rgb(186, 52, 46));
    let (sw, sh) = (surface.width(), surface.height());
    let across = |t: f32, color: Color32| {
        let y = snap(surface.top() + sh * t);
        clipped.rect_filled(Rect::from_min_max(pos2(surface.left(), y), pos2(surface.right(), y + 3.0)), 0, color);
    };
    let along = |t: f32, color: Color32| {
        let x = snap(surface.left() + sw * t);
        clipped.rect_filled(Rect::from_min_max(pos2(x, surface.top()), pos2(x + 3.0, surface.bottom())), 0, color);
    };
    match index % 3 {
        0 => {
            across(0.285, red);
            along(0.052, navy);
        }
        1 => {
            // A sweep of the basketball three-point arc.
            let (c, r) = (pos2(surface.center().x + sw * 0.06, surface.top() + sh * 1.22), sh * 0.88);
            let pts = (0..=40)
                .map(|k| {
                    let a = (-150.0 + 120.0 * k as f32 / 40.0).to_radians();
                    c + r * Vec2::angled(a)
                })
                .collect();
            clipped.add(egui::Shape::line(pts, Stroke::new(3.0, navy)));
        }
        _ => {
            across(0.705, navy);
            along(0.94, red);
        }
    }

    // Window light on the varnish (see below for the rest of the lighting).
    let light_uv = |r: Rect| Rect::from_min_size((r.min.to_vec2() / LIGHT_TILE).to_pos2(), r.size() / LIGHT_TILE);
    p.add(RectShape::filled(surface, corners, lit(255, 150)).with_texture(tex.light.id(), light_uv(surface)));

    // Fit the court: as close to true proportions as the card allows while
    // keeping names readable.
    /// How much wider than true proportions the court may be drawn.
    const STRETCH: f32 = 1.45;
    let (l, w) = (COURT_L, COURT_W);
    let room = surface.shrink(14.0);
    let aspect = w / l * STRETCH; // width ÷ height
    let size = if room.width() / room.height() > aspect {
        vec2(room.height() * aspect, room.height())
    } else {
        vec2(room.width(), room.width() / aspect)
    };
    let lines = Rect::from_center_size(room.center(), size);
    // Snap to whole pixels so the markings come out crisp.
    let lines = Rect::from_min_max(
        pos2(snap(lines.left()), snap(lines.top())),
        pos2(snap(lines.right()), snap(lines.bottom())),
    );
    // `x` runs along the length (top to bottom), `y` across the width.
    let map = |x: f32, y: f32| pos2(lines.left() + lines.width() * y / w, lines.top() + lines.height() * x / l);

    // The blue mat, a little larger than the boundary lines, lying on the
    // floor: a soft shadow where it meets the wood, then a tight one.
    let mat = lines.expand(10.0);
    let mat_corners = CornerRadius::same(2);
    for (offset, blur, alpha) in [([0, 3], 16, 88), ([0, 1], 3, 96)] {
        let shadow = Shadow { offset, blur, spread: 0, color: Color32::from_black_alpha(alpha) };
        clipped.add(shadow.as_shape(mat, mat_corners));
    }
    let (mat_id, mat_uv) = tiled(&tex.mat, mat, Vec2::ZERO);
    p.add(RectShape::filled(mat, mat_corners, lit(255, 200)).with_texture(mat_id, mat_uv));
    // The mat comes in rolls laid side by side: faint seams between them.
    let px = 1.0 / ppp;
    for k in 1..4 {
        let x = snap(mat.left() + mat.width() * k as f32 / 4.0);
        p.rect_filled(Rect::from_min_max(pos2(x, mat.top()), pos2(x + px, mat.bottom())), 0, Color32::from_black_alpha(46));
        p.rect_filled(Rect::from_min_max(pos2(x + px, mat.top()), pos2(x + 2.0 * px, mat.bottom())), 0, Color32::from_white_alpha(16));
    }
    // Its bound edge, catching a little light along the top.
    p.rect_stroke(mat, mat_corners, Stroke::new(1.5, Color32::from_black_alpha(52)), StrokeKind::Inside);
    p.rect_filled(Rect::from_min_max(mat.left_top(), pos2(mat.right(), mat.top() + px)), 0, Color32::from_white_alpha(70));

    // The room's light: darker toward the edges with a glow up the middle,
    // and streaks of window light across the whole row of courts (mapped in
    // screen space, so they line up from court to court). The matte mat
    // catches the streaks more faintly than the varnished wood around it.
    let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.add(RectShape::filled(surface, corners, lit(255, 150)).with_texture(tex.vignette.id(), whole));
    p.add(RectShape::filled(mat, mat_corners, lit(110, 70)).with_texture(tex.light.id(), light_uv(mat)));

    // Court markings: 40 mm white lines, as pixel-aligned bars.
    let line_color = lit(250, 222);
    let lw = (2.4 * ppp).round().max(2.0) / ppp;
    let bar = |a: egui::Pos2, b: egui::Pos2| {
        // A bar centred on the segment a–b (horizontal or vertical).
        let r = if a.y == b.y {
            let y = snap(a.y - lw / 2.0);
            Rect::from_min_max(pos2(a.x, y), pos2(b.x, y + lw))
        } else {
            let x = snap(a.x - lw / 2.0);
            Rect::from_min_max(pos2(x, a.y), pos2(x + lw, b.y))
        };
        p.rect_filled(r, 0, line_color);
    };
    // Boundary lines lie inside the court's dimensions.
    let inner = lines.shrink(lw / 2.0);
    bar(pos2(lines.left(), inner.top()), pos2(lines.right(), inner.top()));
    bar(pos2(lines.left(), inner.bottom()), pos2(lines.right(), inner.bottom()));
    bar(pos2(inner.left(), lines.top()), pos2(inner.left(), lines.bottom()));
    bar(pos2(inner.right(), lines.top()), pos2(inner.right(), lines.bottom()));
    for y in [0.46, w - 0.46] {
        bar(map(0.0, y), map(l, y));
    }
    for x in [0.76, l - 0.76, l / 2.0 - 1.98, l / 2.0 + 1.98] {
        bar(map(x, 0.0), map(x, w));
    }
    bar(map(0.0, w / 2.0), map(l / 2.0 - 1.98, w / 2.0));
    bar(map(l / 2.0 + 1.98, w / 2.0), map(l, w / 2.0));

    // The net, seen from slightly above and in front: a band of mesh standing
    // between its foot and the white tape along the top, strung between two
    // posts just outside the sidelines, and throwing a soft shadow toward us.
    let foot_y = snap(lines.center().y);
    let rise = snap((lines.height() * 0.04).clamp(10.0, 24.0));
    let (left, right) = (lines.left() - 8.0, lines.right() + 8.0);
    let top_y = foot_y - rise;
    let cast = Shadow { offset: [0, 2], blur: 7, spread: 0, color: Color32::from_black_alpha(80) };
    clipped.add(cast.as_shape(Rect::from_min_max(pos2(left, foot_y), pos2(right, foot_y + rise * 0.45)), CornerRadius::ZERO));
    let band = Rect::from_min_max(pos2(left, top_y), pos2(right, foot_y));
    let (net_id, net_uv) = tiled(&tex.net, band, Vec2::ZERO);
    p.add(RectShape::filled(band, CornerRadius::ZERO, lit(255, 215)).with_texture(net_id, net_uv));
    let tape = Rect::from_min_max(pos2(left, top_y - 2.0), pos2(right, top_y + 2.5));
    p.rect_filled(tape.translate(vec2(0.0, 1.5)), 1, Color32::from_black_alpha(60));
    p.rect_filled(tape, 1, lit(255, 236));
    for x in [left, right] {
        // Post: a weighted base on the floor, the upright, and its cap.
        p.circle_filled(pos2(x, foot_y + 1.5), 6.0, Color32::from_black_alpha(70));
        p.circle_filled(pos2(x, foot_y), 4.5, lit(92, 70));
        let upright = Rect::from_min_max(pos2(x - 2.5, top_y), pos2(x + 2.5, foot_y + 1.0));
        p.rect(upright, 2, lit(214, 176), Stroke::new(1.0, lit(120, 84)), StrokeKind::Inside);
        p.circle(pos2(x, top_y), 4.5, lit(250, 228), Stroke::new(1.0, lit(128, 92)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_have_texture() {
        for img in [hardwood(256, 112), court_mat(64, 64), net_mesh(16), window_light(64, 64), vignette(32, 32)] {
            let first = img.pixels[0];
            assert!(img.pixels.iter().any(|p| *p != first), "texture is flat");
        }
        let wood = hardwood(256, 112);
        assert_eq!(wood.size, [256, 112]);
    }

    #[test]
    fn hardwood_tiles_without_a_visible_edge() {
        // Each row's boards wrap around the tile, so the last column must be
        // about as close to the first as any neighbouring pair of columns.
        let (w, h) = (512, 56);
        let img = hardwood(w, h);
        let lum = |x: usize, y: usize| {
            let p = img.pixels[y * w + x];
            p.r() as i32 + p.g() as i32 + p.b() as i32
        };
        let wrap: i32 = (0..h).map(|y| (lum(w - 1, y) - lum(0, y)).abs()).sum();
        let inner: i32 = (0..h).map(|y| (lum(200, y) - lum(201, y)).abs()).sum();
        assert!(wrap < inner * 4 + 60 * h as i32, "seam at the tile edge: {wrap} vs {inner}");
    }

    #[test]
    fn light_is_purely_additive() {
        assert!(window_light(64, 64).pixels.iter().all(|p| p.a() == 0));
    }
}
