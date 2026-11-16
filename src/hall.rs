//! The courts as a lit 3D sports hall.
//!
//! Each court is seen from high behind one baseline: maple floor, the blue
//! mat and its lines, the net, a back wall with floodlights, pools of light
//! from the lamps overhead and their reflections in the varnish.
//!
//! The still part of that picture is ray-cast in software, once per panel
//! size and theme, on a worker thread, and kept as a texture — so a court
//! costs one textured rectangle per frame. Only the moving things (the
//! shuttle and the shimmer of the lights) are drawn live, projected through
//! the same [`Camera`].
//!
//! World space is in metres: `x` across the court, `y` up, `z` along its
//! length away from the viewer, with the net at `z = 0`.

use std::sync::{Arc, OnceLock, mpsc};

use egui::epaint::RectShape;
use egui::{Color32, ColorImage, CornerRadius, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2, pos2, vec2};

use crate::surfaces;

type V3 = [f32; 3];

/// Half the court's width and length.
pub const HALF_W: f32 = 3.05;
pub const HALF_L: f32 = 6.7;
/// The blue mat reaches a little past the lines.
const MAT_X: f32 = HALF_W + 0.8;
const MAT_Z: f32 = HALF_L + 1.1;
/// The back wall.
const WALL_Z: f32 = 10.6;
/// The net posts stand just outside the doubles sidelines.
const POST_X: f32 = HALF_W + 0.1;
/// Floodlights on the back wall (x positions).
const FLOODS: [f32; 3] = [-2.5, 0.0, 2.5];
/// Lamps high overhead, out of sight: (x, z) of the pool each throws on the floor.
const LAMPS: [(f32, f32); 16] = [
    (-5.2, -6.4), (-1.75, -6.4), (1.75, -6.4), (5.2, -6.4),
    (-5.2, -1.6), (-1.75, -1.6), (1.75, -1.6), (5.2, -1.6),
    (-5.2, 3.2), (-1.75, 3.2), (1.75, 3.2), (5.2, 3.2),
    (-5.2, 8.0), (-1.75, 8.0), (1.75, 8.0), (5.2, 8.0),
];
const LAMP_Y: f32 = 9.5;
/// How far behind the near baseline the viewer sits.
const CAM_BACK: f32 = 10.0;
/// How steeply the camera looks down at the court (radians from level).
const PITCH: f32 = 0.92;
/// How much wider than true proportions the picture may be drawn, so the
/// court fills its panel.
const STRETCH: f32 = 1.6;
/// Half the near baseline, as a fraction of the panel's width (at most).
const BASE_FRAC: f32 = 0.43;
/// Name cards' size relative to filling the court's height.
const CARD_SCALE: f32 = 0.74;

// ===========================================================================
// Camera and layout

/// A pinhole camera on the court's centre line, pitched down, with separate
/// horizontal and vertical scales. Screen positions are relative to the
/// panel's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    h: f32,
    zc: f32,
    sin: f32,
    cos: f32,
    fx: f32,
    fy: f32,
    cx: f32,
    cy: f32,
}

impl Camera {
    /// Screen position and depth of a world point.
    pub fn project(&self, p: V3) -> (Pos2, f32) {
        let (ry, rz) = (p[1] - self.h, p[2] - self.zc);
        let depth = (rz * self.cos - ry * self.sin).max(0.01);
        let up = ry * self.cos + rz * self.sin;
        (pos2(self.cx + self.fx * p[0] / depth, self.cy - self.fy * up / depth), depth)
    }

    /// Direction of the ray through a screen position (one unit of depth long).
    fn dir(&self, sx: f32, sy: f32) -> V3 {
        let (dx, dy) = ((sx - self.cx) / self.fx, (self.cy - sy) / self.fy);
        [dx, dy * self.cos - self.sin, dy * self.sin + self.cos]
    }

    /// The height at which something standing at `z` appears at screen row `sy`.
    fn height_at(&self, z: f32, sy: f32) -> f32 {
        let (v, rz) = ((self.cy - sy) / self.fy, z - self.zc);
        self.h + rz * (v * self.cos - self.sin) / (self.cos + v * self.sin)
    }

    /// The same camera for a picture `k` times the size.
    fn scaled(&self, k: f32) -> Self {
        Self { fx: self.fx * k, fy: self.fy * k, cx: self.cx * k, cy: self.cy * k, ..*self }
    }

    /// Points per metre (across, upright) for something `depth` away.
    pub fn scale(&self, depth: f32) -> Vec2 {
        vec2(self.fx / depth, self.fy * self.cos / depth)
    }
}

/// How one court panel is laid out: where the four name cards go, and the
/// camera that puts the net in the gap between the two teams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub cam: Camera,
    /// Card rectangles, far team first (relative to the panel's corner).
    pub cards: [Rect; 4],
    /// The band between the teams, where the net shows.
    pub window: Rect,
    /// Height of the strip of wall above the cards (the court's sign lives there).
    pub top: f32,
    net_h: f32,
    flood_y: f32,
}

