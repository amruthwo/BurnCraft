//! Editing operations on a document and a selection of top-level shapes.

use crate::geom::*;
use crate::model::*;
use geo::{BooleanOps, Buffer, Coord, LineString, MultiPolygon, Polygon};

pub fn selection_bbox(doc: &Document, sel: &[usize]) -> BBox {
    sel.iter().filter_map(|i| doc.shapes.get(*i)).fold(BBox::EMPTY, |b, s| b.union(&s.bbox()))
}

pub fn transform_selection(doc: &mut Document, sel: &[usize], t: Xform) {
    for &i in sel {
        if let Some(s) = doc.shapes.get_mut(i) {
            if !s.locked {
                s.xform = t.then_after(s.xform);
            }
        }
    }
}

pub fn flip(doc: &mut Document, sel: &[usize], horizontal: bool) {
    let c = selection_bbox(doc, sel).center();
    let t = if horizontal { Xform::scale(-1.0, 1.0) } else { Xform::scale(1.0, -1.0) };
    transform_selection(doc, sel, Xform::about(c, t));
}

pub fn rotate(doc: &mut Document, sel: &[usize], deg: f64) {
    let c = selection_bbox(doc, sel).center();
    transform_selection(doc, sel, Xform::about(c, Xform::rotate(deg.to_radians())));
}

#[derive(Clone, Copy, Debug)]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
    HCenter,
    VCenter,
}

/// Align to the last-selected shape (LightBurn and CAD convention), or to the selection box for one shape.
pub fn align(doc: &mut Document, sel: &[usize], a: Align) {
    if sel.len() < 2 {
        return;
    }
    let anchor = doc.shapes[*sel.last().unwrap()].bbox();
    for &i in &sel[..sel.len() - 1] {
        let b = doc.shapes[i].bbox();
        let d = match a {
            Align::Left => p2(anchor.min.x - b.min.x, 0.0),
            Align::Right => p2(anchor.max.x - b.max.x, 0.0),
            Align::Top => p2(0.0, anchor.max.y - b.max.y),
            Align::Bottom => p2(0.0, anchor.min.y - b.min.y),
            Align::HCenter => p2(0.0, anchor.center().y - b.center().y),
            Align::VCenter => p2(anchor.center().x - b.center().x, 0.0),
        };
        transform_selection(doc, &[i], Xform::translate(d.x, d.y));
    }
}

pub fn distribute(doc: &mut Document, sel: &[usize], horizontal: bool) {
    if sel.len() < 3 {
        return;
    }
    let mut items: Vec<(usize, BBox)> = sel.iter().map(|i| (*i, doc.shapes[*i].bbox())).collect();
    items.sort_by(|a, b| {
        let (ka, kb) = if horizontal { (a.1.center().x, b.1.center().x) } else { (a.1.center().y, b.1.center().y) };
        ka.partial_cmp(&kb).unwrap()
    });
    let total: f64 = items.iter().map(|(_, b)| if horizontal { b.width() } else { b.height() }).sum();
    let (first, last) = (items[0].1, items[items.len() - 1].1);
    let span = if horizontal { last.max.x - first.min.x } else { last.max.y - first.min.y };
    let gap = (span - total) / (items.len() - 1) as f64;
    let mut cursor = if horizontal { first.min.x } else { first.min.y };
    for (i, b) in items {
        let d = if horizontal { cursor - b.min.x } else { cursor - b.min.y };
        if horizontal {
            transform_selection(doc, &[i], Xform::translate(d, 0.0));
            cursor += b.width() + gap;
        } else {
            transform_selection(doc, &[i], Xform::translate(0.0, d));
            cursor += b.height() + gap;
        }
    }
}

pub fn move_to(doc: &mut Document, sel: &[usize], center: P2) {
    let c = selection_bbox(doc, sel).center();
    transform_selection(doc, sel, Xform::translate(center.x - c.x, center.y - c.y));
}

/// Group the selection; returns the new group's index.
pub fn group(doc: &mut Document, sel: &[usize]) -> Option<usize> {
    if sel.len() < 2 {
        return None;
    }
    let mut idx: Vec<usize> = sel.to_vec();
    idx.sort();
    let layer = doc.shapes[idx[0]].layer;
    let mut children = Vec::new();
    for i in idx.iter().rev() {
        children.push(doc.shapes.remove(*i));
    }
    children.reverse();
    let at = idx[0].min(doc.shapes.len());
    doc.shapes.insert(at, Shape::new(ShapeKind::Group(children), Xform::IDENTITY, layer));
    Some(at)
}

