//! Node (point) editing on paths: delete several, smooth / corner, line ↔ curve, insert, break.
//! Mirrors LightBurn's node keys (S smooth, C corner, L line, D delete, I insert, M midpoint, B break)
//! and Inkscape's "make selected nodes smooth / segments curves".

use crate::geom::*;
use crate::model::*;

fn neighbours(pd: &PathData, v: usize) -> (Option<usize>, Option<usize>) {
    let prev = pd.prims.iter().find(|p| p.b == v).map(|p| p.a);
    let next = pd.prims.iter().find(|p| p.a == v).map(|p| p.b);
    (prev, next)
}

/// Remove vertices, joining the neighbours of each removed run with a line.
pub fn delete(pd: &mut PathData, verts: &[usize]) -> bool {
    let mut vs: Vec<usize> = verts.iter().copied().filter(|v| *v < pd.verts.len()).collect();
    vs.sort();
    vs.dedup();
    if vs.is_empty() || pd.verts.len() - vs.len() < 2 {
        return false;
    }
    for &v in vs.iter().rev() {
        let (prev, next) = neighbours(pd, v);
        pd.prims.retain(|p| p.a != v && p.b != v);
        if let (Some(a), Some(b)) = (prev, next) {
            if a != b && a != v && b != v {
                pd.prims.push(Prim { kind: PrimKind::Line, a, b });
            }
        }
        pd.verts.remove(v);
        for p in pd.prims.iter_mut() {
            if p.a > v {
                p.a -= 1;
            }
            if p.b > v {
                p.b -= 1;
            }
        }
    }
    sort_prims(pd);
    true
}

/// Order primitives so each starts where the previous ended, when possible.
pub fn sort_prims(pd: &mut PathData) {
    let mut left = std::mem::take(&mut pd.prims);
    let mut out = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let last_b = out.last().map(|p: &Prim| p.b);
        let idx = last_b.and_then(|b| left.iter().position(|p| p.a == b)).unwrap_or_else(|| {
            // start a new chain at a vertex nothing leads into, if any
            left.iter().position(|p| !left.iter().any(|q| q.b == p.a)).unwrap_or(0)
        });
        out.push(left.remove(idx));
    }
    pd.prims = out;
}

/// Make nodes smooth: curved segments on both sides with handles along the neighbour direction.
pub fn smooth(pd: &mut PathData, verts: &[usize]) {
    for &v in verts {
        if v >= pd.verts.len() {
            continue;
        }
        let (prev, next) = neighbours(pd, v);
        let p = pd.verts[v].p;
        let pp = prev.map(|i| pd.verts[i].p);
        let np = next.map(|i| pd.verts[i].p);
        let dir = match (pp, np) {
            (Some(a), Some(b)) => (b - a).normalized(),
            (Some(a), None) => (p - a).normalized(),
            (None, Some(b)) => (b - p).normalized(),
            _ => continue,
        };
        let lin = pp.map(|a| a.dist(p) / 3.0).unwrap_or(0.0);
        let lout = np.map(|b| b.dist(p) / 3.0).unwrap_or(0.0);
        if lin > 0.0 {
            pd.verts[v].c1 = Some(p - dir * lin);
        }
        if lout > 0.0 {
            pd.verts[v].c0 = Some(p + dir * lout);
        }
        for pr in pd.prims.iter_mut() {
            if pr.a == v || pr.b == v {
                pr.kind = PrimKind::Bezier;
            }
        }
        // the far ends of now-curved segments need a handle too (or they'd collapse to the vertex)
        if let Some(i) = prev {
            if pd.verts[i].c0.is_none() {
                pd.verts[i].c0 = Some(pd.verts[i].p.lerp(p, 1.0 / 3.0));
            }
        }
        if let Some(i) = next {
            if pd.verts[i].c1.is_none() {
                pd.verts[i].c1 = Some(pd.verts[i].p.lerp(p, 1.0 / 3.0));
            }
        }
    }
}

/// Make nodes sharp corners: drop their handles (segments stay curves if the other end has a handle).
pub fn corner(pd: &mut PathData, verts: &[usize]) {
    for &v in verts {
        if let Some(vt) = pd.verts.get_mut(v) {
            vt.c0 = None;
            vt.c1 = None;
        }
    }
    retype(pd);
}

/// Turn the segments between selected nodes (or touching one selected node) into straight lines.
pub fn to_lines(pd: &mut PathData, verts: &[usize]) {
    let both = verts.len() > 1;
    let sel = |i: usize| verts.contains(&i);
    let targets: Vec<(usize, usize)> = pd.prims.iter().filter(|p| if both { sel(p.a) && sel(p.b) } else { sel(p.a) || sel(p.b) }).map(|p| (p.a, p.b)).collect();
    for (a, b) in targets {
        pd.verts[a].c0 = None;
        pd.verts[b].c1 = None;
        for p in pd.prims.iter_mut() {
            if p.a == a && p.b == b {
                p.kind = PrimKind::Line;
            }
        }
    }
}

/// Turn segments into curves (handles at thirds) so they can be bent.
pub fn to_curves(pd: &mut PathData, verts: &[usize]) {
    let both = verts.len() > 1;
    let sel = |i: usize| verts.contains(&i);
    for i in 0..pd.prims.len() {
        let p = pd.prims[i];
        if !(if both { sel(p.a) && sel(p.b) } else { sel(p.a) || sel(p.b) }) {
            continue;
        }
        let (a, b) = (pd.verts[p.a].p, pd.verts[p.b].p);
        if pd.verts[p.a].c0.is_none() {
            pd.verts[p.a].c0 = Some(a.lerp(b, 1.0 / 3.0));
        }
        if pd.verts[p.b].c1.is_none() {
            pd.verts[p.b].c1 = Some(a.lerp(b, 2.0 / 3.0));
        }
        pd.prims[i].kind = PrimKind::Bezier;
    }
}

