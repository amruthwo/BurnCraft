//! Image adjustments, dithering and bitmap-to-vector tracing.

use crate::geom::*;
use crate::model::*;

/// Trace defaults, matching LightBurn's Trace Image dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceParams {
    pub threshold: u8,
    pub cutoff: u8,
    pub ignore_less_than: f64,
    pub smoothness: f64,
    pub optimize: f64,
    pub invert: bool,
    pub engine: TraceEngine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TraceEngine {
    /// Follows pixel edges closely (LightBurn-like); good for logos and line art.
    #[default]
    Precise,
    /// VTracer (open source, visioncortex): smooth spline curves; good for photos and hand drawings.
    Smooth,
}

impl TraceEngine {
    pub const ALL: [TraceEngine; 2] = [TraceEngine::Precise, TraceEngine::Smooth];
    pub fn label(self) -> &'static str {
        match self {
            TraceEngine::Precise => "Precise edges",
            TraceEngine::Smooth => "Smooth curves (VTracer)",
        }
    }
}

impl Default for TraceParams {
    fn default() -> Self {
        TraceParams { threshold: 128, cutoff: 0, ignore_less_than: 5.0, smoothness: 1.0, optimize: 1.0, invert: false, engine: TraceEngine::Precise }
    }
}

/// 0..255 brightness per pixel after brightness/contrast/gamma/invert. Transparent counts as white.
pub fn adjusted_gray(data: &BitmapData, adj: &ImageAdjust) -> Vec<u8> {
    let n = (data.width * data.height) as usize;
    let mut out = Vec::with_capacity(n);
    let c = (adj.contrast / 100.0 + 1.0).max(0.0);
    let g = 1.0 / adj.gamma.max(0.05);
    for i in 0..n {
        let px = &data.rgba[i * 4..i * 4 + 4];
        let a = px[3] as f64 / 255.0;
        let lum = (0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64) * a + 255.0 * (1.0 - a);
        let mut v = lum / 255.0;
        v = (v - 0.5) * c + 0.5 + adj.brightness / 100.0;
        v = v.clamp(0.0, 1.0).powf(g);
        if adj.invert {
            v = 1.0 - v;
        }
        // see-through (masked) pixels stay white whatever the adjustments: they never burn
        if px[3] == 0 {
            v = 1.0;
        }
        out.push((v * 255.0).round() as u8);
    }
    out
}

/// The most pixels a picture is resampled to for burning (a guard for tiny line intervals on huge pictures).
const MAX_BURN_PIXELS: f64 = 16_000_000.0;

/// How many dots a picture burns as, across and down: one per line interval (`interval` mm), for a picture
/// `w_mm` × `h_mm` on the bed, at most `max_px` in all.
pub fn burn_grid(w_mm: f64, h_mm: f64, interval: f64, max_px: f64) -> (usize, usize) {
    let step = interval.max(0.001);
    let (mut nw, mut nh) = ((w_mm.abs() / step).round().max(1.0), (h_mm.abs() / step).round().max(1.0));
    if nw * nh > max_px {
        let k = (max_px / (nw * nh)).sqrt();
        nw = (nw * k).floor().max(1.0);
        nh = (nh * k).floor().max(1.0);
    }
    (nw as usize, nh as usize)
}

/// A picture as it burns (see `burn_pixels`).
pub struct BurnPixels {
    /// 0 = burn, 255 = no burn (Grayscale and 3D slice keep their shades).
    pub px: Vec<u8>,
    pub w: usize,
    pub h: usize,
    /// Pixels that are see-through (masked): they never burn, at any power. Empty when there are none.
    pub clear: Vec<bool>,
}

impl BurnPixels {
    pub fn is_clear(&self, i: usize) -> bool {
        self.clear.get(i).copied().unwrap_or(false)
    }
}

/// The picture as it burns: adjusted, resampled to one pixel per burned dot, then put through its mode.
pub fn burn_pixels(data: &BitmapData, adj: &ImageAdjust, w_mm: f64, h_mm: f64, interval: f64) -> BurnPixels {
    burn_pixels_max(data, adj, w_mm, h_mm, interval, MAX_BURN_PIXELS)
}

