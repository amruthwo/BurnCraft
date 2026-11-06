//! Photoshop-style image selections on bitmaps: magic wand and (magnetic) lasso, combined with
//! New / Add / Subtract, then turned into vector outlines.

use crate::geom::*;
use crate::imaging::{trace_pixels, TraceParams};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelMode {
    #[default]
    New,
    Add,
    Subtract,
}

impl SelMode {
    pub const ALL: [SelMode; 3] = [SelMode::New, SelMode::Add, SelMode::Subtract];
    pub fn label(self) -> &'static str {
        match self {
            SelMode::New => "New",
            SelMode::Add => "Add",
            SelMode::Subtract => "Subtract",
        }
    }
}

/// A pixel selection on one bitmap shape.
#[derive(Clone, Debug)]
pub struct Selection {
    pub shape_id: u64,
    pub w: usize,
    pub h: usize,
    pub mask: Vec<bool>,
    /// Outline of the mask in pixel space, for drawing.
    pub outline: Vec<Vec<P2>>,
}

impl Selection {
    /// Redraw the marching ants from the mask, refined the way Selection to Outline will use it.
    pub fn update_outline(&mut self, r: &Refine) {
        self.outline = outlines(&self.mask, self.w, self.h, r);
    }
    pub fn count(&self) -> usize {
        self.mask.iter().filter(|m| **m).count()
    }
}

fn color_dist(a: &[u8], b: &[u8]) -> u32 {
    let d = |i: usize| (a[i] as i32 - b[i] as i32).unsigned_abs();
    d(0).max(d(1)).max(d(2)).max(d(3) / 2)
}

/// Pixels within `tol` (0..255) of the colour at (sx, sy). Contiguous = flood fill, otherwise global.
pub fn magic_wand(rgba: &[u8], w: usize, h: usize, sx: usize, sy: usize, tol: u8, contiguous: bool) -> Vec<bool> {
    let mut mask = vec![false; w * h];
    if sx >= w || sy >= h {
        return mask;
    }
    let seed = &rgba[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4];
    let ok = |i: usize| color_dist(&rgba[i * 4..i * 4 + 4], seed) <= tol as u32;
    if !contiguous {
        for (i, m) in mask.iter_mut().enumerate() {
            *m = ok(i);
        }
        return mask;
    }
    let mut stack = vec![(sx, sy)];
    mask[sy * w + sx] = true;
    while let Some((x, y)) = stack.pop() {
        let mut push = |nx: usize, ny: usize| {
            let i = ny * w + nx;
            if !mask[i] && ok(i) {
                mask[i] = true;
                stack.push((nx, ny));
            }
        };
        if x > 0 {
            push(x - 1, y);
        }
        if x + 1 < w {
            push(x + 1, y);
        }
        if y > 0 {
            push(x, y - 1);
        }
        if y + 1 < h {
            push(x, y + 1);
        }
    }
    mask
}

/// Fill a closed pixel-space polygon into a mask (even-odd).
pub fn polygon_mask(poly: &[P2], w: usize, h: usize) -> Vec<bool> {
    let mut mask = vec![false; w * h];
    if poly.len() < 3 {
        return mask;
    }
    let mut ring = poly.to_vec();
    if !is_closed(&ring) {
        ring.push(ring[0]);
    }
    for y in 0..h {
        let yc = y as f64 + 0.5;
        let mut xs: Vec<f64> = Vec::new();
        for s in ring.windows(2) {
            let (a, b) = (s[0], s[1]);
            if (a.y > yc) != (b.y > yc) {
                xs.push(a.x + (yc - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                continue;
            }
            let x0 = (pair[0] - 0.5).ceil().max(0.0) as usize;
            let x1 = ((pair[1] - 0.5).floor().min(w as f64 - 1.0)).max(-1.0);
            if x1 < 0.0 {
                continue;
            }
            for x in x0..=(x1 as usize) {
                mask[y * w + x] = true;
            }
        }
    }
    mask
}

/// Pull each lasso point onto the strongest nearby edge (a simple "magnetic lasso").
pub fn magnetic(points: &[P2], gray: &[u8], w: usize, h: usize, radius: i32) -> Vec<P2> {
    let g = |x: i32, y: i32| -> f64 { gray[(y.clamp(0, h as i32 - 1) as usize) * w + x.clamp(0, w as i32 - 1) as usize] as f64 };
    let grad = |x: i32, y: i32| -> f64 {
        let gx = g(x + 1, y) - g(x - 1, y);
        let gy = g(x, y + 1) - g(x, y - 1);
        (gx * gx + gy * gy).sqrt()
    };
    points
        .iter()
        .map(|p| {
            let (cx, cy) = (p.x.round() as i32, p.y.round() as i32);
            let mut best = (grad(cx, cy), cx, cy);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    // prefer closer points when edges are similar
                    let v = grad(cx + dx, cy + dy) - ((dx * dx + dy * dy) as f64).sqrt() * 2.0;
                    if v > best.0 {
                        best = (v, cx + dx, cy + dy);
                    }
                }
            }
            if best.0 > 20.0 {
                p2(best.1 as f64 + 0.5, best.2 as f64 + 0.5)
            } else {
                *p
            }
        })
        .collect()
}