impl View {
    pub fn new(size: Vec2) -> Self {
        let (w, h) = (size.x.max(40.0), size.y.max(40.0));
        let top = (h * 0.2).min(if h >= 300.0 { 42.0 } else { 38.0 });

        // A high seat looking steeply down, framed so the whole court is in
        // view: the foot of the back wall runs along the bottom of the sign
        // strip, and a strip of wood past the near end of the mat along the
        // bottom edge. The court is
        // then only as wide as honest proportions (and the panel) allow.
        let zc = -HALF_L - CAM_BACK;
        let cam_h = PITCH.tan() * (1.0 - zc);
        let (sin, cos) = PITCH.sin_cos();
        let mut cam = Camera { h: cam_h, zc, sin, cos, fx: 1.0, fy: 1.0, cx: 0.0, cy: 0.0 };
        let (wall, _) = cam.project([0.0, 0.0, WALL_Z]);
        let (edge, depth) = cam.project([0.0, 0.0, -(MAT_Z + 1.0)]);
        cam.fy = (h - top) / (edge.y - wall.y);
        cam.cy = h - cam.fy * edge.y;
        cam.fx = ((w / 2.0 - 8.0) * depth / MAT_X).min(cam.fy * STRETCH);
        cam.cx = w / 2.0;

        // The name cards: one team between the sign strip and the net's tape,
        // the other between the net's foot and the near baseline.
        let net_h = 1.55;
        let (foot, _) = cam.project([0.0, 0.0, 0.0]);
        let (tape, _) = cam.project([0.0, net_h, 0.0]);
        let (base, _) = cam.project([0.0, 0.0, -HALF_L]);
        let base_y = base.y;
        let gap = 6.0;
        let far = (top + 4.0, tape.y - 4.0);
        let near = (foot.y + 4.0, base_y - 4.0);
        let room = (far.1 - far.0).min(near.1 - near.0);
        let chip_h = (((room - gap) / 2.0) * CARD_SCALE).floor().max(6.0);
        let chip_w = ((BASE_FRAC * w).min(0.82 * h) * 1.68 * CARD_SCALE).min(w - 24.0);
        let card = |y: f32| Rect::from_min_size(pos2((w - chip_w) / 2.0, y), vec2(chip_w, chip_h));
        let pair = |(a, b): (f32, f32)| {
            let y = ((a + b) / 2.0 - chip_h - gap / 2.0).round();
            [card(y), card(y + chip_h + gap)]
        };
        let [c0, c1] = pair(far);
        let [c2, c3] = pair(near);
        let cards = [c0, c1, c2, c3];
        let window = Rect::from_min_max(pos2(0.0, cards[1].bottom()), pos2(w, cards[2].top()));
        let flood_y = cam.height_at(WALL_Z, top * 0.45).max(0.4);
        Self { cam, cards, window, top, net_h, flood_y }
    }

    /// Where the floodlights appear, and how big (in points).
    fn floods(&self) -> impl Iterator<Item = (Pos2, Vec2)> + '_ {
        FLOODS.iter().map(|x| {
            let (p, depth) = self.cam.project([*x, self.flood_y, WALL_Z]);
            (p, self.cam.scale(depth))
        })
    }
}

// ===========================================================================
// The still picture

/// The maple floor, as a mip chain the ray-caster can sample smoothly.
struct Wood {
    levels: Vec<(usize, usize, Vec<V3>)>,
}

/// Texels of the wood texture per metre of floor (a board is 14 texels).
const TEXELS_PER_M: f32 = 14.0 / 0.13;