/// The same with another pixel limit (the on-screen picture).
pub fn burn_pixels_max(data: &BitmapData, adj: &ImageAdjust, w_mm: f64, h_mm: f64, interval: f64, max_px: f64) -> BurnPixels {
    let gray = adjusted_gray(data, adj);
    let (w, h) = (data.width as usize, data.height as usize);
    let (nw, nh) = burn_grid(w_mm, h_mm, interval, max_px);
    let g = if (nw, nh) == (w, h) { gray } else { resample(&gray, w, h, nw, nh) };
    let px = render(&g, nw, nh, adj, (w_mm.abs() / nw as f64, h_mm.abs() / nh as f64));
    let clear = if data.rgba.iter().skip(3).step_by(4).any(|a| *a < 128) {
        let alpha: Vec<u8> = data.rgba.iter().skip(3).step_by(4).copied().collect();
        let a = if (nw, nh) == (w, h) { alpha } else { resample(&alpha, w, h, nw, nh) };
        a.into_iter().map(|a| a < 128).collect()
    } else {
        Vec::new()
    };
    BurnPixels { px, w: nw, h: nh, clear }
}

/// Scale a grayscale picture to `nw` × `nh`: the average of the pixels each new one covers when shrinking,
/// smooth (bilinear) when growing.
pub fn resample(gray: &[u8], w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    if w == 0 || h == 0 || gray.len() < w * h {
        return vec![255; nw * nh];
    }
    let (sx, sy) = (w as f64 / nw as f64, h as f64 / nh as f64);
    let mut out = Vec::with_capacity(nw * nh);
    if sx >= 1.0 && sy >= 1.0 {
        // box average (summed per row once, so big pictures stay quick)
        let mut cols: Vec<(usize, usize)> = Vec::with_capacity(nw);
        for x in 0..nw {
            let x0 = (x as f64 * sx).floor() as usize;
            let x1 = (((x + 1) as f64 * sx).ceil() as usize).clamp(x0 + 1, w);
            cols.push((x0, x1));
        }
        for y in 0..nh {
            let y0 = (y as f64 * sy).floor() as usize;
            let y1 = (((y + 1) as f64 * sy).ceil() as usize).clamp(y0 + 1, h);
            let mut acc = vec![0u32; w];
            for row in y0..y1 {
                for (a, v) in acc.iter_mut().zip(&gray[row * w..row * w + w]) {
                    *a += *v as u32;
                }
            }
            let rows = (y1 - y0) as u32;
            for &(x0, x1) in &cols {
                let sum: u32 = acc[x0..x1].iter().sum();
                out.push(((sum as f64) / ((x1 - x0) as u32 * rows) as f64).round() as u8);
            }
        }
    } else {
        let at = |x: usize, y: usize| gray[y.min(h - 1) * w + x.min(w - 1)] as f64;
        for y in 0..nh {
            let fy = ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (h - 1) as f64);
            let (y0, ty) = (fy.floor() as usize, fy - fy.floor());
            for x in 0..nw {
                let fx = ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (w - 1) as f64);
                let (x0, tx) = (fx.floor() as usize, fx - fx.floor());
                let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
                let bot = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
                out.push((top * (1.0 - ty) + bot * ty).round() as u8);
            }
        }
    }
    out
}

/// Put an adjusted grayscale picture through its mode. `mm_per_px` (across, down) sizes the halftone screen.
pub fn render(gray: &[u8], w: usize, h: usize, adj: &ImageAdjust, mm_per_px: (f64, f64)) -> Vec<u8> {
    match adj.dither {
        Dither::Halftone => halftone(gray, w, h, adj.halftone_lpi, adj.halftone_angle, mm_per_px),
        Dither::Slice3D => {
            let n = adj.slices.clamp(1, 255) as f64;
            gray.iter().map(|g| 255 - (slice_level(*g, adj.slices) as f64 * 255.0 / n).round() as u8).collect()
        }
        d => dither(gray, w, h, d),
    }
}