pub fn combine(old: Option<&[bool]>, new: Vec<bool>, mode: SelMode) -> Vec<bool> {
    match (old, mode) {
        (Some(o), SelMode::Add) if o.len() == new.len() => o.iter().zip(new).map(|(a, b)| *a || b).collect(),
        (Some(o), SelMode::Subtract) if o.len() == new.len() => o.iter().zip(new).map(|(a, b)| *a && !b).collect(),
        (None, SelMode::Subtract) => vec![false; new.len()],
        _ => new,
    }
}

/// How a raw wand / lasso selection is cleaned up before it is shown and turned into outlines.
/// The raw mask is kept, so these can be changed after selecting (and New / Add / Subtract still
/// work on the raw pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refine {
    /// Smooth edges: blur radius in pixels; the blurred mask is cut again at 50 %, which rounds off
    /// the pixel noise that tolerance leaves along an edge.
    pub smooth: f32,
    /// Grow (+) or shrink (−) the selection, in pixels.
    pub grow: i32,
    /// Drop selected islands and fill holes smaller than this (pixels across).
    pub speckle: f32,
    /// Curve smoothing of the finished outline (0 = straight pixel steps, 4 = very round).
    pub curves: f32,
    /// Optimise: how far points may move when redundant ones are dropped (as in Trace Image).
    pub optimize: f32,
}

impl Default for Refine {
    fn default() -> Self {
        Refine { smooth: 2.0, grow: 0, speckle: 4.0, curves: 2.0, optimize: 1.0 }
    }
}

impl Refine {
    /// Exactly the selected pixels, nothing smoothed.
    pub const RAW: Refine = Refine { smooth: 0.0, grow: 0, speckle: 0.0, curves: 0.0, optimize: 0.5 };
}

/// The selection after Grow / Smooth / Remove specks.
pub fn refine(mask: &[bool], w: usize, h: usize, r: &Refine) -> Vec<bool> {
    let mut m = mask.to_vec();
    if r.grow != 0 {
        m = grow(&m, w, h, r.grow);
    }
    let radius = r.smooth.round().max(0.0) as usize;
    if radius > 0 {
        m = blur_threshold(&m, w, h, radius);
    }
    let min_area = (r.speckle.max(0.0) * r.speckle.max(0.0)).round() as usize;
    if min_area > 1 {
        m = remove_specks(&m, w, h, min_area);
    }
    m
}

/// Outlines around the refined selection (pixel coordinates, y down).
pub fn outlines(mask: &[bool], w: usize, h: usize, r: &Refine) -> Vec<Vec<P2>> {
    let m = refine(mask, w, h, r);
    let gray: Vec<u8> = m.iter().map(|m| if *m { 0 } else { 255 }).collect();
    let params = TraceParams { threshold: 128, cutoff: 0, ignore_less_than: 1.0, smoothness: r.curves as f64, optimize: r.optimize as f64, invert: false, engine: Default::default() };
    trace_pixels(&gray, w, h, &params)
}

/// Chamfer (3-4) distance from every pixel to the nearest `true` pixel, ×3 (so 3 = one pixel).
fn distance_to(target: &[bool], w: usize, h: usize) -> Vec<u32> {
    const FAR: u32 = u32::MAX / 4;
    let mut d: Vec<u32> = target.iter().map(|t| if *t { 0 } else { FAR }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 { v = v.min(d[i - 1] + 3); }
            if y > 0 {
                v = v.min(d[i - w] + 3);
                if x > 0 { v = v.min(d[i - w - 1] + 4); }
                if x + 1 < w { v = v.min(d[i - w + 1] + 4); }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w { v = v.min(d[i + 1] + 3); }
            if y + 1 < h {
                v = v.min(d[i + w] + 3);
                if x + 1 < w { v = v.min(d[i + w + 1] + 4); }
                if x > 0 { v = v.min(d[i + w - 1] + 4); }
            }
            d[i] = v;
        }
    }
    d
}

/// Grow (n > 0) or shrink (n < 0) by about n pixels, with round corners.
pub fn grow(mask: &[bool], w: usize, h: usize, n: i32) -> Vec<bool> {
    let lim = 3 * n.unsigned_abs();
    if n > 0 {
        let d = distance_to(mask, w, h);
        mask.iter().zip(d).map(|(m, d)| *m || d <= lim).collect()
    } else {
        let outside: Vec<bool> = mask.iter().map(|m| !m).collect();
        if !outside.iter().any(|o| *o) {
            return mask.to_vec();
        }
        let d = distance_to(&outside, w, h);
        mask.iter().zip(d).map(|(m, d)| *m && d > lim).collect()
    }
}

