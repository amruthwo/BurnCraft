//! CAD-style smart guides ("inference"): snap to endpoints, midpoints, centres, quadrants and crossings; line up
//! with points you hovered over (horizontal / vertical / aligned); draw parallel or perpendicular to a line you
//! hovered over; fall back to "on path" and the grid. Like Onshape / SolidWorks / Fusion sketch inference.
//!
//! Distances and penalties below are in screen pixels.

use crate::geom::*;

/// Object snap distance in screen pixels.
pub const OBJ_SNAP_PX: f64 = 10.0;
/// "Nearest point on a path" loses to a real point unless it is this many pixels closer.
pub const PATH_PENALTY_PX: f64 = 2.0;
/// The grid only snaps within this many pixels of a grid point...
pub const GRID_REACH_PX: f64 = 12.0;
/// ...and loses to an object snap unless it is this many pixels closer.
pub const GRID_PENALTY_PX: f64 = 4.0;
/// Lines and curves this close to the cursor are tested for crossings...
pub const CROSS_PX: f64 = 8.0;
/// ...but only when 2 to 29 of them are near.
pub const CROSS_MAX: usize = 29;
/// Angle of the extra ellipse snap points (45°).
pub const ELLIPSE_DIAGONAL_RAD: f64 = 0.785398;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Endpoint,
    Midpoint,
    Center,
    Quadrant,
    /// Ellipse points at 45°.
    Diagonal,
    Intersection,
    OnPath,
    Grid,
}

impl PointKind {
    pub fn label(self) -> &'static str {
        match self {
            PointKind::Endpoint => "Endpoint",
            PointKind::Midpoint => "Midpoint",
            PointKind::Center => "Center",
            PointKind::Quadrant => "Quadrant",
            PointKind::Diagonal => "45°",
            PointKind::Intersection => "Intersection",
            PointKind::OnPath => "On path",
            PointKind::Grid => "Grid",
        }
    }
}

/// Snap targets near the cursor.
#[derive(Default, Clone, Debug)]
pub struct Scene {
    pub points: Vec<(P2, PointKind)>,
    /// Straight segments (for parallel / perpendicular references).
    pub segments: Vec<(P2, P2)>,
    /// All outlines near the cursor (for "on path" and crossings).
    pub outlines: Vec<Vec<P2>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub a: P2,
    pub b: P2,
    pub label: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct Inference {
    pub p: P2,
    pub mark: Option<(P2, PointKind)>,
    pub guides: Vec<Guide>,
}

/// Where two segments cross (both parameters inside 0..1), if they do.
fn seg_cross(a0: P2, a1: P2, b0: P2, b1: P2) -> Option<P2> {
    let r = a1 - a0;
    let s = b1 - b0;
    let den = r.cross(s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (b0 - a0).cross(s) / den;
    let u = (b0 - a0).cross(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a0 + r * t)
}

/// Crossings of the outline pieces near `w` (the nearby primitives are intersected pairwise).
fn crossings(w: P2, near: f64, scene: &Scene) -> Vec<P2> {
    let mut segs: Vec<(usize, usize, P2, P2)> = Vec::new();
    for (li, l) in scene.outlines.iter().enumerate() {
        for (si, s) in l.windows(2).enumerate() {
            if dist_to_segment(w, s[0], s[1]) < near {
                segs.push((li, si, s[0], s[1]));
                if segs.len() > CROSS_MAX {
                    return Vec::new();
                }
            }
        }
    }
    let mut out = Vec::new();
    if segs.len() < 2 {
        return out;
    }
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let (la, sa, a0, a1) = segs[i];
            let (lb, sb, b0, b1) = segs[j];
            // neighbouring pieces of the same outline only meet at their shared vertex
            if la == lb && sa.abs_diff(sb) <= 1 {
                continue;
            }
            if let Some(p) = seg_cross(a0, a1, b0, b1) {
                out.push(p);
            }
        }
    }
    out
}