/// How many of `slices` passes burn a pixel of this shade in 3D slice mode: white none, black all.
pub fn slice_level(g: u8, slices: u32) -> u32 {
    let n = slices.clamp(1, 255);
    (((255 - g) as f64 / 255.0) * n as f64).round() as u32
}

/// A halftone screen of round dots, `lpi` dots per inch along screen lines turned by `angle` degrees. Each
/// cell's dot grows with the darkness there; past half dark, the light places become shrinking round holes.
pub fn halftone(gray: &[u8], w: usize, h: usize, lpi: f64, angle: f64, mm_per_px: (f64, f64)) -> Vec<u8> {
    let cell = 25.4 / lpi.clamp(1.0, 1000.0);
    let (sn, cs) = angle.to_radians().sin_cos();
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (px, py) = ((x as f64 + 0.5) * mm_per_px.0, (y as f64 + 0.5) * mm_per_px.1);
            let (u, v) = ((px * cs + py * sn) / cell, (-px * sn + py * cs) / cell);
            let (fu, fv) = (u - u.floor() - 0.5, v - v.floor() - 0.5);
            let dark = 1.0 - gray[y * w + x] as f64 / 255.0;
            let burn = if dark <= 0.5 {
                std::f64::consts::PI * (fu * fu + fv * fv) < dark
            } else {
                let (cu, cv) = (0.5 - fu.abs(), 0.5 - fv.abs());
                std::f64::consts::PI * (cu * cu + cv * cv) >= 1.0 - dark
            };
            out.push(if burn { 0 } else { 255 });
        }
    }
    out
}

