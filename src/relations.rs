//! Sketch relations (CAD "constraints"): horizontal, vertical, coincident, parallel, perpendicular, equal,
//! midpoint, tangent and concentric between nodes, lines and circles of top-level shapes. They are made by the
//! smart guides while drawing, or by picking items with the Relations tool, and they are kept by a small
//! iterative solver whenever a dimension changes something.
//!
//! Stored in the project as the root attribute `BcRelations` (JSON), which LightBurn ignores.

use crate::geom::*;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Something a relation refers to. Shapes are named by their id so relations survive reordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ent {
    /// Node `v` of a path.
    Pt { shape: u64, v: usize },
    /// Centre of a circle (ellipse shape).
    Center { shape: u64 },
    /// Straight segment between nodes `a` and `b` of a path.
    Line { shape: u64, a: usize, b: usize },
    /// A circle (ellipse shape with equal radii).
    Circle { shape: u64 },
}

impl Ent {
    pub fn shape(&self) -> u64 {
        match *self {
            Ent::Pt { shape, .. } | Ent::Center { shape } | Ent::Line { shape, .. } | Ent::Circle { shape } => shape,
        }
    }
    fn is_point(&self) -> bool {
        matches!(self, Ent::Pt { .. } | Ent::Center { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelKind {
    Horizontal,
    Vertical,
    Coincident,
    Parallel,
    Perpendicular,
    Equal,
    Midpoint,
    Tangent,
    Concentric,
    /// Driving dimensions (set with the dimension tool): a line's length, a circle's diameter, and the
    /// angle between two lines (degrees), kept by the solver like any other relation.
    Distance,
    Diameter,
    Angle,
}

impl RelKind {
    pub const ALL: [RelKind; 9] =
        [RelKind::Horizontal, RelKind::Vertical, RelKind::Coincident, RelKind::Parallel, RelKind::Perpendicular, RelKind::Equal, RelKind::Midpoint, RelKind::Tangent, RelKind::Concentric];
    pub fn label(self) -> &'static str {
        match self {
            RelKind::Horizontal => "Horizontal",
            RelKind::Vertical => "Vertical",
            RelKind::Coincident => "Coincident",
            RelKind::Parallel => "Parallel",
            RelKind::Perpendicular => "Perpendicular",
            RelKind::Equal => "Equal",
            RelKind::Midpoint => "Midpoint",
            RelKind::Tangent => "Tangent",
            RelKind::Concentric => "Concentric",
            RelKind::Distance => "Length",
            RelKind::Diameter => "Diameter",
            RelKind::Angle => "Angle",
        }
    }
    /// Driving dimensions are drawn by the dimension tool, not as relation tags.
    pub fn is_dimension(self) -> bool {
        matches!(self, RelKind::Distance | RelKind::Diameter | RelKind::Angle)
    }
    /// Short glyph drawn next to the items.
    pub fn glyph(self) -> &'static str {
        match self {
            RelKind::Horizontal => "H",
            RelKind::Vertical => "V",
            RelKind::Coincident => "•",
            RelKind::Parallel => "//",
            RelKind::Perpendicular => "90°",
            RelKind::Equal => "=",
            RelKind::Midpoint => "M",
            RelKind::Tangent => "T",
            RelKind::Concentric => "CC",
            RelKind::Distance => "D",
            RelKind::Diameter => "Ø",
            RelKind::Angle => "A",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelKind,
    pub ents: Vec<Ent>,
    /// The value of a driving dimension (mm, or degrees for angles).
    #[serde(default)]
    pub value: f64,
}

/// Which relations make sense for the picked items, in a stable order.
pub fn applicable(picks: &[Ent]) -> Vec<RelKind> {
    use RelKind::*;
    let pts = picks.iter().filter(|e| e.is_point()).count();
    let lines = picks.iter().filter(|e| matches!(e, Ent::Line { .. })).count();
    let circles = picks.iter().filter(|e| matches!(e, Ent::Circle { .. })).count();
    let n = picks.len();
    let mut out = Vec::new();
    if (n == 1 && lines == 1) || (n == 2 && pts == 2) {
        out.extend([Horizontal, Vertical]);
    }
    if n == 2 && (pts == 2 || (pts == 1 && (lines == 1 || circles == 1))) {
        out.push(Coincident);
    }
    if n == 2 && lines == 2 {
        out.extend([Parallel, Perpendicular, Equal]);
    }
    if n == 2 && circles == 2 {
        out.extend([Equal, Tangent, Concentric]);
    }
    if n == 2 && pts == 1 && lines == 1 {
        out.push(Midpoint);
    }
    if n == 2 && lines == 1 && circles == 1 {
        out.push(Tangent);
    }
    if n == 2 && pts == 1 && circles == 1 && matches!(picks.iter().find(|e| e.is_point()), Some(Ent::Center { .. })) {
        out.push(Concentric);
    }
    out
}

/// Put the picked items in the order the solver expects (points first, then lines, then circles).
pub fn make(kind: RelKind, picks: &[Ent]) -> Relation {
    let mut ents = picks.to_vec();
    ents.sort_by_key(|e| match e {
        Ent::Pt { .. } | Ent::Center { .. } => 0,
        Ent::Line { .. } => 1,
        Ent::Circle { .. } => 2,
    });
    // a centre picked for "concentric" acts as its circle
    if kind == RelKind::Concentric {
        for e in &mut ents {
            if let Ent::Center { shape } = *e {
                *e = Ent::Circle { shape };
            }
        }
    }
    Relation { kind, ents, value: 0.0 }
}

// ------------------------------------------------------------------ geometry access

/// Point variables (world space) by key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    V(u64, usize),
    C(u64),
    R(u64),
}

struct Vars {
    pts: HashMap<Key, P2>,
    radii: HashMap<Key, f64>,
    fixed: HashSet<Key>,
}

fn shape_by_id(doc: &Document, id: u64) -> Option<&Shape> {
    doc.shapes.iter().find(|s| s.id == id)
}

fn circle_of(s: &Shape) -> Option<(P2, f64)> {
    let ShapeKind::Ellipse { rx, ry } = s.kind else { return None };
    let (sx, sy) = (p2(s.xform.a, s.xform.b).len(), p2(s.xform.c, s.xform.d).len());
    ((rx * sx - ry * sy).abs() <= 1e-6 * (rx * sx).max(1.0)).then(|| (s.xform.apply(p2(0.0, 0.0)), rx * sx))
}

/// World position of a node, if it exists.
pub fn node_pos(doc: &Document, shape: u64, v: usize) -> Option<P2> {
    let s = shape_by_id(doc, shape)?;
    let ShapeKind::Path(pd) = &s.kind else { return None };
    pd.verts.get(v).map(|vt| s.xform.apply(vt.p))
}

/// Does this entity still exist?
pub fn valid(doc: &Document, e: &Ent) -> bool {
    match *e {
        Ent::Pt { shape, v } => node_pos(doc, shape, v).is_some(),
        Ent::Line { shape, a, b } => {
            let Some(s) = shape_by_id(doc, shape) else { return false };
            let ShapeKind::Path(pd) = &s.kind else { return false };
            pd.prims.iter().any(|p| p.kind == PrimKind::Line && ((p.a == a && p.b == b) || (p.a == b && p.b == a)))
        }
        Ent::Center { shape } | Ent::Circle { shape } => shape_by_id(doc, shape).and_then(circle_of).is_some(),
    }
}

/// Points of an entity as variable keys (lines give two, circles their centre).
fn keys(e: &Ent) -> Vec<Key> {
    match *e {
        Ent::Pt { shape, v } => vec![Key::V(shape, v)],
        Ent::Center { shape } | Ent::Circle { shape } => vec![Key::C(shape)],
        Ent::Line { shape, a, b } => vec![Key::V(shape, a), Key::V(shape, b)],
    }
}

/// Where to draw an entity's glyph (world).
pub fn anchor(doc: &Document, e: &Ent) -> Option<P2> {
    match *e {
        Ent::Pt { shape, v } => node_pos(doc, shape, v),
        Ent::Line { shape, a, b } => Some(node_pos(doc, shape, a)?.lerp(node_pos(doc, shape, b)?, 0.5)),
        Ent::Center { shape } => shape_by_id(doc, shape).and_then(circle_of).map(|c| c.0),
        Ent::Circle { shape } => shape_by_id(doc, shape).and_then(circle_of).map(|(c, r)| c + p2(r * 0.7071, r * 0.7071)),
    }
}

fn gather(doc: &Document, rels: &[Relation], fixed: &[Key]) -> Vars {
    let mut v = Vars { pts: HashMap::new(), radii: HashMap::new(), fixed: fixed.iter().copied().collect() };
    for r in rels {
        for e in &r.ents {
            for k in keys(e) {
                match k {
                    Key::V(s, i) => {
                        if let Some(p) = node_pos(doc, s, i) {
                            v.pts.insert(k, p);
                        }
                    }
                    Key::C(s) => {
                        if let Some((c, rad)) = shape_by_id(doc, s).and_then(circle_of) {
                            v.pts.insert(k, c);
                            v.radii.insert(Key::R(s), rad);
                        }
                    }
                    Key::R(_) => {}
                }
            }
        }
    }
    v
}

impl Vars {
    fn p(&self, k: Key) -> P2 {
        self.pts[&k]
    }
    fn free(&self, k: Key) -> bool {
        !self.fixed.contains(&k)
    }
    fn line(&self, e: &Ent) -> Option<(Key, Key)> {
        match *e {
            Ent::Line { shape, a, b } => Some((Key::V(shape, a), Key::V(shape, b))),
            _ => None,
        }
    }
    fn radius(&self, shape: u64) -> f64 {
        self.radii[&Key::R(shape)]
    }
}

/// Write solved variables back into the shapes (nodes move with their curve handles; circles move / resize).
fn write_back(doc: &mut Document, v: &Vars) {
    for s in &mut doc.shapes {
        let id = s.id;
        let Some(inv) = s.xform.inverse() else { continue };
        match &mut s.kind {
            ShapeKind::Path(pd) => {
                for (i, vt) in pd.verts.iter_mut().enumerate() {
                    if let Some(p) = v.pts.get(&Key::V(id, i)) {
                        let d = inv.apply(*p) - vt.p;
                        if d.len() > 0.0 {
                            vt.p = vt.p + d;
                            vt.c0 = vt.c0.map(|c| c + d);
                            vt.c1 = vt.c1.map(|c| c + d);
                        }
                    }
                }
            }
            ShapeKind::Ellipse { rx, ry } => {
                if let Some(c) = v.pts.get(&Key::C(id)) {
                    let now = s.xform.apply(p2(0.0, 0.0));
                    s.xform.e += c.x - now.x;
                    s.xform.f += c.y - now.y;
                }
                if let Some(r) = v.radii.get(&Key::R(id)) {
                    let sx = p2(s.xform.a, s.xform.b).len().max(1e-12);
                    let k = (r / sx) / rx.max(1e-12);
                    *rx *= k;
                    *ry *= k;
                }
            }
            _ => {}
        }
    }
}

/// Drop relations whose items are gone (deleted shapes or nodes).
pub fn prune(doc: &mut Document) {
    let rels = std::mem::take(&mut doc.relations);
    doc.relations = rels.into_iter().filter(|r| r.ents.iter().all(|e| valid(doc, e))).collect();
}

/// How strongly an angle error (radians) counts against a length error (mm) in the solver.
const ANGLE_WEIGHT: f64 = 20.0;
/// Radii are solved in units this size, so the solver would rather move a circle than resize it (as in CAD).
const RADIUS_SCALE: f64 = 0.05;

/// Things a relation fixes when solving starts: which side of a line a tangent circle sits on, which way an
/// angle turns, and whether two tangent circles touch outside or inside.
fn senses(v: &Vars, rels: &[Relation]) -> Vec<f64> {
    rels.iter()
        .map(|r| match (r.kind, r.ents.as_slice()) {
            (RelKind::Angle, [l1, l2]) => {
                let (c, k1, k2) = angle_keys(v, l1, l2);
                let (u1, u2) = (v.p(k1) - v.p(c), v.p(k2) - v.p(c));
                if u1.cross(u2) < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            }
            (RelKind::Tangent, [l @ Ent::Line { .. }, Ent::Circle { shape }]) => {
                let (ka, kb) = v.line(l).unwrap();
                let n = (v.p(kb) - v.p(ka)).perp().normalized();
                if (v.p(Key::C(*shape)) - v.p(ka)).dot(n) < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            }
            (RelKind::Tangent, [Ent::Circle { shape: s1 }, Ent::Circle { shape: s2 }]) => {
                let d = v.p(Key::C(*s1)).dist(v.p(Key::C(*s2)));
                let (r1, r2) = (v.radius(*s1), v.radius(*s2));
                if (d - (r1 + r2)).abs() <= (d - (r1 - r2).abs()).abs() {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => 1.0,
        })
        .collect()
}

/// The corner two lines share, and their far ends.
fn angle_keys(v: &Vars, l1: &Ent, l2: &Ent) -> (Key, Key, Key) {
    let (a1, b1) = v.line(l1).unwrap();
    let (a2, b2) = v.line(l2).unwrap();
    if a1 == a2 {
        (a1, b1, b2)
    } else if a1 == b2 {
        (a1, b1, a2)
    } else if b1 == a2 {
        (b1, a1, b2)
    } else if b1 == b2 {
        (b1, a1, a2)
    } else {
        (a1, b1, b2)
    }
}

fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let mut a = a % t;
    if a > std::f64::consts::PI {
        a -= t;
    }
    if a < -std::f64::consts::PI {
        a += t;
    }
    a
}

/// How far each relation is from holding (one or two numbers per relation; 0 = holds).
fn residuals(v: &Vars, rels: &[Relation], sense: &[f64], out: &mut Vec<f64>) {
    use RelKind::*;
    out.clear();
    for (r, s) in rels.iter().zip(sense) {
        let pt = |e: &Ent| keys(e)[0];
        match (r.kind, r.ents.as_slice()) {
            (Horizontal | Vertical, [l @ Ent::Line { .. }]) => {
                let (ka, kb) = v.line(l).unwrap();
                let d = v.p(kb) - v.p(ka);
                out.push(if r.kind == Horizontal { d.y } else { d.x });
            }
            (Horizontal | Vertical, [p, q]) if p.is_point() && q.is_point() => {
                let d = v.p(pt(q)) - v.p(pt(p));
                out.push(if r.kind == Horizontal { d.y } else { d.x });
            }
            (Coincident, [p, q]) if p.is_point() && q.is_point() => {
                let d = v.p(pt(q)) - v.p(pt(p));
                out.extend([d.x, d.y]);
            }
            (Coincident, [p, l @ Ent::Line { .. }]) => {
                let (ka, kb) = v.line(l).unwrap();
                let (a, b) = (v.p(ka), v.p(kb));
                out.push((b - a).normalized().cross(v.p(pt(p)) - a));
            }
            (Coincident, [p, Ent::Circle { shape }]) => out.push(v.p(pt(p)).dist(v.p(Key::C(*shape))) - v.radius(*shape)),
            (Midpoint, [p, l @ Ent::Line { .. }]) => {
                let (ka, kb) = v.line(l).unwrap();
                let d = v.p(ka).lerp(v.p(kb), 0.5) - v.p(pt(p));
                out.extend([d.x, d.y]);
            }
            (Parallel | Perpendicular, [l1 @ Ent::Line { .. }, l2 @ Ent::Line { .. }]) => {
                let (a1, b1) = v.line(l1).unwrap();
                let (a2, b2) = v.line(l2).unwrap();
                let (d1, d2) = ((v.p(b1) - v.p(a1)).normalized(), (v.p(b2) - v.p(a2)).normalized());
                out.push(ANGLE_WEIGHT * if r.kind == Parallel { d1.cross(d2) } else { d1.dot(d2) });
            }
            (Equal, [l1 @ Ent::Line { .. }, l2 @ Ent::Line { .. }]) => {
                let (a1, b1) = v.line(l1).unwrap();
                let (a2, b2) = v.line(l2).unwrap();
                out.push(v.p(a1).dist(v.p(b1)) - v.p(a2).dist(v.p(b2)));
            }
            (Equal, [Ent::Circle { shape: s1 }, Ent::Circle { shape: s2 }]) => out.push(v.radius(*s1) - v.radius(*s2)),
            (Concentric, [Ent::Circle { shape: s1 }, Ent::Circle { shape: s2 }]) => {
                let d = v.p(Key::C(*s2)) - v.p(Key::C(*s1));
                out.extend([d.x, d.y]);
            }
            (Tangent, [l @ Ent::Line { .. }, Ent::Circle { shape }]) => {
                let (ka, kb) = v.line(l).unwrap();
                let n = (v.p(kb) - v.p(ka)).perp().normalized();
                out.push((v.p(Key::C(*shape)) - v.p(ka)).dot(n) * s - v.radius(*shape));
            }
            (Tangent, [Ent::Circle { shape: s1 }, Ent::Circle { shape: s2 }]) => {
                let d = v.p(Key::C(*s1)).dist(v.p(Key::C(*s2)));
                let (r1, r2) = (v.radius(*s1), v.radius(*s2));
                out.push(if *s > 0.0 { d - (r1 + r2) } else { d - (r1 - r2).abs() });
            }
            (Distance, [l @ Ent::Line { .. }]) => {
                let (ka, kb) = v.line(l).unwrap();
                out.push(v.p(ka).dist(v.p(kb)) - r.value);
            }
            (Diameter, [Ent::Circle { shape }]) => out.push(2.0 * v.radius(*shape) - r.value),
            (Angle, [l1 @ Ent::Line { .. }, l2 @ Ent::Line { .. }]) => {
                let (c, k1, k2) = angle_keys(v, l1, l2);
                let (u1, u2) = (v.p(k1) - v.p(c), v.p(k2) - v.p(c));
                let cur = u1.cross(u2).atan2(u1.dot(u2));
                out.push(ANGLE_WEIGHT * wrap(cur - s * r.value.to_radians()));
            }
            _ => {}
        }
    }
}

/// Solve a small dense linear system A x = b (Gaussian elimination with partial pivoting).
fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let piv = (c..n).max_by(|i, j| a[*i][c].abs().partial_cmp(&a[*j][c].abs()).unwrap())?;
        if a[piv][c].abs() < 1e-14 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            if f != 0.0 {
                for k in c..n {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}

/// Make every relation hold at once, moving the sketch as little as possible and never moving `fixed` points /
/// radii. Like a CAD sketch it first tries moving things without resizing circles, and only resizes them if it
/// must. Returns the largest remaining error (mm; angles weighted).
pub fn solve(doc: &mut Document, fixed: &[Key]) -> f64 {
    prune(doc);
    let mut keep_sizes = fixed.to_vec();
    for r in &doc.relations {
        for e in &r.ents {
            if let Ent::Circle { shape } = e {
                keep_sizes.push(Key::R(*shape));
            }
        }
    }
    let before = doc.clone();
    let e = solve_with(doc, &keep_sizes);
    if e < 1e-6 {
        return e;
    }
    *doc = before;
    solve_with(doc, fixed)
}

/// Solve only the relations tied (directly or through other shapes) to the shapes `ids`. If they can't all be
/// met, nothing is moved: an impossible relation never distorts the sketch. Returns the remaining error.
pub fn solve_connected(doc: &mut Document, ids: &[u64], fixed: &[Key]) -> f64 {
    prune(doc);
    let mut set: HashSet<u64> = ids.iter().copied().collect();
    loop {
        let mut grew = false;
        for r in &doc.relations {
            if r.ents.iter().any(|e| set.contains(&e.shape())) {
                for e in &r.ents {
                    grew |= set.insert(e.shape());
                }
            }
        }
        if !grew {
            break;
        }
    }
    let all = std::mem::take(&mut doc.relations);
    doc.relations = all.iter().filter(|r| r.ents.iter().any(|e| set.contains(&e.shape()))).cloned().collect();
    if doc.relations.is_empty() {
        doc.relations = all;
        return 0.0;
    }
    let before = doc.shapes.clone();
    let lines: Vec<Ent> = doc.relations.iter().flat_map(|r| r.ents.iter().copied()).filter(|e| matches!(e, Ent::Line { .. } | Ent::Circle { .. })).collect();
    let size = |doc: &Document, e: &Ent| -> f64 {
        match *e {
            Ent::Line { shape, a, b } => match (node_pos(doc, shape, a), node_pos(doc, shape, b)) {
                (Some(p), Some(q)) => p.dist(q),
                _ => 0.0,
            },
            Ent::Circle { shape } => doc.shapes.iter().find(|s| s.id == shape).and_then(circle_of).map(|c| c.1).unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let sizes: Vec<f64> = lines.iter().map(|e| size(doc, e)).collect();
    let mut e = solve(doc, fixed);
    doc.relations = all;
    if lines.iter().zip(&sizes).any(|(l, s0)| *s0 > 1e-6 && size(doc, l) < 1e-3 * s0.max(1.0)) {
        e = e.max(1.0);
    }
    if e > 1e-4 {
        doc.shapes = before;
    }
    e
}

/// Damped least squares (Levenberg–Marquardt) over the free points and radii.
fn solve_with(doc: &mut Document, fixed: &[Key]) -> f64 {
    if doc.relations.is_empty() {
        return 0.0;
    }
    let rels = doc.relations.clone();
    let mut v = gather(doc, &rels, fixed);
    // the unknowns: free point coordinates and free radii
    let mut pts: Vec<Key> = v.pts.keys().copied().filter(|k| v.free(*k)).collect();
    pts.sort_by_key(|k| format!("{k:?}"));
    let mut rads: Vec<Key> = v.radii.keys().copied().filter(|k| v.free(*k)).collect();
    rads.sort_by_key(|k| format!("{k:?}"));
    let n = pts.len() * 2 + rads.len();
    let sense = senses(&v, &rels);
    let mut r = Vec::new();
    residuals(&v, &rels, &sense, &mut r);
    let worst = |r: &[f64]| r.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if n == 0 || worst(&r) < 1e-9 {
        return worst(&r);
    }
    let get = |v: &Vars| -> Vec<f64> {
        let mut x = Vec::with_capacity(n);
        for k in &pts {
            let p = v.pts[k];
            x.extend([p.x, p.y]);
        }
        for k in &rads {
            x.push(v.radii[k] / RADIUS_SCALE);
        }
        x
    };
    let set = |v: &mut Vars, x: &[f64]| {
        for (i, k) in pts.iter().enumerate() {
            v.pts.insert(*k, p2(x[2 * i], x[2 * i + 1]));
        }
        for (i, k) in rads.iter().enumerate() {
            v.radii.insert(*k, (x[pts.len() * 2 + i] * RADIUS_SCALE).max(1e-6));
        }
    };
    let x0 = get(&v);
    let mut x = x0.clone();
    let cost = |r: &[f64]| r.iter().map(|e| e * e).sum::<f64>();
    let mut lambda = 1e-3;
    let mut rt = Vec::new();
    for _ in 0..100 {
        let c0 = cost(&r);
        if worst(&r) < 1e-9 {
            break;
        }
        // numerical Jacobian
        let m = r.len();
        let mut jac = vec![vec![0.0; n]; m];
        for j in 0..n {
            let h = 1e-7 * (1.0 + x[j].abs());
            let mut xp = x.clone();
            xp[j] += h;
            set(&mut v, &xp);
            residuals(&v, &rels, &sense, &mut rt);
            for i in 0..m {
                jac[i][j] = (rt[i] - r[i]) / h;
            }
        }
        // normal equations; the damping also pulls towards the starting sketch (smallest change)
        let mut jtj = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            for a in 0..n {
                let ja = jac[i][a];
                if ja == 0.0 {
                    continue;
                }
                g[a] += ja * r[i];
                for b in 0..n {
                    jtj[a][b] += ja * jac[i][b];
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut a = jtj.clone();
            let mut rhs: Vec<f64> = g.iter().map(|e| -e).collect();
            for k in 0..n {
                a[k][k] += lambda * (1.0 + jtj[k][k]);
                rhs[k] -= 1e-9 * (x[k] - x0[k]);
            }
            let Some(dx) = solve_linear(a, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let xn: Vec<f64> = x.iter().zip(&dx).map(|(a, b)| a + b).collect();
            set(&mut v, &xn);
            residuals(&v, &rels, &sense, &mut rt);
            if cost(&rt) < c0 {
                x = xn;
                std::mem::swap(&mut r, &mut rt);
                lambda = (lambda * 0.3).max(1e-9);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        set(&mut v, &x);
        if !improved {
            break;
        }
    }
    set(&mut v, &x);
    write_back(doc, &v);
    worst(&r)
}

/// Keys to hold still for an entity (its points, and a circle's radius too).
pub fn hold(e: &Ent) -> Vec<Key> {
    let mut k = keys(e);
    if let Ent::Circle { shape } = *e {
        k.push(Key::R(shape));
    }
    k
}

/// Every key of a top-level shape (all its nodes, or its centre and radius).
pub fn shape_keys(doc: &Document, shape_index: usize) -> Vec<Key> {
    let n = match doc.shapes.get(shape_index).map(|s| &s.kind) {
        Some(ShapeKind::Path(pd)) => pd.verts.len(),
        _ => 0,
    };
    keys_of(doc, shape_index, &(0..n).collect::<Vec<_>>())
}

/// Lines and circles of other shapes passing through `p` (for "on path" and crossing snaps).
pub fn items_through(doc: &Document, p: P2, skip_shape: u64) -> Vec<Ent> {
    let mut out = Vec::new();
    for s in doc.shapes.iter() {
        if s.id == skip_shape {
            continue;
        }
        match &s.kind {
            ShapeKind::Path(pd) => {
                for pr in &pd.prims {
                    if pr.kind == PrimKind::Line && dist_to_segment(p, s.xform.apply(pd.verts[pr.a].p), s.xform.apply(pd.verts[pr.b].p)) < 1e-6 {
                        out.push(Ent::Line { shape: s.id, a: pr.a, b: pr.b });
                    }
                }
            }
            ShapeKind::Ellipse { .. } => {
                if let Some((c, r)) = circle_of(s) {
                    if (c.dist(p) - r).abs() < 1e-4 {
                        out.push(Ent::Circle { shape: s.id });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Keys of the points a dimension holds still while the other relations are solved.
pub fn keys_of(doc: &Document, shape_index: usize, verts: &[usize]) -> Vec<Key> {
    let Some(s) = doc.shapes.get(shape_index) else { return Vec::new() };
    let mut out: Vec<Key> = verts.iter().map(|v| Key::V(s.id, *v)).collect();
    if matches!(s.kind, ShapeKind::Ellipse { .. }) {
        out.push(Key::C(s.id));
        out.push(Key::R(s.id));
    }
    out
}

// ------------------------------------------------------------------ finding items

/// The relation item nearest `w` within `tol` (world): a node, circle centre, straight segment or circle.
pub fn pick(doc: &Document, w: P2, tol: f64) -> Option<Ent> {
    let mut best: Option<(f64, Ent)> = None;
    let mut offer = |d: f64, e: Ent, bias: f64| {
        if d <= tol && best.map(|(b, _)| d + bias < b).unwrap_or(true) {
            best = Some((d + bias, e));
        }
    };
    for s in doc.shapes.iter().rev() {
        match &s.kind {
            ShapeKind::Path(pd) => {
                for (i, vt) in pd.verts.iter().enumerate().take(5000) {
                    offer(s.xform.apply(vt.p).dist(w), Ent::Pt { shape: s.id, v: i }, 0.0);
                }
                for pr in pd.prims.iter().take(5000) {
                    if pr.kind == PrimKind::Line {
                        let (a, b) = (s.xform.apply(pd.verts[pr.a].p), s.xform.apply(pd.verts[pr.b].p));
                        // lines lose to nodes near their ends
                        offer(dist_to_segment(w, a, b), Ent::Line { shape: s.id, a: pr.a, b: pr.b }, tol * 0.4);
                    }
                }
            }
            ShapeKind::Ellipse { .. } => {
                if let Some((c, r)) = circle_of(s) {
                    offer(c.dist(w), Ent::Center { shape: s.id }, 0.0);
                    offer((c.dist(w) - r).abs(), Ent::Circle { shape: s.id }, tol * 0.4);
                }
            }
            _ => {}
        }
    }
    best.map(|b| b.1)
}

/// The item at an exact world point (a node or a circle centre), used to turn snaps into relations.
pub fn ent_at(doc: &Document, p: P2, skip_shape: u64) -> Option<Ent> {
    for s in doc.shapes.iter().rev() {
        if s.id == skip_shape {
            continue;
        }
        match &s.kind {
            ShapeKind::Path(pd) => {
                if let Some(i) = pd.verts.iter().position(|v| s.xform.apply(v.p).dist(p) < 1e-6) {
                    return Some(Ent::Pt { shape: s.id, v: i });
                }
            }
            ShapeKind::Ellipse { .. } => {
                if circle_of(s).map(|(c, _)| c.dist(p) < 1e-6).unwrap_or(false) {
                    return Some(Ent::Center { shape: s.id });
                }
            }
            _ => {}
        }
    }
    None
}

/// The straight segment with these end points (either way round), or the one whose midpoint is `mid`.
pub fn line_at(doc: &Document, a: P2, b: Option<P2>, skip_shape: u64) -> Option<Ent> {
    for s in doc.shapes.iter().rev() {
        if s.id == skip_shape {
            continue;
        }
        let ShapeKind::Path(pd) = &s.kind else { continue };
        for pr in &pd.prims {
            if pr.kind != PrimKind::Line {
                continue;
            }
            let (p, q) = (s.xform.apply(pd.verts[pr.a].p), s.xform.apply(pd.verts[pr.b].p));
            let hit = match b {
                Some(b) => (p.dist(a) < 1e-6 && q.dist(b) < 1e-6) || (p.dist(b) < 1e-6 && q.dist(a) < 1e-6),
                None => p.lerp(q, 0.5).dist(a) < 1e-6,
            };
            if hit {
                return Some(Ent::Line { shape: s.id, a: pr.a, b: pr.b });
            }
        }
    }
    None
}

// ------------------------------------------------------------------ offsets

/// An offset result and the shape it was made from, so its distance stays editable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetLink {
    pub result: u64,
    pub source: u64,
    pub opts: crate::ops::OffsetOpts,
}

/// The link for an offset result, if its source still exists.
pub fn offset_link(doc: &Document, result: u64) -> Option<OffsetLink> {
    doc.offsets.iter().rev().find(|l| l.result == result && doc.shapes.iter().any(|s| s.id == l.source)).copied()
}

/// Remake an offset result at a new distance; keeps the shape's id, layer and name.
pub fn set_offset(doc: &mut Document, result: u64, dist: f64) -> bool {
    let Some(mut link) = offset_link(doc, result) else { return false };
    link.opts.dist = dist;
    let Some(src) = doc.shapes.iter().find(|s| s.id == link.source) else { return false };
    let Some((kind, x)) = crate::ops::offset_shape(src, &link.opts) else { return false };
    let Some(s) = doc.shapes.iter_mut().find(|s| s.id == result) else { return false };
    s.kind = kind;
    s.xform = x;
    for l in doc.offsets.iter_mut().filter(|l| l.result == result) {
        l.opts.dist = dist;
    }
    true
}

fn offsets_to_file(doc: &Document) -> Option<String> {
    use crate::ops::{OffsetDir, OffsetJoin};
    let pos = |id: u64| doc.shapes.iter().position(|s| s.id == id);
    let v: Vec<serde_json::Value> = doc
        .offsets
        .iter()
        .filter_map(|l| {
            let o = &l.opts;
            let dir = match o.dir {
                OffsetDir::Outward => 0,
                OffsetDir::Inward => 1,
                OffsetDir::Both => 2,
            };
            let join = match o.join {
                OffsetJoin::Round => 0,
                OffsetJoin::Bevel => 1,
                OffsetJoin::Corner => 2,
            };
            Some(serde_json::json!({ "result": pos(l.result)?, "source": pos(l.source)?, "dist": o.dist, "dir": dir, "join": join, "outer": o.outer_only, "optimize": o.optimize }))
        })
        .collect();
    (!v.is_empty()).then(|| serde_json::Value::Array(v).to_string())
}

fn offsets_from_file(doc: &Document, attr: &str) -> Vec<OffsetLink> {
    use crate::ops::{OffsetDir, OffsetJoin, OffsetOpts};
    let Ok(serde_json::Value::Array(v)) = serde_json::from_str::<serde_json::Value>(attr) else { return Vec::new() };
    v.iter()
        .filter_map(|o| {
            let id = |k: &str| o.get(k)?.as_u64().and_then(|i| doc.shapes.get(i as usize)).map(|s| s.id);
            let opts = OffsetOpts {
                dist: o.get("dist")?.as_f64()?,
                dir: match o.get("dir").and_then(|x| x.as_u64()) {
                    Some(1) => OffsetDir::Inward,
                    Some(2) => OffsetDir::Both,
                    _ => OffsetDir::Outward,
                },
                join: match o.get("join").and_then(|x| x.as_u64()) {
                    Some(1) => OffsetJoin::Bevel,
                    Some(2) => OffsetJoin::Corner,
                    _ => OffsetJoin::Round,
                },
                outer_only: o.get("outer").and_then(|x| x.as_bool()).unwrap_or(false),
                optimize: o.get("optimize").and_then(|x| x.as_bool()).unwrap_or(true),
                ..OffsetOpts::default()
            };
            Some(OffsetLink { result: id("result")?, source: id("source")?, opts })
        })
        .collect()
}

// ------------------------------------------------------------------ file

pub fn to_attr(rels: &[Relation]) -> Option<String> {
    (!rels.is_empty()).then(|| serde_json::to_string(rels).unwrap_or_default())
}

pub fn from_attr(s: &str) -> Vec<Relation> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Replace every shape reference (ids ⇄ list positions for the file); relations that can't be mapped are dropped.
pub fn remap(rels: &[Relation], f: impl Fn(u64) -> Option<u64>) -> Vec<Relation> {
    rels.iter()
        .filter_map(|r| {
            let ents = r
                .ents
                .iter()
                .map(|e| {
                    Some(match *e {
                        Ent::Pt { shape, v } => Ent::Pt { shape: f(shape)?, v },
                        Ent::Center { shape } => Ent::Center { shape: f(shape)? },
                        Ent::Line { shape, a, b } => Ent::Line { shape: f(shape)?, a, b },
                        Ent::Circle { shape } => Ent::Circle { shape: f(shape)? },
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Relation { kind: r.kind, ents, value: r.value })
        })
        .collect()
}

/// For the file (root attributes `BcRelations`, `BcOffsets`): shape ids become positions in the shape list.
pub fn to_file(doc: &Document) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(r) = to_attr(&remap(&doc.relations, |id| doc.shapes.iter().position(|s| s.id == id).map(|i| i as u64))) {
        out.push(("BcRelations", r));
    }
    if let Some(o) = offsets_to_file(doc) {
        out.push(("BcOffsets", o));
    }
    if let Some(a) = arrays_to_file(doc) {
        out.push(("BcArrays", a));
    }
    out
}

fn arrays_to_file(doc: &Document) -> Option<String> {
    let pos = |id: &u64| doc.shapes.iter().position(|s| s.id == *id);
    let v: Vec<serde_json::Value> = doc
        .arrays
        .iter()
        .filter_map(|a| {
            let sources: Vec<usize> = a.sources.iter().map(pos).collect::<Option<_>>()?;
            let copies: Vec<usize> = a.copies.iter().filter_map(pos).collect();
            let kind = match &a.kind {
                ArrayKind::Grid { cols, rows, gap_x, gap_y } => serde_json::json!({ "kind": "grid", "cols": cols, "rows": rows, "gx": gap_x, "gy": gap_y }),
                ArrayKind::Circular { count, span, rotate, center } => serde_json::json!({ "kind": "circular", "count": count, "span": span, "rotate": rotate, "cx": center.x, "cy": center.y }),
                ArrayKind::Path { curve, opts } => serde_json::json!({ "kind": "path", "curve": pos(curve)?, "by_spacing": opts.by_spacing, "copies": opts.copies, "spacing": opts.spacing, "rotate": opts.rotate, "scale": opts.scale, "scale_pct": opts.scale_pct }),
            };
            Some(serde_json::json!({ "how": kind, "sources": sources, "copies": copies }))
        })
        .collect();
    (!v.is_empty()).then(|| serde_json::Value::Array(v).to_string())
}

fn arrays_from_file(doc: &Document, attr: &str) -> Vec<ArrayLink> {
    let Ok(serde_json::Value::Array(v)) = serde_json::from_str::<serde_json::Value>(attr) else { return Vec::new() };
    let id = |x: &serde_json::Value| x.as_u64().and_then(|i| doc.shapes.get(i as usize)).map(|s| s.id);
    v.iter()
        .filter_map(|a| {
            let h = a.get("how")?;
            let n = |k: &str| h.get(k).and_then(|x| x.as_f64());
            let b = |k: &str| h.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
            let kind = match h.get("kind")?.as_str()? {
                "grid" => ArrayKind::Grid { cols: n("cols")? as usize, rows: n("rows")? as usize, gap_x: n("gx")?, gap_y: n("gy")? },
                "circular" => ArrayKind::Circular { count: n("count")? as usize, span: n("span")?, rotate: b("rotate"), center: p2(n("cx")?, n("cy")?) },
                "path" => ArrayKind::Path {
                    curve: id(h.get("curve")?)?,
                    opts: crate::sketch::PathCopyOpts { by_spacing: b("by_spacing"), copies: n("copies")? as usize, spacing: n("spacing")?, rotate: b("rotate"), scale: b("scale"), scale_pct: n("scale_pct").unwrap_or(100.0) },
                },
                _ => return None,
            };
            let list = |k: &str| -> Option<Vec<u64>> { a.get(k)?.as_array()?.iter().map(id).collect() };
            Some(ArrayLink { kind, sources: list("sources")?, copies: list("copies").unwrap_or_default() })
        })
        .collect()
}

/// From the file: positions become the ids the shapes got on load.
pub fn from_file(doc: &mut Document, relations: Option<&str>, offsets: Option<&str>, arrays: Option<&str>) {
    if let Some(a) = arrays {
        doc.arrays = arrays_from_file(doc, a);
    }
    if let Some(r) = relations {
        doc.relations = remap(&from_attr(r), |i| doc.shapes.get(i as usize).map(|s| s.id));
    }
    if let Some(o) = offsets {
        doc.offsets = offsets_from_file(doc, o);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path(pts: &[P2]) -> Shape {
        Shape::new(ShapeKind::Path(PathData::polyline(pts, false)), Xform::IDENTITY, 0)
    }
    #[test]
    fn horizontal_kept_when_end_is_fixed() {
        let mut d = Document::default();
        d.shapes.push(path(&[p2(0.0, 0.0), p2(10.0, 3.0)]));
        let id = d.shapes[0].id;
        d.relations.push(Relation { kind: RelKind::Horizontal, ents: vec![Ent::Line { shape: id, a: 0, b: 1 }], value: 0.0 });
        solve(&mut d, &[Key::V(id, 0)]);
        let a = node_pos(&d, id, 0).unwrap();
        let b = node_pos(&d, id, 1).unwrap();
        assert_eq!(a, p2(0.0, 0.0));
        assert!((b.y - 0.0).abs() < 1e-6);
    }
    #[test]
    fn parallel_and_equal_lines() {
        let mut d = Document::default();
        d.shapes.push(path(&[p2(0.0, 0.0), p2(10.0, 0.0)]));
        d.shapes.push(path(&[p2(0.0, 5.0), p2(6.0, 9.0)]));
        let (i1, i2) = (d.shapes[0].id, d.shapes[1].id);
        let l1 = Ent::Line { shape: i1, a: 0, b: 1 };
        let l2 = Ent::Line { shape: i2, a: 0, b: 1 };
        d.relations.push(make(RelKind::Parallel, &[l1, l2]));
        d.relations.push(make(RelKind::Equal, &[l1, l2]));
        let err = solve(&mut d, &[Key::V(i1, 0), Key::V(i1, 1)]);
        assert!(err < 1e-5, "{err}");
        let (a, b) = (node_pos(&d, i2, 0).unwrap(), node_pos(&d, i2, 1).unwrap());
        assert!((b.y - a.y).abs() < 1e-4 && (a.dist(b) - 10.0).abs() < 1e-4, "{a:?} {b:?}");
    }
    #[test]
    fn tangent_circle_moves_to_line() {
        let mut d = Document::default();
        d.shapes.push(path(&[p2(0.0, 0.0), p2(20.0, 0.0)]));
        d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 3.0, ry: 3.0 }, Xform::translate(10.0, 5.0), 0));
        let (i1, i2) = (d.shapes[0].id, d.shapes[1].id);
        d.relations.push(make(RelKind::Tangent, &[Ent::Circle { shape: i2 }, Ent::Line { shape: i1, a: 0, b: 1 }]));
        solve(&mut d, &[Key::V(i1, 0), Key::V(i1, 1)]);
        let c = d.shapes[1].xform.apply(p2(0.0, 0.0));
        assert!((c.y - 3.0).abs() < 1e-5, "{c:?}");
    }
    #[test]
    fn relations_and_offsets_survive_saving() {
        let mut d = Document::default();
        d.shapes.push(path(&[p2(0.0, 0.0), p2(10.0, 0.0)]));
        d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 3.0, ry: 3.0 }, Xform::translate(5.0, 3.0), 0));
        let (i1, i2) = (d.shapes[0].id, d.shapes[1].id);
        d.relations.push(make(RelKind::Tangent, &[Ent::Line { shape: i1, a: 0, b: 1 }, Ent::Circle { shape: i2 }]));
        crate::ops::offset(&mut d, &[1], &crate::ops::OffsetOpts { dist: 1.5, ..Default::default() }).unwrap();
        let text = crate::lbrn::save_string(&d, &[]);
        let back = crate::lbrn::load_str(&text).unwrap().doc;
        assert_eq!(back.relations.len(), 1);
        assert_eq!(back.relations[0].ents[1], Ent::Circle { shape: back.shapes[1].id });
        let link = offset_link(&back, back.shapes[2].id).unwrap();
        assert_eq!(link.source, back.shapes[1].id);
        assert!((link.opts.dist - 1.5).abs() < 1e-9);
    }
    #[test]
    fn impossible_relations_never_distort_and_copies_are_left_alone() {
        // a triangle with a line that is both horizontal and vertical (can't be), plus an unrelated copy
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 3.0), p2(4.0, 9.0)], true)), Xform::IDENTITY, 0));
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(20.0, 0.0), p2(30.0, 3.0), p2(24.0, 9.0)], true)), Xform::IDENTITY, 0));
        let (a, b) = (d.shapes[0].id, d.shapes[1].id);
        let l = Ent::Line { shape: a, a: 0, b: 1 };
        d.relations.push(make(RelKind::Horizontal, &[l]));
        d.relations.push(make(RelKind::Vertical, &[l]));
        let before = format!("{:?}", d.shapes[0].kind);
        // moving the copy only looks at the copy's relations (none)
        let held = shape_keys(&d, 1);
        assert_eq!(solve_connected(&mut d, &[b], &held), 0.0);
        assert_eq!(format!("{:?}", d.shapes[0].kind), before);
        // moving the original: the relations can't be met, so nothing is distorted
        let e = solve_connected(&mut d, &[a], &[]);
        assert!(e > 1e-4);
        assert_eq!(format!("{:?}", d.shapes[0].kind), before);
    }
    #[test]
    fn applicable_choices() {
        let l = Ent::Line { shape: 1, a: 0, b: 1 };
        let c = Ent::Circle { shape: 2 };
        assert!(applicable(&[l]).contains(&RelKind::Horizontal));
        assert!(applicable(&[l, c]).contains(&RelKind::Tangent));
        let back = from_attr(&to_attr(&[make(RelKind::Tangent, &[l, c])]).unwrap());
        assert_eq!(back[0].kind, RelKind::Tangent);
    }
}

// ------------------------------------------------------------------ arrays

/// How an array was made, so it can be opened again and changed.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrayKind {
    Grid { cols: usize, rows: usize, gap_x: f64, gap_y: f64 },
    Circular { count: usize, span: f64, rotate: bool, center: P2 },
    Path { curve: u64, opts: crate::sketch::PathCopyOpts },
}

/// An array: the shapes it was made from and the copies it made.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayLink {
    pub kind: ArrayKind,
    pub sources: Vec<u64>,
    pub copies: Vec<u64>,
}

