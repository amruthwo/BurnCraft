//! Sketch geometry: stars, rounded corners, arcs, fillets/chamfers, trim, circular arrays.

use crate::geom::*;
use crate::model::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Points of a star (closed), first point straight up.
pub fn star(points: u32, outer: P2, inner_ratio: f64) -> Vec<P2> {
    let n = points.max(3) as usize;
    let mut pts: Vec<P2> = (0..n * 2)
        .map(|i| {
            let a = FRAC_PI_2 + i as f64 * PI / n as f64;
            let k = if i % 2 == 0 { 1.0 } else { inner_ratio.clamp(0.05, 0.99) };
            p2(outer.x * k * a.cos(), outer.y * k * a.sin())
        })
        .collect();
    pts.push(pts[0]);
    pts
}

/// Star: 2N points at angle 2π·i/(2N), the outer ones on the
/// ellipse and every other one scaled by `ratio`. With `bulge` > 0 the tips get curve handles along the tangent,
/// each `bulge · 0.5 · d` long (d = distance to the next tip), and the inner points get handles 0.33 of the way
/// towards their neighbours' handles.
pub const STAR_DEFAULT_POINTS: u32 = 5;
pub const STAR_DEFAULT_RATIO: f64 = 0.4;
pub const STAR_MAX_POINTS: u32 = 40;
pub const POLYGON_DEFAULT_SIDES: u32 = 6;
pub const POLYGON_MAX_SIDES: u32 = 40;

/// The star the Star tool draws (rounded corners when `radius` > 0 and there's no bulge).
pub fn star_shape_path(points: u32, rx: f64, ry: f64, ratio: f64, bulge: f64, radius: f64) -> PathData {
    let pd = star_path(points, p2(rx, ry), ratio, bulge);
    if radius > 0.0 && bulge <= 0.0 {
        round_corners(&pd.flatten(0.01), radius)
    } else {
        pd
    }
}

pub fn star_path(points: u32, outer: P2, ratio: f64, bulge: f64) -> PathData {
    let n = points.clamp(3, STAR_MAX_POINTS) as usize;
    let m = n * 2;
    let step = TAU / m as f64;
    let pts: Vec<P2> = (0..m)
        .map(|i| {
            let a = step * i as f64;
            let k = if i % 2 == 0 { 1.0 } else { ratio.clamp(0.05, 1.0) };
            p2(a.sin() * outer.x * k, a.cos() * outer.y * k)
        })
        .collect();
    if bulge <= 0.0 {
        return PathData::polyline(&pts, true);
    }
    let mut verts: Vec<Vert> = pts.iter().map(|p| Vert::new(*p)).collect();
    for i in (0..m).step_by(2) {
        let a = step * i as f64;
        let d = pts[i].dist(pts[(i + 2) % m]);
        let tangent = p2(a.cos() * outer.x, -a.sin() * outer.y).normalized();
        let len = bulge * 0.5 * d;
        verts[i].c0 = Some(pts[i] + tangent * len);
        verts[i].c1 = Some(pts[i] - tangent * len);
    }
    for i in (1..m).step_by(2) {
        let prev = verts[i - 1].c0.unwrap();
        let next = verts[(i + 1) % m].c1.unwrap();
        verts[i].c1 = Some(pts[i].lerp(prev, 0.33));
        verts[i].c0 = Some(pts[i].lerp(next, 0.33));
    }
    let prims = (0..m).map(|i| Prim { kind: PrimKind::Bezier, a: i, b: (i + 1) % m }).collect();
    PathData { verts, prims }
}

/// Regular polygon points (closed), first point straight up.
pub fn polygon(n: u32, rx: f64, ry: f64) -> Vec<P2> {
    let n = n.max(3) as usize;
    let mut pts: Vec<P2> = (0..n)
        .map(|i| {
            let a = FRAC_PI_2 + i as f64 * TAU / n as f64;
            p2(rx * a.cos(), ry * a.sin())
        })
        .collect();
    pts.push(pts[0]);
    pts
}

/// Round every corner of a closed polyline with radius `r` (clamped to fit).
pub fn round_corners(closed: &[Vec<P2>], r: f64) -> PathData {
    let mut pd = PathData::from_polylines(closed);
    if r <= 0.0 {
        return pd;
    }
    let spec = Fillet { kind: FilletKind::Round, d1: r, d2: r, rho: 0.5, angle_deg: 45.0 };
    let n = pd.verts.len();
    // process from the end so indices of untouched vertices stay valid
    for v in (0..n).rev() {
        fillet_vertex(&mut pd, v, &spec);
    }
    pd
}

// ------------------------------------------------------------------ arcs

/// Cubic segments approximating a circular arc from angle a0 sweeping `sweep` radians.
pub fn arc_path(center: P2, r: f64, a0: f64, sweep: f64) -> PathData {
    let segs = ((sweep.abs() / FRAC_PI_2).ceil() as usize).max(1);
    let step = sweep / segs as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan() * r;
    let mut verts: Vec<Vert> = Vec::new();
    for i in 0..=segs {
        let a = a0 + step * i as f64;
        let p = center + p2(a.cos(), a.sin()) * r;
        let t = p2(-a.sin(), a.cos());
        verts.push(Vert { p, c0: (i < segs).then(|| p + t * k), c1: (i > 0).then(|| p - t * k) });
    }
    let prims = (0..segs).map(|i| Prim { kind: PrimKind::Bezier, a: i, b: i + 1 }).collect();
    PathData { verts, prims }
}

