//! Shared pieces for the vector importers (DXF, PDF): loose segments joined into outlines quickly, arcs and
//! ellipses as Bézier curves, and the CAD colour numbers (ACI) used by DXF.

use crate::geom::*;
use crate::model::*;
use std::collections::HashMap;

/// One piece of an outline: a straight line, or a cubic Bézier curve when it has control points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub a: P2,
    pub c0: Option<P2>,
    pub c1: Option<P2>,
    pub b: P2,
}

impl Seg {
    pub fn line(a: P2, b: P2) -> Seg {
        Seg { a, c0: None, c1: None, b }
    }
    pub fn curve(a: P2, c0: P2, c1: P2, b: P2) -> Seg {
        Seg { a, c0: Some(c0), c1: Some(c1), b }
    }
    pub fn reversed(self) -> Seg {
        Seg { a: self.b, c0: self.c1, c1: self.c0, b: self.a }
    }
    pub fn map(self, f: &dyn Fn(P2) -> P2) -> Seg {
        Seg { a: f(self.a), c0: self.c0.map(f), c1: self.c1.map(f), b: f(self.b) }
    }
    fn is_curve(&self) -> bool {
        self.c0.is_some() || self.c1.is_some()
    }
}

/// Part of an ellipse, `p(t) = c + u·cos t + v·sin t` from `t0` to `t1` (radians; either direction), as
/// curves of at most 90° each. A circle has `u = (r, 0)`, `v = (0, r)`; any affine image of it works too.
pub fn ellipse_arc(c: P2, u: P2, v: P2, t0: f64, t1: f64) -> Vec<Seg> {
    let sweep = t1 - t0;
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let dt = sweep / n as f64;
    let k = 4.0 / 3.0 * (dt / 4.0).tan();
    let p = |t: f64| c + u * t.cos() + v * t.sin();
    let d = |t: f64| u * (-t.sin()) + v * t.cos();
    (0..n)
        .map(|i| {
            let (a, b) = (t0 + dt * i as f64, t0 + dt * (i + 1) as f64);
            Seg::curve(p(a), p(a) + d(a) * k, p(b) - d(b) * k, p(b))
        })
        .collect()
}

/// A whole circle.
pub fn circle(c: P2, r: f64) -> Vec<Seg> {
    ellipse_arc(c, p2(r, 0.0), p2(0.0, r), 0.0, std::f64::consts::TAU)
}

/// The arc between two points with a "bulge" (the tangent of a quarter of its angle; positive turns left),
/// as CAD polylines store arcs. A bulge of 0 is a straight line.
pub fn bulge_arc(a: P2, b: P2, bulge: f64) -> Vec<Seg> {
    if bulge.abs() < 1e-9 || a.dist(b) < 1e-12 {
        return vec![Seg::line(a, b)];
    }
    let theta = 4.0 * bulge.atan(); // signed sweep
    let chord = b - a;
    let half = chord.len() / 2.0;
    let r = half / (theta / 2.0).sin().abs();
    // centre: from the chord's middle, along its left normal (for a positive bulge under half a turn)
    let mid = (a + b) * 0.5;
    let n = p2(-chord.y, chord.x) * (1.0 / chord.len());
    let h = r * (theta / 2.0).cos().abs();
    let side = if (bulge > 0.0) == (bulge.abs() < 1.0) { 1.0 } else { -1.0 };
    let c = mid + n * (h * side);
    let t0 = (a.y - c.y).atan2(a.x - c.x);
    ellipse_arc(c, p2(r, 0.0), p2(0.0, r), t0, t0 + theta)
}