/// Mask a picture with closed outlines given in its pixel coordinates (y down, even-odd): pixels outside them
/// (or inside, with `keep_inside` false) become see-through, which never burns. Returns how many changed.
pub fn mask_pixels(rgba: &mut [u8], w: usize, h: usize, rings_px: &[Vec<P2>], keep_inside: bool) -> usize {
    let mut changed = 0;
    let mut xs: Vec<f64> = Vec::new();
    for y in 0..h {
        let cy = y as f64 + 0.5;
        xs.clear();
        for r in rings_px {
            for s in r.windows(2) {
                let (a, b) = (s[0], s[1]);
                if (a.y > cy) != (b.y > cy) {
                    xs.push(a.x + (cy - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        let mut k = 0;
        for x in 0..w {
            let cx = x as f64 + 0.5;
            while k < xs.len() && xs[k] < cx {
                k += 1;
            }
            let inside = k % 2 == 1;
            let i = (y * w + x) * 4 + 3;
            if inside != keep_inside && i < rgba.len() && rgba[i] != 0 {
                rgba[i] = 0;
                changed += 1;
            }
        }
    }
    changed
}

/// Dithered preview (0 = burn, 255 = no burn).
pub fn dither(gray: &[u8], w: usize, h: usize, mode: Dither) -> Vec<u8> {
    match mode {
        Dither::Grayscale => gray.to_vec(),
        Dither::Halftone => halftone(gray, w, h, ImageAdjust::DEFAULT_LPI, ImageAdjust::DEFAULT_ANGLE, (0.1, 0.1)),
        Dither::Slice3D => render(gray, w, h, &ImageAdjust { dither: Dither::Slice3D, ..Default::default() }, (0.1, 0.1)),
        Dither::Stucki => error_diffuse(
            gray,
            w,
            h,
            &[(1, 0, 8.0), (2, 0, 4.0), (-2, 1, 2.0), (-1, 1, 4.0), (0, 1, 8.0), (1, 1, 4.0), (2, 1, 2.0), (-2, 2, 1.0), (-1, 2, 2.0), (0, 2, 4.0), (1, 2, 2.0), (2, 2, 1.0)],
            42.0,
        ),
        // Atkinson passes on only 6/8 of the error, which keeps highlights and shadows clean
        Dither::Atkinson => error_diffuse(gray, w, h, &[(1, 0, 1.0), (2, 0, 1.0), (-1, 1, 1.0), (0, 1, 1.0), (1, 1, 1.0), (0, 2, 1.0)], 8.0),
        Dither::Sierra => error_diffuse(gray, w, h, &[(1, 0, 5.0), (2, 0, 3.0), (-2, 1, 2.0), (-1, 1, 4.0), (0, 1, 5.0), (1, 1, 4.0), (2, 1, 2.0), (-1, 2, 2.0), (0, 2, 3.0), (1, 2, 2.0)], 32.0),
        Dither::Threshold => gray.iter().map(|v| if *v < 128 { 0 } else { 255 }).collect(),
        Dither::FloydSteinberg => error_diffuse(gray, w, h, &[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)], 16.0),
        Dither::Jarvis => error_diffuse(
            gray,
            w,
            h,
            &[(1, 0, 7.0), (2, 0, 5.0), (-2, 1, 3.0), (-1, 1, 5.0), (0, 1, 7.0), (1, 1, 5.0), (2, 1, 3.0), (-2, 2, 1.0), (-1, 2, 3.0), (0, 2, 5.0), (1, 2, 3.0), (2, 2, 1.0)],
            48.0,
        ),
    }
}

fn error_diffuse(gray: &[u8], w: usize, h: usize, kernel: &[(i32, i32, f32)], div: f32) -> Vec<u8> {
    let mut buf: Vec<f32> = gray.iter().map(|v| *v as f32).collect();
    let mut out = vec![0u8; gray.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let old = buf[i];
            let new = if old < 128.0 { 0.0 } else { 255.0 };
            out[i] = new as u8;
            let err = old - new;
            for (dx, dy, k) in kernel {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && nx < w as i32 && ny < h as i32 {
                    buf[ny as usize * w + nx as usize] += err * k / div;
                }
            }
        }
    }
    out
}

/// Which pixels count as "ink" for the given settings.
pub fn ink_mask(gray: &[u8], p: &TraceParams) -> Vec<bool> {
    gray.iter().map(|v| (*v < p.threshold && *v >= p.cutoff) != p.invert).collect()
}

/// Trace with the chosen engine (pixel coordinates, y down).
pub fn trace(gray: &[u8], w: usize, h: usize, p: &TraceParams) -> Vec<Vec<P2>> {
    match p.engine {
        TraceEngine::Precise => trace_pixels(gray, w, h, p),
        TraceEngine::Smooth => trace_smooth(gray, w, h, p),
    }
}

/// Smooth spline tracing with VTracer in binary mode.
pub fn trace_smooth(gray: &[u8], w: usize, h: usize, p: &TraceParams) -> Vec<Vec<P2>> {
    let mask = ink_mask(gray, p);
    let mut img = vtracer::ColorImage::new_w_h(w, h);
    for (i, m) in mask.iter().enumerate() {
        let v = if *m { 0 } else { 255 };
        img.pixels[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
    }
    let cfg = vtracer::Config {
        color_mode: vtracer::ColorMode::Binary,
        filter_speckle: (p.ignore_less_than.max(0.0) * p.ignore_less_than.max(0.0)).round() as usize,
        corner_threshold: (60.0 + 30.0 * p.smoothness.clamp(0.0, 4.0)) as i32,
        length_threshold: (2.0 + p.optimize.clamp(0.0, 4.0) * 1.5).min(10.0),
        splice_threshold: 45,
        mode: visioncortex::PathSimplifyMode::Spline,
        path_precision: Some(3),
        ..Default::default()
    };
    let Ok(svg) = vtracer::convert(img, cfg) else { return Vec::new() };
    let mut out = Vec::new();
    for path in &svg.paths {
        let (d, off) = path.path.to_svg_string(true, visioncortex::PointF64::default(), Some(3));
        for l in crate::mcp::parse_svg_path(&d) {
            out.push(l.into_iter().map(|q| p2(q.x + off.x, q.y + off.y)).collect());
        }
    }
    out
}

/// Trace a bitmap into closed outlines in pixel coordinates (y down).
pub fn trace_pixels(gray: &[u8], w: usize, h: usize, p: &TraceParams) -> Vec<Vec<P2>> {
    let ink = |x: i64, y: i64| -> bool {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return false;
        }
        let v = gray[y as usize * w + x as usize];
        let dark = v < p.threshold && v >= p.cutoff;
        dark != p.invert
    };
    // Directed crack edges around ink pixels, ink on the right (y down).
    use std::collections::HashMap;
    let mut out_edges: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    let mut edge_count = 0usize;
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !ink(x, y) {
                continue;
            }
            let mut add = |a: (i64, i64), b: (i64, i64)| {
                out_edges.entry(a).or_default().push(b);
                edge_count += 1;
            };
            if !ink(x, y - 1) {
                add((x, y), (x + 1, y));
            }
            if !ink(x + 1, y) {
                add((x + 1, y), (x + 1, y + 1));
            }
            if !ink(x, y + 1) {
                add((x + 1, y + 1), (x, y + 1));
            }
            if !ink(x - 1, y) {
                add((x, y + 1), (x, y));
            }
        }
    }
    let mut loops: Vec<Vec<P2>> = Vec::new();
    let mut starts: Vec<(i64, i64)> = out_edges.keys().copied().collect();
    starts.sort();
    for s in starts {
        while let Some(next) = out_edges.get_mut(&s).and_then(|v| v.pop()) {
            let mut ring = vec![s, next];
            let mut cur = next;
            let mut guard = 0usize;
            while cur != s && guard <= edge_count {
                guard += 1;
                match out_edges.get_mut(&cur).and_then(|v| v.pop()) {
                    Some(n) => {
                        ring.push(n);
                        cur = n;
                    }
                    None => break,
                }
            }
            if ring.len() < 4 {
                continue;
            }
            // midpoints of crack edges remove the staircase
            let mut pts: Vec<P2> = ring.windows(2).map(|w| p2((w[0].0 + w[1].0) as f64 * 0.5, (w[0].1 + w[1].1) as f64 * 0.5)).collect();
            let area = signed_area(&pts).abs();
            if area < p.ignore_less_than * p.ignore_less_than * 0.5 {
                continue;
            }
            pts.push(pts[0]);
            // optimise (drop redundant points), then smooth
            let mut pts = simplify(&pts, 0.25 + 0.5 * p.optimize.clamp(0.0, 4.0));
            for _ in 0..(p.smoothness.clamp(0.0, 4.0).round() as usize) {
                pts = chaikin_closed(&pts);
            }
            if pts.len() >= 4 {
                loops.push(pts);
            }
        }
    }
    loops
}

fn chaikin_closed(pts: &[P2]) -> Vec<P2> {
    let ring = if is_closed(pts) { &pts[..pts.len() - 1] } else { pts };
    let n = ring.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(n * 2 + 1);
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        out.push(a.lerp(b, 0.25));
        out.push(a.lerp(b, 0.75));
    }
    out.push(out[0]);
    out
}