/// Infer the best point for cursor `w`. `px` is one screen pixel in world units; `anchor` is the previous point
/// of the shape being drawn; `acquired` are points the user rested on; `ref_seg` a line the user rested on;
/// `grid` the grid snap spacing when grid snapping is on.
pub fn infer(w: P2, px: f64, scene: &Scene, anchor: Option<P2>, acquired: &[P2], ref_seg: Option<(P2, P2)>, grid: Option<f64>) -> Inference {
    let tol = OBJ_SNAP_PX * px;
    let grid_pt = grid.filter(|g| *g > 0.0).map(|g| p2((w.x / g).round() * g, (w.y / g).round() * g)).filter(|q| q.dist(w) < GRID_REACH_PX * px);

    // 1. object snaps: nearest wins, "on path" carries a 2 px handicap (score = distance + penalty)
    let mut best: Option<(f64, P2, PointKind)> = None;
    let mut offer = |score: f64, p: P2, k: PointKind| {
        if best.map(|(b, _, _)| score < b).unwrap_or(true) {
            best = Some((score, p, k));
        }
    };
    for (p, k) in &scene.points {
        let d = p.dist(w);
        if d <= tol {
            offer(d, *p, *k);
        }
    }
    for p in crossings(w, CROSS_PX * px, scene) {
        let d = p.dist(w);
        if d <= tol {
            offer(d, p, PointKind::Intersection);
        }
    }
    for l in &scene.outlines {
        for s in l.windows(2) {
            let ab = s[1] - s[0];
            let l2 = ab.dot(ab);
            if l2 < 1e-18 {
                continue;
            }
            let t = ((w - s[0]).dot(ab) / l2).clamp(0.0, 1.0);
            let q = s[0] + ab * t;
            let d = q.dist(w);
            if d <= tol {
                offer(d + PATH_PENALTY_PX * px, q, PointKind::OnPath);
            }
        }
    }
    if let Some((score, p, k)) = best {
        // the grid point beats the object snap only when it is 4 px closer
        if let Some(q) = grid_pt.filter(|q| q.dist(w) + GRID_PENALTY_PX * px < score) {
            return Inference { p: q, mark: Some((q, PointKind::Grid)), guides: Vec::new() };
        }
        let mut guides = Vec::new();
        if let Some(a) = anchor {
            if (p.y - a.y).abs() < 1e-6 && (p.x - a.x).abs() > 1e-6 {
                guides.push(Guide { a, b: p, label: "Horizontal" });
            } else if (p.x - a.x).abs() < 1e-6 && (p.y - a.y).abs() > 1e-6 {
                guides.push(Guide { a, b: p, label: "Vertical" });
            }
        }
        return Inference { p, mark: Some((p, k)), guides };
    }

    // 2. alignment with the anchor (horizontal / vertical) and with acquired points (aligned)
    let mut refs: Vec<(P2, bool)> = acquired.iter().map(|p| (*p, false)).collect();
    if let Some(a) = anchor {
        refs.insert(0, (a, true));
    }
    let mut x_lock: Option<(f64, P2, bool)> = None;
    let mut y_lock: Option<(f64, P2, bool)> = None;
    for (r, is_anchor) in &refs {
        let dx = (w.x - r.x).abs();
        let dy = (w.y - r.y).abs();
        if dx < tol && x_lock.map(|(d, _, _)| dx < d).unwrap_or(true) {
            x_lock = Some((dx, *r, *is_anchor));
        }
        if dy < tol && y_lock.map(|(d, _, _)| dy < d).unwrap_or(true) {
            y_lock = Some((dy, *r, *is_anchor));
        }
    }
    if x_lock.is_some() || y_lock.is_some() {
        let mut p = w;
        let mut guides = Vec::new();
        if let Some((_, r, is_anchor)) = x_lock {
            p.x = r.x;
            guides.push((r, is_anchor, true));
        }
        if let Some((_, r, is_anchor)) = y_lock {
            p.y = r.y;
            guides.push((r, is_anchor, false));
        }
        let guides = guides
            .into_iter()
            .map(|(r, is_anchor, vertical)| Guide { a: r, b: p, label: if is_anchor { if vertical { "Vertical" } else { "Horizontal" } } else { "Aligned" } })
            .collect();
        return Inference { p, mark: None, guides };
    }

    // 3. parallel / perpendicular to a line the user rested on
    if let (Some(a), Some((s0, s1))) = (anchor, ref_seg) {
        let v = w - a;
        let len = v.len();
        let d = (s1 - s0).normalized();
        if len > tol && d.len() > 0.5 {
            let ang_tol = (tol / len).min(0.08);
            for (dir, label) in [(d, "Parallel"), (d.perp(), "Perpendicular")] {
                let cross = v.normalized().cross(dir).abs();
                if cross < ang_tol {
                    let p = a + dir * v.dot(dir);
                    return Inference { p, mark: None, guides: vec![Guide { a, b: p, label }, Guide { a: s0, b: s1, label: "" }] };
                }
            }
        }
    }

    // 4. grid
    if let Some(q) = grid_pt {
        return Inference { p: q, mark: None, guides: Vec::new() };
    }
    Inference { p: w, mark: None, guides: Vec::new() }
}

/// Shift lock for the line tool: compare |dy| − |dx| with −length,
/// 0 and +length and take the nearest: horizontal, 45° or vertical. The 45° case uses the larger of |dx|, |dy|
/// for both, so the point stays on the side of the cursor rather than keeping the length.
pub fn axis_snap(from: P2, p: P2) -> P2 {
    let d = p - from;
    let len = d.len();
    if len < 1e-9 {
        return p;
    }
    let diff = d.y.abs() - d.x.abs();
    let (to_h, to_45, to_v) = ((diff + len).abs(), diff.abs(), (diff - len).abs());
    let d = if to_v > to_h && to_45 > to_h {
        p2(d.x, 0.0)
    } else if to_h > to_v && to_45 > to_v {
        p2(0.0, d.y)
    } else if to_h > to_45 && to_v > to_45 {
        let m = d.x.abs().max(d.y.abs());
        p2(m.copysign(d.x), m.copysign(d.y))
    } else {
        d
    };
    from + d
}