/// Append `other` to `pd` as its own sub-path(s).
pub fn append_path(pd: &mut PathData, other: &PathData) {
    let base = pd.verts.len();
    pd.verts.extend(other.verts.iter().copied());
    pd.prims.extend(other.prims.iter().map(|p| Prim { kind: p.kind, a: p.a + base, b: p.b + base }));
}

/// A full circle as a path (four quarter arcs).
pub fn circle_path(c: P2, r: f64) -> PathData {
    let mut pd = ellipse_path(r, r);
    for v in &mut pd.verts {
        v.p = v.p + c;
        v.c0 = v.c0.map(|q| q + c);
        v.c1 = v.c1.map(|q| q + c);
    }
    pd
}

/// The circle a closed polyline follows, if every point is within `tol` of it.
pub fn ring_circle(ring: &[P2], tol: f64) -> Option<(P2, f64)> {
    let pts = if is_closed(ring) { &ring[..ring.len() - 1] } else { ring };
    if pts.len() < 8 || !is_closed(ring) {
        return None;
    }
    let c = pts.iter().fold(p2(0.0, 0.0), |a, p| a + *p) * (1.0 / pts.len() as f64);
    let r = pts.iter().map(|p| p.dist(c)).sum::<f64>() / pts.len() as f64;
    // the pieces between the points must hug the circle too (an octagon is not a circle)
    let hugs = (0..pts.len()).all(|k| (pts[k].lerp(pts[(k + 1) % pts.len()], 0.5).dist(c) - r).abs() <= tol);
    (r > tol && hugs && pts.iter().all(|p| (p.dist(c) - r).abs() <= tol)).then_some((c, r))
}

/// Turn a dense polyline (e.g. an offset result) back into lines and true circular arcs: every run of 4 or more
/// points that lies within `tol` of one circle, turning one way in small steps, becomes an arc.
pub fn fit_arcs(line: &[P2], tol: f64) -> PathData {
    let closed = is_closed(line) && line.len() > 3;
    let mut p: Vec<P2> = line.to_vec();
    if closed {
        p.pop();
    }
    let n = p.len();
    if n < 2 {
        return PathData::default();
    }
    if closed {
        if let Some((c, r)) = ring_circle(line, tol) {
            return circle_path(c, r);
        }
        // start on the longest straight piece so an arc isn't cut in two by the seam
        let start = (0..n).max_by(|a, b| p[*a].dist(p[(*a + 1) % n]).partial_cmp(&p[*b].dist(p[(*b + 1) % n])).unwrap()).unwrap_or(0);
        p.rotate_left((start + 1) % n);
    }
    let at = |k: usize| p[k % n];
    let last = if closed { n } else { n - 1 };
    let fits = |i: usize, j: usize| -> Option<(P2, f64, f64)> {
        let (c, r) = circle_3pt(at(i), at((i + j) / 2), at(j))?;
        if !(r.is_finite() && r < 1e5) {
            return None;
        }
        let mut sweep = 0.0;
        let mut sign = 0.0;
        for k in i..j {
            let (u, v) = (at(k) - c, at(k + 1) - c);
            let step = u.cross(v).atan2(u.dot(v));
            if step.abs() > FRAC_PI_2 / 2.0 || (sign != 0.0 && step.signum() != sign) {
                return None;
            }
            sign = step.signum();
            sweep += step;
        }
        if (i..=j).all(|k| (at(k).dist(c) - r).abs() <= tol) && (i..j).all(|k| (at(k).lerp(at(k + 1), 0.5).dist(c) - r).abs() <= tol) {
            Some((c, r, sweep))
        } else {
            None
        }
    };
    let mut pd = PathData { verts: vec![Vert::new(at(0))], prims: Vec::new() };
    let mut i = 0;
    while i < last {
        let mut best = None;
        let mut j = i + 3;
        while j <= last {
            match fits(i, j) {
                Some(f) => {
                    best = Some((j, f));
                    j += 1;
                }
                None => break,
            }
        }
        let (j, arc) = match best {
            Some((j, (c, r, sweep))) => {
                let a0 = (at(i).y - c.y).atan2(at(i).x - c.x);
                (j, Some(arc_path(c, r, a0, sweep)))
            }
            None => (i + 1, None),
        };
        let closing = closed && j == last;
        let cur = pd.verts.len() - 1;
        match arc {
            Some(arc) => {
                pd.verts[cur].c0 = arc.verts[0].c0;
                let mut prev = cur;
                let m = arc.verts.len();
                for (k, v) in arc.verts.iter().enumerate().skip(1) {
                    if k == m - 1 && closing {
                        pd.verts[0].c1 = v.c1;
                        pd.prims.push(Prim { kind: PrimKind::Bezier, a: prev, b: 0 });
                    } else {
                        let mut v = *v;
                        if k == m - 1 {
                            v.p = at(j);
                            v.c0 = None;
                        }
                        pd.verts.push(v);
                        pd.prims.push(Prim { kind: PrimKind::Bezier, a: prev, b: pd.verts.len() - 1 });
                        prev = pd.verts.len() - 1;
                    }
                }
            }
            None => {
                if closing {
                    pd.prims.push(Prim { kind: PrimKind::Line, a: cur, b: 0 });
                } else {
                    pd.verts.push(Vert::new(at(j)));
                    pd.prims.push(Prim { kind: PrimKind::Line, a: cur, b: pd.verts.len() - 1 });
                }
            }
        }
        i = j;
    }
    pd
}

