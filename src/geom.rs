//! 2-D geometry: points, affine transforms, bounding boxes, curve flattening.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

pub const fn p2(x: f64, y: f64) -> P2 {
    P2 { x, y }
}

impl P2 {
    pub fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn dist(self, o: P2) -> f64 {
        (self - o).len()
    }
    pub fn lerp(self, o: P2, t: f64) -> P2 {
        self + (o - self) * t
    }
    pub fn dot(self, o: P2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: P2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn perp(self) -> P2 {
        p2(-self.y, self.x)
    }
    pub fn normalized(self) -> P2 {
        let l = self.len();
        if l > 1e-12 {
            self * (1.0 / l)
        } else {
            self
        }
    }
}

impl Add for P2 {
    type Output = P2;
    fn add(self, o: P2) -> P2 {
        p2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for P2 {
    type Output = P2;
    fn sub(self, o: P2) -> P2 {
        p2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for P2 {
    type Output = P2;
    fn mul(self, s: f64) -> P2 {
        p2(self.x * s, self.y * s)
    }
}
impl Neg for P2 {
    type Output = P2;
    fn neg(self) -> P2 {
        p2(-self.x, -self.y)
    }
}

/// Affine transform in the same order LightBurn writes `<XForm>a b c d e f</XForm>`:
/// x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Xform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Xform {
    pub const IDENTITY: Xform = Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(dx: f64, dy: f64) -> Xform {
        Xform { e: dx, f: dy, ..Self::IDENTITY }
    }
    pub fn scale(sx: f64, sy: f64) -> Xform {
        Xform { a: sx, d: sy, ..Self::IDENTITY }
    }
    pub fn rotate(rad: f64) -> Xform {
        let (s, c) = rad.sin_cos();
        Xform { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    /// Transform about a fixed point.
    pub fn about(center: P2, t: Xform) -> Xform {
        Xform::translate(center.x, center.y).then_after(t).then_after(Xform::translate(-center.x, -center.y))
    }
    /// `self ∘ other`: apply `other` first, then `self`.
    pub fn then_after(self, o: Xform) -> Xform {
        Xform {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    /// Apply `self` first, then `o`.
    pub fn then(self, o: Xform) -> Xform {
        o.then_after(self)
    }
    pub fn apply(&self, p: P2) -> P2 {
        p2(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }
    pub fn apply_vec(&self, p: P2) -> P2 {
        p2(self.a * p.x + self.c * p.y, self.b * p.x + self.d * p.y)
    }
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    pub fn inverse(&self) -> Option<Xform> {
        let det = self.det();
        if det.abs() < 1e-15 {
            return None;
        }
        let ia = self.d / det;
        let ib = -self.b / det;
        let ic = -self.c / det;
        let id = self.a / det;
        Some(Xform { a: ia, b: ib, c: ic, d: id, e: -(ia * self.e + ic * self.f), f: -(ib * self.e + id * self.f) })
    }
    /// Average scale factor (for stroke widths and tolerances).
    pub fn mean_scale(&self) -> f64 {
        self.det().abs().sqrt().max(1e-9)
    }
    pub fn rotation_deg(&self) -> f64 {
        self.b.atan2(self.a).to_degrees()
    }
    pub fn to_lbrn(&self) -> String {
        format!("{} {} {} {} {} {}", fmt_num(self.a), fmt_num(self.b), fmt_num(self.c), fmt_num(self.d), fmt_num(self.e), fmt_num(self.f))
    }
    pub fn parse(s: &str) -> Option<Xform> {
        let v: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if v.len() == 6 {
            Some(Xform { a: v[0], b: v[1], c: v[2], d: v[3], e: v[4], f: v[5] })
        } else {
            None
        }
    }
}

/// Short number formatting like LightBurn files (up to 8 significant decimals, no trailing zeros).
pub fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: P2,
    pub max: P2,
}

impl BBox {
    pub const EMPTY: BBox = BBox { min: p2(f64::INFINITY, f64::INFINITY), max: p2(f64::NEG_INFINITY, f64::NEG_INFINITY) };

    pub fn from_points<'a>(pts: impl IntoIterator<Item = &'a P2>) -> BBox {
        let mut b = BBox::EMPTY;
        for p in pts {
            b.add(*p);
        }
        b
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }
    pub fn add(&mut self, p: P2) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&self, o: &BBox) -> BBox {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        BBox { min: p2(self.min.x.min(o.min.x), self.min.y.min(o.min.y)), max: p2(self.max.x.max(o.max.x), self.max.y.max(o.max.y)) }
    }
    pub fn width(&self) -> f64 {
        (self.max.x - self.min.x).max(0.0)
    }
    pub fn height(&self) -> f64 {
        (self.max.y - self.min.y).max(0.0)
    }
    pub fn center(&self) -> P2 {
        (self.min + self.max) * 0.5
    }
    pub fn contains(&self, p: P2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn contains_box(&self, o: &BBox) -> bool {
        !o.is_empty() && self.contains(o.min) && self.contains(o.max)
    }
    pub fn intersects(&self, o: &BBox) -> bool {
        !(o.min.x > self.max.x || o.max.x < self.min.x || o.min.y > self.max.y || o.max.y < self.min.y)
    }
    pub fn expand(&self, m: f64) -> BBox {
        BBox { min: self.min - p2(m, m), max: self.max + p2(m, m) }
    }
}

pub fn cubic_point(p0: P2, c0: P2, c1: P2, p1: P2, t: f64) -> P2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c0 * (3.0 * u * u * t) + c1 * (3.0 * u * t * t) + p1 * (t * t * t)
}

/// Flatten a cubic bezier into `out` (excluding the first point) with a chord tolerance.
pub fn flatten_cubic(p0: P2, c0: P2, c1: P2, p1: P2, tol: f64, out: &mut Vec<P2>) {
    let est = (p0.dist(c0) + c0.dist(c1) + c1.dist(p1)).max(1e-9);
    let n = ((est / tol.max(1e-6)).sqrt() * 1.5).ceil().clamp(2.0, 400.0) as usize;
    for i in 1..=n {
        out.push(cubic_point(p0, c0, c1, p1, i as f64 / n as f64));
    }
}

pub fn flatten_quad(p0: P2, c: P2, p1: P2, tol: f64, out: &mut Vec<P2>) {
    flatten_cubic(p0, p0.lerp(c, 2.0 / 3.0), p1.lerp(c, 2.0 / 3.0), p1, tol, out)
}

/// Distance from point to segment.
pub fn dist_to_segment(p: P2, a: P2, b: P2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-18 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

pub fn segments_intersect(a: P2, b: P2, c: P2, d: P2) -> bool {
    let d1 = (b - a).cross(c - a);
    let d2 = (b - a).cross(d - a);
    let d3 = (d - c).cross(a - c);
    let d4 = (d - c).cross(b - c);
    (d1 * d2 < 0.0) && (d3 * d4 < 0.0)
}

/// Even-odd point in polygon test over several closed rings.
pub fn point_in_rings(p: P2, rings: &[Vec<P2>]) -> bool {
    let mut inside = false;
    for ring in rings {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (pi, pj) = (ring[i], ring[j]);
            if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

pub fn polyline_length(pts: &[P2]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

pub fn is_closed(pts: &[P2]) -> bool {
    pts.len() > 2 && pts[0].dist(*pts.last().unwrap()) < 1e-6
}

/// Signed area (positive = counter-clockwise).
pub fn signed_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += pts[i].cross(pts[j]);
    }
    a * 0.5
}

/// Ramer–Douglas–Peucker simplification.
pub fn simplify(pts: &[P2], tol: f64) -> Vec<P2> {
    if pts.len() < 3 || tol <= 0.0 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((s, e)) = stack.pop() {
        let mut best = 0.0;
        let mut idx = 0;
        for i in s + 1..e {
            let d = dist_to_segment(pts[i], pts[s], pts[e]);
            if d > best {
                best = d;
                idx = i;
            }
        }
        if best > tol && idx > 0 {
            keep[idx] = true;
            stack.push((s, idx));
            stack.push((idx, e));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverse_roundtrip() {
        let t = Xform::rotate(0.3).then(Xform::translate(5.0, -2.0)).then(Xform::scale(2.0, 3.0));
        let p = p2(1.5, 7.0);
        let q = t.inverse().unwrap().apply(t.apply(p));
        assert!(p.dist(q) < 1e-9);
    }
    #[test]
    fn fmt() {
        assert_eq!(fmt_num(1.0), "1");
        assert_eq!(fmt_num(0.25), "0.25");
        assert_eq!(fmt_num(-0.0000001), "0");
    }
}
