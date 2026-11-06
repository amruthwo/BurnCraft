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
        out.push((v * 255.0).round() as u8);
    }
    out
}

/// Dithered preview (0 = burn, 255 = no burn).
pub fn dither(gray: &[u8], w: usize, h: usize, mode: Dither) -> Vec<u8> {
    match mode {
        Dither::Grayscale => gray.to_vec(),
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
}