/// Circle through three points, as (centre, radius).
pub fn circle_3pt(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.dot(a), b.dot(b), c.dot(c));
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let ctr = p2(ux, uy);
    Some((ctr, ctr.dist(a)))
}

/// Arc from `a` through `b` to `c`.
pub fn arc_3pt(a: P2, b: P2, c: P2) -> Option<PathData> {
    let (ctr, r) = circle_3pt(a, b, c)?;
    let ang = |p: P2| (p.y - ctr.y).atan2(p.x - ctr.x);
    let (aa, ab, ac) = (ang(a), ang(b), ang(c));
    let norm = |x: f64| x.rem_euclid(TAU);
    // counter-clockwise sweep from a to c; if b isn't on it, go clockwise
    let ccw = norm(ac - aa);
    let sweep = if norm(ab - aa) <= ccw { ccw } else { ccw - TAU };
    Some(arc_path(ctr, r, aa, sweep))
}

/// Arc around `center` starting at `start`, ending at the angle of `end` (radius from start).
pub fn arc_center(center: P2, start: P2, end: P2, ccw: bool) -> PathData {
    let r = center.dist(start);
    let a0 = (start.y - center.y).atan2(start.x - center.x);
    let a1 = (end.y - center.y).atan2(end.x - center.x);
    let mut sweep = (a1 - a0).rem_euclid(TAU);
    if !ccw {
        sweep -= TAU;
    }
    if sweep.abs() < 1e-6 {
        sweep = if ccw { TAU } else { -TAU };
    }
    arc_path(center, r, a0, sweep)
}

/// Arc leaving `start` along `tangent`, ending at `end`.
pub fn arc_tangent(start: P2, tangent: P2, end: P2) -> Option<PathData> {
    let t = tangent.normalized();
    let chord = end - start;
    let n = t.perp();
    let denom = 2.0 * chord.dot(n);
    if denom.abs() < 1e-9 {
        return Some(PathData::polyline(&[start, end], false));
    }
    let r = chord.dot(chord) / denom;
    let center = start + n * r;
    let a0 = (start.y - center.y).atan2(start.x - center.x);
    let a1 = (end.y - center.y).atan2(end.x - center.x);
    let ccw = r > 0.0;
    let mut sweep = (a1 - a0).rem_euclid(TAU);
    if !ccw {
        sweep -= TAU;
    }
    Some(arc_path(center, r.abs(), a0, sweep))
}

// ------------------------------------------------------------------ fillet / chamfer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilletKind {
    Round,
    Conic,
    ChamferEqual,
    ChamferTwo,
    ChamferAngle,
}