/// Map pixel-space outlines of a bitmap shape into its parent space.
pub fn pixels_to_shape_space(lines: Vec<Vec<P2>>, shape_w: f64, shape_h: f64, img_w: u32, img_h: u32, xform: &Xform) -> Vec<Vec<P2>> {
    let sx = shape_w / img_w.max(1) as f64;
    let sy = shape_h / img_h.max(1) as f64;
    lines
        .into_iter()
        .map(|l| l.into_iter().map(|p| xform.apply(p2(p.x * sx - shape_w / 2.0, shape_h / 2.0 - p.y * sy))).collect())
        .collect()
}

/// Trace a bitmap shape; returns outlines in its parent space.
#[allow(dead_code)]
pub fn trace_shape(shape: &Shape, p: &TraceParams) -> Vec<Vec<P2>> {
    let ShapeKind::Bitmap { w, h, data, adjust } = &shape.kind else { return Vec::new() };
    let gray = adjusted_gray(data, adjust);
    let lines = trace(&gray, data.width as usize, data.height as usize, p);
    pixels_to_shape_space(lines, *w, *h, data.width, data.height, &shape.xform)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traces_square() {
        let (w, h) = (20usize, 20usize);
        let mut g = vec![255u8; w * h];
        for y in 5..15 {
            for x in 5..15 {
                g[y * w + x] = 0;
            }
        }
        let loops = trace_pixels(&g, w, h, &TraceParams { smoothness: 0.0, ..Default::default() });
        assert_eq!(loops.len(), 1);
        let b = BBox::from_points(&loops[0]);
        assert!((b.width() - 10.0).abs() < 1.5);
    }

    fn burned(px: &[u8]) -> f64 {
        px.iter().filter(|v| **v < 128).count() as f64 / px.len() as f64
    }

    #[test]
    fn new_dithers_keep_the_shade() {
        let (w, h) = (64, 64);
        for (g, want) in [(64u8, 0.75), (128, 0.5), (192, 0.25)] {
            let gray = vec![g; w * h];
            for d in [Dither::Stucki, Dither::Sierra, Dither::FloydSteinberg, Dither::Jarvis] {
                let out = dither(&gray, w, h, d);
                assert!(out.iter().all(|v| *v == 0 || *v == 255));
                assert!((burned(&out) - want).abs() < 0.04, "{d:?} at {g}: {}", burned(&out));
            }
            // Atkinson drops a quarter of the error: lighter in the light, darker in the dark, never inverted
            let a = burned(&dither(&gray, w, h, Dither::Atkinson));
            assert!((a - want).abs() < 0.2, "Atkinson at {g}: {a}");
        }
        assert_eq!(burned(&dither(&vec![0; 100], 10, 10, Dither::Atkinson)), 1.0);
        assert_eq!(burned(&dither(&vec![255; 100], 10, 10, Dither::Stucki)), 0.0);
    }

    #[test]
    fn halftone_dots_cover_the_right_share() {
        // 0.05 mm pixels, 40 lines per inch (0.635 mm cells), 20 mm square
        let (w, h) = (400, 400);
        for (g, want) in [(0u8, 1.0), (64, 0.75), (128, 0.5), (191, 0.25), (255, 0.0)] {
            for angle in [0.0, 45.0] {
                let out = halftone(&vec![g; w * h], w, h, 40.0, angle, (0.05, 0.05));
                assert!((burned(&out) - want).abs() < 0.06, "{g} at {angle}°: {}", burned(&out));
            }
        }
    }

    #[test]
    fn slices_resampling_and_masks() {
        assert_eq!(slice_level(0, 8), 8);
        assert_eq!(slice_level(255, 8), 0);
        assert_eq!(slice_level(128, 8), 4);
        // shrinking averages, growing is smooth
        let checker: Vec<u8> = (0..16).map(|i| if (i % 4 + i / 4) % 2 == 0 { 0 } else { 255 }).collect();
        assert!(resample(&checker, 4, 4, 2, 2).iter().all(|v| (127..=128).contains(v)));
        assert_eq!(resample(&[100; 4], 2, 2, 7, 5), vec![100; 35]);
        // a 300 mm picture at 0.001 mm would be 9·10^10 dots: kept to the limit, same shape
        let (nw, nh) = burn_grid(300.0, 150.0, 0.001, 1e6);
        assert!(nw * nh <= 1_000_000 && (nw as f64 / nh as f64 - 2.0).abs() < 0.01);
        assert_eq!(burn_grid(10.0, 5.0, 0.1, 1e9), (100, 50));
        // keep the inside of a 6 × 6 pixel square in a 10 × 10 picture
        let mut rgba = vec![255u8; 400];
        let sq = vec![p2(2.0, 2.0), p2(8.0, 2.0), p2(8.0, 8.0), p2(2.0, 8.0), p2(2.0, 2.0)];
        assert_eq!(mask_pixels(&mut rgba, 10, 10, std::slice::from_ref(&sq), true), 64);
        assert_eq!(rgba[(5 * 10 + 5) * 4 + 3], 255);
        assert_eq!(rgba[3], 0);
        let mut rgba = vec![255u8; 400];
        assert_eq!(mask_pixels(&mut rgba, 10, 10, &[sq], false), 36);
    }
}
