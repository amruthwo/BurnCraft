//! Cut-path adjustments made while building a job: kerf offset, tabs (bridges) and overscan.
//! All distances are in mm on the bed.

use crate::geom::*;
use crate::model::Tabs;

/// Outlines with this many closed lines or more skip the inside/outside test (it compares every pair).
const MAX_NESTING_TEST: usize = 4000;

/// How deep each line is nested inside the other closed lines (0 = outermost; odd = a hole). Open lines and
/// very large layers count as 0.
pub fn nesting_depth(lines: &[Vec<P2>]) -> Vec<usize> {
    let closed: Vec<bool> = lines.iter().map(|l| is_closed(l) && l.len() > 3).collect();
    if lines.len() >= MAX_NESTING_TEST {
        return vec![0; lines.len()];
    }
    let boxes: Vec<BBox> = lines.iter().map(|l| BBox::from_points(l)).collect();
    (0..lines.len())
        .map(|i| {
            if !closed[i] {
                return 0;
            }
            (0..lines.len()).filter(|&j| j != i && closed[j] && boxes[j].contains_box(&boxes[i]) && boxes[j] != boxes[i] && point_in_rings(lines[i][0], std::slice::from_ref(&lines[j]))).count()
        })
        .collect()
}

/// Apply a kerf offset to a layer's lines: closed outlines grow by `kerf` (outer edges move out, holes move
/// in), so the finished part keeps its drawn size. A negative kerf does the opposite (for inlays). Open
/// lines are left as they are.
pub fn apply_kerf(lines: Vec<Vec<P2>>, kerf: f64) -> Vec<Vec<P2>> {
    if kerf.abs() < 1e-6 {
        return lines;
    }
    let (closed, open): (Vec<Vec<P2>>, Vec<Vec<P2>>) = lines.into_iter().partition(|l| is_closed(l) && l.len() > 3);
    if closed.is_empty() {
        return open;
    }
    let mut out = crate::ops::kerf_offset(&closed, kerf);
    out.extend(open);
    out
}

