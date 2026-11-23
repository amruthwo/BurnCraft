//! Envelope and mesh warps: bend shapes as if printed on a stretchy sheet. Every point is moved by where it
//! sits in the selection's box (u across, v up, both 0..1).

use crate::geom::*;
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WarpKind {
    #[default]
    Arch,
    Arc,
    Bulge,
    Wave,
    Rise,
    Fisheye,
    Perspective,
    Mesh,
}

impl WarpKind {
    pub const ALL: [WarpKind; 8] = [WarpKind::Arch, WarpKind::Arc, WarpKind::Bulge, WarpKind::Wave, WarpKind::Rise, WarpKind::Fisheye, WarpKind::Perspective, WarpKind::Mesh];
    pub fn label(self) -> &'static str {
        match self {
            WarpKind::Arch => "Arch",
            WarpKind::Arc => "Arc",
            WarpKind::Bulge => "Bulge",
            WarpKind::Wave => "Wave / flag",
            WarpKind::Rise => "Rise",
            WarpKind::Fisheye => "Fisheye",
            WarpKind::Perspective => "Perspective",
            WarpKind::Mesh => "Mesh",
        }
    }
    /// Shaped by the amount slider (and its handle); the others by moving their handles.
    pub fn uses_amount(self) -> bool {
        !matches!(self, WarpKind::Perspective | WarpKind::Mesh)
    }
}

/// A warp being set up: the kind, the amount (−1..1), and the handles' offsets from where they start (mm):
/// 4 corners for perspective (bottom-left, bottom-right, top-left, top-right), 3 × 3 points for the mesh (row
/// by row from the bottom).
#[derive(Clone, Debug, PartialEq)]
pub struct Warp {
    pub kind: WarpKind,
    pub amount: f64,
    pub corners: [P2; 4],
    pub mesh: [P2; 9],
}

impl Default for Warp {
    fn default() -> Self {
        Warp { kind: WarpKind::Arch, amount: 0.3, corners: [P2::default(); 4], mesh: [P2::default(); 9] }
    }
}

impl Warp {
    /// The point the amount handle starts from (u, v in the box): one this kind of warp moves.
    fn amount_point(&self) -> (f64, f64) {
        match self.kind {
            WarpKind::Wave => (0.25, 1.0),
            WarpKind::Rise => (1.0, 1.0),
            WarpKind::Fisheye => (0.5, 0.75),
            _ => (0.5, 1.0),
        }
    }

    /// How far the amount handle rises (mm) for an amount of 1.
    fn lift_per_amount(&self, b: &BBox) -> f64 {
        let (u, v) = self.amount_point();
        let base = p2(b.min.x + u * b.width(), b.min.y + v * b.height());
        let one = Warp { amount: 1.0, ..self.clone() };
        one.map(b, base).y - base.y
    }

    /// Where the handles are on the bed, for a selection box `b`.
    pub fn handles(&self, b: &BBox) -> Vec<P2> {
        let at = |u: f64, v: f64| p2(b.min.x + u * b.width(), b.min.y + v * b.height());
        match self.kind {
            WarpKind::Perspective => [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].iter().zip(&self.corners).map(|((u, v), d)| at(*u, *v) + *d).collect(),
            WarpKind::Mesh => (0..9).map(|k| at((k % 3) as f64 / 2.0, (k / 3) as f64 / 2.0) + self.mesh[k]).collect(),
            // the amount handle: on a point the warp moves, where it moves it to
            _ => {
                let (u, v) = self.amount_point();
                vec![self.map(b, at(u, v))]
            }
        }
    }