impl Wood {
    fn new() -> Self {
        let img = surfaces::hardwood(1024, 504);
        let [mut w, mut h] = img.size;
        let mut px: Vec<V3> = img.pixels.iter().map(|c| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]).collect();
        let mut levels = Vec::new();
        loop {
            levels.push((w, h, px.clone()));
            if w % 2 != 0 || h % 2 != 0 || h < 64 {
                break;
            }
            let (nw, nh) = (w / 2, h / 2);
            let mut next = Vec::with_capacity(nw * nh);
            for y in 0..nh {
                for x in 0..nw {
                    let at = |dx: usize, dy: usize| px[(y * 2 + dy) * w + x * 2 + dx];
                    let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
                    next.push([0, 1, 2].map(|k| (a[k] + b[k] + c[k] + d[k]) / 4.0));
                }
            }
            (w, h, px) = (nw, nh, next);
        }
        Self { levels }
    }

    fn bilinear(&self, level: usize, u: f32, v: f32) -> V3 {
        let (w, h, px) = &self.levels[level];
        let k = (1u32 << level) as f32;
        let (x, y) = (u / k - 0.5, v / k - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |dx: i64, dy: i64| {
            let xi = (x0 as i64 + dx).rem_euclid(*w as i64) as usize;
            let yi = (y0 as i64 + dy).rem_euclid(*h as i64) as usize;
            px[yi * w + xi]
        };
        let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
        [0, 1, 2].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// The floor's colour at texel (u, v), averaged over a footprint of
    /// `fu` × `fv` texels (boards run along `u`).
    fn sample(&self, u: f32, v: f32, fu: f32, fv: f32) -> V3 {
        let max = (self.levels.len() - 1) as f32;
        let lod = fv.max(1.0).log2().min(max);
        let l0 = lod.floor() as usize;
        let l1 = (l0 + 1).min(self.levels.len() - 1);
        let f = lod - l0 as f32;
        let taps = (fu / (1u32 << l0) as f32 / 2.0).ceil().clamp(1.0, 4.0) as usize;
        let mut sum = [0.0; 3];
        for k in 0..taps {
            let du = ((k as f32 + 0.5) / taps as f32 - 0.5) * fu;
            let (a, b) = (self.bilinear(l0, u + du, v), self.bilinear(l1, u + du, v));
            for c in 0..3 {
                sum[c] += a[c] + (b[c] - a[c]) * f;
            }
        }
        sum.map(|c| c / taps as f32)
    }
}

fn mix(a: V3, b: V3, t: f32) -> V3 {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a pixel `fp` wide lies within `half` of zero, at offset `c`.
fn stripe(c: f32, half: f32, fp: f32) -> f32 {
    ((half - c.abs()) / fp + 0.5).clamp(0.0, 1.0)
}

/// Coverage of the span `lo..hi` by a pixel `fp` wide at `v`.
fn band(v: f32, lo: f32, hi: f32, fp: f32) -> f32 {
    ((v - lo) / fp + 0.5).clamp(0.0, 1.0) * ((hi - v) / fp + 0.5).clamp(0.0, 1.0)
}

fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// How the hall is lit, per theme.
struct Mood {
    /// Light everywhere, the extra under each overhead lamp, and how much
    /// of that the lamps off to the sides give.
    ambient: f32,
    pool: f32,
    side: f32,
    /// Strength of the lights' reflections in the floor.
    gloss: f32,
    /// Haze in the lamps' beams.
    haze: f32,
    wall: V3,
    wall_ambient: f32,
    wash: f32,
    flood: f32,
    vignette: f32,
}

impl Mood {
    fn new(dark: bool) -> Self {
        if dark {
            Self { ambient: 0.17, pool: 0.6, side: 0.3, gloss: 1.0, haze: 0.13, wall: [0.075, 0.125, 0.19], wall_ambient: 0.34, wash: 1.25, flood: 0.85, vignette: 0.22 }
        } else {
            Self { ambient: 0.56, pool: 0.44, side: 0.85, gloss: 0.8, haze: 0.06, wall: [0.14, 0.31, 0.34], wall_ambient: 0.62, wash: 0.95, flood: 1.0, vignette: 0.14 }
        }
    }
}

const WARM: V3 = [1.0, 0.95, 0.84];

struct Scene<'a> {
    cam: Camera,
    net_h: f32,
    flood_y: f32,
    /// Height of the wall at the top of the picture.
    wall_top: f32,
    mood: Mood,
    wood: &'a Wood,
    size: (usize, usize),
}

impl Scene<'_> {
    fn pixel(&self, sx: f32, sy: f32) -> V3 {
        let cam = &self.cam;
        let d = cam.dir(sx, sy);
        let floor_t = if d[1] < -1e-5 { cam.h / -d[1] } else { f32::INFINITY };
        let (mut col, hit_t);
        if cam.zc + floor_t * d[2] <= WALL_Z {
            hit_t = floor_t;
            let z = cam.zc + floor_t * d[2];
            // How much floor one pixel covers, across and in depth.
            let below = cam.dir(sx, sy + 1.0);
            let fpz = (z - (cam.zc + cam.h / -below[1] * below[2])).abs().max(1e-4);
            col = self.floor(floor_t * d[0], z, floor_t / cam.fx, fpz, d);
        } else {
            hit_t = (WALL_Z - cam.zc) / d[2];
            col = self.wall(hit_t * d[0], cam.h + hit_t * d[1], hit_t / cam.fx, hit_t / cam.fy);
        }
        self.haze(&mut col, d, hit_t);
        let net_t = -cam.zc / d[2];
        if net_t < hit_t {
            self.net(&mut col, net_t * d[0], cam.h + net_t * d[1], net_t / cam.fx, net_t / cam.fy);
        }
        // Darker toward the corners, like a lens.
        let (u, v) = (sx / self.size.0 as f32 - 0.5, sy / self.size.1 as f32 - 0.5);
        let vig = 1.0 - self.mood.vignette * ((u * u + v * v) * 2.0).min(1.0);
        // A soft shoulder instead of clipping the highlights.
        col.map(|c| {
            let c = c * vig;
            if c <= 0.7 { c } else { 0.7 + 0.3 * (1.0 - (-(c - 0.7) / 0.3).exp()) }
        })
    }

    fn floor(&self, x: f32, z: f32, fpx: f32, fpz: f32, d: V3) -> V3 {
        let m = &self.mood;
        // Maple boards running the length of the hall.
        let k = TEXELS_PER_M;
        let mut wood = self.wood.sample(z * k + 300.0, x * k + 97.0, fpz * k, fpx * k);
        // Lines of other sports under the varnish, running on beneath the mat.
        let old = [
            (stripe(z + 4.35, 0.04, fpz), [0.62, 0.2, 0.17]),
            (stripe(z - 4.9, 0.04, fpz), [0.14, 0.26, 0.5]),
            (stripe(x - 5.15, 0.04, fpx), [0.14, 0.26, 0.5]),
            (stripe(x + 5.7, 0.04, fpx), [0.62, 0.2, 0.17]),
        ];
        for (cov, paint) in old {
            wood = mix(wood, paint, cov * 0.42);
        }
        // The mat lies on the floor and shades the wood along its edge.
        let on_mat = stripe(x, MAT_X, fpx) * stripe(z, MAT_Z, fpz);
        let outside = (x.abs() - MAT_X).max(z.abs() - MAT_Z).max(0.0);
        wood = wood.map(|c| c * (1.0 - 0.32 * (-outside / 0.09).exp()));
        let mottle = surfaces::vnoise(x * 1.3 + 64.0, z * 1.3 + 64.0, 4096, 4096, 41) - 0.5;
        let seam = stripe(x.abs() - 1.93, 0.012, fpx).max(stripe(x, 0.012, fpx)) * 0.16;
        let shade = (1.0 + mottle * 0.07) * (1.0 - seam);
        let mat = [0.105 * shade, 0.3 * shade, 0.475 * shade];

        // Court markings, drawn a little bolder than life so they read on a TV.
        let (lw, cw) = (0.036, 0.05);
        let long = stripe(z, HALF_L, fpz);
        let wide = stripe(x, HALF_W, fpx);
        let lines = [
            stripe(x.abs() - (HALF_W - lw), lw, fpx) * long,
            stripe(x.abs() - (HALF_W - 0.46), lw, fpx) * long,
            stripe(x, lw, fpx) * stripe(z.abs() - (HALF_L + 1.98) / 2.0, (HALF_L - 1.98) / 2.0, fpz),
            stripe(z.abs() - (HALF_L - cw), cw, fpz) * wide,
            stripe(z.abs() - (HALF_L - 0.76), cw, fpz) * wide,
            stripe(z.abs() - 1.98, cw, fpz) * wide,
        ]
        .into_iter()
        .fold(0.0, f32::max);
        let albedo = mix(mix(wood, mat, on_mat), [0.95, 0.95, 0.93], lines);

        // Pools of light under the lamps.
        let mut light = m.ambient;
        for (lx, lz) in LAMPS {
            let r2 = (x - lx) * (x - lx) + (z - lz) * (z - lz);
            let power = if lx.abs() < HALF_W { m.pool } else { m.pool * m.side };
            light += power * (-r2 / (2.0 * 1.6 * 1.6)).exp();
        }
        // The net's soft shadow, and the gloom where floor meets wall.
        let under_net = (-(z * z) / (0.3 * 0.3)).exp() * stripe(x, POST_X, 0.3);
        light *= 1.0 - 0.2 * under_net;
        light *= 0.72 + 0.28 * smooth(0.0, 1.6, WALL_Z - z);

        // The lights mirrored in the varnish: long streaks toward the viewer.
        // The matte mat only smears them.
        let rough = 1.0 + 1.1 * on_mat;
        let (r_az, r_el) = (d[0] / d[2], -d[1] / d[0].hypot(d[2]));
        let mut gleam = 0.0;
        let mut mirror = |l: V3, power: f32, sa: f32, se: f32| {
            let (lx, ly, lz) = (l[0] - x, l[1], l[2] - z);
            if lz > 0.1 {
                let a = (lx / lz - r_az) / (sa * rough);
                let e = (ly / lx.hypot(lz) - r_el) / (se * rough);
                gleam += power * (-(a * a + e * e)).exp();
            }
        };
        for fx in FLOODS {
            mirror([fx, self.flood_y, WALL_Z], m.flood, 0.026, 0.12);
        }
        for (lx, lz) in LAMPS {
            mirror([lx, LAMP_Y, lz], 0.5, 0.03, 0.1);
        }
        // The lit wall itself shows faintly in the far floor.
        let wall_glow = 0.1 * m.wash * smooth(0.25, 0.6, r_el) * (1.0 - smooth(0.6, 1.0, r_el));
        let grazing = 1.0 - (-d[1] / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()).clamp(0.0, 1.0);
        let fresnel = 0.3 + 0.7 * grazing * grazing;
        let shine = (gleam + wall_glow) * fresnel * m.gloss * (1.0 - 0.55 * on_mat);
        [0, 1, 2].map(|k| albedo[k] * light + shine * WARM[k])
    }

    fn wall(&self, x: f32, y: f32, fpx: f32, fpy: f32) -> V3 {
        let m = &self.mood;
        // Padded panels with a seam between them, a kickboard and a rail.
        let panel = 1.7;
        let seam = stripe((x / panel - (x / panel).round()) * panel, 0.014, fpx);
        let mut albedo = m.wall.map(|c| c * (1.0 - 0.3 * seam));
        let rail = band(y, 1.05, 1.17, fpy);
        albedo = mix(albedo, m.wall.map(|c| c * 1.5 + 0.03), rail);
        albedo = mix(albedo, [0.05, 0.055, 0.06], band(y, -1.0, 0.16, fpy));

        // Wall-washers above the top of the picture throw scallops of light down it.
        let mut light = m.wall_ambient * (0.6 + 0.4 * smooth(0.0, 1.4, y));
        let (off, wash_y) = (1.5_f32, self.wall_top + 1.3);
        for k in -3..=3 {
            let (lx, ly) = (k as f32 * 3.4 - x, wash_y - y);
            let d2 = lx * lx + ly * ly + off * off;
            let down = (ly / d2.sqrt()).max(0.0);
            light += m.wash * 9.5 * off / d2.sqrt() * down.powi(4) / d2;
        }
        let mut col = albedo.map(|c| c * light);

        // The floodlights: a housing, three bright cells, and their glare.
        for fx in FLOODS {
            let (dx, dy) = (x - fx, y - self.flood_y);
            let housing = stripe(dx, 0.6, fpx) * stripe(dy, 0.2, fpy);
            col = mix(col, [0.03, 0.035, 0.04], housing);
            for cell in [-0.38, 0.0, 0.38] {
                let lit = stripe(dx - cell, 0.155, fpx) * stripe(dy, 0.13, fpy);
                col = mix(col, [2.4, 2.3, 2.0], lit);
            }
            let r2 = dx * dx * 0.55 + dy * dy;
            let glare = m.flood * (0.75 * (-r2 / 0.22).exp() + 0.2 * (-r2 / 2.4).exp());
            for k in 0..3 {
                col[k] += glare * WARM[k];
            }
        }
        col
    }

    /// Light caught in the air under each overhead lamp.
    fn haze(&self, col: &mut V3, d: V3, hit_t: f32) {
        let cam = &self.cam;
        let dh2 = d[0] * d[0] + d[2] * d[2];
        let mut glow = 0.0;
        for (lx, lz) in LAMPS {
            // Where this ray passes closest to the lamp's axis.
            let t = (lx * d[0] + (lz - cam.zc) * d[2]) / dh2;
            let y = cam.h + t * d[1];
            if t <= 0.0 || y > LAMP_Y {
                continue;
            }
            let (px, pz) = (t * d[0] - lx, cam.zc + t * d[2] - lz);
            let r = 0.3 + (LAMP_Y - y) * 0.2;
            let through = ((hit_t - t) / r + 0.5).clamp(0.0, 1.0);
            glow += (-(px * px + pz * pz) / (r * r)).exp() / r * through * smooth(-0.5, 2.5, y);
        }
        for k in 0..3 {
            col[k] += glow * self.mood.haze * WARM[k];
        }
    }

    fn net(&self, col: &mut V3, x: f32, y: f32, fpx: f32, fpy: f32) {
        let top = self.net_h;
        if x.abs() > POST_X + 0.3 || y > top + 0.3 || y < -0.1 {
            return;
        }
        let lit = (self.mood.ambient + self.mood.pool).min(1.0);
        let (tape, depth) = (0.075 * top, 0.5 * top);
        // The mesh: a faint veil, and cords about a pixel thick.
        let mesh = stripe(x, POST_X, fpx) * band(y, top - depth, top - tape, fpy);
        if mesh > 0.0 {
            let cell = |v: f32, size: f32, fp: f32| {
                let c = (v / size - (v / size).round()).abs() * size;
                (1.0 - c / fp).clamp(0.0, 1.0)
            };
            let cords = cell(x, (5.0 * fpx).max(0.11), fpx).max(cell(y - top, (5.0 * fpy).max(0.11), fpy));
            let edge = band(y, top - depth - fpy, top - depth + 1.5 * fpy, fpy);
            *col = col.map(|c| c * (1.0 - 0.2 * mesh));
            *col = mix(*col, [0.8 * lit, 0.8 * lit, 0.78 * lit], (0.42 * cords).max(0.8 * edge) * mesh);
        }
        // The white tape along the top, with a shaded underside.
        let band_y = (y - (top - tape)) / tape;
        let white = 0.82 + 0.16 * band_y.clamp(0.0, 1.0);
        *col = mix(*col, [white * lit, white * lit, 0.98 * white * lit], stripe(x, POST_X, fpx) * band(y, top - tape, top, fpy));
        // The posts: round steel, lit from above.
        for side in [-1.0, 1.0] {
            let off = (x - side * POST_X) / 0.06;
            let cov = stripe(x - side * POST_X, 0.06, fpx) * band(y, 0.0, top + 0.05, fpy);
            let steel = (0.42 + 0.5 * (1.0 - (off + 0.3) * (off + 0.3)).max(0.0)) * lit;
            *col = mix(*col, [steel, steel, steel * 1.04], cov);
            let foot = stripe(x - side * POST_X, 0.2, fpx) * band(y, -0.03, 0.06, fpy);
            *col = mix(*col, [0.16 * lit, 0.17 * lit, 0.19 * lit], foot);
        }
    }
}