/// Join loose segments whose ends meet (within `tol`) into continuous outlines, closing those that come back
/// to their start. Segments can be in any order and direction. Fast for many thousands of pieces.
pub fn build_paths(segs: &[Seg], tol: f64) -> PathData {
    let tol = tol.max(1e-9);
    let segs: Vec<Seg> = segs.iter().copied().filter(|s| s.is_curve() || s.a.dist(s.b) > tol * 1e-3).collect();
    let key = |p: P2| ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
    let mut ends: HashMap<(i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        ends.entry(key(s.a)).or_default().push((i, false));
        ends.entry(key(s.b)).or_default().push((i, true));
    }
    let mut used = vec![false; segs.len()];
    // an unused segment with an end at `p`: (index, whether it's its end point)
    let find = |p: P2, used: &[bool]| -> Option<(usize, bool)> {
        let (kx, ky) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(i, at_end) in ends.get(&(kx + dx, ky + dy)).into_iter().flatten() {
                    let q = if at_end { segs[i].b } else { segs[i].a };
                    if !used[i] && q.dist(p) <= tol {
                        return Some((i, at_end));
                    }
                }
            }
        }
        None
    };
    let mut pd = PathData::default();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain = std::collections::VecDeque::from([segs[start]]);
        // onwards from its end, then back from its start
        while let Some((j, at_end)) = find(chain.back().unwrap().b, &used) {
            used[j] = true;
            chain.push_back(if at_end { segs[j].reversed() } else { segs[j] });
        }
        while let Some((j, at_end)) = find(chain.front().unwrap().a, &used) {
            used[j] = true;
            chain.push_front(if at_end { segs[j] } else { segs[j].reversed() });
        }
        append_chain(&mut pd, chain.make_contiguous(), tol);
    }
    pd
}

/// Add one chain of segments (each starting where the last ended) as a sub-path.
pub fn append_chain(pd: &mut PathData, chain: &[Seg], tol: f64) {
    if chain.is_empty() {
        return;
    }
    let closed = chain.len() >= 2 && chain[0].a.dist(chain[chain.len() - 1].b) <= tol || chain.len() == 1 && chain[0].is_curve() && chain[0].a.dist(chain[0].b) <= tol;
    let base = pd.verts.len();
    pd.verts.push(Vert { p: chain[0].a, c0: chain[0].c0, c1: None });
    for (i, s) in chain.iter().enumerate() {
        let a = pd.verts.len() - 1;
        let last = i + 1 == chain.len();
        let b = if closed && last && a != base {
            pd.verts[base].c1 = s.c1;
            base
        } else {
            pd.verts.push(Vert { p: s.b, c0: chain.get(i + 1).and_then(|n| n.c0), c1: s.c1 });
            pd.verts.len() - 1
        };
        pd.prims.push(Prim { kind: if s.is_curve() { PrimKind::Bezier } else { PrimKind::Line }, a, b });
    }
}

// ----------------------------------------------------------------------------------------- CAD colours (ACI)

/// The colour of a CAD colour number (AutoCAD Color Index). Number 7 is "white or black, whichever shows":
/// black here, as on a light page.
pub fn aci_rgb(i: u16) -> [u8; 3] {
    match i {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        0 | 7 => [0, 0, 0],
        8 => [128, 128, 128],
        9 => [192, 192, 192],
        10..=249 => {
            // 24 hues, each in five shades, alternately full and half colour
            let hue = ((i / 10) - 1) as f64 * 15.0;
            let j = i % 10;
            let v = [255.0, 165.0, 127.0, 76.0, 38.0][(j / 2) as usize];
            let (r, g, b) = hue_rgb(hue);
            let mix = |c: f64| if j % 2 == 0 { c * v } else { v * (0.5 + 0.5 * c) };
            [mix(r).round() as u8, mix(g).round() as u8, mix(b).round() as u8]
        }
        250 => [51, 51, 51],
        251 => [80, 80, 80],
        252 => [105, 105, 105],
        253 => [130, 130, 130],
        254 => [190, 190, 190],
        _ => [255, 255, 255],
    }
}

fn hue_rgb(h: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}

/// The CAD colour number closest to `rgb` (1–255).
pub fn nearest_aci(rgb: [u8; 3]) -> u16 {
    let d = |a: [u8; 3]| (0..3).map(|k| (a[k] as i32 - rgb[k] as i32).pow(2)).sum::<i32>();
    (1..=255u16).min_by_key(|&i| d(aci_rgb(i))).unwrap_or(7)
}