impl FilletKind {
    pub const ALL: [FilletKind; 5] = [FilletKind::Round, FilletKind::Conic, FilletKind::ChamferEqual, FilletKind::ChamferTwo, FilletKind::ChamferAngle];
    pub fn label(self) -> &'static str {
        match self {
            FilletKind::Round => "Fillet — round",
            FilletKind::Conic => "Fillet — conic",
            FilletKind::ChamferEqual => "Chamfer — equal sides",
            FilletKind::ChamferTwo => "Chamfer — two distances",
            FilletKind::ChamferAngle => "Chamfer — distance + angle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fillet {
    pub kind: FilletKind,
    /// Radius (fillets) or first distance (chamfers).
    pub d1: f64,
    pub d2: f64,
    /// Conic shape factor 0..1 (0.5 ≈ parabola-like, higher = sharper).
    pub rho: f64,
    pub angle_deg: f64,
}

impl Default for Fillet {
    fn default() -> Self {
        Fillet { kind: FilletKind::Round, d1: 3.0, d2: 3.0, rho: 0.5, angle_deg: 45.0 }
    }
}

/// Replace corner vertex `v` with a fillet or chamfer. Returns false if `v` isn't a corner.
pub fn fillet_vertex(pd: &mut PathData, v: usize, f: &Fillet) -> bool {
    let Some(ip) = pd.prims.iter().position(|p| p.b == v) else { return false };
    let Some(op) = pd.prims.iter().position(|p| p.a == v) else { return false };
    let vp = pd.verts[v].p;
    let prev = pd.prims[ip].a;
    let next = pd.prims[op].b;
    // directions away from the corner, following handles when the neighbours are curves
    let toward = |h: Option<P2>, other: P2| (h.filter(|c| c.dist(vp) > 1e-9).unwrap_or(other) - vp).normalized();
    let u_in = toward(pd.verts[v].c1, pd.verts[prev].p);
    let u_out = toward(pd.verts[v].c0, pd.verts[next].p);
    let cosang = u_in.dot(u_out).clamp(-1.0, 1.0);
    let theta = cosang.acos(); // interior angle at the corner
    if theta < 1e-3 || (PI - theta) < 1e-3 {
        return false;
    }
    let len_in = pd.verts[prev].p.dist(vp);
    let len_out = pd.verts[next].p.dist(vp);
    let (d_in, d_out) = match f.kind {
        FilletKind::Round | FilletKind::Conic => {
            let d = f.d1 / (theta / 2.0).tan();
            (d, d)
        }
        FilletKind::ChamferEqual => (f.d1, f.d1),
        FilletKind::ChamferTwo => (f.d1, f.d2),
        FilletKind::ChamferAngle => {
            let a = f.angle_deg.to_radians().clamp(0.01, PI - theta - 0.01);
            (f.d1, f.d1 * a.sin() / (PI - theta - a).sin())
        }
    };
    let d_in = d_in.min(len_in * 0.999);
    let d_out = d_out.min(len_out * 0.999);
    let p1 = vp + u_in * d_in;
    let p2_ = vp + u_out * d_out;
    let (c0, c1, kind) = match f.kind {
        FilletKind::Round => {
            let turn = PI - theta;
            let r = f.d1.min(d_in.min(d_out) * (theta / 2.0).tan());
            let k = 4.0 / 3.0 * (turn / 4.0).tan() * r;
            (Some(p1 - u_in * k), Some(p2_ - u_out * k), PrimKind::Bezier)
        }
        FilletKind::Conic => {
            let w = f.rho.clamp(0.05, 0.95);
            (Some(p1.lerp(vp, w)), Some(p2_.lerp(vp, w)), PrimKind::Bezier)
        }
        _ => (None, None, PrimKind::Line),
    };
    let old_c0 = pd.verts[v].c0;
    let old_c1 = pd.verts[v].c1;
    // v becomes p1 (end of incoming), a new vertex becomes p2 (start of outgoing)
    pd.verts[v] = Vert { p: p1, c0, c1: old_c1.map(|_| p1 + u_in * (d_in.min(len_in - d_in).max(0.0) / 3.0)) };
    let n = pd.verts.len();
    pd.verts.push(Vert { p: p2_, c0: old_c0.map(|_| p2_ + u_out * (d_out.min(len_out - d_out).max(0.0) / 3.0)), c1 });
    pd.prims[op].a = n;
    pd.prims.insert(ip + 1, Prim { kind, a: v, b: n });
    true
}

// ------------------------------------------------------------------ trim

fn cumulative(l: &[P2]) -> Vec<f64> {
    let mut c = vec![0.0];
    for w in l.windows(2) {
        let last = *c.last().unwrap();
        c.push(last + w[0].dist(w[1]));
    }
    c
}

fn point_at(l: &[P2], cum: &[f64], s: f64) -> P2 {
    let i = match cum.iter().position(|c| *c >= s) {
        Some(0) => return l[0],
        Some(i) => i,
        None => return *l.last().unwrap(),
    };
    let seg = cum[i] - cum[i - 1];
    let t = if seg > 0.0 { (s - cum[i - 1]) / seg } else { 0.0 };
    l[i - 1].lerp(l[i], t)
}

fn sub(l: &[P2], cum: &[f64], a: f64, b: f64) -> Vec<P2> {
    let mut out = vec![point_at(l, cum, a)];
    for (i, c) in cum.iter().enumerate() {
        if *c > a && *c < b {
            out.push(l[i]);
        }
    }
    out.push(point_at(l, cum, b));
    out
}

fn seg_param(a: P2, b: P2, c: P2, d: P2) -> Option<f64> {
    let r = b - a;
    let s = d - c;
    let den = r.cross(s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (c - a).cross(s) / den;
    let u = (c - a).cross(r) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u)).then_some(t.clamp(0.0, 1.0))
}

/// Remove the piece of polyline `target` around `click` between its nearest crossings with `cutters`
/// (and with itself). Returns what's left (possibly nothing).
pub fn trim(target: &[P2], click: P2, cutters: &[Vec<P2>]) -> Vec<Vec<P2>> {
    trim_split(target, click, cutters).0
}