/// Ray-cast the hall into a `w` × `h` picture.
fn render(w: usize, h: usize, view: &View, dark: bool, wood: &Wood) -> ColorImage {
    let cam = view.cam.scaled(w as f32 / view_width(view));
    let scene = Scene {
        cam,
        net_h: view.net_h,
        flood_y: view.flood_y,
        wall_top: cam.height_at(WALL_Z, 0.0),
        mood: Mood::new(dark),
        wood,
        size: (w, h),
    };
    let mut pixels = vec![Color32::BLACK; w * h];
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8);
    let rows = h.div_ceil(threads).max(1);
    let scene = &scene;
    std::thread::scope(|s| {
        for (k, chunk) in pixels.chunks_mut(rows * w).enumerate() {
            s.spawn(move || {
                for (j, row) in chunk.chunks_mut(w).enumerate() {
                    let y = k * rows + j;
                    for (x, px) in row.iter_mut().enumerate() {
                        let c = scene.pixel(x as f32 + 0.5, y as f32 + 0.5);
                        // Dithered, so the dark wall's gradients don't band.
                        let n = hash(x as u32, y as u32) - 0.5;
                        let q = |v: f32| (v * 255.0 + n).round().clamp(0.0, 255.0) as u8;
                        *px = Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]));
                    }
                }
            });
        }
    });
    ColorImage::new([w, h], pixels)
}