/// Box-blur the mask twice (close to a Gaussian) and cut it again at 50 %.
fn blur_threshold(mask: &[bool], w: usize, h: usize, radius: usize) -> Vec<bool> {
    let mut v: Vec<f32> = mask.iter().map(|m| if *m { 1.0 } else { 0.0 }).collect();
    let mut tmp = vec![0.0f32; v.len()];
    // one pass of a box blur along a line of `n` values (`stride` apart), edges repeated
    let pass = |src: &[f32], dst: &mut [f32], start: usize, stride: usize, n: usize, pre: &mut Vec<f32>| {
        pre.clear();
        pre.push(0.0);
        for k in 0..n {
            let last = *pre.last().unwrap();
            pre.push(last + src[start + k * stride]);
        }
        let r = radius as i64;
        let (first, lastv) = (src[start], src[start + (n - 1) * stride]);
        for k in 0..n as i64 {
            let (a, b) = (k - r, k + r);
            let (ca, cb) = (a.max(0) as usize, b.min(n as i64 - 1) as usize);
            let mut sum = pre[cb + 1] - pre[ca];
            if a < 0 { sum += (-a) as f32 * first; }
            if b > n as i64 - 1 { sum += (b - (n as i64 - 1)) as f32 * lastv; }
            dst[start + k as usize * stride] = sum / (2 * r + 1) as f32;
        }
    };
    let mut pre = Vec::new();
    for _ in 0..2 {
        for y in 0..h {
            pass(&v, &mut tmp, y * w, 1, w, &mut pre);
        }
        for x in 0..w {
            pass(&tmp, &mut v, x, w, h, &mut pre);
        }
    }
    v.iter().map(|x| *x >= 0.5).collect()
}

/// Remove selected islands smaller than `min_area` pixels, and fill holes that small
/// (holes touching the image edge are left alone).
fn remove_specks(mask: &[bool], w: usize, h: usize, min_area: usize) -> Vec<bool> {
    let mut out = mask.to_vec();
    let mut seen = vec![false; mask.len()];
    let mut comp = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if seen[start] {
            continue;
        }
        let val = mask[start];
        comp.clear();
        stack.push(start);
        seen[start] = true;
        let mut edge = false;
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % w, i / w);
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                edge = true;
            }
            let mut visit = |j: usize| {
                if !seen[j] && mask[j] == val {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 { visit(i - 1); }
            if x + 1 < w { visit(i + 1); }
            if y > 0 { visit(i - w); }
            if y + 1 < h { visit(i + w); }
        }
        if comp.len() < min_area && (val || !edge) {
            for &i in &comp {
                out[i] = !val;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wand_selects_region() {
        let (w, h) = (10, 10);
        let mut rgba = vec![255u8; w * h * 4];
        for y in 2..6 {
            for x in 2..6 {
                let i = (y * w + x) * 4;
                rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let m = magic_wand(&rgba, w, h, 3, 3, 10, true);
        assert_eq!(m.iter().filter(|b| **b).count(), 16);
        let all_white = magic_wand(&rgba, w, h, 0, 0, 10, false);
        assert_eq!(all_white.iter().filter(|b| **b).count(), 84);
    }
    #[test]
    fn lasso_and_subtract() {
        let sq = vec![p2(0.0, 0.0), p2(8.0, 0.0), p2(8.0, 8.0), p2(0.0, 8.0)];
        let m = polygon_mask(&sq, 10, 10);
        assert_eq!(m.iter().filter(|b| **b).count(), 64);
        let small = polygon_mask(&[p2(0.0, 0.0), p2(4.0, 0.0), p2(4.0, 4.0), p2(0.0, 4.0)], 10, 10);
        let c = combine(Some(&m), small, SelMode::Subtract);
        assert_eq!(c.iter().filter(|b| **b).count(), 48);
    }
    #[test]
    fn refine_smooths_specks_and_grows() {
        // a 20×20 square with a ragged edge, a stray pixel and a pinhole
        let (w, h) = (40, 40);
        let mut m = vec![false; w * h];
        for y in 10..30 {
            for x in 10..30 {
                m[y * w + x] = true;
            }
        }
        for y in (10..30).step_by(3) {
            m[y * w + 30] = true; // teeth on the right edge
        }
        m[2 * w + 2] = true; // speck
        m[20 * w + 20] = false; // pinhole
        let raw = refine(&m, w, h, &Refine::RAW);
        assert_eq!(raw, m);
        let r = refine(&m, w, h, &Refine::default());
        assert!(!r[2 * w + 2], "speck removed");
        assert!(r[20 * w + 20], "pinhole filled");
        // teeth smoothed away: each row's right edge ends in the same column
        let ends: Vec<usize> = (14..26).map(|y| (0..w).rev().find(|x| r[y * w + x]).unwrap()).collect();
        assert!(ends.iter().all(|e| *e == ends[0]), "ragged edge smoothed: {ends:?}");
        // grow by 3 adds about 3 px each side, shrink takes it back
        let g = grow(&raw, w, h, 3);
        assert!(g[20 * w + 7] && !g[20 * w + 6]);
        let s = grow(&g, w, h, -3);
        assert!(s[20 * w + 10] && !s[20 * w + 9]);
        // the refined outline is a single smooth loop
        let o = outlines(&m, w, h, &Refine::default());
        assert_eq!(o.len(), 1);
    }
}