/// A BurnCraft layer named in another program (C00…C29, T1, T2), so layers come back as they were.
pub fn layer_from_name(name: &str) -> Option<usize> {
    let n = name.trim();
    match n {
        "T1" => Some(COLOR_LAYERS),
        "T2" => Some(CONSTRUCTION_LAYER),
        _ => {
            let k: usize = n.strip_prefix('C')?.parse().ok()?;
            (n.len() == 3 && k < COLOR_LAYERS).then_some(k)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(pd: &PathData) -> BBox {
        let mut b = BBox::EMPTY;
        for p in pd.flatten(0.001).into_iter().flatten() {
            b.add(p);
        }
        b
    }

    #[test]
    fn loose_segments_join_into_closed_outlines() {
        // a square's sides, shuffled and some backwards, plus a separate open line
        let (a, b, c, d) = (p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0), p2(0.0, 10.0));
        let segs = [Seg::line(c, b), Seg::line(p2(20.0, 0.0), p2(30.0, 0.0)), Seg::line(a, b), Seg::line(d, a), Seg::line(c, d)];
        let pd = build_paths(&segs, 0.001);
        let lines = pd.flatten(0.01);
        assert_eq!(lines.len(), 2);
        let sq = lines.iter().find(|l| l.len() == 5).unwrap();
        assert!(is_closed(sq), "{sq:?}");
        assert!(lines.iter().any(|l| l.len() == 2 && !is_closed(l)));
        // thousands of pieces stay quick
        let many: Vec<Seg> = (0..20000).map(|i| Seg::line(p2(i as f64, 0.0), p2(i as f64 + 1.0, 0.0))).rev().collect();
        let t = std::time::Instant::now();
        let pd = build_paths(&many, 0.001);
        assert_eq!(pd.flatten(0.01).len(), 1);
        assert!(t.elapsed().as_secs_f64() < 1.0);
    }

    #[test]
    fn arcs_bulges_and_circles() {
        let pd = build_paths(&circle(p2(5.0, 5.0), 5.0), 1e-6);
        let b = bounds(&pd);
        assert!((b.width() - 10.0).abs() < 1e-3 && (b.min.x).abs() < 1e-3, "{b:?}");
        assert!(is_closed(&pd.flatten(0.01)[0]));
        // a half-circle bulge (1) from (0,0) to (10,0): turning left means it bulges below the chord
        let pd = build_paths(&bulge_arc(p2(0.0, 0.0), p2(10.0, 0.0), 1.0), 1e-6);
        let b = bounds(&pd);
        assert!((b.min.y + 5.0).abs() < 1e-3 && b.max.y.abs() < 1e-3, "{b:?}");
        let pd = build_paths(&bulge_arc(p2(0.0, 0.0), p2(10.0, 0.0), -1.0), 1e-6);
        assert!((bounds(&pd).max.y - 5.0).abs() < 1e-3);
        // a quarter circle: tan(90°/4)
        let q = bulge_arc(p2(10.0, 0.0), p2(0.0, 10.0), (std::f64::consts::FRAC_PI_2 / 4.0).tan());
        let b = bounds(&build_paths(&q, 1e-6));
        assert!((b.max.x - 10.0).abs() < 1e-3 && (b.max.y - 10.0).abs() < 1e-3 && b.min.x.abs() < 1e-3, "{b:?}");
    }

    #[test]
    fn cad_colours_and_layer_names() {
        assert_eq!(aci_rgb(1), [255, 0, 0]);
        assert_eq!(aci_rgb(11), [255, 128, 128]);
        assert_eq!(aci_rgb(14), [127, 0, 0]);
        assert_eq!(nearest_aci([255, 0, 0]), 1);
        assert_eq!(nearest_aci([0, 0, 0]), 7);
        assert_eq!(layer_from_name("C05"), Some(5));
        assert_eq!(layer_from_name("T2"), Some(CONSTRUCTION_LAYER));
        assert_eq!(layer_from_name("Walls"), None);
        assert_eq!(layer_from_name("C99"), None);
    }
}