pub fn ungroup(doc: &mut Document, sel: &[usize]) -> Vec<usize> {
    let mut idx: Vec<usize> = sel.to_vec();
    idx.sort();
    let mut new_sel = Vec::new();
    for i in idx.into_iter().rev() {
        let s = doc.shapes.remove(i);
        if let ShapeKind::Group(ch) = s.kind {
            let n = ch.len();
            for (k, mut c) in ch.into_iter().enumerate() {
                c.xform = s.xform.then_after(c.xform);
                doc.shapes.insert(i + k, c);
            }
            for v in new_sel.iter_mut() {
                *v += n - 1;
            }
            new_sel.extend(i..i + n);
        } else {
            doc.shapes.insert(i, s);
            new_sel.push(i);
        }
    }
    new_sel
}

pub fn delete(doc: &mut Document, sel: &[usize]) {
    let mut idx: Vec<usize> = sel.to_vec();
    idx.sort();
    idx.dedup();
    for i in idx.into_iter().rev() {
        if i < doc.shapes.len() && !doc.shapes[i].locked {
            doc.shapes.remove(i);
        }
    }
}

pub fn push_front(doc: &mut Document, sel: &[usize]) -> Vec<usize> {
    // "front" = drawn first = start of the list (LightBurn: "make this object draw first")
    let mut idx = sel.to_vec();
    idx.sort();
    let mut moved: Vec<Shape> = idx.iter().rev().map(|i| doc.shapes.remove(*i)).collect();
    moved.reverse();
    let n = moved.len();
    for (k, s) in moved.into_iter().enumerate() {
        doc.shapes.insert(k, s);
    }
    (0..n).collect()
}

pub fn push_back(doc: &mut Document, sel: &[usize]) -> Vec<usize> {
    let mut idx = sel.to_vec();
    idx.sort();
    let mut moved: Vec<Shape> = idx.iter().rev().map(|i| doc.shapes.remove(*i)).collect();
    moved.reverse();
    let start = doc.shapes.len();
    let n = moved.len();
    doc.shapes.extend(moved);
    (start..start + n).collect()
}

pub fn grid_array(doc: &mut Document, sel: &[usize], cols: usize, rows: usize, gap_x: f64, gap_y: f64) -> Vec<usize> {
    let bb = selection_bbox(doc, sel);
    let originals: Vec<Shape> = sel.iter().map(|i| doc.shapes[*i].clone()).collect();
    let mut new_sel = sel.to_vec();
    for r in 0..rows.max(1) {
        for c in 0..cols.max(1) {
            if r == 0 && c == 0 {
                continue;
            }
            let dx = c as f64 * (bb.width() + gap_x);
            let dy = -(r as f64) * (bb.height() + gap_y);
            for o in &originals {
                let mut s = o.clone();
                s.renew_ids();
                s.xform = Xform::translate(dx, dy).then_after(s.xform);
                doc.shapes.push(s);
                new_sel.push(doc.shapes.len() - 1);
            }
        }
    }
    new_sel
}

pub fn convert_to_path(doc: &mut Document, sel: &[usize]) {
    for &i in sel {
        if let Some(p) = doc.shapes[i].to_path() {
            doc.shapes[i] = p;
        }
    }
}

pub fn break_apart(doc: &mut Document, sel: &[usize]) -> Vec<usize> {
    let mut idx = sel.to_vec();
    idx.sort();
    let mut new_sel = Vec::new();
    for i in idx.into_iter().rev() {
        let s = &doc.shapes[i];
        if s.is_bitmap() {
            continue;
        }
        let lines = s.world_outline(0.02);
        if lines.len() < 2 {
            new_sel.push(i);
            continue;
        }
        let layer = s.layer;
        doc.shapes.remove(i);
        for (k, l) in lines.iter().enumerate() {
            doc.shapes.insert(i + k, Shape::new(ShapeKind::Path(PathData::from_polylines(&[l.clone()])), Xform::IDENTITY, layer));
        }
        for v in new_sel.iter_mut() {
            *v += lines.len() - 1;
        }
        new_sel.extend(i..i + lines.len());
    }
    new_sel
}

// --------------------------------------------------------------- booleans and offset (via `geo`)