/// Total length of a polyline.
pub fn length(l: &[P2]) -> f64 {
    l.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Where along a polyline (as a distance from its start) the point nearest to `p` is, and how far away it is.
pub fn project(l: &[P2], p: P2) -> (f64, f64) {
    let (mut best_s, mut best_d, mut run) = (0.0, f64::INFINITY, 0.0);
    for w in l.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len2 = ab.x * ab.x + ab.y * ab.y;
        let t = if len2 > 0.0 { (((p - a).x * ab.x + (p - a).y * ab.y) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + ab * t;
        let d = q.dist(p);
        if d < best_d {
            (best_d, best_s) = (d, run + t * len2.sqrt());
        }
        run += len2.sqrt();
    }
    (best_s, best_d)
}

/// The point `s` mm along a polyline.
fn point_at(l: &[P2], s: f64) -> P2 {
    let mut run = 0.0;
    for w in l.windows(2) {
        let d = w[0].dist(w[1]);
        if run + d >= s && d > 0.0 {
            return w[0] + (w[1] - w[0]) * ((s - run) / d);
        }
        run += d;
    }
    *l.last().unwrap_or(&P2::default())
}

/// The piece of a polyline from `s0` to `s1` mm along it (s0 ≤ s1 ≤ its length).
fn piece(l: &[P2], s0: f64, s1: f64) -> Vec<P2> {
    let mut out = vec![point_at(l, s0)];
    let mut run = 0.0;
    for w in l.windows(2) {
        run += w[0].dist(w[1]);
        if run > s0 + 1e-9 && run < s1 - 1e-9 {
            out.push(w[1]);
        }
    }
    out.push(point_at(l, s1));
    out
}

/// Where the automatic tabs go on a closed outline of length `len` (distances from its start), or none when
/// the outline is too short to hold them.
pub fn auto_tab_positions(len: f64, t: &Tabs) -> Vec<f64> {
    let width = t.width.max(0.0);
    let n = if t.by_count { t.count as usize } else if t.spacing > 0.0 { (len / t.spacing).floor().max(1.0) as usize } else { 0 };
    let n = n.min(1000);
    // keep at least as much cut as tab, so small shapes don't fall apart into nothing
    if n == 0 || width <= 0.0 || len < 2.0 * width * n as f64 {
        return Vec::new();
    }
    (0..n).map(|i| (i as f64 + 0.5) * len / n as f64).collect()
}

/// Cut a closed outline into the pieces that are burned, leaving gaps of `width` mm centred on each tab
/// position. The pieces are in order around the outline, starting just after a tab. Returns the outline
/// whole when there are no tabs.
pub fn split_at_tabs(l: &[P2], tabs: &[f64], width: f64) -> Vec<Vec<P2>> {
    let len = length(l);
    if tabs.is_empty() || width <= 0.0 || len <= 0.0 {
        return vec![l.to_vec()];
    }
    // gaps as (start, end) along the outline, unrolled so none wraps past the end
    let mut gaps: Vec<(f64, f64)> = tabs.iter().map(|t| (t.rem_euclid(len) - width / 2.0, t.rem_euclid(len) + width / 2.0)).collect();
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    // start the walk at the end of the first gap; everything is measured from there, around once
    let start = gaps[0].1;
    let mut cuts: Vec<(f64, f64)> = Vec::new();
    let mut cursor = start;
    for g in gaps.iter().skip(1).chain(std::iter::once(&(gaps[0].0 + len, gaps[0].1 + len))) {
        if g.0 > cursor + 1e-6 {
            cuts.push((cursor, g.0));
        }
        cursor = cursor.max(g.1);
    }
    // the outline twice over, so pieces that run past its end are one polyline
    let mut twice = l.to_vec();
    twice.extend_from_slice(&l[1..]);
    let base = start.div_euclid(len) * len;
    cuts.into_iter().map(|(a, b)| piece(&twice, a - base, b - base)).filter(|p| p.len() >= 2 && length(p) > 1e-4).collect()
}

/// How far a fill or image line runs on with the laser off at each end (overscan), so the head is at full
/// speed while it burns. LightBurn's rule: the overscan percentage of the speed, as mm (2.5% at 100 mm/s =
/// 2.5 mm).
pub fn overscan_distance(speed: f64, percent: f64) -> f64 {
    (speed * percent / 100.0).clamp(0.0, 100.0)
}

/// How far from `p` along the unit direction `dir` the head can go, up to `want`, without leaving `bed`.
pub fn room(p: P2, dir: P2, want: f64, bed: Option<BBox>) -> f64 {
    let Some(b) = bed else { return want };
    let mut t = want;
    for (pc, dc, lo, hi) in [(p.x, dir.x, b.min.x, b.max.x), (p.y, dir.y, b.min.y, b.max.y)] {
        if dc > 1e-12 {
            t = t.min((hi - pc) / dc);
        } else if dc < -1e-12 {
            t = t.min((lo - pc) / dc);
        }
    }
    t.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<P2> {
        vec![p2(x, y), p2(x + s, y), p2(x + s, y + s), p2(x, y + s), p2(x, y)]
    }

    #[test]
    fn kerf_grows_parts_and_shrinks_holes() {
        // a 20 mm square with a 10 mm square hole
        let out = apply_kerf(vec![square(0.0, 0.0, 20.0), square(5.0, 5.0, 10.0), vec![p2(30.0, 0.0), p2(40.0, 0.0)]], 0.1);
        assert_eq!(out.len(), 3);
        let boxes: Vec<BBox> = out.iter().map(|l| BBox::from_points(l)).collect();
        let outer = boxes.iter().find(|b| b.width() > 15.0 && b.width() < 25.0).unwrap();
        let hole = boxes.iter().find(|b| b.width() < 15.0 && b.width() > 5.0).unwrap();
        assert!((outer.width() - 20.2).abs() < 1e-3, "outer {}", outer.width());
        assert!((hole.width() - 9.8).abs() < 1e-3, "hole {}", hole.width());
        assert!(out.iter().any(|l| l.len() == 2 && l[0] == p2(30.0, 0.0)), "open lines unchanged");
        // a negative kerf does the opposite
        let out = apply_kerf(vec![square(0.0, 0.0, 20.0)], -0.1);
        assert!((BBox::from_points(&out[0]).width() - 19.8).abs() < 1e-3);
    }

    #[test]
    fn tabs_leave_gaps_of_the_right_width() {
        let sq = square(0.0, 0.0, 10.0); // 40 mm around
        let pos = auto_tab_positions(40.0, &Tabs { on: true, by_count: true, count: 4, width: 1.0, ..Default::default() });
        assert_eq!(pos, vec![5.0, 15.0, 25.0, 35.0], "the middle of each side");
        let pieces = split_at_tabs(&sq, &pos, 1.0);
        assert_eq!(pieces.len(), 4);
        let cut: f64 = pieces.iter().map(|p| length(p)).sum();
        assert!((cut - 36.0).abs() < 1e-9, "4 mm of tabs left uncut: {cut}");
        // each piece goes round a corner, from just after one tab to just before the next
        assert!(pieces[0][0].dist(p2(5.5, 0.0)) < 1e-9 && pieces[0].last().unwrap().dist(p2(10.0, 4.5)) < 1e-9, "{:?}", pieces[0]);
        // the last piece runs over the outline's start point and comes out as one line
        assert!(pieces[3][0].dist(p2(0.0, 4.5)) < 1e-9 && pieces[3].last().unwrap().dist(p2(4.5, 0.0)) < 1e-9, "{:?}", pieces[3]);
        // spacing: one per 15 mm → 2 tabs on 40 mm
        assert_eq!(auto_tab_positions(40.0, &Tabs { by_count: false, spacing: 15.0, width: 1.0, ..Default::default() }).len(), 2);
        // too small for its tabs: none
        assert!(auto_tab_positions(6.0, &Tabs { count: 4, width: 1.0, ..Default::default() }).is_empty());
        // a tab at the very start of the outline wraps round
        let pieces = split_at_tabs(&sq, &[0.0], 2.0);
        assert_eq!(pieces.len(), 1);
        assert!((length(&pieces[0]) - 38.0).abs() < 1e-9);
        assert!(pieces[0][0].dist(p2(1.0, 0.0)) < 1e-9 && pieces[0].last().unwrap().dist(p2(0.0, 1.0)) < 1e-9);
        // overlapping tabs make one gap
        assert_eq!(split_at_tabs(&sq, &[10.0, 10.5], 1.0).len(), 1);
    }

    #[test]
    fn nesting_and_projection() {
        let d = nesting_depth(&[square(0.0, 0.0, 20.0), square(5.0, 5.0, 10.0), square(7.0, 7.0, 2.0), square(30.0, 0.0, 5.0)]);
        assert_eq!(d, vec![0, 1, 2, 0]);
        let (s, dist) = project(&square(0.0, 0.0, 10.0), p2(10.5, 3.0));
        assert!((s - 13.0).abs() < 1e-9 && (dist - 0.5).abs() < 1e-9);
    }

    #[test]
    fn overscan_stays_on_the_bed() {
        assert!((overscan_distance(100.0, 2.5) - 2.5).abs() < 1e-9);
        let bed = Some(BBox { min: p2(0.0, 0.0), max: p2(100.0, 100.0) });
        assert!((room(p2(1.0, 50.0), p2(-1.0, 0.0), 2.5, bed) - 1.0).abs() < 1e-9);
        assert!((room(p2(50.0, 50.0), p2(1.0, 0.0), 2.5, bed) - 2.5).abs() < 1e-9);
        assert_eq!(room(p2(1.0, 50.0), p2(-1.0, 0.0), 2.5, None), 2.5);
    }

    #[test]
    fn tabs_are_saved_in_both_project_formats() {
        use crate::model::*;
        let mut doc = Document::default();
        let mut s = Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 0);
        s.tabs = vec![p2(0.0, 5.0), p2(-5.0, 1.25)];
        doc.shapes.push(s);
        doc.sync_cuts();
        doc.cut_mut(0).tabs = Tabs { on: true, by_count: false, count: 3, spacing: 25.0, width: 0.8, skip_inner: false };
        doc.cut_mut(0).kerf = 0.08;
        let l = crate::lbrn::load_str(&crate::lbrn::save_string(&doc, &[])).unwrap();
        assert_eq!(l.doc.shapes[0].tabs, doc.shapes[0].tabs);
        assert_eq!(l.doc.cut(0).unwrap().tabs, doc.cut(0).unwrap().tabs);
        assert_eq!(l.doc.cut(0).unwrap().kerf, 0.08);
        assert!(l.doc.cut(0).unwrap().extra.is_empty(), "nothing left over as an unknown key");
        let b = crate::burn::load(&crate::burn::save(&doc, &[]).unwrap()).unwrap();
        assert_eq!(b.doc.shapes[0].tabs, doc.shapes[0].tabs);
        assert_eq!(b.doc.cut(0).unwrap().tabs, doc.cut(0).unwrap().tabs);
        // default tab settings aren't written into LightBurn files
        let plain = Document { cuts: vec![CutSetting::new(0)], ..Default::default() };
        assert!(!crate::lbrn::save_string(&plain, &[]).contains("BcTabs"));
    }

    #[test]
    fn picture_modes_are_saved_and_layer_dither_is_read() {
        use crate::model::*;
        let png = crate::platform::png_bytes(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
        let data = crate::lbrn::decode_bitmap(png, "").unwrap();
        let mut doc = Document::default();
        let adjust = ImageAdjust { dither: Dither::Halftone, halftone_lpi: 25.0, halftone_angle: 30.0, invert: true, ..Default::default() };
        doc.shapes.push(Shape::new(ShapeKind::Bitmap { w: 20.0, h: 10.0, data, adjust }, Xform::translate(20.0, 20.0), 3));
        doc.sync_cuts();
        doc.cut_mut(3).mode = CutMode::Image;
        let text = crate::lbrn::save_string(&doc, &[]);
        assert!(text.contains("<ditherMode Value=\"halftone\""), "LightBurn is told the layer's mode");
        let back = |text: &str| match crate::lbrn::load_str(text).unwrap().doc.shapes[0].kind.clone() {
            ShapeKind::Bitmap { adjust, .. } => adjust,
            _ => unreachable!(),
        };
        let a = back(&text);
        assert_eq!((a.dither, a.halftone_lpi, a.halftone_angle, a.invert), (Dither::Halftone, 25.0, 30.0, true));
        // a LightBurn file: no BurnCraft setting on the picture, the layer says stucki
        let lb = text.replace(" BcDither=\"Halftone\"", "").replace("Value=\"halftone\"", "Value=\"stucki\"");
        assert!(!lb.contains("BcDither"));
        assert_eq!(back(&lb).dither, Dither::Stucki);
        // .burn keeps everything too
        let mut d2 = doc.clone();
        if let ShapeKind::Bitmap { adjust, .. } = &mut d2.shapes[0].kind {
            (adjust.dither, adjust.slices) = (Dither::Slice3D, 12);
        }
        let b = crate::burn::load(&crate::burn::save(&d2, &[]).unwrap()).unwrap();
        let ShapeKind::Bitmap { adjust, .. } = &b.doc.shapes[0].kind else { unreachable!() };
        assert_eq!((adjust.dither, adjust.slices), (Dither::Slice3D, 12));
    }
}