fn retype(pd: &mut PathData) {
    for i in 0..pd.prims.len() {
        let p = pd.prims[i];
        let curved = pd.verts[p.a].c0.is_some() || pd.verts[p.b].c1.is_some();
        pd.prims[i].kind = if curved { PrimKind::Bezier } else { PrimKind::Line };
    }
}

/// Insert a node in the middle of every segment between selected nodes (LightBurn "M").
pub fn insert_midpoints(pd: &mut PathData, verts: &[usize]) -> Vec<usize> {
    let both = verts.len() > 1;
    let sel = |i: usize| verts.contains(&i);
    let mut added = Vec::new();
    let mut i = 0;
    while i < pd.prims.len() {
        let p = pd.prims[i];
        if p.a < pd.verts.len() && p.b < pd.verts.len() && (if both { sel(p.a) && sel(p.b) } else { sel(p.a) }) {
            let va = pd.verts[p.a];
            let vb = pd.verts[p.b];
            let n = pd.verts.len();
            match p.kind {
                PrimKind::Line => {
                    pd.verts.push(Vert::new(va.p.lerp(vb.p, 0.5)));
                    pd.prims[i] = Prim { kind: PrimKind::Line, a: p.a, b: n };
                    pd.prims.insert(i + 1, Prim { kind: PrimKind::Line, a: n, b: p.b });
                }
                PrimKind::Bezier => {
                    // de Casteljau split at t = 0.5
                    let (p0, c0, c1, p3) = (va.p, va.c0.unwrap_or(va.p), vb.c1.unwrap_or(vb.p), vb.p);
                    let (a1, b1, c1m) = (p0.lerp(c0, 0.5), c0.lerp(c1, 0.5), c1.lerp(p3, 0.5));
                    let (a2, b2) = (a1.lerp(b1, 0.5), b1.lerp(c1m, 0.5));
                    let m = a2.lerp(b2, 0.5);
                    pd.verts[p.a].c0 = Some(a1);
                    pd.verts[p.b].c1 = Some(c1m);
                    pd.verts.push(Vert { p: m, c0: Some(b2), c1: Some(a2) });
                    pd.prims[i] = Prim { kind: PrimKind::Bezier, a: p.a, b: n };
                    pd.prims.insert(i + 1, Prim { kind: PrimKind::Bezier, a: n, b: p.b });
                }
            }
            added.push(n);
            i += 2;
        } else {
            i += 1;
        }
    }
    added
}

/// Split the path at the selected nodes (LightBurn "B"): each node becomes two unconnected ends.
pub fn break_at(pd: &mut PathData, verts: &[usize]) {
    for &v in verts {
        if v >= pd.verts.len() {
            continue;
        }
        if let Some(i) = pd.prims.iter().position(|p| p.a == v) {
            if pd.prims.iter().any(|p| p.b == v) {
                let copy = pd.verts[v];
                pd.verts.push(copy);
                let n = pd.verts.len() - 1;
                pd.prims[i].a = n;
            }
        }
    }
    sort_prims(pd);
}

/// Grow a node selection to neighbouring nodes (Inkscape's scroll-to-expand).
pub fn grow(pd: &PathData, verts: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = verts.to_vec();
    for p in &pd.prims {
        if verts.contains(&p.a) && !out.contains(&p.b) {
            out.push(p.b);
        }
        if verts.contains(&p.b) && !out.contains(&p.a) {
            out.push(p.a);
        }
    }
    out
}

/// Shrink a selection by dropping nodes on its boundary.
pub fn shrink(pd: &PathData, verts: &[usize]) -> Vec<usize> {
    if verts.len() <= 1 {
        return verts.to_vec();
    }
    verts
        .iter()
        .copied()
        .filter(|v| {
            let (a, b) = neighbours(pd, *v);
            a.map(|a| verts.contains(&a)).unwrap_or(true) && b.map(|b| verts.contains(&b)).unwrap_or(true)
        })
        .collect()
}

/// Move selected nodes (and their handles) by a local-space offset.
pub fn translate(pd: &mut PathData, verts: &[usize], d: P2) {
    for &v in verts {
        if let Some(vt) = pd.verts.get_mut(v) {
            vt.p = vt.p + d;
            vt.c0 = vt.c0.map(|c| c + d);
            vt.c1 = vt.c1.map(|c| c + d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> PathData {
        PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0), p2(0.0, 10.0)], true)
    }
    #[test]
    fn delete_two() {
        let mut pd = square();
        assert!(delete(&mut pd, &[1, 2]));
        assert_eq!(pd.verts.len(), 2);
        assert!(!pd.flatten(0.1).is_empty());
    }
    #[test]
    fn smooth_then_corner() {
        let mut pd = square();
        smooth(&mut pd, &[1]);
        assert!(pd.prims.iter().any(|p| p.kind == PrimKind::Bezier));
        corner(&mut pd, &[0, 1, 2, 3]);
        assert!(pd.prims.iter().all(|p| p.kind == PrimKind::Line));
    }
    #[test]
    fn midpoint_and_grow() {
        let mut pd = square();
        let added = insert_midpoints(&mut pd, &[0, 1]);
        assert_eq!(added.len(), 1);
        assert_eq!(pd.verts.len(), 5);
        assert!(grow(&pd, &[0]).len() >= 3);
    }
}