/// The width (in points) a view was laid out for.
fn view_width(view: &View) -> f32 {
    view.window.width()
}

// ===========================================================================
// The cached picture, and what moves in front of it

#[derive(Clone, Copy, PartialEq, Debug)]
struct Key {
    w: u32,
    h: u32,
    dark: bool,
}

pub struct Hall {
    wood: Arc<OnceLock<Wood>>,
    /// A soft round blob, for glows (added) and shadows (tinted black).
    blob: TextureHandle,
    cache: Vec<(Key, TextureHandle, f64)>,
    job: Option<(Key, mpsc::Receiver<ColorImage>)>,
    /// Sizes asked for but not painted yet: key, first asked, last asked.
    pending: Vec<(Key, f64, f64)>,
    missed_at: f64,
}

/// Pictures kept, so going between the day's few layouts never re-renders.
const CACHE: usize = 6;
/// The largest picture rendered (bigger panels stretch it).
const MAX_PIXELS: f32 = 1.6e6;

impl Hall {
    pub fn load(ctx: &egui::Context) -> Self {
        const N: usize = 64;
        let pixels = (0..N * N)
            .map(|i| {
                let (x, y) = ((i % N) as f32 + 0.5, (i / N) as f32 + 0.5);
                let r2 = ((x / N as f32 - 0.5).powi(2) + (y / N as f32 - 0.5).powi(2)) * 4.0;
                let v = ((-r2 * 4.0).exp() - (-4.0_f32).exp()).max(0.0);
                let v = (v * 255.0) as u8;
                Color32::from_rgba_premultiplied(v, v, v, v)
            })
            .collect();
        let blob = ctx.load_texture("hall-blob", ColorImage::new([N, N], pixels), egui::TextureOptions::LINEAR);
        Self { wood: Arc::new(OnceLock::new()), blob, cache: Vec::new(), job: None, pending: Vec::new(), missed_at: 0.0 }
    }

