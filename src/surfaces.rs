//! The procedural maple floor of the hall: generated once and sampled by the
//! ray-caster in `hall.rs`.

use egui::{Color32, ColorImage};

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

/// Smooth value noise in [0, 1) that repeats every `px` × `py` lattice cells.
pub fn vnoise(x: f32, y: f32, px: u32, py: u32, seed: u32) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_have_texture() {
        let wood = hardwood(256, 112);
        let first = wood.pixels[0];
        assert!(wood.pixels.iter().any(|p| *p != first), "texture is flat");
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
}
