//! Generators: finger-joint boxes, living hinges, spur gears and jigsaw puzzles, made as ordinary shapes.
//! BurnCraft's own code (written from the usual geometry of each, not ported from another project).

use crate::geom::*;
use crate::model::*;
use geo::{BooleanOps, LineString, MultiPolygon, Polygon};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GenKind {
    #[default]
    Box,
    Hinge,
    Gear,
    Puzzle,
}

impl GenKind {
    pub const ALL: [GenKind; 4] = [GenKind::Box, GenKind::Hinge, GenKind::Gear, GenKind::Puzzle];
    pub fn label(self) -> &'static str {
        match self {
            GenKind::Box => "Finger-joint box",
            GenKind::Hinge => "Living hinge",
            GenKind::Gear => "Gear",
            GenKind::Puzzle => "Jigsaw puzzle",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxParams {
    /// Width (x), depth (y) and height (z), mm.
    pub w: f64,
    pub d: f64,
    pub h: f64,
    /// The sizes are the inside of the box (else the outside).
    pub inside: bool,
    pub thickness: f64,
    /// About how wide each finger is.
    pub finger: f64,
    pub closed_top: bool,
}

impl Default for BoxParams {
    fn default() -> Self {
        BoxParams { w: 80.0, d: 60.0, h: 40.0, inside: true, thickness: 3.0, finger: 10.0, closed_top: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HingeParams {
    pub w: f64,
    pub h: f64,
    /// Slit length, the bridge left between slits in a column, and the distance between columns.
    pub slit: f64,
    pub bridge: f64,
    pub spacing: f64,
}

impl Default for HingeParams {
    fn default() -> Self {
        HingeParams { w: 40.0, h: 60.0, slit: 20.0, bridge: 3.0, spacing: 1.5 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GearParams {
    pub teeth: u32,
    /// Module (mm of pitch diameter per tooth); circular pitch = π × module.
    pub module: f64,
    pub pressure_angle: f64,
    /// Centre hole diameter (0 = none).
    pub bore: f64,
}

impl Default for GearParams {
    fn default() -> Self {
        GearParams { teeth: 20, module: 2.0, pressure_angle: 20.0, bore: 5.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PuzzleParams {
    pub w: f64,
    pub h: f64,
    pub cols: u32,
    pub rows: u32,
    /// Changes which way the tabs point.
    pub seed: u32,
}

impl Default for PuzzleParams {
    fn default() -> Self {
        PuzzleParams { w: 150.0, h: 100.0, cols: 6, rows: 4, seed: 1 }
    }
}

fn ring_ls(r: &[P2]) -> LineString<f64> {
    LineString::from(r.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>())
}

fn rect_ring(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P2> {
    vec![p2(x0, y0), p2(x1, y0), p2(x1, y1), p2(x0, y1), p2(x0, y0)]
}

/// How an edge of a panel meets the next panel.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Edge {
    /// Straight (an open top).
    Plain,
    /// Fingers at both ends (this panel owns the even pieces).
    Fingers,
    /// Slots at both ends (the other panel owns the even pieces).
    Slots,
}

/// An odd number of pieces about `finger` wide along `len`, at least 3.
fn pieces(len: f64, finger: f64) -> usize {
    let n = (len / finger.max(0.5)).round().max(3.0) as usize;
    if n % 2 == 0 {
        n + 1
    } else {
        n
    }
}

/// One panel `w` × `h`, its edges given bottom, right, top, left; `t` deep notches cut where the other panel
/// owns the piece. Returns the outline (its corner at 0,0).
fn panel(w: f64, h: f64, t: f64, finger: f64, edges: [Edge; 4]) -> Vec<Vec<P2>> {
    let mut notches: Vec<Polygon<f64>> = Vec::new();
    let mut notch = |x0: f64, y0: f64, x1: f64, y1: f64| notches.push(Polygon::new(ring_ls(&rect_ring(x0, y0, x1, y1)), vec![]));
    for (k, e) in edges.iter().enumerate() {
        if *e == Edge::Plain {
            continue;
        }
        let len = if k % 2 == 0 { w } else { h };
        let n = pieces(len, finger);
        let step = len / n as f64;
        for i in 0..n {
            // this panel lacks the odd pieces when it has the fingers, the even ones when it has the slots
            let missing = (i % 2 == 1) == (*e == Edge::Fingers);
            if !missing {
                continue;
            }
            let (a, b) = (i as f64 * step, (i + 1) as f64 * step);
            match k {
                0 => notch(a, -1.0, b, t),
                1 => notch(w - t, a, w + 1.0, b),
                2 => notch(a, h - t, b, h + 1.0),
                _ => notch(-1.0, a, t, b),
            }
        }
    }
    let body = MultiPolygon(vec![Polygon::new(ring_ls(&rect_ring(0.0, 0.0, w, h)), vec![])]);
    // the notches are merged first: overlapping ones (at the corners) taken away together leave slivers
    let notches = notches.into_iter().fold(MultiPolygon(Vec::new()), |acc, n| acc.union(&MultiPolygon(vec![n])));
    let cut = body.difference(&notches);
    crate::ops::from_multipolygon(&cut)
}

/// The box's parts, laid out in a row from (0, 0), each one shape. Names say which part is which.
pub fn finger_box(p: &BoxParams) -> Vec<(String, Vec<Vec<P2>>)> {
    // the walls can be at most a third of the smallest side, so every panel keeps some material
    let smallest = if p.inside { p.w.min(p.d).min(p.h) + 2.0 * p.thickness } else { p.w.min(p.d).min(p.h) };
    let t = p.thickness.clamp(0.2, 50.0).min((smallest / 3.0).max(0.2));
    let add = |v: f64, n: f64| if p.inside { v + n * t } else { v };
    let (w, d) = (add(p.w, 2.0), add(p.d, 2.0));
    let h = add(p.h, if p.closed_top { 2.0 } else { 1.0 });
    let f = p.finger;
    use Edge::*;
    let top_edge = if p.closed_top { Fingers } else { Plain };
    // front and back own the vertical corners and their bottom (and top) edges' end pieces; the sides slot
    // between them; the bottom and top slot into everything
    let mut parts = vec![
        ("Bottom".to_string(), panel(w, d, t, f, [Slots, Slots, Slots, Slots])),
        ("Front".to_string(), panel(w, h, t, f, [Fingers, Fingers, top_edge, Fingers])),
        ("Back".to_string(), panel(w, h, t, f, [Fingers, Fingers, top_edge, Fingers])),
        ("Left side".to_string(), panel(d, h, t, f, [Fingers, Slots, top_edge, Slots])),
        ("Right side".to_string(), panel(d, h, t, f, [Fingers, Slots, top_edge, Slots])),
    ];
    if p.closed_top {
        parts.push(("Top".to_string(), panel(w, d, t, f, [Slots, Slots, Slots, Slots])));
    }
    parts.retain(|(_, r)| !r.is_empty());
    // side by side with a 5 mm gap
    let mut x = 0.0;
    for (_, rings) in &mut parts {
        let b = rings.iter().flatten().fold(BBox::EMPTY, |mut b, p| {
            b.add(*p);
            b
        });
        for r in rings.iter_mut() {
            for q in r.iter_mut() {
                q.x += x - b.min.x;
            }
        }
        x += b.width() + 5.0;
    }
    parts
}

/// Living hinge: columns of slits across a `w` × `h` area, every other column shifted by half a slit, so the
/// sheet bends along the columns. Open lines (cuts), from (0, 0).
pub fn living_hinge(p: &HingeParams) -> Vec<Vec<P2>> {
    let (w, h) = (p.w.max(1.0), p.h.max(1.0));
    let slit = p.slit.clamp(1.0, h.max(1.0));
    let bridge = p.bridge.clamp(0.2, 100.0);
    let pitch = slit + bridge;
    let mut out = Vec::new();
    let sp = p.spacing.max(0.2);
    let cols = ((w / sp).floor() as usize).min(5000);
    // the columns centred across the area
    let x0 = (w - cols as f64 * sp) / 2.0;
    for c in 0..=cols {
        let x = x0 + c as f64 * sp;
        // even columns start with a half slit at the edge (so the edges stay joined every other column)
        let mut y = if c % 2 == 0 { -slit / 2.0 } else { bridge / 2.0 };
        while y < h {
            let (a, b) = (y.max(0.0), (y + slit).min(h));
            if b - a > 0.1 {
                out.push(vec![p2(x, a), p2(x, b)]);
            }
            y += pitch;
        }
    }
    out
}

/// The gear's root diameter: a bore has to be smaller than this (90% of it, to leave a ring).
pub fn root_diameter(p: &GearParams) -> f64 {
    let n = p.teeth.clamp(6, 400) as f64;
    let m = p.module.clamp(0.1, 50.0);
    ((m * n / 2.0 - 1.25 * m).max(0.1)) * 2.0
}

/// A spur gear with involute teeth, centred on (0, 0): its outline and the bore.
pub fn gear(p: &GearParams) -> Vec<Vec<P2>> {
    let n = p.teeth.clamp(6, 400) as f64;
    let m = p.module.clamp(0.1, 50.0);
    let alpha = p.pressure_angle.clamp(10.0, 35.0).to_radians();
    let r = m * n / 2.0;
    let rb = r * alpha.cos();
    let ra = r + m;
    let rf = (r - 1.25 * m).max(0.1);
    let inv = |a: f64| a.tan() - a;
    // half the tooth's angle at radius rho (from its middle line)
    let half = |rho: f64| -> f64 {
        let phi = (rb / rho.max(rb)).acos();
        std::f64::consts::PI / (2.0 * n) + inv(alpha) - inv(phi)
    };
    let polar = |rho: f64, a: f64| p2(rho * a.cos(), rho * a.sin());
    let steps = 10;
    let mut ring: Vec<P2> = Vec::new();
    let pitch_a = std::f64::consts::TAU / n;
    for k in 0..n as usize {
        let c = k as f64 * pitch_a;
        // one flank from the root up to the tip (radii from the base circle, or the root if it's above it)
        let r0 = rb.max(rf);
        let flank: Vec<f64> = (0..=steps).map(|i| r0 + (ra - r0) * i as f64 / steps as f64).collect();
        // rising flank (the tooth's clockwise side), then the tip, then the other flank going down
        if rf < rb {
            ring.push(polar(rf, c - half(rb)));
        }
        for rho in &flank {
            ring.push(polar(*rho, c - half(*rho)));
        }
        for rho in flank.iter().rev() {
            ring.push(polar(*rho, c + half(*rho)));
        }
        if rf < rb {
            ring.push(polar(rf, c + half(rb)));
        }
        // the root between this tooth and the next
        let (a0, a1) = (c + half(rb.max(rf)), c + pitch_a - half(rb.max(rf)));
        for i in 1..4 {
            ring.push(polar(rf, a0 + (a1 - a0) * i as f64 / 4.0));
        }
    }
    ring.push(ring[0]);
    let mut out = vec![ring];
    if p.bore > 0.0 && p.bore / 2.0 < rf * 0.9 {
        let br = p.bore / 2.0;
        let mut hole: Vec<P2> = (0..64).map(|i| polar(br, i as f64 * std::f64::consts::TAU / 64.0)).collect();
        hole.push(hole[0]);
        out.push(hole);
    }
    out
}

/// A small, repeatable random sequence for the puzzle's tab directions.
fn coin(seed: u32, a: u32, b: u32) -> bool {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ a.wrapping_mul(0x85EB_CA6B) ^ b.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x & 1 == 1
}

/// One puzzle edge from `a` to `b` with a round tab on one side (`up`: to the left of a → b).
fn tab_edge(a: P2, b: P2, up: bool) -> Vec<P2> {
    let d = b - a;
    let len = d.len();
    let t = d * (1.0 / len.max(1e-9));
    let n = p2(-t.y, t.x) * if up { 1.0 } else { -1.0 };
    let at = |u: f64, v: f64| a + t * (u * len) + n * (v * len);
    let mut pts = vec![at(0.0, 0.0), at(0.38, 0.0)];
    // the neck, then the round head (a circle of 0.1 × the edge around (0.5, 0.13)), then back
    for k in 0..=24 {
        let ang = (225.0 - 270.0 * k as f64 / 24.0).to_radians();
        pts.push(at(0.5 + 0.1 * ang.cos(), 0.13 + 0.1 * ang.sin()));
    }
    pts.push(at(0.62, 0.0));
    pts.push(at(1.0, 0.0));
    pts
}

/// A jigsaw puzzle over a `w` × `h` area from (0, 0): the border and the cuts between pieces.
pub fn puzzle(p: &PuzzleParams) -> Vec<Vec<P2>> {
    let (cols, rows) = (p.cols.clamp(1, 100), p.rows.clamp(1, 100));
    let (cw, ch) = (p.w.max(1.0) / cols as f64, p.h.max(1.0) / rows as f64);
    let mut out = vec![rect_ring(0.0, 0.0, cols as f64 * cw, rows as f64 * ch)];
    // lines between columns (each a chain of edges with tabs)
    for c in 1..cols {
        let x = c as f64 * cw;
        let mut line: Vec<P2> = Vec::new();
        for r in 0..rows {
            let e = tab_edge(p2(x, r as f64 * ch), p2(x, (r + 1) as f64 * ch), coin(p.seed, c, r * 2));
            line.extend(if line.is_empty() { &e[..] } else { &e[1..] });
        }
        out.push(line);
    }
    for r in 1..rows {
        let y = r as f64 * ch;
        let mut line: Vec<P2> = Vec::new();
        for c in 0..cols {
            let e = tab_edge(p2(c as f64 * cw, y), p2((c + 1) as f64 * cw, y), coin(p.seed, r + 1000, c * 2 + 1));
            line.extend(if line.is_empty() { &e[..] } else { &e[1..] });
        }
        out.push(line);
    }
    out
}

/// A shape from outlines (one path), at its own place.
pub fn shape(rings: &[Vec<P2>], layer: usize, name: &str) -> Shape {
    let mut s = Shape::new(ShapeKind::Path(PathData::from_polylines(rings)), Xform::IDENTITY, layer);
    s.name = name.to_string();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;

    fn bounds(rings: &[Vec<P2>]) -> BBox {
        rings.iter().flatten().fold(BBox::EMPTY, |mut b, p| {
            b.add(*p);
            b
        })
    }

    #[test]
    fn a_box_has_the_right_parts_sizes_and_volume() {
        let p = BoxParams { w: 80.0, d: 60.0, h: 40.0, inside: true, thickness: 3.0, finger: 10.0, closed_top: true };
        let parts = finger_box(&p);
        assert_eq!(parts.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Bottom", "Front", "Back", "Left side", "Right side", "Top"]);
        let size = |name: &str| bounds(&parts.iter().find(|(n, _)| n == name).unwrap().1);
        // outside 86 × 66 × 46: front and back are full size, the sides fit between them
        let f = size("Front");
        assert!((f.width() - 86.0).abs() < 1e-9 && (f.height() - 46.0).abs() < 1e-9, "{f:?}");
        let b = size("Bottom");
        assert!((b.width() - 86.0).abs() < 1e-9 && (b.height() - 66.0).abs() < 1e-9, "{b:?}");
        // every part is one closed outline
        assert!(parts.iter().all(|(_, r)| r.len() == 1 && is_closed(&r[0])));
        // the parts fill the box's walls exactly once: their material adds up to the walls' volume
        let area: f64 = parts.iter().map(|(_, r)| crate::ops::to_multipolygon(r).unsigned_area()).sum();
        let (w, d, h, t) = (86.0, 66.0, 46.0, 3.0);
        let walls = w * d * h - (w - 2.0 * t) * (d - 2.0 * t) * (h - 2.0 * t);
        assert!((area * t - walls).abs() < 1e-3, "{} vs {walls}", area * t);
        // walls too thick for the box are kept to a third of its smallest side, so no part comes out empty
        let tiny = finger_box(&BoxParams { w: 5.0, d: 5.0, h: 5.0, inside: false, thickness: 3.0, ..p.clone() });
        assert_eq!(tiny.len(), 6);
        assert!(tiny.iter().all(|(_, r)| !r.is_empty()));
        // an open top: one part fewer, and the walls' volume without the lid
        let open = finger_box(&BoxParams { closed_top: false, ..p });
        assert_eq!(open.len(), 5);
        let area: f64 = open.iter().map(|(_, r)| crate::ops::to_multipolygon(r).unsigned_area()).sum();
        let h = 43.0;
        let walls = w * d * h - (w - 2.0 * t) * (d - 2.0 * t) * (h - t);
        assert!((area * t - walls).abs() < 1e-3, "{} vs {walls}", area * t);
    }

    #[test]
    fn hinge_gear_and_puzzle() {
        let lines = living_hinge(&HingeParams::default());
        assert!(lines.len() > 20 && lines.iter().all(|l| l.len() == 2));
        let b = bounds(&lines);
        assert!(b.min.x >= 0.0 && b.max.x <= 40.0 + 1e-9 && b.min.y >= 0.0 && b.max.y <= 60.0 + 1e-9);
        assert!((b.center().x - 20.0).abs() < 1e-9, "the columns are centred: {b:?}");
        // a 20-tooth module-2 gear: 44 mm across the tips, a 5 mm bore
        let g = gear(&GearParams::default());
        assert_eq!(g.len(), 2);
        let b = bounds(&g[..1]);
        assert!((b.width() - 44.0).abs() < 0.2, "{b:?}");
        assert!(is_closed(&g[0]) && crate::ops::to_multipolygon(&g[..1]).0.len() == 1, "one clean outline");
        assert!(root_diameter(&GearParams::default()) > 5.0 / 0.9);
        let tips = g[0].iter().filter(|p| (p.len() - 22.0).abs() < 1e-6).count();
        assert_eq!(tips, 40, "two tip points per tooth");
        // a 6 × 4 puzzle: the border, 5 column lines and 3 row lines
        let pz = puzzle(&PuzzleParams::default());
        assert_eq!(pz.len(), 1 + 5 + 3);
        let b = bounds(&pz);
        assert!(b.min.x > -1e-9 && b.max.x < 150.0 + 1e-9 && b.min.y > -1e-9 && b.max.y < 100.0 + 1e-9);
        // the same seed gives the same puzzle, another seed another
        assert_eq!(puzzle(&PuzzleParams::default()), pz);
        assert_ne!(puzzle(&PuzzleParams { seed: 2, ..Default::default() }), pz);
    }
}