fn ring_to_ls(r: &[P2]) -> LineString<f64> {
    LineString::from(r.iter().map(|p| Coord { x: p.x, y: p.y }).collect::<Vec<_>>())
}

/// Closed outlines → polygons with holes (even-odd nesting).
pub fn to_multipolygon(rings: &[Vec<P2>]) -> MultiPolygon<f64> {
    let rings: Vec<&Vec<P2>> = rings.iter().filter(|r| is_closed(r) && r.len() > 3).collect();
    let depth: Vec<usize> = rings.iter().enumerate().map(|(i, r)| rings.iter().enumerate().filter(|(j, o)| *j != i && point_in_rings(r[0], std::slice::from_ref(*o))).count()).collect();
    let mut polys = Vec::new();
    for (i, r) in rings.iter().enumerate() {
        if depth[i] % 2 != 0 {
            continue;
        }
        let holes: Vec<LineString<f64>> = rings
            .iter()
            .enumerate()
            .filter(|(j, h)| depth[*j] == depth[i] + 1 && point_in_rings(h[0], std::slice::from_ref(*r)))
            .map(|(_, h)| ring_to_ls(h))
            .collect();
        polys.push(Polygon::new(ring_to_ls(r), holes));
    }
    MultiPolygon(polys)
}

pub fn from_multipolygon(mp: &MultiPolygon<f64>) -> Vec<Vec<P2>> {
    let mut out = Vec::new();
    for poly in &mp.0 {
        for ls in std::iter::once(poly.exterior()).chain(poly.interiors().iter()) {
            let mut pts: Vec<P2> = ls.0.iter().map(|c| p2(c.x, c.y)).collect();
            if pts.len() > 2 {
                if !is_closed(&pts) {
                    pts.push(pts[0]);
                }
                out.push(pts);
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

/// Combine the selection; the result replaces the selected shapes. Returns the new index.
pub fn boolean(doc: &mut Document, sel: &[usize], op: BoolOp) -> Result<usize, String> {
    if sel.len() < 2 {
        return Err("Select at least two closed shapes.".into());
    }
    let shapes: Vec<&Shape> = sel.iter().map(|i| &doc.shapes[*i]).collect();
    if shapes.iter().any(|s| s.is_bitmap() || !s.is_closed()) {
        return Err("All selected shapes must be closed outlines (no images or open lines).".into());
    }
    let layer = shapes[0].layer;
    let mps: Vec<MultiPolygon<f64>> = shapes.iter().map(|s| to_multipolygon(&s.world_outline(0.02))).collect();
    let mut acc = mps[0].clone();
    for (k, m) in mps.iter().enumerate().skip(1) {
        acc = match op {
            BoolOp::Union => acc.union(m),
            BoolOp::Intersect => acc.intersection(m),
            BoolOp::Subtract => {
                let _ = k;
                acc.difference(m)
            }
        };
    }
    let lines = from_multipolygon(&acc);
    if lines.is_empty() {
        return Err("The result is empty.".into());
    }
    let mut idx = sel.to_vec();
    idx.sort();
    for i in idx.iter().rev() {
        doc.shapes.remove(*i);
    }
    let at = idx[0].min(doc.shapes.len());
    doc.shapes.insert(at, Shape::new(ShapeKind::Path(PathData::from_polylines(&lines)), Xform::IDENTITY, layer));
    Ok(at)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetDir {
    Outward,
    Inward,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetJoin {
    Round,
    Bevel,
    Corner,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetOpts {
    pub dist: f64,
    pub dir: OffsetDir,
    pub join: OffsetJoin,
    /// Drop points closer than this to a straight line (0 = keep all).
    pub simplify: f64,
    /// Only offset the outermost outline of each shape (ignore holes and inner parts).
    pub outer_only: bool,
    /// "Delete Original Objects"
    pub replace: bool,
    /// "Select Resulting Objects"
    pub select_result: bool,
    /// "Optimize / Simplify Results": run the curve fitter (here: point reduction with `simplify` tolerance)
    pub optimize: bool,
}

impl OffsetOpts {
    /// Distance field range and step in the Offset dialog.
    pub const MAX_DIST: f64 = 1000.0;
    pub const DIST_STEP: f64 = 1.0;
    /// Second spin box of the dialog: 0..40 in steps of 0.0625.
    pub const MAX_TOL: f64 = 40.0;
    pub const TOL_STEP: f64 = 0.0625;
}

impl Default for OffsetOpts {
    fn default() -> Self {
        OffsetOpts { dist: 2.0, dir: OffsetDir::Outward, join: OffsetJoin::Round, simplify: 0.0625, outer_only: false, replace: false, select_result: true, optimize: true }
    }
}

fn buffer(mp: &MultiPolygon<f64>, d: f64, join: OffsetJoin) -> MultiPolygon<f64> {
    use geo::algorithm::buffer::{BufferStyle, LineJoin};
    let style = BufferStyle::new(d);
    let style = match join {
        OffsetJoin::Round => style,
        OffsetJoin::Bevel => style.line_join(LineJoin::Bevel),
        OffsetJoin::Corner => style.line_join(LineJoin::Miter(4.0)),
    };
    mp.buffer_with_style(style)
}

/// Offset outlines of one shape (in parent space), before any point reduction.
fn offset_rings(s: &Shape, o: &OffsetOpts) -> Vec<Vec<P2>> {
    offset_lines(s, &OffsetOpts { optimize: false, ..*o })
}

/// A circle shape (ellipse with equal radii under the transform): centre and radius in parent space.
pub fn circle_of(s: &Shape) -> Option<(P2, f64)> {
    let ShapeKind::Ellipse { rx, ry } = s.kind else { return None };
    let (sx, sy) = (p2(s.xform.a, s.xform.b).len(), p2(s.xform.c, s.xform.d).len());
    ((rx * sx - ry * sy).abs() <= 1e-6 * (rx * sx).max(1.0)).then(|| (s.xform.apply(p2(0.0, 0.0)), rx * sx))
}

/// The offset of one shape as a new shape kind + transform: circles give exact circles; everything else is
/// rebuilt from the offset outline with true arcs where it is round, so it can be dimensioned like a sketch.
pub fn offset_shape(s: &Shape, o: &OffsetOpts) -> Option<(ShapeKind, Xform)> {
    let d = o.dist.abs();
    if let Some((c, r)) = circle_of(s) {
        let mut radii = Vec::new();
        if matches!(o.dir, OffsetDir::Outward | OffsetDir::Both) {
            radii.push(r + d);
        }
        if matches!(o.dir, OffsetDir::Inward | OffsetDir::Both) && r - d > 1e-6 {
            radii.push(r - d);
        }
        return match radii.as_slice() {
            [] => None,
            [one] => Some((ShapeKind::Ellipse { rx: *one, ry: *one }, Xform::translate(c.x, c.y))),
            _ => {
                let mut pd = PathData::default();
                for r in radii {
                    crate::sketch::append_path(&mut pd, &crate::sketch::circle_path(c, r));
                }
                Some((ShapeKind::Path(pd), Xform::IDENTITY))
            }
        };
    }
    let rings = offset_rings(s, o);
    if rings.is_empty() {
        return None;
    }
    // the outline was flattened at 0.02 mm, so allow a little more when matching circles
    let tol = 0.06;
    if rings.len() == 1 {
        if let Some((c, r)) = crate::sketch::ring_circle(&rings[0], tol) {
            return Some((ShapeKind::Ellipse { rx: r, ry: r }, Xform::translate(c.x, c.y)));
        }
    }
    let mut pd = PathData::default();
    for r in &rings {
        let fitted = if o.optimize { crate::sketch::fit_arcs(r, tol) } else { PathData::from_polylines(std::slice::from_ref(r)) };
        crate::sketch::append_path(&mut pd, &fitted);
    }
    Some((ShapeKind::Path(pd), Xform::IDENTITY))
}

/// Offset outlines of one shape (in parent space) per the options.
pub fn offset_lines(s: &Shape, o: &OffsetOpts) -> Vec<Vec<P2>> {
    if s.is_bitmap() || o.dist.abs() < 1e-9 {
        return Vec::new();
    }
    let outline = s.world_outline(0.02);
    let mut mp = to_multipolygon(&outline);
    if o.outer_only {
        mp = MultiPolygon(mp.0.into_iter().map(|p| Polygon::new(p.exterior().clone(), vec![])).collect());
    }
    let d = o.dist.abs();
    let mut lines = Vec::new();
    if !mp.0.is_empty() {
        if matches!(o.dir, OffsetDir::Outward | OffsetDir::Both) {
            lines.extend(from_multipolygon(&buffer(&mp, d, o.join)));
        }
        if matches!(o.dir, OffsetDir::Inward | OffsetDir::Both) {
            lines.extend(from_multipolygon(&buffer(&mp, -d, o.join)));
        }
    }
    // open lines: outline around them
    for l in outline.iter().filter(|l| !is_closed(l) && l.len() > 1) {
        let line = geo::LineString::from(l.iter().map(|p| Coord { x: p.x, y: p.y }).collect::<Vec<_>>());
        lines.extend(from_multipolygon(&line.buffer(d)));
    }
    if o.optimize && o.simplify > 0.0 {
        lines = lines.into_iter().map(|l| simplify(&l, o.simplify)).filter(|l| l.len() > 3).collect();
    }
    lines
}

/// Preview of the offset result for the selection (parent space).
pub fn offset_preview(doc: &Document, sel: &[usize], o: &OffsetOpts) -> Vec<Vec<P2>> {
    sel.iter().filter_map(|i| doc.shapes.get(*i)).flat_map(|s| offset_lines(s, o)).collect()
}

/// Apply the offset; adds new shapes (or replaces originals) and returns their indices.
pub fn offset(doc: &mut Document, sel: &[usize], o: &OffsetOpts) -> Result<Vec<usize>, String> {
    let mut new_shapes = Vec::new();
    for &i in sel {
        let s = &doc.shapes[i];
        if let Some((kind, x)) = offset_shape(s, o) {
            new_shapes.push((i, Shape::new(kind, x, s.layer)));
        }
    }
    if new_shapes.is_empty() {
        return Err("Nothing to offset: select shapes or lines (not images).".into());
    }
    let mut out = Vec::new();
    if o.replace {
        for (i, s) in new_shapes {
            doc.shapes[i] = s;
            out.push(i);
        }
    } else {
        for (i, s) in new_shapes {
            // remember where it came from, so the dimension tool can show and change the offset distance
            doc.offsets.push(crate::relations::OffsetLink { result: s.id, source: doc.shapes[i].id, opts: *o });
            doc.shapes.push(s);
            out.push(doc.shapes.len() - 1);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sq(x: f64, y: f64, s: f64) -> Shape {
        Shape::new(ShapeKind::Rect { w: s, h: s, cr: 0.0 }, Xform::translate(x, y), 0)
    }
    #[test]
    fn union_two_squares() {
        let mut d = Document::default();
        d.shapes.push(sq(0.0, 0.0, 10.0));
        d.shapes.push(sq(5.0, 0.0, 10.0));
        let i = boolean(&mut d, &[0, 1], BoolOp::Union).unwrap();
        let b = d.shapes[i].bbox();
        assert!((b.width() - 15.0).abs() < 1e-6);
        assert_eq!(d.shapes.len(), 1);
    }
    #[test]
    fn offset_grows() {
        let mut d = Document::default();
        d.shapes.push(sq(0.0, 0.0, 10.0));
        let o = offset(&mut d, &[0], &OffsetOpts { dist: 1.0, simplify: 0.0, ..Default::default() }).unwrap();
        let b = d.shapes[o[0]].bbox();
        assert!((b.width() - 12.0).abs() < 0.05);
    }
    #[test]
    fn group_ungroup() {
        let mut d = Document::default();
        d.shapes.push(sq(0.0, 0.0, 10.0));
        d.shapes.push(sq(20.0, 0.0, 10.0));
        let g = group(&mut d, &[0, 1]).unwrap();
        assert_eq!(d.shapes.len(), 1);
        let s = ungroup(&mut d, &[g]);
        assert_eq!(s.len(), 2);
        assert_eq!(d.shapes.len(), 2);
    }
}

/// True when two of the selected closed shapes overlap — LightBurn then warns
/// "Intersecting Shapes Detected": offsets are applied per shape.
pub fn selection_overlaps(doc: &Document, sel: &[usize]) -> bool {
    let polys: Vec<(BBox, MultiPolygon<f64>)> = sel
        .iter()
        .filter_map(|i| doc.shapes.get(*i))
        .filter(|s| !s.is_bitmap())
        .map(|s| (s.bbox(), to_multipolygon(&s.world_outline(0.05))))
        .collect();
    for i in 0..polys.len() {
        for j in i + 1..polys.len() {
            if polys[i].0.intersects(&polys[j].0) && !polys[i].1.intersection(&polys[j].1).0.is_empty() {
                return true;
            }
        }
    }
    false
}
