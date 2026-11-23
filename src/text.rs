//! Text to vector outlines, using an installed system font (see `fonts.rs`) or the built-in font.

use crate::geom::*;
use crate::model::TextStyle;
use ab_glyph::{Font, FontArc, OutlineCurve};
use std::sync::OnceLock;

fn default_font() -> Option<&'static FontArc> {
    static FONT: OnceLock<Option<FontArc>> = OnceLock::new();
    FONT.get_or_init(|| {
        let defs = egui::FontDefinitions::default();
        let name = defs.families.get(&egui::FontFamily::Proportional).and_then(|v| v.first().cloned())?;
        let data = defs.font_data.get(&name)?;
        FontArc::try_from_vec((*data.font).as_ref().to_vec()).ok()
    })
    .as_ref()
}

/// Outlines for `text` with cap-ish height `height` mm, baseline at y=0, centred horizontally.
/// Y axis points up (workspace convention). `font_spec` is a Qt-style font description.
pub fn text_outline(text: &str, font_spec: &str, height: f64, style: &TextStyle, tol: f64) -> Vec<Vec<P2>> {
    let spec = crate::fonts::FontSpec::parse(font_spec);
    let chosen = crate::fonts::load_face(&spec);
    // bold / italic the font has no face for are simulated: outlines grown a little, or slanted
    let (fake_bold, fake_italic) = chosen.as_ref().map(|l| (l.fake_bold, l.fake_italic)).unwrap_or((spec.bold, spec.italic));
    let font: &FontArc = match chosen.as_ref().map(|l| &l.font) {
        Some(f) => f,
        None => match default_font() {
            Some(f) => f,
            None => return Vec::new(),
        },
    };
    let text = style.case.apply(text);
    let units = font.height_unscaled() as f64;
    let scale = if units > 0.0 { height * 1.4 / units } else { 1.0 };
    let spacing_units = style.letter_spacing / scale.max(1e-9);
    let em = font.units_per_em().map(|e| e as f64).unwrap_or(units);
    let bold_grow = if fake_bold { em * BOLD_GROW } else { 0.0 };
    let mut lines_out: Vec<Vec<P2>> = Vec::new();
    let line_h = units * 1.15;
    // on a curve the text is one line
    let text = if style.path.is_some() { text.lines().collect::<Vec<_>>().join(" ") } else { text };
    for (li, line) in text.lines().enumerate() {
        // where each glyph's middle is along the line (font units, from the line's start)
        let mut centres: Vec<f64> = Vec::new();
        let mut pen = 0.0f64;
        // one entry per glyph so welding can merge them
        let mut glyphs: Vec<Vec<Vec<P2>>> = Vec::new();
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for ch in line.chars() {
            let gid = font.glyph_id(ch);
            if let Some(p) = prev {
                pen += font.kern_unscaled(p, gid) as f64;
            }
            let mut glyph_lines: Vec<Vec<P2>> = Vec::new();
            if let Some(outline) = font.outline(gid) {
                let mut cur: Vec<P2> = Vec::new();
                let mut last: Option<P2> = None;
                for c in &outline.curves {
                    let (start, end) = match c {
                        OutlineCurve::Line(a, b) => (pt(*a), pt(*b)),
                        OutlineCurve::Quad(a, _, b) => (pt(*a), pt(*b)),
                        OutlineCurve::Cubic(a, _, _, b) => (pt(*a), pt(*b)),
                    };
                    if last.map(|l| l.dist(start) > 1e-3).unwrap_or(true) {
                        if cur.len() > 1 {
                            glyph_lines.push(offset(&cur, pen));
                        }
                        cur = vec![start];
                    }
                    match c {
                        OutlineCurve::Line(_, _) => cur.push(end),
                        OutlineCurve::Quad(_, q, _) => flatten_quad(start, pt(*q), end, tol / scale.max(1e-9), &mut cur),
                        OutlineCurve::Cubic(_, q1, q2, _) => flatten_cubic(start, pt(*q1), pt(*q2), end, tol / scale.max(1e-9), &mut cur),
                    }
                    last = Some(end);
                }
                if cur.len() > 1 {
                    glyph_lines.push(offset(&cur, pen));
                }
            }
            if fake_italic {
                for l in &mut glyph_lines {
                    for p in l.iter_mut() {
                        p.x += ITALIC_SLANT * p.y;
                    }
                }
            }
            if bold_grow > 0.0 && !glyph_lines.is_empty() {
                let closed: Vec<Vec<P2>> = glyph_lines.into_iter().map(close).collect();
                glyph_lines = crate::ops::grow(&closed, bold_grow);
                // shift right so the letter keeps its left edge, and widen its advance to match
                for l in &mut glyph_lines {
                    for p in l.iter_mut() {
                        p.x += bold_grow;
                    }
                }
            }
            glyphs.push(glyph_lines);
            let adv = font.h_advance_unscaled(gid) as f64 + 2.0 * bold_grow;
            centres.push(pen + adv / 2.0);
            pen += adv + spacing_units;
            prev = Some(gid);
        }
        if !glyphs.is_empty() {
            pen -= spacing_units;
        }
        let half = pen / 2.0;
        let dy = -(li as f64) * line_h;
        let place = |l: Vec<P2>| -> Vec<P2> { l.into_iter().map(|p| p2((p.x - half) * scale, (p.y + dy) * scale)).collect() };
        if let Some(tp) = &style.path {
            // each letter keeps its shape and is turned to follow the curve at its middle
            let cap = height;
            let placed: Vec<Vec<Vec<P2>>> = glyphs
                .into_iter()
                .zip(&centres)
                .map(|(g, c)| {
                    let mid = (c - half) * scale;
                    g.into_iter().map(close).map(place).map(|l| l.into_iter().map(|p| along(tp, mid, p2(p.x - mid, p.y), cap)).collect::<Vec<P2>>()).collect()
                })
                .collect();
            // letters that crowd together on a tight curve are merged, when weld is on
            if style.weld {
                lines_out.extend(weld(placed));
            } else {
                lines_out.extend(placed.into_iter().flatten());
            }
            continue;
        }
        if style.weld {
            lines_out.extend(weld(glyphs.into_iter().map(|g| g.into_iter().map(close).map(place).collect()).collect()));
        } else {
            for g in glyphs {
                lines_out.extend(g.into_iter().map(place));
            }
        }
    }
    lines_out
}

