//! Joining open paths whose ends meet into one continuous path (and closing a path whose two ends meet),
//! like a CAD sketch where touching curves bound a region. Used automatically when a new line, arc or curve
//! is drawn onto the end of another, when two loose ends are made coincident, and by Join / Close Paths
//! (like LightBurn's "Auto-Join selected shapes": selected open paths, tolerance 0.05 mm).

use crate::geom::*;
use crate::model::*;
use std::collections::HashMap;

/// Join / Close tolerance in mm.
pub const AUTO_JOIN_TOL: f64 = 0.05;
/// Ends this close are treated as the same point when a new shape is drawn (snapped points are exact).
pub const SNAP_JOIN_TOL: f64 = 0.001;

#[derive(Clone, Debug)]
struct Node {
    p: P2,
    cin: Option<P2>,
    cout: Option<P2>,
    /// (shape id, node index) of every original node merged into this one.
    origins: Vec<(u64, usize)>,
}

#[derive(Clone, Debug)]
struct Chain {
    nodes: Vec<Node>,
    kinds: Vec<PrimKind>,
    closed: bool,
    /// Index of the shape it came from (after merging: the group's representative).
    owner: usize,
}

impl Chain {
    fn reverse(&mut self) {
        self.nodes.reverse();
        for n in &mut self.nodes {
            std::mem::swap(&mut n.cin, &mut n.cout);
        }
        self.kinds.reverse();
    }
    fn first(&self) -> P2 {
        self.nodes[0].p
    }
    fn last(&self) -> P2 {
        self.nodes[self.nodes.len() - 1].p
    }
}

/// The continuous pieces of a path shape, in world space.
fn chains_of(s: &Shape, owner: usize) -> Vec<Chain> {
    let ShapeKind::Path(pd) = &s.kind else { return Vec::new() };
    let x = s.xform;
    let node = |v: usize| -> Node {
        let vt = pd.verts[v];
        Node { p: x.apply(vt.p), cin: vt.c1.map(|c| x.apply(c)), cout: vt.c0.map(|c| x.apply(c)), origins: vec![(s.id, v)] }
    };
    let mut out = Vec::new();
    let mut cur: Option<(Chain, usize, usize)> = None; // chain, first vertex, last vertex
    for pr in &pd.prims {
        if pr.a >= pd.verts.len() || pr.b >= pd.verts.len() {
            continue;
        }
        let continues = matches!(&cur, Some((_, _, last)) if *last == pr.a);
        if !continues {
            if let Some((c, _, _)) = cur.take() {
                out.push(c);
            }
            cur = Some((Chain { nodes: vec![node(pr.a)], kinds: Vec::new(), closed: false, owner }, pr.a, pr.a));
        }
        let (c, first, last) = cur.as_mut().unwrap();
        c.kinds.push(pr.kind);
        if pr.b == *first && c.nodes.len() > 1 {
            c.nodes[0].cin = pd.verts[pr.b].c1.map(|q| x.apply(q));
            c.closed = true;
            out.push(cur.take().unwrap().0);
        } else {
            c.nodes.push(node(pr.b));
            *last = pr.b;
        }
    }
    if let Some((c, _, _)) = cur {
        out.push(c);
    }
    out
}

fn find(parent: &mut HashMap<usize, usize>, a: usize) -> usize {
    let p = *parent.get(&a).unwrap_or(&a);
    if p == a {
        a
    } else {
        let r = find(parent, p);
        parent.insert(a, r);
        r
    }
}