    /// Drag handle `i` to `to` (bed coordinates).
    pub fn set_handle(&mut self, b: &BBox, i: usize, to: P2) {
        let at = |u: f64, v: f64| p2(b.min.x + u * b.width(), b.min.y + v * b.height());
        match self.kind {
            WarpKind::Perspective if i < 4 => {
                let base = [at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0)][i];
                self.corners[i] = to - base;
            }
            WarpKind::Mesh if i < 9 => self.mesh[i] = to - at((i % 3) as f64 / 2.0, (i / 3) as f64 / 2.0),
            WarpKind::Perspective | WarpKind::Mesh => {}
            // the amount handle moves up and down, staying under the pointer
            _ => {
                let (u, v) = self.amount_point();
                let base = at(u, v);
                let lift = self.lift_per_amount(b);
                if lift.abs() > 1e-9 {
                    self.amount = ((to.y - base.y) / lift).clamp(-1.0, 1.0);
                }
            }
        }
    }

    /// Where a point of the selection goes.
    pub fn map(&self, b: &BBox, p: P2) -> P2 {
        let (w, h) = (b.width().max(1e-9), b.height().max(1e-9));
        let (u, v) = ((p.x - b.min.x) / w, (p.y - b.min.y) / h);
        let a = self.amount;
        let pi = std::f64::consts::PI;
        match self.kind {
            // the top bends, the bottom stays straight
            WarpKind::Arch => p2(p.x, p.y + a * h * v * (pi * u).sin()),
            // everything bends together
            WarpKind::Arc => p2(p.x, p.y + a * h * (pi * u).sin()),
            // taller (or thinner) in the middle
            WarpKind::Bulge => {
                let cy = b.min.y + h / 2.0;
                p2(p.x, cy + (p.y - cy) * (1.0 + a * (pi * u).sin()))
            }
            WarpKind::Wave => p2(p.x, p.y + a * h * 0.5 * (2.0 * pi * u).sin()),
            // rises from left to right along an easing curve
            WarpKind::Rise => p2(p.x, p.y + a * h * (1.0 - (pi * u).cos()) / 2.0),
            // magnified in the middle (or squeezed)
            WarpKind::Fisheye => {
                let c = b.center();
                let (dx, dy) = ((p.x - c.x) / (w / 2.0), (p.y - c.y) / (h / 2.0));
                let r2 = (dx * dx + dy * dy).min(1.0);
                let k = 1.0 + a * (1.0 - r2);
                p2(c.x + (p.x - c.x) * k, c.y + (p.y - c.y) * k)
            }
            WarpKind::Perspective => {
                // the box's corners moved: blend between them
                let q = |i: usize, base: P2| base + self.corners[i];
                let (q00, q10, q01, q11) = (q(0, b.min), q(1, p2(b.max.x, b.min.y)), q(2, p2(b.min.x, b.max.y)), q(3, b.max));
                let bottom = q00 * (1.0 - u) + q10 * u;
                let top = q01 * (1.0 - u) + q11 * u;
                bottom * (1.0 - v) + top * v
            }
            WarpKind::Mesh => {
                // a smooth surface through a 3 × 3 grid of handles (unmoved handles change nothing)
                let bern = |t: f64| [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
                let (bu, bv) = (bern(u), bern(v));
                let mut out = p;
                for j in 0..3 {
                    for i in 0..3 {
                        out = out + self.mesh[j * 3 + i] * (bu[i] * bv[j] * mesh_gain(i) * mesh_gain(j));
                    }
                }
                out
            }
        }
    }
}

/// A handle's pull on the surface: the middle control point of a quadratic only reaches half way, so it's
/// doubled to move the curve through the handle.
fn mesh_gain(i: usize) -> f64 {
    if i == 1 {
        2.0
    } else {
        1.0
    }
}

/// The selection's outlines, with long straight pieces cut short so they can bend.
pub fn outlines(doc: &Document, sel: &[usize]) -> Vec<(usize, Vec<Vec<P2>>)> {
    sel.iter()
        .filter_map(|i| doc.shapes.get(*i).map(|s| (*i, s)))
        // pictures (and groups holding one) and locked shapes are left as they are
        .filter(|(_, s)| !s.has_bitmap() && !s.locked)
        .map(|(i, s)| {
            let lines = s.world_outline(0.02).into_iter().map(|l| densify(&l, 0.5)).collect();
            (i, lines)
        })
        .collect()
}

/// Add points so no piece is longer than `step` mm.
pub fn densify(l: &[P2], step: f64) -> Vec<P2> {
    let mut out = Vec::with_capacity(l.len());
    for w in l.windows(2) {
        let n = ((w[0].dist(w[1]) / step).ceil() as usize).clamp(1, 2000);
        for k in 0..n {
            out.push(w[0] + (w[1] - w[0]) * (k as f64 / n as f64));
        }
    }
    if let Some(p) = l.last() {
        out.push(*p);
    }
    out
}

