//! Dimensions for the dimension tool: measure segments, arcs/fillets, circles and boxes, and change them by
//! typing a new value (CAD-style driving dimensions, without a full constraint solver).

use crate::geom::*;
use crate::model::*;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimTarget {
    /// Straight segment `prim` of a path.
    Seg { shape: usize, prim: usize },
    /// Circular arc (or fillet) `prim` of a path.
    Arc { shape: usize, prim: usize },
    RectW { shape: usize },
    RectH { shape: usize },
    RectRadius { shape: usize },
    Diameter { shape: usize },
    BoxW { shape: usize },
    BoxH { shape: usize },
    /// Distance of an offset result from the shape it was made from.
    Offset { shape: usize },
    /// Angle at node `v` between the line arriving (prim `pin`) and the line leaving (prim `pout`).
    Angle { shape: usize, v: usize, pin: usize, pout: usize },
    /// A star's outer and inner radius.
    StarOuter { shape: usize },
    StarInner { shape: usize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimKind {
    /// Measured between `a` and `b` (world).
    Linear,
    /// Radius about `center`, drawn from the centre to `b`.
    Radius { center: P2 },
    /// Diameter across `a`–`b`.
    Diameter,
    /// Offset distance, from the source outline `a` to the offset outline `b`.
    Offset,
    /// Angle in degrees at `center` between the rays to `a` and `b`.
    Angle { center: P2 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    /// A driving dimension holds its value when other things change (CAD); a driven one only reports it.
    pub driving: bool,
    pub target: DimTarget,
    pub kind: DimKind,
    pub a: P2,
    pub b: P2,
    pub value: f64,
}

impl Dim {
    pub fn prefix(&self) -> &'static str {
        match self.kind {
            DimKind::Linear => "",
            DimKind::Radius { .. } => "R ",
            DimKind::Diameter => "Ø ",
            DimKind::Offset => "Offset ",
            DimKind::Angle { .. } => "",
        }
    }
    pub fn is_angle(&self) -> bool {
        matches!(self.kind, DimKind::Angle { .. })
    }
}

/// The star parameters a star shape was drawn with: (points, ratio, bulge, rx, ry, corner radius).
pub fn star_params(s: &Shape) -> Option<(u32, f64, f64, f64, f64, f64)> {
    let v = s.extra_attrs.iter().find(|(k, _)| k == "BcStar")?.1.clone();
    let n: Vec<f64> = v.split(';').filter_map(|t| t.parse().ok()).collect();
    (n.len() == 6).then(|| (n[0] as u32, n[1], n[2], n[3], n[4], n[5]))
}

pub fn set_star_params(s: &mut Shape, points: u32, ratio: f64, bulge: f64, rx: f64, ry: f64, radius: f64) {
    s.extra_attrs.retain(|(k, _)| k != "BcStar");
    s.extra_attrs.push(("BcStar".into(), format!("{points};{ratio};{bulge};{rx};{ry};{radius}")));
    s.kind = ShapeKind::Path(crate::sketch::star_shape_path(points, rx, ry, ratio, bulge, radius));
}

/// Angle dimensions at the corners of a path where two straight lines meet.
pub fn angle_dims(doc: &Document, shape: usize) -> Vec<Dim> {
    let Some(s) = doc.shapes.get(shape) else { return Vec::new() };
    let ShapeKind::Path(pd) = &s.kind else { return Vec::new() };
    if star_params(s).is_some() {
        return Vec::new();
    }
    let x = s.xform;
    let mut out = Vec::new();
    for v in 0..pd.verts.len().min(400) {
        let ins: Vec<usize> = (0..pd.prims.len()).filter(|i| pd.prims[*i].b == v && pd.prims[*i].kind == PrimKind::Line).collect();
        let outs: Vec<usize> = (0..pd.prims.len()).filter(|i| pd.prims[*i].a == v && pd.prims[*i].kind == PrimKind::Line).collect();
        if let ([pin], [pout]) = (ins.as_slice(), outs.as_slice()) {
            let c = x.apply(pd.verts[v].p);
            let a = x.apply(pd.verts[pd.prims[*pin].a].p);
            let b = x.apply(pd.verts[pd.prims[*pout].b].p);
            let (u1, u2) = (a - c, b - c);
            if u1.len() < 1e-9 || u2.len() < 1e-9 {
                continue;
            }
            let ang = u1.cross(u2).atan2(u1.dot(u2)).abs().to_degrees();
            if (ang - 180.0).abs() < 1e-6 {
                continue;
            }
            let target = DimTarget::Angle { shape, v, pin: *pin, pout: *pout };
            out.push(Dim { driving: is_driving(doc, target), target, kind: DimKind::Angle { center: c }, a, b, value: ang });
        }
    }
    out
}

/// Is there a driving dimension (relation) holding this target?
pub fn is_driving(doc: &Document, target: DimTarget) -> bool {
    match driving_relation(doc, target, 0.0) {
        Some(r) => doc.relations.iter().any(|x| x.kind == r.kind && x.ents == r.ents),
        // shape parameters (rectangle size, star radii, offset distance) always drive their shape
        None => matches!(target, DimTarget::RectW { .. } | DimTarget::RectH { .. } | DimTarget::RectRadius { .. } | DimTarget::StarOuter { .. } | DimTarget::StarInner { .. } | DimTarget::Offset { .. }),
    }
}

/// The relation that makes this dimension driving, if it can be one.
pub fn driving_relation(doc: &Document, target: DimTarget, value: f64) -> Option<crate::relations::Relation> {
    use crate::relations::{Ent, RelKind, Relation};
    match target {
        DimTarget::Seg { shape, prim } => {
            let s = doc.shapes.get(shape)?;
            let ShapeKind::Path(pd) = &s.kind else { return None };
            let p = pd.prims.get(prim)?;
            Some(Relation { kind: RelKind::Distance, ents: vec![Ent::Line { shape: s.id, a: p.a, b: p.b }], value })
        }
        DimTarget::Diameter { shape } => {
            let s = doc.shapes.get(shape)?;
            matches!(s.kind, ShapeKind::Ellipse { .. }).then(|| Relation { kind: RelKind::Diameter, ents: vec![Ent::Circle { shape: s.id }], value })
        }
        DimTarget::Angle { shape, pin, pout, .. } => {
            let s = doc.shapes.get(shape)?;
            let ShapeKind::Path(pd) = &s.kind else { return None };
            let (a, b) = (pd.prims.get(pin)?, pd.prims.get(pout)?);
            Some(Relation { kind: RelKind::Angle, ents: vec![Ent::Line { shape: s.id, a: a.a, b: a.b }, Ent::Line { shape: s.id, a: b.a, b: b.b }], value })
        }
        _ => None,
    }
}

/// Circle a cubic segment follows, if it is (close to) a circular arc: (centre, radius).
pub fn arc_of(p0: P2, c0: P2, c1: P2, p1: P2) -> Option<(P2, f64)> {
    let m = cubic_point(p0, c0, c1, p1, 0.5);
    let (ctr, r) = crate::sketch::circle_3pt(p0, m, p1)?;
    if !r.is_finite() || r > 1e6 {
        return None;
    }
    for t in [0.25, 0.75] {
        let q = cubic_point(p0, c0, c1, p1, t);
        if (q.dist(ctr) - r).abs() > r * 0.02 + 1e-6 {
            return None;
        }
    }
    Some((ctr, r))
}

fn local_lengths(x: &Xform) -> (f64, f64) {
    (p2(x.a, x.b).len(), p2(x.c, x.d).len())
}

/// Dimensions of one top-level shape.
pub fn dims_for(doc: &Document, shape: usize) -> Vec<Dim> {
    let Some(s) = doc.shapes.get(shape) else { return Vec::new() };
    let x = s.xform;
    let mut out = Vec::new();
    if let Some((_, ratio, _, rx, ry, _)) = star_params(s) {
        let c = x.apply(p2(0.0, 0.0));
        let (sx, _) = local_lengths(&x);
        let tip = x.apply(p2(0.0, ry));
        let inner = c + (tip - c) * ratio;
        let inner = inner + (x.apply(p2(rx * 0.3, 0.0)) - c) * 0.0;
        out.push(Dim { driving: true, target: DimTarget::StarOuter { shape }, kind: DimKind::Radius { center: c }, a: c, b: tip, value: ry * sx });
        let side = x.apply(p2(rx * ratio * (std::f64::consts::PI / 5.0).sin(), ry * ratio * (std::f64::consts::PI / 5.0).cos()));
        let _ = inner;
        out.push(Dim { driving: true, target: DimTarget::StarInner { shape }, kind: DimKind::Radius { center: c }, a: c, b: side, value: ry * ratio * sx });
        return out;
    }
    match &s.kind {
        ShapeKind::Path(pd) => {
            let mut pi = 0;
            while pi < pd.prims.len().min(400) {
                let pr = pd.prims[pi];
                let (Some(va), Some(vb)) = (pd.verts.get(pr.a), pd.verts.get(pr.b)) else {
                    pi += 1;
                    continue;
                };
                let (a, b) = (x.apply(va.p), x.apply(vb.p));
                match pr.kind {
                    PrimKind::Line => {
                        if a.dist(b) > 1e-6 {
                            out.push(Dim { driving: false, target: DimTarget::Seg { shape, prim: pi }, kind: DimKind::Linear, a, b, value: a.dist(b) });
                        }
                        pi += 1;
                    }
                    PrimKind::Bezier => match arc_group(pd, &x, pi) {
                        Some(g) => {
                            let m = pd.prims[(g.first + g.last) / 2];
                            let (vm0, vm1) = (pd.verts[m.a], pd.verts[m.b]);
                            let mid = cubic_point(x.apply(vm0.p), x.apply(vm0.c0.unwrap_or(vm0.p)), x.apply(vm1.c1.unwrap_or(vm1.p)), x.apply(vm1.p), 0.5);
                            if g.closed {
                                // a whole circle (e.g. an offset of a round shape): one diameter
                                let dir = (mid - g.center).normalized();
                                out.push(Dim { driving: false, target: DimTarget::Arc { shape, prim: pi }, kind: DimKind::Diameter, a: g.center - dir * g.r, b: g.center + dir * g.r, value: 2.0 * g.r });
                            } else {
                                out.push(Dim { driving: false, target: DimTarget::Arc { shape, prim: pi }, kind: DimKind::Radius { center: g.center }, a: g.center, b: mid, value: g.r });
                            }
                            pi = g.last + 1;
                        }
                        None => pi += 1,
                    },
                }
            }
        }
        ShapeKind::Rect { w, h, cr } => {
            let (sx, sy) = local_lengths(&x);
            let (hw, hh) = (w / 2.0, h / 2.0);
            out.push(Dim { driving: false, target: DimTarget::RectW { shape }, kind: DimKind::Linear, a: x.apply(p2(-hw, -hh)), b: x.apply(p2(hw, -hh)), value: w * sx });
            out.push(Dim { driving: false, target: DimTarget::RectH { shape }, kind: DimKind::Linear, a: x.apply(p2(hw, -hh)), b: x.apply(p2(hw, hh)), value: h * sy });
            if *cr > 0.0 {
                let r = cr.min(hw).min(hh);
                let ctr = x.apply(p2(hw - r, hh - r));
                let edge = x.apply(p2(hw - r, hh - r) + p2(1.0, 1.0).normalized() * r);
                out.push(Dim { driving: false, target: DimTarget::RectRadius { shape }, kind: DimKind::Radius { center: ctr }, a: ctr, b: edge, value: r * sx });
            }
        }
        ShapeKind::Ellipse { rx, ry } | ShapeKind::Polygon { rx, ry, .. } => {
            let (sx, sy) = local_lengths(&x);
            if (rx * sx - ry * sy).abs() < 1e-6 * (rx * sx).max(1.0) {
                out.push(Dim { driving: false, target: DimTarget::Diameter { shape }, kind: DimKind::Diameter, a: x.apply(p2(-rx, 0.0)), b: x.apply(p2(*rx, 0.0)), value: 2.0 * rx * sx });
            } else {
                box_dims(s, shape, &mut out);
            }
        }
        _ => box_dims(s, shape, &mut out),
    }
    for d in &mut out {
        d.driving = is_driving(doc, d.target);
    }
    if let Some(link) = crate::relations::offset_link(doc, s.id) {
        if let Some(src) = doc.shapes.iter().find(|t| t.id == link.source) {
            // from the source's rightmost point straight across to the nearest point of the offset outline
            let src_pts: Vec<P2> = src.world_outline(0.05).into_iter().flatten().collect();
            if let Some(a) = src_pts.iter().copied().max_by(|p, q| p.x.partial_cmp(&q.x).unwrap()) {
                let res_pts: Vec<P2> = s.world_outline(0.05).into_iter().flatten().collect();
                if let Some(b) = res_pts.iter().copied().min_by(|p, q| p.dist(a).partial_cmp(&q.dist(a)).unwrap()) {
                    out.push(Dim { driving: true, target: DimTarget::Offset { shape }, kind: DimKind::Offset, a, b, value: link.opts.dist.abs() });
                }
            }
        }
    }
    out
}

/// A run of curve pieces that follow one circle (an arc split into quarter pieces, or a whole circle).
struct ArcGroup {
    first: usize,
    last: usize,
    center: P2,
    r: f64,
    closed: bool,
}

fn prim_arc(pd: &PathData, x: &Xform, pi: usize) -> Option<(P2, f64)> {
    let pr = pd.prims.get(pi)?;
    if pr.kind != PrimKind::Bezier {
        return None;
    }
    let (va, vb) = (pd.verts.get(pr.a)?, pd.verts.get(pr.b)?);
    arc_of(x.apply(va.p), x.apply(va.c0.unwrap_or(va.p)), x.apply(vb.c1.unwrap_or(vb.p)), x.apply(vb.p))
}

fn arc_group(pd: &PathData, x: &Xform, first: usize) -> Option<ArcGroup> {
    let (center, r) = prim_arc(pd, x, first)?;
    let same = |c: P2, rr: f64| c.dist(center) <= 1e-3 * r.max(1.0) && (rr - r).abs() <= 1e-3 * r.max(1.0);
    let mut last = first;
    while let Some((c, rr)) = prim_arc(pd, x, last + 1) {
        if pd.prims[last + 1].a != pd.prims[last].b || !same(c, rr) {
            break;
        }
        last += 1;
    }
    let closed = pd.prims[last].b == pd.prims[first].a && last > first;
    Some(ArcGroup { first, last, center, r, closed })
}

/// The shape a dimension belongs to.
pub fn target_shape(t: DimTarget) -> Option<usize> {
    Some(match t {
        DimTarget::Seg { shape, .. } | DimTarget::Arc { shape, .. } | DimTarget::RectW { shape } | DimTarget::RectH { shape } | DimTarget::RectRadius { shape } | DimTarget::Diameter { shape } | DimTarget::BoxW { shape } | DimTarget::BoxH { shape } | DimTarget::Offset { shape } | DimTarget::Angle { shape, .. } | DimTarget::StarOuter { shape } | DimTarget::StarInner { shape } => shape,
    })
}

/// Points a dimension holds still while the sketch relations are solved after it changes.
pub fn held_keys(doc: &Document, target: DimTarget) -> Vec<crate::relations::Key> {
    let all = |shape: usize| -> Vec<crate::relations::Key> {
        let n = match doc.shapes.get(shape).map(|s| &s.kind) {
            Some(ShapeKind::Path(pd)) => pd.verts.len(),
            _ => 0,
        };
        crate::relations::keys_of(doc, shape, &(0..n).collect::<Vec<_>>())
    };
    match target {
        // a length you type keeps its start point where it is; the rest of the sketch adjusts to all the relations
        DimTarget::Seg { shape, prim } => match doc.shapes.get(shape).map(|s| &s.kind) {
            Some(ShapeKind::Path(pd)) => pd.prims.get(prim).map(|p| crate::relations::keys_of(doc, shape, &[p.a])).unwrap_or_default(),
            _ => Vec::new(),
        },
        DimTarget::Arc { shape, prim } => match doc.shapes.get(shape) {
            Some(s @ Shape { kind: ShapeKind::Path(pd), .. }) => match arc_group(pd, &s.xform, prim) {
                Some(g) if !g.closed => crate::relations::keys_of(doc, shape, &[pd.prims[g.first].a, pd.prims[g.last].b]),
                _ => all(shape),
            },
            _ => Vec::new(),
        },
        // a circle's diameter: its centre stays, its radius is set by the driving dimension
        DimTarget::Diameter { shape } if matches!(doc.shapes.get(shape).map(|s| &s.kind), Some(ShapeKind::Ellipse { .. })) => doc.shapes.get(shape).map(|s| vec![crate::relations::Key::C(s.id)]).unwrap_or_default(),
        DimTarget::RectW { shape } | DimTarget::RectH { shape } | DimTarget::RectRadius { shape } | DimTarget::Diameter { shape } | DimTarget::BoxW { shape } | DimTarget::BoxH { shape } | DimTarget::Offset { shape } | DimTarget::StarOuter { shape } | DimTarget::StarInner { shape } => all(shape),
        // an angle you type keeps its corner where it is; the other points move so every relation and
        // driving dimension (including earlier angles) holds
        DimTarget::Angle { shape, v, .. } => crate::relations::keys_of(doc, shape, &[v]),
    }
}

fn box_dims(s: &Shape, shape: usize, out: &mut Vec<Dim>) {
    let b = s.bbox();
    if b.is_empty() {
        return;
    }
    out.push(Dim { driving: false, target: DimTarget::BoxW { shape }, kind: DimKind::Linear, a: b.min, b: p2(b.max.x, b.min.y), value: b.width() });
    out.push(Dim { driving: false, target: DimTarget::BoxH { shape }, kind: DimKind::Linear, a: p2(b.max.x, b.min.y), b: b.max, value: b.height() });
}

/// Replace primitives `pi..=pj` (from vertex a of the first to b of the last) with the cubic segments of `arc`
/// (world coordinates), keeping a and b.
fn splice_arc(pd: &mut PathData, pi: usize, pj: usize, arc: &PathData, inv: &Xform) {
    let pr = Prim { kind: PrimKind::Bezier, a: pd.prims[pi].a, b: pd.prims[pj].b };
    let n = arc.verts.len();
    if n < 2 {
        return;
    }
    let to_local = |v: Vert| Vert { p: inv.apply(v.p), c0: v.c0.map(|c| inv.apply(c)), c1: v.c1.map(|c| inv.apply(c)) };
    let first = to_local(arc.verts[0]);
    let last = to_local(arc.verts[n - 1]);
    pd.verts[pr.a].p = first.p;
    pd.verts[pr.a].c0 = first.c0;
    pd.verts[pr.b].p = last.p;
    pd.verts[pr.b].c1 = last.c1;
    let mut chain = vec![pr.a];
    for v in &arc.verts[1..n - 1] {
        pd.verts.push(to_local(*v));
        chain.push(pd.verts.len() - 1);
    }
    chain.push(pr.b);
    let new_prims: Vec<Prim> = chain.windows(2).map(|w| Prim { kind: PrimKind::Bezier, a: w[0], b: w[1] }).collect();
    pd.prims.splice(pi..=pj, new_prims);
}

fn line_dir_into(pd: &PathData, x: &Xform, v: usize, pi: usize) -> Option<(P2, P2)> {
    // the straight segment arriving at vertex v (other than prim pi): returns (far end, v) in world
    pd.prims.iter().enumerate().find(|(i, p)| *i != pi && p.b == v && p.kind == PrimKind::Line).map(|(_, p)| (x.apply(pd.verts[p.a].p), x.apply(pd.verts[v].p)))
}

fn line_dir_out(pd: &PathData, x: &Xform, v: usize, pi: usize) -> Option<(P2, P2)> {
    pd.prims.iter().enumerate().find(|(i, p)| *i != pi && p.a == v && p.kind == PrimKind::Line).map(|(_, p)| (x.apply(pd.verts[p.b].p), x.apply(pd.verts[v].p)))
}

fn line_intersection(p: P2, r: P2, q: P2, s: P2) -> Option<P2> {
    let den = r.cross(s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).cross(s) / den;
    Some(p + r * t)
}

/// Change the measured value. Returns false if the value can't be applied.
pub fn apply(doc: &mut Document, target: DimTarget, value: f64) -> bool {
    if !(value.is_finite() && value > 0.0) {
        return false;
    }
    match target {
        DimTarget::Seg { shape, prim } => {
            let s = &mut doc.shapes[shape];
            let x = s.xform;
            let Some(inv) = x.inverse() else { return false };
            let ShapeKind::Path(pd) = &mut s.kind else { return false };
            let pr = pd.prims[prim];
            let (a, b) = (x.apply(pd.verts[pr.a].p), x.apply(pd.verts[pr.b].p));
            let dir = (b - a).normalized();
            let nb = inv.apply(a + dir * value);
            let d = nb - pd.verts[pr.b].p;
            crate::nodes::translate(pd, &[pr.b], d);
            true
        }
        DimTarget::Arc { shape, prim } => {
            let s = &mut doc.shapes[shape];
            let x = s.xform;
            let Some(inv) = x.inverse() else { return false };
            let ShapeKind::Path(pd) = &mut s.kind else { return false };
            let Some(g) = arc_group(pd, &x, prim) else { return false };
            if g.closed {
                // a whole circle: the value is its diameter; scale the ring about its centre
                let k = value / 2.0 / g.r.max(1e-9);
                let c = inv.apply(g.center);
                let ring: std::collections::BTreeSet<usize> = pd.prims[g.first..=g.last].iter().flat_map(|p| [p.a, p.b]).collect();
                for v in ring {
                    let vt = &mut pd.verts[v];
                    vt.p = c + (vt.p - c) * k;
                    vt.c0 = vt.c0.map(|q| c + (q - c) * k);
                    vt.c1 = vt.c1.map(|q| c + (q - c) * k);
                }
                return true;
            }
            let pj = g.last;
            let pr = Prim { kind: PrimKind::Bezier, a: pd.prims[prim].a, b: pd.prims[pj].b };
            let (va, vb) = (pd.verts[pr.a], pd.verts[pr.b]);
            let (a, b) = (x.apply(va.p), x.apply(vb.p));
            let mp = pd.prims[(prim + pj) / 2];
            let (m0, m1) = (pd.verts[mp.a], pd.verts[mp.b]);
            let (mc0, mc1) = (x.apply(m0.c0.unwrap_or(m0.p)), x.apply(m1.c1.unwrap_or(m1.p)));
            let old_c = g.center;
            // a fillet between two straight lines: keep it tangent, sliding its ends along the lines
            if let (Some((pa, _)), Some((pb, _))) = (line_dir_into(pd, &x, pr.a, prim), line_dir_out(pd, &x, pr.b, pj)) {
                let (u_in, u_out) = ((a - pa).normalized(), (b - pb).normalized());
                if let Some(corner) = line_intersection(pa, u_in, pb, u_out) {
                    let (to_in, to_out) = ((pa - corner).normalized(), (pb - corner).normalized());
                    let theta = to_in.dot(to_out).clamp(-1.0, 1.0).acos();
                    if theta > 1e-3 && PI - theta > 1e-3 {
                        let d = value / (theta / 2.0).tan();
                        if d < corner.dist(pa) && d < corner.dist(pb) {
                            let (na, nb) = (corner + to_in * d, corner + to_out * d);
                            let bis = (to_in + to_out).normalized();
                            let ctr = corner + bis * (value / (theta / 2.0).sin());
                            let a0 = (na.y - ctr.y).atan2(na.x - ctr.x);
                            let a1 = (nb.y - ctr.y).atan2(nb.x - ctr.x);
                            let mut sweep = (a1 - a0).rem_euclid(TAU);
                            if sweep > PI {
                                sweep -= TAU;
                            }
                            let arc = crate::sketch::arc_path(ctr, value, a0, sweep);
                            splice_arc(pd, prim, pj, &arc, &inv);
                            return true;
                        }
                    }
                }
            }
            // any other arc: keep its end points, change how far it bows out
            let chord = a.dist(b);
            if value < chord / 2.0 - 1e-9 {
                return false;
            }
            let mid = a.lerp(b, 0.5);
            let n = (b - a).perp().normalized();
            let side = if (old_c - mid).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            let h = (value * value - chord * chord / 4.0).max(0.0).sqrt();
            let ctr = mid + n * (h * side);
            let old_mid = cubic_point(x.apply(m0.p), mc0, mc1, x.apply(m1.p), 0.5);
            let a0 = (a.y - ctr.y).atan2(a.x - ctr.x);
            let a1 = (b.y - ctr.y).atan2(b.x - ctr.x);
            // pick the sweep that passes on the same side of the chord as before
            let ccw = (a1 - a0).rem_euclid(TAU);
            let mid_ccw = a0 + ccw / 2.0;
            let probe = ctr + p2(mid_ccw.cos(), mid_ccw.sin()) * value;
            let same_side = (probe - mid).dot(old_mid - mid) > 0.0;
            let sweep = if same_side { ccw } else { ccw - TAU };
            let arc = crate::sketch::arc_path(ctr, value, a0, sweep);
            splice_arc(pd, prim, pj, &arc, &inv);
            true
        }
        DimTarget::RectW { shape } | DimTarget::RectH { shape } | DimTarget::RectRadius { shape } => {
            let s = &mut doc.shapes[shape];
            let (sx, sy) = local_lengths(&s.xform);
            let ShapeKind::Rect { w, h, cr } = &mut s.kind else { return false };
            match target {
                DimTarget::RectW { .. } => *w = value / sx.max(1e-9),
                DimTarget::RectH { .. } => *h = value / sy.max(1e-9),
                _ => *cr = (value / sx.max(1e-9)).min(w.min(*h) / 2.0),
            }
            true
        }
        DimTarget::Diameter { shape } => {
            let s = &mut doc.shapes[shape];
            let (sx, _) = local_lengths(&s.xform);
            match &mut s.kind {
                ShapeKind::Ellipse { rx, ry } | ShapeKind::Polygon { rx, ry, .. } => {
                    let k = value / 2.0 / sx.max(1e-9) / rx.max(1e-9);
                    *rx *= k;
                    *ry *= k;
                    true
                }
                _ => false,
            }
        }
        DimTarget::BoxW { shape } | DimTarget::BoxH { shape } => {
            let b = doc.shapes[shape].bbox();
            let (old, horizontal) = if matches!(target, DimTarget::BoxW { .. }) { (b.width(), true) } else { (b.height(), false) };
            if old < 1e-9 {
                return false;
            }
            let k = value / old;
            let t = Xform::about(b.center(), if horizontal { Xform::scale(k, 1.0) } else { Xform::scale(1.0, k) });
            let s = &mut doc.shapes[shape];
            s.xform = t.then_after(s.xform);
            true
        }
        DimTarget::Offset { shape } => {
            let id = doc.shapes[shape].id;
            crate::relations::set_offset(doc, id, value)
        }
        DimTarget::Angle { shape, v, pin, pout } => {
            if !(value > 0.0 && value < 180.0) {
                return false;
            }
            let s = &mut doc.shapes[shape];
            let x = s.xform;
            let Some(inv) = x.inverse() else { return false };
            let ShapeKind::Path(pd) = &mut s.kind else { return false };
            let (ia, ib) = (pd.prims[pin].a, pd.prims[pout].b);
            let (c, a, b) = (x.apply(pd.verts[v].p), x.apply(pd.verts[ia].p), x.apply(pd.verts[ib].p));
            let (u1, u2) = (a - c, b - c);
            let cur = u1.cross(u2).atan2(u1.dot(u2));
            let sign = if cur < 0.0 { -1.0 } else { 1.0 };
            let want = u1.y.atan2(u1.x) + sign * value.to_radians();
            let nb = c + p2(want.cos(), want.sin()) * u2.len();
            let d = inv.apply(nb) - pd.verts[ib].p;
            crate::nodes::translate(pd, &[ib], d);
            true
        }
        DimTarget::StarOuter { shape } | DimTarget::StarInner { shape } => {
            let s = &mut doc.shapes[shape];
            let Some((n, ratio, bulge, rx, ry, radius)) = star_params(s) else { return false };
            let (sx, _) = local_lengths(&s.xform);
            let v = value / sx.max(1e-9);
            if matches!(target, DimTarget::StarOuter { .. }) {
                let k = v / ry.max(1e-9);
                set_star_params(s, n, ratio, bulge, rx * k, ry * k, radius);
            } else {
                let r = (v / ry.max(1e-9)).clamp(0.05, 1.0);
                set_star_params(s, n, r, bulge, rx, ry, radius);
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_length_edit() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0)], false)), Xform::IDENTITY, 0));
        let dims = dims_for(&d, 0);
        assert_eq!(dims.len(), 1);
        assert!(apply(&mut d, dims[0].target, 25.0));
        assert!((dims_for(&d, 0)[0].value - 25.0).abs() < 1e-9);
    }
    #[test]
    fn fillet_radius_edit_stays_tangent() {
        let mut pd = PathData::polyline(&[p2(0.0, 0.0), p2(20.0, 0.0), p2(20.0, 20.0)], false);
        crate::sketch::fillet_vertex(&mut pd, 1, &crate::sketch::Fillet { d1: 3.0, ..Default::default() });
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(pd), Xform::IDENTITY, 0));
        let r = dims_for(&d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Arc { .. })).unwrap();
        assert!((r.value - 3.0).abs() < 0.05);
        assert!(apply(&mut d, r.target, 6.0));
        let r2 = dims_for(&d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Arc { .. })).unwrap();
        assert!((r2.value - 6.0).abs() < 0.1, "{}", r2.value);
        // the straight parts got shorter by the larger fillet
        let lens: Vec<f64> = dims_for(&d, 0).iter().filter(|x| matches!(x.target, DimTarget::Seg { .. })).map(|x| x.value).collect();
        assert!(lens.iter().all(|l| (l - 14.0).abs() < 0.1), "{:?}", lens);
    }
    #[test]
    fn offset_circle_is_a_circle_with_editable_offset() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 5.0, ry: 5.0 }, Xform::translate(3.0, 3.0), 0));
        let o = crate::ops::offset(&mut d, &[0], &crate::ops::OffsetOpts { dist: 2.0, ..Default::default() }).unwrap();
        let dm = dims_for(&d, o[0]);
        assert!(dm.iter().any(|x| x.kind == DimKind::Diameter && (x.value - 14.0).abs() < 1e-9));
        let off = dm.iter().find(|x| x.kind == DimKind::Offset).unwrap();
        assert!((off.value - 2.0).abs() < 1e-9);
        assert!(apply(&mut d, off.target, 4.0));
        assert!((d.shapes[o[0]].bbox().width() - 18.0).abs() < 0.05);
    }
    #[test]
    fn offset_rounded_square_gets_arcs_not_segments() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Rect { w: 20.0, h: 20.0, cr: 0.0 }, Xform::IDENTITY, 0));
        let o = crate::ops::offset(&mut d, &[0], &crate::ops::OffsetOpts { dist: 3.0, ..Default::default() }).unwrap();
        let dm = dims_for(&d, o[0]);
        let radii: Vec<f64> = dm.iter().filter(|x| matches!(x.kind, DimKind::Radius { .. })).map(|x| x.value).collect();
        let lines = dm.iter().filter(|x| x.kind == DimKind::Linear).count();
        assert_eq!(radii.len(), 4, "{:?}", dm.iter().map(|x| (x.kind, x.value)).collect::<Vec<_>>());
        assert!(radii.iter().all(|r| (r - 3.0).abs() < 0.05), "{radii:?}");
        assert_eq!(lines, 4);
    }
    #[test]
    fn two_angle_dimensions_on_a_triangle_both_hold() {
        // the reported case: 30° on one corner, then 90° on another; the 30° must stay
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(40.0, 0.0), p2(15.0, 25.0)], true)), Xform::IDENTITY, 0));
        let set = |d: &mut Document, v: usize, deg: f64| {
            let dim = angle_dims(d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Angle { v: vv, .. } if vv == v)).unwrap();
            let held = held_keys(d, dim.target);
            let rel = driving_relation(d, dim.target, deg).unwrap();
            assert!(apply(d, dim.target, deg));
            d.relations.retain(|x| !(x.kind == rel.kind && x.ents == rel.ents));
            d.relations.push(rel);
            crate::relations::solve(d, &held)
        };
        set(&mut d, 0, 30.0);
        let err = set(&mut d, 1, 90.0);
        assert!(err < 1e-4, "solver error {err}");
        let angle = |d: &Document, v: usize| angle_dims(d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Angle { v: vv, .. } if vv == v)).unwrap().value;
        assert!((angle(&d, 0) - 30.0).abs() < 0.01, "first corner {}", angle(&d, 0));
        assert!((angle(&d, 1) - 90.0).abs() < 0.01, "second corner {}", angle(&d, 1));
        assert!(angle_dims(&d, 0).iter().filter(|x| x.driving).count() == 2);
    }
    #[test]
    fn length_and_angles_together() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(40.0, 0.0), p2(15.0, 25.0)], true)), Xform::IDENTITY, 0));
        let seg = dims_for(&d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Seg { prim: 0, .. })).unwrap();
        let held = held_keys(&d, seg.target);
        let rel = driving_relation(&d, seg.target, 50.0).unwrap();
        apply(&mut d, seg.target, 50.0);
        d.relations.push(rel);
        crate::relations::solve(&mut d, &held);
        let dim = angle_dims(&d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Angle { v: 0, .. })).unwrap();
        let held = held_keys(&d, dim.target);
        let rel = driving_relation(&d, dim.target, 45.0).unwrap();
        apply(&mut d, dim.target, 45.0);
        d.relations.push(rel);
        let err = crate::relations::solve(&mut d, &held);
        assert!(err < 1e-4);
        let len = dims_for(&d, 0).into_iter().find(|x| matches!(x.target, DimTarget::Seg { prim: 0, .. })).unwrap().value;
        assert!((len - 50.0).abs() < 0.01, "the 50 mm side kept its length: {len}");
    }
    #[test]
    fn circle_diameter() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 5.0, ry: 5.0 }, Xform::translate(3.0, 3.0), 0));
        let dm = dims_for(&d, 0);
        assert_eq!(dm[0].kind, DimKind::Diameter);
        assert!(apply(&mut d, dm[0].target, 30.0));
        assert!((d.shapes[0].bbox().width() - 30.0).abs() < 0.1);
    }
}