/// The straight segment nearest to `w` within `tol`, if any (to remember as a parallel/perpendicular reference).
pub fn segment_near(w: P2, tol: f64, scene: &Scene) -> Option<(P2, P2)> {
    scene
        .segments
        .iter()
        .map(|s| (dist_to_segment(w, s.0, s.1), *s))
        .filter(|(d, _)| *d < tol)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> Scene {
        Scene {
            points: vec![(p2(0.0, 0.0), PointKind::Endpoint), (p2(5.0, 0.0), PointKind::Midpoint), (p2(10.0, 0.0), PointKind::Endpoint)],
            segments: vec![(p2(0.0, 0.0), p2(10.0, 0.0))],
            outlines: vec![vec![p2(0.0, 0.0), p2(10.0, 0.0)]],
        }
    }
    #[test]
    fn snaps_to_endpoint_and_midpoint() {
        let i = infer(p2(9.8, 0.2), 0.05, &scene(), None, &[], None, None);
        assert_eq!(i.mark.unwrap().1, PointKind::Endpoint);
        let i = infer(p2(5.1, 0.1), 0.05, &scene(), None, &[], None, None);
        assert_eq!(i.p, p2(5.0, 0.0));
    }
    #[test]
    fn on_path_needs_to_be_two_pixels_closer() {
        // 0.3 from the endpoint, 0.1 from the line: the line wins only by more than 2 px (0.1 here)
        let i = infer(p2(9.7, 0.1), 0.05, &scene(), None, &[], None, None);
        assert_eq!(i.mark.unwrap().1, PointKind::OnPath);
        let i = infer(p2(9.9, 0.1), 0.05, &scene(), None, &[], None, None);
        assert_eq!(i.mark.unwrap().1, PointKind::Endpoint);
    }
    #[test]
    fn crossing_lines_snap_to_their_intersection() {
        let sc = Scene { outlines: vec![vec![p2(-5.0, 0.0), p2(5.0, 0.0)], vec![p2(0.0, -5.0), p2(0.0, 5.0)]], ..Default::default() };
        let i = infer(p2(0.1, 0.12), 0.05, &sc, None, &[], None, None);
        assert_eq!(i.mark.unwrap().1, PointKind::Intersection);
        assert!(i.p.dist(p2(0.0, 0.0)) < 1e-9);
    }
    #[test]
    fn grid_reach_and_penalty() {
        // nothing near: grid within 12 px
        let i = infer(p2(1.3, 2.2), 0.01, &Scene::default(), None, &[], None, Some(1.0));
        assert!(i.p.dist(p2(1.0, 2.0)) > 1e-9, "1.3,2.2 is more than 12 px from a grid point");
        let i = infer(p2(1.02, 2.03), 0.01, &Scene::default(), None, &[], None, Some(1.0));
        assert_eq!(i.p, p2(1.0, 2.0));
    }
    #[test]
    fn horizontal_and_aligned() {
        let i = infer(p2(20.0, 3.3), 0.05, &Scene::default(), Some(p2(0.0, 3.0)), &[], None, None);
        assert_eq!(i.p, p2(20.0, 3.0));
        assert_eq!(i.guides[0].label, "Horizontal");
        let i = infer(p2(7.2, 30.0), 0.05, &Scene::default(), None, &[p2(7.0, 0.0)], None, None);
        assert_eq!(i.p.x, 7.0);
        assert_eq!(i.guides[0].label, "Aligned");
    }
    #[test]
    fn parallel_and_perpendicular() {
        let seg = Some((p2(0.0, 0.0), p2(10.0, 10.0)));
        let j = infer(p2(15.0, 5.2), 0.05, &Scene::default(), Some(p2(10.0, 0.0)), &[], seg, None);
        assert_eq!(j.guides[0].label, "Parallel");
        let k = infer(p2(5.1, 5.0), 0.05, &Scene::default(), Some(p2(10.0, 0.0)), &[], seg, None);
        assert_eq!(k.guides[0].label, "Perpendicular");
    }
    #[test]
    fn shift_lock_is_horizontal_vertical_or_45() {
        let o = p2(0.0, 0.0);
        assert_eq!(axis_snap(o, p2(10.0, 2.0)), p2(10.0, 0.0));
        assert_eq!(axis_snap(o, p2(-1.0, -9.0)), p2(0.0, -9.0));
        assert_eq!(axis_snap(o, p2(10.0, -8.0)), p2(10.0, -10.0));
    }
}