    /// True once every court painted lately had its finished picture.
    #[cfg(debug_assertions)]
    pub fn settled(&self, now: f64) -> bool {
        self.job.is_none() && !self.cache.is_empty() && now - self.missed_at > 0.3
    }

    /// Paint the hall into `rect` (which should be pixel-aligned) and return
    /// its layout.
    pub fn paint(&mut self, p: &egui::Painter, rect: Rect, dark: bool, radius: u8) -> View {
        let ctx = p.ctx();
        let now = ctx.input(|i| i.time);
        let view = View::new(rect.size());
        let ppp = ctx.pixels_per_point();
        let k = (MAX_PIXELS / (rect.area() * ppp * ppp).max(1.0)).sqrt().min(1.0) * ppp;
        let key = Key { w: (rect.width() * k).round().max(8.0) as u32, h: (rect.height() * k).round().max(8.0) as u32, dark };

        if let Some((job_key, rx)) = &self.job {
            match rx.try_recv() {
                Ok(img) => {
                    let tex = ctx.load_texture("hall", img, egui::TextureOptions::LINEAR);
                    self.cache.push((*job_key, tex, now));
                    if self.cache.len() > CACHE {
                        let oldest = (0..self.cache.len()).min_by(|a, b| self.cache[*a].2.total_cmp(&self.cache[*b].2)).unwrap_or(0);
                        drop(self.cache.remove(oldest));
                    }
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        let hit = self.cache.iter_mut().find(|e| e.0 == key).map(|e| {
            e.2 = now;
            e.1.id()
        });
        if hit.is_none() {
            self.missed_at = now;
            self.request(ctx, key, view, now);
        }
        // Until the right picture is ready, stretch the nearest one.
        let shown = hit.or_else(|| {
            let score = |e: &&(Key, TextureHandle, f64)| (e.0.dark != dark) as u32 * 100_000 + e.0.w.abs_diff(key.w) + e.0.h.abs_diff(key.h);
            self.cache.iter().min_by_key(score).map(|e| e.1.id())
        });
        let corners = CornerRadius::same(radius);
        match shown {
            Some(id) => {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                p.add(RectShape::filled(rect, corners, Color32::WHITE).with_texture(id, uv));
            }
            None => {
                let c = Mood::new(dark).wall.map(|c| (c * 255.0 * 0.6) as u8);
                p.rect_filled(rect, corners, Color32::from_rgb(c[0], c[1], c[2]));
            }
        }
        view
    }

    fn request(&mut self, ctx: &egui::Context, key: Key, view: View, now: f64) {
        let first = match self.pending.iter_mut().find(|e| e.0 == key) {
            Some(e) => {
                e.2 = now;
                e.1
            }
            None => {
                self.pending.push((key, now, now));
                now
            }
        };
        self.pending.retain(|e| now - e.2 < 0.5);
        // While the window is being resized, wait for the size to hold still
        // (a change of theme has nothing to wait for).
        if self.job.is_some() || (now - first < 0.12 && self.cache.iter().any(|e| e.0.dark == key.dark)) {
            return;
        }
        self.pending.retain(|e| e.0 != key);
        let (tx, rx) = mpsc::channel();
        let (wood, ctx) = (self.wood.clone(), ctx.clone());
        std::thread::spawn(move || {
            let wood = wood.get_or_init(Wood::new);
            let _ = tx.send(render(key.w as usize, key.h as usize, &view, key.dark, wood));
            ctx.request_repaint();
        });
        self.job = Some((key, rx));
    }

    fn blob(&self, mesh: &mut egui::Mesh, c: Pos2, size: Vec2, color: Color32) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        mesh.add_rect_with_uv(Rect::from_center_size(c, size), uv, color);
    }

    /// The living part of the picture: the floodlights' shimmer and, while a
    /// game is on, a shuttle in play. `t` is in seconds.
    pub fn paint_live(&self, p: &egui::Painter, rect: Rect, view: &View, court: usize, t: f64, dark: bool, rally: bool) {
        let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
        let origin = rect.min.to_vec2();
        let mut glow = egui::Mesh::with_texture(self.blob.id());
        for (k, (c, scale)) in view.floods().enumerate() {
            // Each lamp breathes on its own slow, slightly irregular beat.
            let phase = court as f64 * 1.7 + k as f64 * 2.3;
            let beat = (t * 0.83 + phase).sin() * 0.6 + (t * 2.9 + phase * 1.3).sin() * 0.4;
            let a = (if dark { 0.2 } else { 0.15 }) * (0.55 + 0.45 * beat as f32);
            let tint = Color32::from_rgba_premultiplied((255.0 * a) as u8, (240.0 * a) as u8, (205.0 * a) as u8, 0);
            self.blob(&mut glow, c + origin, vec2(scale.x * 4.6, scale.y * 3.6), tint);
        }
        if !rally {
            p.add(Shape::mesh(glow));
            return;
        }
        let (at, ahead) = rally_at(court, t);
        let (c, depth) = view.cam.project(at);
        let c = c + origin;
        let (tip, _) = view.cam.project(ahead);
        let scale = view.cam.scale(depth);
        // Its shadow on the floor, fainter the higher it flies.
        let (ground, gd) = view.cam.project([at[0], 0.0, at[2]]);
        let gs = view.cam.scale(gd);
        let fade = 1.0 / (1.0 + at[1] * 0.45);
        self.blob(&mut glow, ground + origin, vec2(gs.x * 0.75, gs.y * 0.4).max(vec2(5.0, 3.0)), Color32::from_black_alpha((120.0 * fade) as u8));
        // A touch of bloom: it's the brightest thing in the air.
        self.blob(&mut glow, c, Vec2::splat(scale.x * 0.9), Color32::from_rgba_premultiplied(46, 44, 38, 0));
        p.add(Shape::mesh(glow));

        // The shuttle flies cork first, skirt trailing.
        let size = (scale.x * 0.3).clamp(6.0, 20.0);
        let fwd = ((tip + origin) - c).normalized();
        let fwd = if fwd.is_finite() && fwd != Vec2::ZERO { fwd } else { vec2(0.0, -1.0) };
        let side = fwd.rot90();
        let shade = if dark { 0.9 } else { 1.0 };
        let white = Color32::from_gray((252.0 * shade) as u8);
        let quill = Color32::from_gray((150.0 * shade) as u8);
        let pt = |along: f32, across: f32| c - fwd * (along * size) + side * (across * size);
        let skirt = vec![pt(0.1, -0.16), pt(0.1, 0.16), pt(0.95, 0.42), pt(0.95, -0.42)];
        p.add(Shape::convex_polygon(skirt, white, Stroke::new(0.75, quill)));
        p.line_segment([pt(0.5, -0.29), pt(0.5, 0.29)], Stroke::new(0.75, quill));
        p.circle(pt(0.0, 0.0), size * 0.2, Color32::from_rgb((244.0 * shade) as u8, (232.0 * shade) as u8, (206.0 * shade) as u8), Stroke::new(0.75, quill));
    }
}

/// Where the shuttle is `t` seconds into a court's (endlessly looping) rally,
/// and a point just ahead of it on its flight.
fn rally_at(court: usize, t: f64) -> (V3, V3) {
    const SHOTS: u32 = 14;
    let seed = court as u32 * 31 + 5;
    // Each shot is struck from one of these points and lands on the next.
    let spot = |k: u32| -> V3 {
        let k = k % SHOTS;
        let side = if k % 2 == 0 { -1.0 } else { 1.0 };
        let x = (hash(seed, k * 4) - 0.5) * 4.6;
        let z = side * (1.3 + hash(seed, k * 4 + 1) * 4.6);
        [x, 0.9 + hash(seed, k * 4 + 2) * 1.5, z]
    };
    let time = |k: u32| 1.25 + 0.75 * hash(seed, k * 4 + 3) as f64;
    let total: f64 = (0..SHOTS).map(time).sum();
    let mut left = (t + court as f64 * 0.77).rem_euclid(total);
    let mut k = 0;
    while k + 1 < SHOTS && left >= time(k) {
        left -= time(k);
        k += 1;
    }
    let (a, b) = (spot(k), spot(k + 1));
    // Lobs, drives and drops: how far above the straight line it arcs,
    // always enough to clear the net.
    let lift = 0.5 + hash(seed, k * 4 + 3).powi(2) * 3.6;
    let cross = (a[2] / (a[2] - b[2])).clamp(0.05, 0.95);
    let need = (2.0 - (a[1] + (b[1] - a[1]) * cross)) / (4.0 * cross * (1.0 - cross));
    let lift = lift.max(need);
    let at = |u: f32| -> V3 {
        // A shuttle leaves the racket fast and slows as it drops.
        let e = 1.0 - (1.0 - u.clamp(0.0, 1.0)).powf(1.6);
        [a[0] + (b[0] - a[0]) * e, a[1] + (b[1] - a[1]) * e + 4.0 * lift * e * (1.0 - e), a[2] + (b[2] - a[2]) * e]
    };
    let u = (left / time(k)) as f32;
    (at(u), at(u + 0.02))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_net_shows_between_the_teams() {
        for size in [vec2(436.0, 390.0), vec2(436.0, 230.0), vec2(300.0, 180.0), vec2(700.0, 520.0), vec2(1380.0, 450.0), vec2(240.0, 90.0)] {
            let v = View::new(size);
            let (foot, _) = v.cam.project([0.0, 0.0, 0.0]);
            let (tape, _) = v.cam.project([0.0, v.net_h, 0.0]);
            assert!(v.window.contains(foot) && v.window.y_range().contains(tape.y), "{size:?}: {foot:?} {tape:?}");
            assert!(tape.y < foot.y);
            // The near baseline lies along the bottom, inside the panel.
            let (base, _) = v.cam.project([HALF_W, 0.0, -HALF_L]);
            assert!(base.y > v.cards[3].bottom() && base.y < size.y && base.x < size.x, "{size:?}: {base:?}");
            // Cards keep their order and stay inside the panel.
            for pair in v.cards.windows(2) {
                assert!(pair[0].bottom() <= pair[1].top());
            }
            assert!(v.cards[0].top() >= v.top && v.cards[3].bottom() <= size.y);
        }
    }

    #[test]
    fn rays_and_projection_agree() {
        let cam = View::new(vec2(436.0, 390.0)).cam;
        for p in [[1.0, 0.0, -3.0], [-2.5, 1.55, 0.0], [4.0, 5.0, WALL_Z]] {
            let (s, depth) = cam.project(p);
            let d = cam.dir(s.x, s.y);
            let back = [d[0] * depth, cam.h + d[1] * depth, cam.zc + d[2] * depth];
            for k in 0..3 {
                assert!((back[k] - p[k]).abs() < 1e-2, "{p:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn the_picture_shows_wood_mat_and_lights() {
        let view = View::new(vec2(120.0, 100.0));
        let wood = Wood::new();
        for dark in [false, true] {
            let img = render(120, 100, &view, dark, &wood);
            assert_eq!(img.size, [120, 100]);
            let at = |x: usize, y: usize| img.pixels[y * 120 + x];
            // Blue mat in the near court (just inside the baseline), warm wood
            // out beside the net and past the near end of the mat.
            let (inside, _) = view.cam.project([0.0, 0.0, -HALF_L + 0.3]);
            let (mat, floor) = (at(60, inside.y as usize), at(2, view.window.bottom() as usize));
            assert!(mat.b() > mat.r(), "dark={dark}: {mat:?}");
            let below = at(60, 99);
            assert!(below.r() > below.b(), "dark={dark}: wood below the mat: {below:?}");
            assert!(floor.r() > floor.b(), "dark={dark}: {floor:?}");
            // The floodlights are the brightest thing in the picture, up on the wall.
            let (p, _) = view.floods().nth(1).unwrap();
            assert!(at(p.x as usize, p.y as usize).r() > 240, "dark={dark}");
        }
    }

    #[test]
    fn the_rally_loops_over_the_net() {
        let mut sides = [false; 2];
        for step in 0..2000 {
            let (p, _) = rally_at(1, step as f64 * 0.05);
            assert!(p[1] > 0.5 && p[1] < 9.0 && p[0].abs() < 3.0 && p[2].abs() < 6.5, "{p:?}");
            if p[2].abs() < 0.15 {
                assert!(p[1] > 1.6, "through the net: {p:?}");
            }
            sides[(p[2] > 0.0) as usize] = true;
        }
        assert!(sides[0] && sides[1]);
    }
}