/// Like `trim`, also returning the piece that would be removed (for hover previews).
pub fn trim_split(target: &[P2], click: P2, cutters: &[Vec<P2>]) -> (Vec<Vec<P2>>, Vec<P2>) {
    if target.len() < 2 {
        return (Vec::new(), target.to_vec());
    }
    let closed = is_closed(target);
    let cum = cumulative(target);
    let total = *cum.last().unwrap();
    let mut cuts: Vec<f64> = Vec::new();
    for i in 0..target.len() - 1 {
        let (a, b) = (target[i], target[i + 1]);
        for c in cutters {
            for w in c.windows(2) {
                if let Some(t) = seg_param(a, b, w[0], w[1]) {
                    cuts.push(cum[i] + t * (cum[i + 1] - cum[i]));
                }
            }
        }
        // self crossings with non-adjacent segments
        for j in 0..target.len() - 1 {
            if j + 1 >= i && j <= i + 1 {
                continue;
            }
            if closed && ((i == 0 && j == target.len() - 2) || (j == 0 && i == target.len() - 2)) {
                continue;
            }
            if let Some(t) = seg_param(a, b, target[j], target[j + 1]) {
                cuts.push(cum[i] + t * (cum[i + 1] - cum[i]));
            }
        }
    }
    let eps = total * 1e-6 + 1e-9;
    cuts.retain(|s| closed || (*s > eps && *s < total - eps));
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cuts.dedup_by(|a, b| (*a - *b).abs() < eps * 10.0);
    // where along the line was the click?
    let mut sc = 0.0;
    let mut best = f64::INFINITY;
    for i in 0..target.len() - 1 {
        let (a, b) = (target[i], target[i + 1]);
        let ab = b - a;
        let l2 = ab.dot(ab);
        let t = if l2 > 0.0 { ((click - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = click.dist(a + ab * t);
        if d < best {
            best = d;
            sc = cum[i] + t * (cum[i + 1] - cum[i]);
        }
    }
    if closed {
        if cuts.len() < 2 {
            return (Vec::new(), target.to_vec());
        }
        let s0 = cuts.iter().copied().filter(|s| *s < sc).last().unwrap_or(*cuts.last().unwrap());
        let s1 = cuts.iter().copied().find(|s| *s > sc).unwrap_or(cuts[0]);
        // keep s1 → s0 going forwards, wrapping past the start
        let piece = if s1 < s0 {
            sub(target, &cum, s1, s0)
        } else {
            let mut a = sub(target, &cum, s1, total);
            a.extend(sub(target, &cum, 0.0, s0).into_iter().skip(1));
            a
        };
        let removed = if s0 < s1 {
            sub(target, &cum, s0, s1)
        } else {
            let mut a = sub(target, &cum, s0, total);
            a.extend(sub(target, &cum, 0.0, s1).into_iter().skip(1));
            a
        };
        return (vec![piece], removed);
    }
    let s0 = cuts.iter().copied().filter(|s| *s < sc).last().unwrap_or(0.0);
    let s1 = cuts.iter().copied().find(|s| *s > sc).unwrap_or(total);
    let mut out = Vec::new();
    if s0 > eps {
        out.push(sub(target, &cum, 0.0, s0));
    }
    if s1 < total - eps {
        out.push(sub(target, &cum, s1, total));
    }
    (out, sub(target, &cum, s0, s1))
}

// ------------------------------------------------------------------ circular array

/// Reflection about the line through `a` and `b`.
pub fn mirror_xform(a: P2, b: P2) -> Option<Xform> {
    let d = (b - a).normalized();
    if d.len() < 0.5 {
        return None;
    }
    let (m00, m01, m11) = (d.x * d.x - d.y * d.y, 2.0 * d.x * d.y, d.y * d.y - d.x * d.x);
    let m = Xform { a: m00, b: m01, c: m01, d: m11, e: 0.0, f: 0.0 };
    let t = a - m.apply(a);
    Some(Xform { e: t.x, f: t.y, ..m })
}

/// Mirrored copies of the selection about the line a–b; returns the copies' indices.
pub fn mirror_copy(doc: &mut Document, sel: &[usize], a: P2, b: P2) -> Vec<usize> {
    let Some(m) = mirror_xform(a, b) else { return Vec::new() };
    let mut out = Vec::new();
    for &i in sel {
        let Some(s) = doc.shapes.get(i) else { continue };
        let mut c = s.clone();
        c.renew_ids();
        c.xform = m.then_after(c.xform);
        doc.shapes.push(c);
        out.push(doc.shapes.len() - 1);
    }
    out
}

/// The longest outline of a shape (world), used as the curve for patterns.
fn curve_of(doc: &Document, curve: usize) -> Option<Vec<P2>> {
    doc.shapes.get(curve)?.world_outline(0.02).into_iter().max_by(|a, b| polyline_length(a).partial_cmp(&polyline_length(b)).unwrap())
}

/// `n` stations (point, direction angle) spread evenly along a polyline: from end to end on an open curve,
/// evenly all the way round on a closed one.
pub fn stations(line: &[P2], n: usize) -> Vec<(P2, f64)> {
    let total = polyline_length(line);
    if n == 0 || line.len() < 2 || total < 1e-9 {
        return Vec::new();
    }
    let closed = is_closed(line);
    let step = if closed { total / n as f64 } else if n > 1 { total / (n - 1) as f64 } else { 0.0 };
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut walked) = (0usize, 0.0);
    for i in 0..n {
        let want = if !closed && n == 1 { total / 2.0 } else { (step * i as f64).min(total) };
        while seg + 1 < line.len() - 1 && walked + line[seg].dist(line[seg + 1]) < want {
            walked += line[seg].dist(line[seg + 1]);
            seg += 1;
        }
        let (a, b) = (line[seg], line[seg + 1]);
        let len = a.dist(b).max(1e-12);
        let t = ((want - walked) / len).clamp(0.0, 1.0);
        out.push((a.lerp(b, t), (b.y - a.y).atan2(b.x - a.x)));
    }
    out
}

/// Where a shape is held when it is placed on a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveAnchor {
    Centre,
    /// The bottom middle sits on the curve (like text on a path).
    Bottom,
}

fn anchor_point(b: &BBox, anchor: CurveAnchor) -> P2 {
    match anchor {
        CurveAnchor::Centre => b.center(),
        CurveAnchor::Bottom => p2(b.center().x, b.min.y),
    }
}

/// Copy Along Path dialog (same options as LightBurn's): Number of Copies
/// 1–2000 (default 4, chosen by default), or Space Between Copies up to 2000 (default 10); Rotate Copies off;
/// Scale Copies 1–2000 % (default 100).
pub const PATH_COPIES_DEFAULT: usize = 4;
pub const PATH_COPIES_MAX: usize = 2000;
pub const PATH_SPACING_DEFAULT: f64 = 10.0;
pub const PATH_SPACING_MAX: f64 = 2000.0;
/// Below this spacing nothing is made.
pub const PATH_SPACING_MIN: f64 = 0.001;
pub const PATH_SCALE_DEFAULT: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathCopyOpts {
    /// Use `spacing` (Space Between Copies) instead of `copies` (Number of Copies).
    pub by_spacing: bool,
    pub copies: usize,
    pub spacing: f64,
    pub rotate: bool,
    pub scale: bool,
    /// Size of the last copy, in percent of the original.
    pub scale_pct: f64,
}

impl Default for PathCopyOpts {
    fn default() -> Self {
        PathCopyOpts { by_spacing: false, copies: PATH_COPIES_DEFAULT, spacing: PATH_SPACING_DEFAULT, rotate: false, scale: false, scale_pct: PATH_SCALE_DEFAULT }
    }
}

/// Distance along a polyline of the point nearest `p`.
fn nearest_distance(line: &[P2], p: P2) -> f64 {
    let (mut walked, mut best) = (0.0, (f64::MAX, 0.0));
    for s in line.windows(2) {
        let ab = s[1] - s[0];
        let l = ab.len();
        let t = if l > 1e-12 { ((p - s[0]).dot(ab) / (l * l)).clamp(0.0, 1.0) } else { 0.0 };
        let d = (s[0] + ab * t).dist(p);
        if d < best.0 {
            best = (d, walked + l * t);
        }
        walked += l;
    }
    best.1
}

/// Point and direction at distance `d` along a polyline (wrapping round a closed one).
fn point_dir_at(line: &[P2], d: f64) -> (P2, f64) {
    let total = polyline_length(line);
    let d = if is_closed(line) && total > 0.0 { d.rem_euclid(total) } else { d.clamp(0.0, total) };
    let mut walked = 0.0;
    for s in line.windows(2) {
        let l = s[0].dist(s[1]);
        if walked + l >= d && l > 1e-12 {
            let t = (d - walked) / l;
            return (s[0].lerp(s[1], t), (s[1].y - s[0].y).atan2(s[1].x - s[0].x));
        }
        walked += l;
    }
    let n = line.len();
    (line[n - 1], (line[n - 1].y - line[n - 2].y).atan2(line[n - 1].x - line[n - 2].x))
}

/// Copy Along Path: the copies start from
/// the point of the path nearest the selection's centre and keep the selection's
/// offset from it. With a count, the spacing is length ÷ copies on a closed path and length ÷ (copies − 1) on an
/// open one; with a spacing, the count is ⌊length ÷ spacing⌋ + 1. A closed path stops half a step before
/// coming round again, an open one at its end. Rotate turns each copy by how much the path has turned since
/// the start; Scale grows copy i to 1 + min(1, i / copies) · (scale − 1). The selection itself is copy 0.
pub fn path_pattern(doc: &mut Document, sel: &[usize], curve: usize, o: &PathCopyOpts) -> Vec<usize> {
    let Some(line) = curve_of(doc, curve) else { return Vec::new() };
    let items: Vec<usize> = sel.iter().copied().filter(|i| *i != curve).collect();
    if items.is_empty() || line.len() < 2 {
        return Vec::new();
    }
    let total = polyline_length(&line);
    let closed = is_closed(&line);
    let center = crate::ops::selection_bbox(doc, &items).center();
    let d0 = nearest_distance(&line, center);
    let avail = if closed { total } else { total - d0 };
    let (copies, step) = if o.by_spacing {
        let s = o.spacing;
        if s < PATH_SPACING_MIN {
            return Vec::new();
        }
        ((avail / s) as usize + 1, s)
    } else {
        let div = if closed { o.copies } else { o.copies.saturating_sub(1) };
        if div == 0 {
            return Vec::new();
        }
        (o.copies, avail / div as f64)
    };
    if step < PATH_SPACING_MIN {
        return Vec::new();
    }
    let end = if closed { avail - step * 0.5 } else { avail + 0.001 };
    let (p0, a0) = point_dir_at(&line, d0);
    let k_last = o.scale_pct / 100.0;
    let mut out = Vec::new();
    let mut i = 1usize;
    let mut dist = step;
    while dist < end && i < PATH_COPIES_MAX {
        let (p, a) = point_dir_at(&line, d0 + dist);
        let k = if o.scale { 1.0 + (i as f64 / copies.max(1) as f64).min(1.0) * (k_last - 1.0) } else { 1.0 };
        let t = Xform::translate(p.x, p.y)
            .then_after(if o.rotate { Xform::rotate(a - a0) } else { Xform::IDENTITY })
            .then_after(Xform::scale(k, k))
            .then_after(Xform::translate(-p0.x, -p0.y));
        for &s in &items {
            let mut c = doc.shapes[s].clone();
            c.renew_ids();
            c.xform = t.then_after(c.xform);
            doc.shapes.push(c);
            out.push(doc.shapes.len() - 1);
        }
        i += 1;
        dist += step * if o.scale { k } else { 1.0 };
    }
    out
}

/// Mirror Across Line: the last
/// selected shape is the mirror line. It must be a single segment; a curve counts if its points at t = 0.25,
/// 0.5 and 0.75 are within 0.25 mm of the chord. Returns the line's end points.
pub const MIRROR_STRAIGHT_TOL: f64 = 0.25;

pub fn mirror_line_of(s: &Shape) -> Option<(P2, P2)> {
    let ShapeKind::Path(pd) = &s.kind else { return None };
    let [pr] = pd.prims.as_slice() else { return None };
    let (va, vb) = (pd.verts.get(pr.a)?, pd.verts.get(pr.b)?);
    let (a, b) = (s.xform.apply(va.p), s.xform.apply(vb.p));
    if pr.kind == PrimKind::Bezier {
        let (c0, c1) = (s.xform.apply(va.c0.unwrap_or(va.p)), s.xform.apply(vb.c1.unwrap_or(vb.p)));
        for t in [0.25, 0.5, 0.75] {
            if dist_to_segment(cubic_point(a, c0, c1, b, t), a, b) > MIRROR_STRAIGHT_TOL {
                return None;
            }
        }
    }
    (a.dist(b) > 1e-9).then_some((a, b))
}

/// Move the selected shapes onto the curve shape `curve`, in their left-to-right order, evenly spaced;
/// `rotate` turns each to follow the curve.
pub fn align_to_path(doc: &mut Document, sel: &[usize], curve: usize, rotate: bool, anchor: CurveAnchor) -> bool {
    let Some(line) = curve_of(doc, curve) else { return false };
    let mut items: Vec<(usize, BBox)> = sel.iter().copied().filter(|i| *i != curve).map(|i| (i, doc.shapes[i].bbox())).collect();
    if items.is_empty() {
        return false;
    }
    items.sort_by(|a, b| a.1.center().x.partial_cmp(&b.1.center().x).unwrap());
    for ((i, b), (p, ang)) in items.iter().zip(stations(&line, items.len())) {
        let r = anchor_point(b, anchor);
        let t = Xform::translate(p.x, p.y).then_after(if rotate { Xform::rotate(ang) } else { Xform::IDENTITY }).then_after(Xform::translate(-r.x, -r.y));
        let s = &mut doc.shapes[*i];
        s.xform = t.then_after(s.xform);
    }
    true
}

/// Copy the selection `count - 1` times around `center`. `span_deg` = 360 spreads copies evenly round
/// a full circle; anything else spaces them from the first to the last over that angle.
pub fn circular_array(doc: &mut Document, sel: &[usize], center: P2, count: usize, span_deg: f64, rotate_copies: bool) -> Vec<usize> {
    let count = count.max(2);
    let full = (span_deg.abs() - 360.0).abs() < 1e-6;
    let step = if full { span_deg / count as f64 } else { span_deg / (count - 1) as f64 };
    let originals: Vec<Shape> = sel.iter().map(|i| doc.shapes[*i].clone()).collect();
    let sel_center = crate::ops::selection_bbox(doc, sel).center();
    let mut new_sel = sel.to_vec();
    for k in 1..count {
        let a = (step * k as f64).to_radians();
        let t = if rotate_copies {
            Xform::about(center, Xform::rotate(a))
        } else {
            let moved = Xform::about(center, Xform::rotate(a)).apply(sel_center);
            Xform::translate(moved.x - sel_center.x, moved.y - sel_center.y)
        };
        for o in &originals {
            let mut s = o.clone();
            s.renew_ids();
            s.xform = t.then_after(s.xform);
            doc.shapes.push(s);
            new_sel.push(doc.shapes.len() - 1);
        }
    }
    new_sel
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_point_arc_passes_through() {
        let pd = arc_3pt(p2(10.0, 0.0), p2(0.0, 10.0), p2(-10.0, 0.0)).unwrap();
        let f = pd.flatten(0.01);
        let top = f[0].iter().fold(f64::MIN, |m, p| m.max(p.y));
        assert!((top - 10.0).abs() < 0.05);
    }
    #[test]
    fn mirror_line_must_be_one_straight_segment() {
        let line = Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0)], false)), Xform::IDENTITY, 0);
        assert!(mirror_line_of(&line).is_some());
        let two = Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 5.0)], false)), Xform::IDENTITY, 0);
        assert!(mirror_line_of(&two).is_none());
        let arc = Shape::new(ShapeKind::Path(arc_path(p2(0.0, 0.0), 10.0, 0.0, 1.0)), Xform::IDENTITY, 0);
        assert!(mirror_line_of(&arc).is_none(), "a bowed curve is not a mirror line");
    }
    #[test]
    fn mirror_about_a_slanted_line() {
        let m = mirror_xform(p2(0.0, 0.0), p2(1.0, 1.0)).unwrap();
        assert!(m.apply(p2(3.0, 0.0)).dist(p2(0.0, 3.0)) < 1e-12);
        let m = mirror_xform(p2(5.0, 0.0), p2(5.0, 10.0)).unwrap();
        assert!(m.apply(p2(2.0, 4.0)).dist(p2(8.0, 4.0)) < 1e-12);
    }
    #[test]
    fn pattern_along_a_line_and_align() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(100.0, 0.0)], false)), Xform::IDENTITY, 0));
        d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 2.0, ry: 2.0 }, Xform::translate(50.0, 50.0), 0));
        // LightBurn: 5 copies on an open 100 mm path from its start → spacing 25, the original is copy 0
        d.shapes[1].xform = Xform::translate(0.0, 10.0);
        let made = path_pattern(&mut d, &[1, 0], 0, &PathCopyOpts { copies: 5, ..Default::default() });
        assert_eq!(made.len(), 4);
        let xs: Vec<f64> = made.iter().map(|i| d.shapes[*i].bbox().center().x).collect();
        assert!(xs.iter().zip([25.0, 50.0, 75.0, 100.0]).all(|(a, b)| (a - b).abs() < 0.05), "{xs:?}");
        // the copies keep the original's 10 mm offset from the path
        assert!(made.iter().all(|i| (d.shapes[*i].bbox().center().y - 10.0).abs() < 0.05));
        // by spacing: ⌊100 / 30⌋ + 1 = 4 places, so 3 copies
        let made = path_pattern(&mut d, &[1, 0], 0, &PathCopyOpts { by_spacing: true, spacing: 30.0, ..Default::default() });
        assert_eq!(made.len(), 3);
        d.shapes[1].xform = Xform::translate(50.0, 50.0);
        assert!(align_to_path(&mut d, &[1, 0], 0, false, CurveAnchor::Bottom));
        let b = d.shapes[1].bbox();
        assert!(b.min.y.abs() < 0.05 && (b.center().x - 50.0).abs() < 0.05, "{b:?}");
    }
    #[test]
    fn fillet_square_corner() {
        let mut pd = PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0), p2(0.0, 10.0)], true);
        assert!(fillet_vertex(&mut pd, 1, &Fillet { d1: 2.0, ..Default::default() }));
        let f = pd.flatten(0.01);
        assert!(is_closed(&f[0]));
        let b = BBox::from_points(&f[0]);
        assert!((b.width() - 10.0).abs() < 1e-6);
        // the corner point itself is gone
        assert!(f[0].iter().all(|p| p.dist(p2(10.0, 0.0)) > 0.5));
    }
    #[test]
    fn chamfer_angle_distance() {
        let mut pd = PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0)], false);
        assert!(fillet_vertex(&mut pd, 1, &Fillet { kind: FilletKind::ChamferAngle, d1: 2.0, angle_deg: 45.0, ..Default::default() }));
        assert_eq!(pd.verts.len(), 4);
        assert!(pd.verts.iter().any(|v| v.p.dist(p2(10.0, 2.0)) < 1e-6));
    }
    #[test]
    fn trim_middle_of_line() {
        let line = vec![p2(0.0, 0.0), p2(10.0, 0.0)];
        let cutters = vec![vec![p2(3.0, -1.0), p2(3.0, 1.0)], vec![p2(7.0, -1.0), p2(7.0, 1.0)]];
        let left = trim(&line, p2(5.0, 0.0), &cutters);
        assert_eq!(left.len(), 2);
        assert!((left[0].last().unwrap().x - 3.0).abs() < 1e-9);
        assert!((left[1][0].x - 7.0).abs() < 1e-9);
    }
    #[test]
    fn trim_closed_square() {
        let sq = vec![p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0), p2(0.0, 10.0), p2(0.0, 0.0)];
        let cutter = vec![vec![p2(5.0, -5.0), p2(5.0, 15.0)]];
        let left = trim(&sq, p2(10.0, 5.0), &cutter);
        assert_eq!(left.len(), 1);
        let b = BBox::from_points(&left[0]);
        assert!(b.max.x < 5.0 + 1e-6);
    }
    #[test]
    fn circular_copies() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Rect { w: 2.0, h: 2.0, cr: 0.0 }, Xform::translate(10.0, 0.0), 0));
        let s = circular_array(&mut d, &[0], p2(0.0, 0.0), 4, 360.0, true);
        assert_eq!(s.len(), 4);
        assert!(d.shapes[2].bbox().center().dist(p2(-10.0, 0.0)) < 1e-6);
    }
}
