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
    let chosen = crate::fonts::load(&crate::fonts::FontSpec::parse(font_spec));
    let font: &FontArc = match chosen.as_ref() {
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
    let mut lines_out: Vec<Vec<P2>> = Vec::new();
    let line_h = units * 1.15;
    for (li, line) in text.lines().enumerate() {
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
            glyphs.push(glyph_lines);
            pen += font.h_advance_unscaled(gid) as f64 + spacing_units;
            prev = Some(gid);
        }
        if !glyphs.is_empty() {
            pen -= spacing_units;
        }
        let half = pen / 2.0;
        let dy = -(li as f64) * line_h;
        let place = |l: Vec<P2>| -> Vec<P2> { l.into_iter().map(|p| p2((p.x - half) * scale, (p.y + dy) * scale)).collect() };
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