/// Where a point of a letter goes on a text path: `s` is the letter's middle along the text (mm from the text's
/// middle), `local` the point relative to it (x along, y up from the baseline), `cap` the text height.
pub fn along(tp: &crate::model::TextPath, s: f64, local: P2, cap: f64) -> P2 {
    let s = s + tp.offset;
    if tp.curve.len() < 2 {
        // a circle around the text's position: over the top clockwise, or inside along the bottom
        let r = tp.radius.max(0.1);
        let (inside, flip) = (tp.inside, tp.flip);
        // the angle where the letter's middle sits, and the direction the text runs
        let (a, dir) = match (inside, flip) {
            (false, false) => (std::f64::consts::FRAC_PI_2 - s / r, -1.0),
            (true, false) => (-std::f64::consts::FRAC_PI_2 + s / r, 1.0),
            (false, true) => (-std::f64::consts::FRAC_PI_2 + s / r, 1.0),
            (true, true) => (std::f64::consts::FRAC_PI_2 - s / r, -1.0),
        };
        let base = p2(r * a.cos(), r * a.sin());
        // the text's direction along the circle, and "up" for the letters (to its left)
        let t = p2(-a.sin() * dir, a.cos() * dir);
        let up = p2(-t.y, t.x);
        return base + t * local.x + up * local.y;
    }
    // a curve: measured from its middle; letters on its left (the top, going left to right), or hanging below
    let pts: Vec<P2> = if tp.flip { tp.curve.iter().rev().copied().collect() } else { tp.curve.clone() };
    let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let target = total / 2.0 + s;
    // the direction at the start: of the first piece that has a length
    let t0 = pts.windows(2).map(|w| w[1] - w[0]).find(|d| d.len() > 1e-12).map(|d| d.normalized()).unwrap_or(p2(1.0, 0.0));
    let (mut run, mut base, mut t) = (0.0, pts[0], t0);
    if target <= 0.0 {
        base = pts[0] + t * target;
    } else {
        let mut found = false;
        for w in pts.windows(2) {
            let d = w[0].dist(w[1]);
            if d > 0.0 && run + d >= target {
                t = (w[1] - w[0]).normalized();
                base = w[0] + t * (target - run);
                found = true;
                break;
            }
            run += d;
            if d > 0.0 {
                t = (w[1] - w[0]).normalized();
            }
        }
        if !found {
            base = *pts.last().unwrap() + t * (target - total);
        }
    }
    let up = p2(-t.y, t.x);
    let y = if tp.inside { local.y - cap } else { local.y };
    base + t * local.x + up * y
}

/// Simulated bold: outlines grown by this much of the em size on each side.
pub const BOLD_GROW: f64 = 0.025;
/// Simulated italic: x moves right by this times the height above the baseline.
pub const ITALIC_SLANT: f64 = 0.2;

fn close(mut l: Vec<P2>) -> Vec<P2> {
    if !is_closed(&l) && l.len() > 2 {
        l.push(l[0]);
    }
    l
}

/// Union every glyph's outline so overlapping letters (script fonts, tight spacing) become one shape.
fn weld(glyphs: Vec<Vec<Vec<P2>>>) -> Vec<Vec<P2>> {
    use geo::BooleanOps;
    let mut acc: Option<geo::MultiPolygon<f64>> = None;
    for g in glyphs {
        let mp = crate::ops::to_multipolygon(&g);
        if mp.0.is_empty() {
            continue;
        }
        acc = Some(match acc {
            Some(a) => a.union(&mp),
            None => mp,
        });
    }
    acc.map(|a| crate::ops::from_multipolygon(&a)).unwrap_or_default()
}