/// Join the open pieces of shapes `idx` whose ends are within `tol`, closing pieces whose own ends meet.
/// Shapes that get joined become one path (keeping the id, layer and name of the last of them); relations
/// that pointed at their nodes follow. Returns the indices of the joined shapes, or nothing if nothing joined.
pub fn join(doc: &mut Document, idx: &[usize], tol: f64) -> Vec<usize> {
    let mut idx: Vec<usize> = idx.iter().copied().filter(|i| matches!(doc.shapes.get(*i).map(|s| &s.kind), Some(ShapeKind::Path(_)))).collect();
    idx.sort_unstable();
    idx.dedup();
    let mut chains: Vec<Chain> = idx.iter().flat_map(|i| chains_of(&doc.shapes[*i], *i)).collect();
    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut touched: Vec<usize> = Vec::new();
    loop {
        let mut changed = false;
        // close pieces whose two ends meet
        for c in chains.iter_mut() {
            if !c.closed && c.nodes.len() > 2 && c.first().dist(c.last()) <= tol {
                let last = c.nodes.pop().unwrap();
                c.nodes[0].cin = last.cin;
                c.nodes[0].origins.extend(last.origins);
                c.closed = true;
                touched.push(c.owner);
                changed = true;
            }
        }
        // the closest pair of open ends within the tolerance
        let mut best: Option<(f64, usize, usize, bool, bool)> = None; // (dist, i, j, reverse i, reverse j)
        for i in 0..chains.len() {
            if chains[i].closed {
                continue;
            }
            for j in i + 1..chains.len() {
                if chains[j].closed {
                    continue;
                }
                let (a, b) = (&chains[i], &chains[j]);
                for (d, ri, rj) in [(a.last().dist(b.first()), false, false), (a.last().dist(b.last()), false, true), (a.first().dist(b.first()), true, false), (a.first().dist(b.last()), true, true)] {
                    if d <= tol && best.map(|bb| d < bb.0).unwrap_or(true) {
                        best = Some((d, i, j, ri, rj));
                    }
                }
            }
        }
        if let Some((_, i, j, ri, rj)) = best {
            let mut b = chains.remove(j);
            let a = &mut chains[i];
            if ri {
                a.reverse();
            }
            if rj {
                b.reverse();
            }
            let head = b.nodes.remove(0);
            let end = a.nodes.last_mut().unwrap();
            end.cout = head.cout;
            end.origins.extend(head.origins);
            a.nodes.extend(b.nodes);
            a.kinds.extend(b.kinds);
            let (ra, rb) = (find(&mut parent, a.owner), find(&mut parent, b.owner));
            if ra != rb {
                let root = ra.max(rb);
                parent.insert(ra, root);
                parent.insert(rb, root);
            }
            touched.push(a.owner);
            touched.push(b.owner);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    if touched.is_empty() {
        return Vec::new();
    }
    // groups of shapes that became one, by representative (the highest index)
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &idx {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let touched_roots: std::collections::BTreeSet<usize> = touched.iter().map(|t| find(&mut parent, *t)).collect();
    let mut map: HashMap<(u64, usize), (u64, usize)> = HashMap::new();
    let mut remove: Vec<usize> = Vec::new();
    for root in &touched_roots {
        let members = &groups[root];
        let rep = *members.iter().max().unwrap();
        let id = doc.shapes[rep].id;
        let mut pd = PathData::default();
        for c in chains.iter().filter(|c| members.contains(&c.owner)) {
            let base = pd.verts.len();
            let n = c.nodes.len();
            for (k, nd) in c.nodes.iter().enumerate() {
                pd.verts.push(Vert { p: nd.p, c0: nd.cout, c1: nd.cin });
                for o in &nd.origins {
                    map.insert(*o, (id, base + k));
                }
            }
            for (k, kind) in c.kinds.iter().enumerate() {
                pd.prims.push(Prim { kind: *kind, a: base + k, b: base + (k + 1) % n });
            }
        }
        let s = &mut doc.shapes[rep];
        s.kind = ShapeKind::Path(pd);
        s.xform = Xform::IDENTITY;
        remove.extend(members.iter().copied().filter(|m| *m != rep));
    }
    crate::relations::remap_nodes(doc, &map);
    remove.sort_unstable_by(|a, b| b.cmp(a));
    let reps: Vec<u64> = touched_roots.iter().map(|r| doc.shapes[*groups[r].iter().max().unwrap()].id).collect();
    for r in remove {
        doc.shapes.remove(r);
    }
    reps.iter().filter_map(|id| doc.shapes.iter().position(|s| s.id == *id)).collect()
}

/// After drawing shape `new`: join it to any open path whose end it touches.
pub fn join_new(doc: &mut Document, new: usize) -> Vec<usize> {
    let Some(s) = doc.shapes.get(new) else { return Vec::new() };
    let ends: Vec<P2> = chains_of(s, new).iter().filter(|c| !c.closed).flat_map(|c| [c.first(), c.last()]).collect();
    if ends.is_empty() {
        return Vec::new();
    }
    let mut cands = vec![new];
    for (i, o) in doc.shapes.iter().enumerate() {
        if i == new || o.locked || !matches!(o.kind, ShapeKind::Path(_)) || !o.bbox().expand(SNAP_JOIN_TOL).contains_any(&ends) {
            continue;
        }
        if chains_of(o, i).iter().filter(|c| !c.closed).any(|c| ends.iter().any(|e| e.dist(c.first()) <= SNAP_JOIN_TOL || e.dist(c.last()) <= SNAP_JOIN_TOL)) {
            cands.push(i);
        }
    }
    join(doc, &cands, SNAP_JOIN_TOL)
}

trait ContainsAny {
    fn contains_any(&self, pts: &[P2]) -> bool;
}

impl ContainsAny for BBox {
    fn contains_any(&self, pts: &[P2]) -> bool {
        pts.iter().any(|p| self.contains(*p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_arcs_make_a_closed_shape() {
        let mut d = Document::default();
        let a = crate::sketch::arc_path(p2(0.0, 0.0), 10.0, 0.0, std::f64::consts::PI);
        let b = crate::sketch::arc_path(p2(0.0, 0.0), 10.0, std::f64::consts::PI, std::f64::consts::PI);
        d.shapes.push(Shape::new(ShapeKind::Path(a), Xform::IDENTITY, 0));
        d.shapes.push(Shape::new(ShapeKind::Path(b), Xform::IDENTITY, 0));
        let out = join_new(&mut d, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(d.shapes.len(), 1);
        assert!(d.shapes[0].is_closed(), "the two arcs now bound a region");
    }
    #[test]
    fn auto_join_closes_and_keeps_relations() {
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0)], false)), Xform::IDENTITY, 0));
        d.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(10.0, 10.0), p2(0.0, 10.03), p2(0.02, 0.0)], false)), Xform::IDENTITY, 0));
        let id0 = d.shapes[0].id;
        d.relations.push(crate::relations::make(crate::relations::RelKind::Horizontal, &[crate::relations::Ent::Line { shape: id0, a: 0, b: 1 }]));
        let out = join(&mut d, &[0, 1], AUTO_JOIN_TOL);
        assert_eq!((out.len(), d.shapes.len()), (1, 1));
        assert!(d.shapes[0].is_closed());
        crate::relations::prune(&mut d);
        assert_eq!(d.relations.len(), 1, "the horizontal relation followed the join");
    }
}