/// Bend the selected shapes (pictures, groups with pictures and locked shapes are left as they are). Each becomes a path on its layer; text becomes
/// outlines. Returns how many shapes changed and how many were text.
pub fn apply(doc: &mut Document, sel: &[usize], w: &Warp) -> (usize, usize) {
    let b = crate::ops::selection_bbox(doc, sel);
    if b.is_empty() {
        return (0, 0);
    }
    let (mut n, mut texts) = (0, 0);
    for (i, lines) in outlines(doc, sel) {
        let warped: Vec<Vec<P2>> = lines.into_iter().map(|l| l.into_iter().map(|p| w.map(&b, p)).collect()).collect();
        let s = &doc.shapes[i];
        if matches!(s.kind, ShapeKind::Text { .. }) {
            texts += 1;
        }
        let mut ns = Shape::new(ShapeKind::Path(PathData::from_polylines(&warped)), Xform::IDENTITY, s.layer);
        (ns.name, ns.locked, ns.cut) = (s.name.clone(), s.locked, s.cut);
        doc.shapes[i] = ns;
        n += 1;
    }
    (n, texts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warps_leave_the_corners_where_they_should_be() {
        let b = BBox { min: p2(0.0, 0.0), max: p2(100.0, 20.0) };
        let none = |k: WarpKind| Warp { kind: k, amount: 0.0, ..Default::default() };
        for k in WarpKind::ALL {
            for p in [p2(0.0, 0.0), p2(37.0, 11.0), p2(100.0, 20.0)] {
                assert!(none(k).map(&b, p).dist(p) < 1e-9, "{k:?} with nothing set changes nothing");
            }
        }
        // arch: the bottom stays, the top's middle rises by amount × height
        let w = Warp { kind: WarpKind::Arch, amount: 0.5, ..Default::default() };
        assert!(w.map(&b, p2(50.0, 0.0)).dist(p2(50.0, 0.0)) < 1e-9);
        assert!(w.map(&b, p2(50.0, 20.0)).dist(p2(50.0, 30.0)) < 1e-9);
        assert!(w.map(&b, p2(0.0, 20.0)).dist(p2(0.0, 20.0)) < 1e-9, "the ends stay");
        // the amount handle sits where the warp lifts the top, and dragging it sets the amount
        let mut w = Warp { kind: WarpKind::Arc, amount: 0.0, ..Default::default() };
        w.set_handle(&b, 0, p2(50.0, 26.0));
        assert!((w.amount - 0.3).abs() < 1e-9);
        assert!(w.handles(&b)[0].dist(p2(50.0, 26.0)) < 1e-9);
        // every amount-driven kind: the handle stays under the pointer
        for k in WarpKind::ALL.into_iter().filter(|k| k.uses_amount()) {
            let mut w = Warp { kind: k, amount: 0.0, ..Default::default() };
            let h0 = w.handles(&b)[0];
            let to = p2(h0.x, h0.y + 3.0);
            w.set_handle(&b, 0, to);
            assert!(w.amount != 0.0 && w.handles(&b)[0].dist(to) < 1e-9, "{k:?}: {:?} vs {to:?}", w.handles(&b)[0]);
        }
        // perspective: a corner moved takes the box's corner with it
        let mut w = Warp { kind: WarpKind::Perspective, ..Default::default() };
        w.set_handle(&b, 3, p2(90.0, 30.0));
        assert!(w.map(&b, p2(100.0, 20.0)).dist(p2(90.0, 30.0)) < 1e-9);
        assert!(w.map(&b, p2(0.0, 0.0)).dist(p2(0.0, 0.0)) < 1e-9);
        // mesh: a moved corner handle moves that corner exactly
        let mut w = Warp { kind: WarpKind::Mesh, ..Default::default() };
        w.set_handle(&b, 8, p2(110.0, 25.0));
        assert!(w.map(&b, p2(100.0, 20.0)).dist(p2(110.0, 25.0)) < 1e-9);
    }

    #[test]
    fn applying_turns_text_and_shapes_into_bent_paths() {
        let mut doc = Document::default();
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 40.0, h: 10.0, cr: 0.0 }, Xform::translate(50.0, 50.0), 2));
        doc.sync_cuts();
        let before = doc.shapes[0].bbox();
        let (n, texts) = apply(&mut doc, &[0], &Warp { kind: WarpKind::Arch, amount: 0.5, ..Default::default() });
        assert_eq!((n, texts), (1, 0));
        let s = &doc.shapes[0];
        assert!(matches!(s.kind, ShapeKind::Path(_)) && s.layer == 2);
        let after = s.bbox();
        assert!((after.max.y - (before.max.y + 5.0)).abs() < 0.05, "{after:?}");
        assert!((after.min.y - before.min.y).abs() < 1e-6);
        assert!(s.world_outline(0.01).iter().all(|l| is_closed(l)), "still closed");
        // locked shapes stay as they are
        doc.shapes[0].locked = true;
        let before = doc.shapes[0].bbox();
        assert_eq!(apply(&mut doc, &[0], &Warp { kind: WarpKind::Arch, amount: 0.5, ..Default::default() }).0, 0);
        assert_eq!(doc.shapes[0].bbox(), before);
    }
}
