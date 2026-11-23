//! Stencil bridges (so the middles of letters such as O, A and B stay attached when cut), and making shapes
//! bolder or thinner.

use crate::geom::*;
use crate::model::*;
use geo::{BooleanOps, LineString, MultiPolygon, Polygon};

/// Cut bridges `width` mm wide through every part that has holes: a strip straight up and down through the
/// middle of each hole, across that part only, so the piece inside the hole stays joined to the rest.
pub fn bridges(rings: &[Vec<P2>], width: f64) -> Vec<Vec<P2>> {
    let mp = crate::ops::to_multipolygon(rings);
    let w = width.clamp(0.05, 50.0);
    let mut out: Vec<Polygon<f64>> = Vec::new();
    for poly in mp.0 {
        if poly.interiors().is_empty() {
            out.push(poly);
            continue;
        }
        let pb = ring_box(poly.exterior());
        let mut part = MultiPolygon(vec![poly.clone()]);
        for hole in poly.interiors() {
            let c = p2(strip_x(hole), 0.0);
            let strip = Polygon::new(LineString::from(vec![(c.x - w / 2.0, pb.min.y - 1.0), (c.x + w / 2.0, pb.min.y - 1.0), (c.x + w / 2.0, pb.max.y + 1.0), (c.x - w / 2.0, pb.max.y + 1.0), (c.x - w / 2.0, pb.min.y - 1.0)]), vec![]);
            part = part.difference(&MultiPolygon(vec![strip]));
        }
        out.extend(part.0);
    }
    crate::ops::from_multipolygon(&MultiPolygon(out))
}

/// Where a vertical bridge crosses a hole: the middle of the widest span across the hole at its middle height
/// (so it always goes through the hole, even a C-shaped one).
fn strip_x(hole: &LineString<f64>) -> f64 {
    let b = ring_box(hole);
    let mut best = (b.center().x, -1.0);
    for f in [0.5, 0.35, 0.65, 0.2, 0.8] {
        let y = b.min.y + b.height() * f;
        let mut xs: Vec<f64> = hole.0.windows(2).filter(|w| (w[0].y > y) != (w[1].y > y)).map(|w| w[0].x + (y - w[0].y) * (w[1].x - w[0].x) / (w[1].y - w[0].y)).collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2).filter(|c| c.len() == 2) {
            let span = pair[1] - pair[0];
            if span > best.1 {
                best = ((pair[0] + pair[1]) / 2.0, span);
            }
        }
        if best.1 > 0.0 {
            break;
        }
    }
    best.0
}

fn ring_box(l: &LineString<f64>) -> BBox {
    let mut b = BBox::EMPTY;
    for c in &l.0 {
        b.add(p2(c.x, c.y));
    }
    b
}

/// What to do to the selected shapes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Grow (or with a negative amount, thin) every outline by this many mm.
    Bolden(f64),
    /// Add stencil bridges this wide.
    Bridges(f64),
}

/// Change the selected closed shapes. Each becomes a path on its layer (text becomes outlines). Shapes with
/// open lines, pictures (or groups holding one) and locked shapes are left as they are. Returns (changed, texts among them, skipped).
pub fn apply(doc: &mut Document, sel: &[usize], change: Change) -> (usize, usize, usize) {
    let (mut n, mut texts, mut skipped) = (0, 0, 0);
    for &i in sel {
        let Some(s) = doc.shapes.get(i) else { continue };
        let lines = s.world_outline(0.02);
        if s.has_bitmap() || s.locked || lines.is_empty() || !lines.iter().all(|l| is_closed(l)) {
            skipped += 1;
            continue;
        }
        let new = match change {
            Change::Bolden(d) => crate::ops::grow(&lines, d),
            Change::Bridges(w) => bridges(&lines, w),
        };
        if new.is_empty() {
            // thinned away to nothing: keep it, and say so
            skipped += 1;
            continue;
        }
        if matches!(s.kind, ShapeKind::Text { .. }) {
            texts += 1;
        }
        let mut ns = Shape::new(ShapeKind::Path(PathData::from_polylines(&new)), Xform::IDENTITY, s.layer);
        (ns.name, ns.locked, ns.cut) = (s.name.clone(), s.locked, s.cut);
        doc.shapes[i] = ns;
        n += 1;
    }
    (n, texts, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(c: P2, r: f64) -> Vec<P2> {
        let mut v: Vec<P2> = (0..64).map(|k| {
            let a = k as f64 * std::f64::consts::TAU / 64.0;
            c + p2(r * a.cos(), r * a.sin())
        }).collect();
        v.push(v[0]);
        v
    }

    fn area(rings: &[Vec<P2>]) -> f64 {
        use geo::Area;
        crate::ops::to_multipolygon(rings).unsigned_area()
    }

    #[test]
    fn an_o_gets_two_bridges_and_stays_in_two_halves() {
        // an "O": a ring 10 mm across with a 6 mm hole
        let o = vec![ring(p2(0.0, 0.0), 5.0), ring(p2(0.0, 0.0), 3.0)];
        let out = bridges(&o, 1.0);
        let mp = crate::ops::to_multipolygon(&out);
        assert_eq!(mp.0.len(), 2, "split into left and right halves");
        assert!(mp.0.iter().all(|p| p.interiors().is_empty()), "no holes left: the middle is joined top and bottom");
        // about 1 mm × 2 mm of ring removed at the top and the bottom
        let lost = area(&o) - area(&out);
        assert!((lost - 4.0).abs() < 0.5, "{lost}");
        // a C-shaped hole (its box's middle is outside it) still gets bridged
        let mut c_hole: Vec<P2> = (0..=48).map(|k| {
            let a = 0.6 + k as f64 * (std::f64::consts::TAU - 1.2) / 48.0;
            p2(3.0 * a.cos(), 3.0 * a.sin())
        }).collect();
        c_hole.extend((0..=48).rev().map(|k| {
            let a = 0.6 + k as f64 * (std::f64::consts::TAU - 1.2) / 48.0;
            p2(2.0 * a.cos(), 2.0 * a.sin())
        }));
        c_hole.push(c_hole[0]);
        let shape = vec![ring(p2(0.0, 0.0), 5.0), c_hole];
        let out = crate::ops::to_multipolygon(&bridges(&shape, 0.5));
        assert!(out.0.iter().all(|p| p.interiors().is_empty()), "no hole left unbridged");
        // a shape without holes is left alone
        let dot = vec![ring(p2(20.0, 0.0), 2.0)];
        assert!((area(&bridges(&dot, 1.0)) - area(&dot)).abs() < 1e-9);
    }

    #[test]
    fn bolden_and_thin_change_the_size() {
        let mut doc = Document::default();
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 0));
        doc.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&[vec![p2(0.0, 0.0), p2(5.0, 0.0)]])), Xform::IDENTITY, 0));
        let (n, _, skipped) = apply(&mut doc, &[0, 1], Change::Bolden(0.5));
        assert_eq!((n, skipped), (1, 1), "the open line is skipped");
        assert!((doc.shapes[0].bbox().width() - 11.0).abs() < 1e-6);
        apply(&mut doc, &[0], Change::Bolden(-1.0));
        assert!((doc.shapes[0].bbox().width() - 9.0).abs() < 1e-6);
        // thinned away entirely: kept as it was
        let before = doc.shapes[0].bbox();
        let (n, _, skipped) = apply(&mut doc, &[0], Change::Bolden(-10.0));
        assert_eq!((n, skipped), (0, 1));
        assert_eq!(doc.shapes[0].bbox(), before);
    }
}