/// The array a shape belongs to (as a source or a copy), if it still exists.
pub fn array_of(doc: &Document, id: u64) -> Option<usize> {
    doc.arrays.iter().rposition(|a| (a.sources.contains(&id) || a.copies.contains(&id)) && a.sources.iter().all(|s| doc.shapes.iter().any(|x| x.id == *s)))
}

/// Point relations at new nodes after paths were joined: (old shape id, node) → (new shape id, node).
pub fn remap_nodes(doc: &mut Document, map: &HashMap<(u64, usize), (u64, usize)>) {
    for r in &mut doc.relations {
        for e in &mut r.ents {
            match *e {
                Ent::Pt { shape, v } => {
                    if let Some((s, nv)) = map.get(&(shape, v)) {
                        *e = Ent::Pt { shape: *s, v: *nv };
                    }
                }
                Ent::Line { shape, a, b } => {
                    if let (Some((s1, na)), Some((s2, nb))) = (map.get(&(shape, a)), map.get(&(shape, b))) {
                        if s1 == s2 {
                            *e = Ent::Line { shape: *s1, a: *na, b: *nb };
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // a coincident relation between two nodes that are now the same node has done its job
    doc.relations.retain(|r| !(r.kind == RelKind::Coincident && r.ents.len() == 2 && r.ents[0] == r.ents[1]));
}