fn pt(p: ab_glyph::Point) -> P2 {
    p2(p.x as f64, p.y as f64)
}

fn offset(l: &[P2], dx: f64) -> Vec<P2> {
    l.iter().map(|p| p2(p.x + dx, p.y)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(lines: &[Vec<P2>]) -> BBox {
        let mut b = BBox::EMPTY;
        for p in lines.iter().flatten() {
            b.add(*p);
        }
        b
    }

    #[test]
    fn bold_and_italic_are_simulated_when_the_font_has_no_such_face() {
        // the built-in font has one face only, so both are simulated
        let st = TextStyle::default();
        let plain = text_outline("Il", ",-1,100,5,50,0,0,0,0,0", 10.0, &st, 0.05);
        let bold = text_outline("Il", ",-1,100,5,75,0,0,0,0,0", 10.0, &st, 0.05);
        let italic = text_outline("Il", ",-1,100,5,50,1,0,0,0,0", 10.0, &st, 0.05);
        assert!(!plain.is_empty());
        let (p, b, i) = (bounds(&plain), bounds(&bold), bounds(&italic));
        // bold: thicker strokes, so wider and a little taller
        assert!(b.width() > p.width() + 0.2, "bold {} vs {}", b.width(), p.width());
        assert!(b.height() > p.height());
        // italic: same height, leaning right (wider, top moved right)
        assert!((i.height() - p.height()).abs() < 1e-6);
        assert!(i.width() > p.width() + 0.5, "italic {} vs {}", i.width(), p.width());
        assert_eq!(crate::fonts::simulated(&crate::fonts::FontSpec::parse(",-1,100,5,75,1,0,0,0,0")), (true, true));
    }

    #[test]
    fn text_follows_a_circle_or_a_curve() {
        use crate::model::TextPath;
        let font = crate::fonts::DEFAULT_FONT;
        let style = |tp: TextPath| TextStyle { path: Some(tp), ..Default::default() };
        let dist = |lines: &[Vec<P2>]| lines.iter().flatten().map(|p| p.len()).fold((f64::INFINITY, 0.0f64), |(lo, hi), d| (lo.min(d), hi.max(d)));
        // outside a 30 mm circle: every point between the circle and a letter's height (plus descenders) out
        let out = text_outline("HELLO", font, 5.0, &style(TextPath { radius: 30.0, ..Default::default() }), 0.05);
        assert!(!out.is_empty());
        let (lo, hi) = dist(&out);
        assert!(lo > 29.5 && hi < 37.5, "{lo} {hi}");
        // over the top
        assert!(out.iter().flatten().all(|p| p.y > 0.0));
        // inside: along the bottom, within the circle
        let inside = text_outline("HELLO", font, 5.0, &style(TextPath { radius: 30.0, inside: true, ..Default::default() }), 0.05);
        let (lo, hi) = dist(&inside);
        assert!(lo > 22.5 && hi < 30.5, "{lo} {hi}");
        assert!(inside.iter().flatten().all(|p| p.y < 0.0));
        // a straight curve gives the same as straight text
        let flat = text_outline("Hi", font, 5.0, &TextStyle::default(), 0.05);
        let on_line = text_outline("Hi", font, 5.0, &style(TextPath { curve: vec![p2(-50.0, 0.0), p2(50.0, 0.0)], ..Default::default() }), 0.05);
        let b = |l: &[Vec<P2>]| l.iter().flatten().fold(BBox::EMPTY, |mut b, p| {
            b.add(*p);
            b
        });
        let (a, c) = (b(&flat), b(&on_line));
        assert!(a.min.dist(c.min) < 1e-6 && a.max.dist(c.max) < 1e-6, "{a:?} {c:?}");
        // shifted along it
        let moved = text_outline("Hi", font, 5.0, &style(TextPath { curve: vec![p2(-50.0, 0.0), p2(50.0, 0.0)], offset: 10.0, ..Default::default() }), 0.05);
        assert!((b(&moved).min.x - (a.min.x + 10.0)).abs() < 1e-6);
    }

    #[test]
    fn text_paths_are_saved_in_lightburn_files() {
        use crate::model::*;
        let mut doc = Document::default();
        let style = TextStyle { path: Some(TextPath { curve: vec![p2(0.0, 0.0), p2(10.0, 5.0), p2(20.0, 0.0)], offset: 2.5, flip: true, ..Default::default() }), ..Default::default() };
        doc.shapes.push(Shape::new(ShapeKind::Text { text: "Arc".into(), font: crate::fonts::DEFAULT_FONT.into(), height: 5.0, style: style.clone(), backup: None }, Xform::translate(30.0, 30.0), 0));
        doc.sync_cuts();
        let l = crate::lbrn::load_str(&crate::lbrn::save_string(&doc, &[])).unwrap();
        let ShapeKind::Text { style: back, .. } = &l.doc.shapes[0].kind else { unreachable!() };
        assert_eq!(back.path, style.path);
    }
}
