//! DXF in and out (written for BurnCraft from the published DXF reference).
//!
//! Import reads text DXF files: LINE, LWPOLYLINE and POLYLINE (with arc "bulges"), ARC, CIRCLE, ELLIPSE,
//! SPLINE (drawn finely as lines) and block references (INSERT, nested, with their placement), with the units
//! from the header and colours put on the nearest layer. Pieces whose ends meet are joined into closed
//! outlines. Text and dimensions are left out with a note, and nothing a drawing points to (other drawings,
//! pictures) is ever read. Export writes a plain DXF (R12) in millimetres with each layer as a DXF layer.

use crate::geom::*;
use crate::model::*;
use crate::vecio::{self, Seg};
use std::collections::HashMap;

/// Entity kinds that are text, and are left out (they'd need fonts).
const TEXTISH: &[&str] = &["TEXT", "MTEXT", "DIMENSION", "ATTRIB", "ATTDEF", "LEADER", "MLEADER", "MULTILEADER", "TOLERANCE", "ARCALIGNEDTEXT", "RTEXT"];

/// One entity: its kind and its (code, value) pairs in order.
#[derive(Clone, Debug)]
struct Ent {
    kind: String,
    codes: Vec<(i32, String)>,
}

impl Ent {
    fn s(&self, code: i32) -> Option<&str> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
    fn f(&self, code: i32) -> Option<f64> {
        self.s(code).and_then(|v| v.trim().parse().ok())
    }
    fn fd(&self, code: i32, d: f64) -> f64 {
        self.f(code).unwrap_or(d)
    }
    fn i(&self, code: i32) -> Option<i64> {
        self.s(code).and_then(|v| v.trim().parse().ok())
    }
    fn all(&self, code: i32) -> Vec<f64> {
        self.codes.iter().filter(|(c, _)| *c == code).filter_map(|(_, v)| v.trim().parse().ok()).collect()
    }
    fn p(&self, x: i32) -> P2 {
        p2(self.fd(x, 0.0), self.fd(x + 10, 0.0))
    }
    /// Drawn with its plane facing down (extrusion −Z): its own x runs the other way.
    fn mirrored(&self) -> bool {
        self.f(230).is_some_and(|z| z < 0.0)
    }
}

struct Block {
    base: P2,
    ents: Vec<Ent>,
}

struct Layer {
    rgb: [u8; 3],
}

/// What to say about things left out.
#[derive(Default)]
struct Skipped {
    text: usize,
    other: HashMap<String, usize>,
}

fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut lines = text.lines();
    let mut out = Vec::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        match c.trim().parse::<i32>() {
            Ok(code) => out.push((code, v.trim_end_matches('\r').trim().to_string())),
            Err(_) => break,
        }
    }
    out
}

/// Millimetres per drawing unit, from `$INSUNITS` (None when the file doesn't say).
fn unit_mm(code: i64) -> Option<f64> {
    Some(match code {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        8 => 25.4e-6,
        9 => 0.0254,
        10 => 914.4,
        11 => 1e-7,
        12 => 1e-6,
        13 => 1e-3,
        14 => 100.0,
        _ => return None,
    })
}

/// Read a DXF drawing. Coordinates come out in mm, where the drawing put them.
pub fn import(data: &[u8]) -> Result<crate::svg::Imported, String> {
    if data.starts_with(b"AutoCAD Binary DXF") {
        return Err("This is a binary DXF. Save it as a text (ASCII) DXF in your CAD program, then import that.".into());
    }
    let text = String::from_utf8_lossy(data);
    let pairs = pairs(&text);
    if !pairs.iter().any(|(c, v)| *c == 0 && v == "SECTION") {
        return Err("This doesn't look like a DXF drawing.".into());
    }
    // sections
    let mut units: Option<f64> = None;
    let mut unit_said = false;
    let mut layers: HashMap<String, Layer> = HashMap::new();
    let mut blocks: HashMap<String, Block> = HashMap::new();
    let mut ents: Vec<Ent> = Vec::new();
    let mut section = String::new();
    let mut i = 0;
    let mut block: Option<(String, Block)> = None;
    while i < pairs.len() {
        let (code, v) = (&pairs[i].0, &pairs[i].1);
        if *code == 0 && v == "SECTION" {
            section = pairs.get(i + 1).map(|p| p.1.clone()).unwrap_or_default();
            i += 2;
            continue;
        }
        if *code == 0 && v == "ENDSEC" {
            section.clear();
            i += 1;
            continue;
        }
        if section == "HEADER" {
            if *code == 9 && v == "$INSUNITS" {
                if let Some(n) = pairs.get(i + 1).and_then(|p| p.1.trim().parse::<i64>().ok()) {
                    units = unit_mm(n);
                    unit_said = units.is_some();
                }
            }
            i += 1;
            continue;
        }
        if *code != 0 {
            i += 1;
            continue;
        }
        // an entity (or table entry): collect its codes up to the next 0
        let mut e = Ent { kind: v.clone(), codes: Vec::new() };
        i += 1;
        while i < pairs.len() && pairs[i].0 != 0 {
            e.codes.push(pairs[i].clone());
            i += 1;
        }
        match section.as_str() {
            "TABLES" if e.kind == "LAYER" => {
                let rgb = match e.i(420) {
                    Some(t) => [(t >> 16) as u8, (t >> 8) as u8, t as u8],
                    None => vecio::aci_rgb(e.i(62).unwrap_or(7).unsigned_abs().min(256) as u16),
                };
                layers.insert(e.s(2).unwrap_or("0").to_string(), Layer { rgb });
            }
            "BLOCKS" => match e.kind.as_str() {
                "BLOCK" => {
                    // external references (flag 4) point at other files: never read
                    let xref = e.i(70).is_some_and(|f| f & 4 != 0);
                    let name = e.s(2).unwrap_or("").to_string();
                    block = Some((if xref { String::new() } else { name }, Block { base: e.p(10), ents: Vec::new() }));
                }
                "ENDBLK" => {
                    if let Some((name, b)) = block.take() {
                        if !name.is_empty() {
                            blocks.insert(name, b);
                        }
                    }
                }
                _ => {
                    if let Some((_, b)) = block.as_mut() {
                        b.ents.push(e);
                    }
                }
            },
            "ENTITIES" => ents.push(e),
            _ => {}
        }
    }
    let k = units.unwrap_or(1.0);
    let mut groups: HashMap<([u8; 3], Option<usize>), Vec<Seg>> = HashMap::new();
    let mut skipped = Skipped::default();
    let ctx = Ctx { layers: &layers, blocks: &blocks, left: std::cell::Cell::new(MAX_DRAWN) };
    ctx.draw(&ents, Xform::scale(k, k), None, None, 0, &mut groups, &mut skipped);
    let too_many = ctx.left.get() == 0;
    let mut out = crate::svg::Imported { paths: Vec::new(), images: Vec::new(), skipped_text: skipped.text, on_layers: Vec::new(), notes: Vec::new() };
    let mut keys: Vec<_> = groups.keys().copied().collect();
    keys.sort();
    for key in keys {
        let pd = vecio::build_paths(&groups[&key], 0.005);
        if pd.prims.is_empty() {
            continue;
        }
        match key.1 {
            Some(l) => out.on_layers.push((pd, l)),
            None => out.paths.push((pd, key.0)),
        }
    }
    if too_many {
        out.notes.push(format!("the drawing repeats its blocks into more than {MAX_DRAWN} pieces, so only the first {MAX_DRAWN} came in"));
    }
    if !unit_said {
        out.notes.push("the drawing doesn't say its units, so they were read as millimetres".into());
    }
    let mut other: Vec<_> = skipped.other.into_iter().collect();
    other.sort();
    for (kind, n) in other {
        let what = match kind.as_str() {
            "IMAGE" => "linked picture(s) (pictures in other files are never read)".to_string(),
            "HATCH" | "SOLID" => "filled area(s) (their outlines come in where they're drawn as lines)".to_string(),
            "XREF" => "reference(s) to other drawings (other files are never read)".to_string(),
            _ => format!("{} item(s)", kind.to_lowercase()),
        };
        out.notes.push(format!("{n} {what} left out"));
    }
    if out.paths.is_empty() && out.on_layers.is_empty() {
        return Err(if out.skipped_text > 0 { "This DXF only has text. Explode the text into lines (or convert it to geometry) in your CAD program first.".into() } else { "This DXF has no shapes BurnCraft can use.".into() });
    }
    Ok(out)
}

struct Ctx<'a> {
    layers: &'a HashMap<String, Layer>,
    blocks: &'a HashMap<String, Block>,
    /// Entities still allowed to be drawn: blocks inside blocks (and MINSERT grids) can multiply a small
    /// file into billions of pieces, so the drawing stops here instead of freezing BurnCraft.
    left: std::cell::Cell<u64>,
}

/// The most entities one DXF import draws, counting every block copy.
const MAX_DRAWN: u64 = 2_000_000;

impl Ctx<'_> {
    /// Draw `ents` through `x`. `parent_rgb` / `parent_layer` are the block reference's colour and layer, for
    /// entities that take them ("by block", and layer 0 inside a block).
    #[allow(clippy::too_many_arguments)]
    fn draw(&self, ents: &[Ent], x: Xform, parent_rgb: Option<[u8; 3]>, parent_layer: Option<&str>, depth: usize, groups: &mut HashMap<([u8; 3], Option<usize>), Vec<Seg>>, skipped: &mut Skipped) {
        if depth > 16 {
            return;
        }
        let mut i = 0;
        while i < ents.len() {
            if self.left.get() == 0 {
                return;
            }
            self.left.set(self.left.get() - 1);
            let e = &ents[i];
            i += 1;
            let layer_name = match (e.s(8).unwrap_or("0"), parent_layer) {
                ("0", Some(p)) => p.to_string(),
                (l, _) => l.to_string(),
            };
            let rgb = match (e.i(420), e.i(62)) {
                (Some(t), _) => [(t >> 16) as u8, (t >> 8) as u8, t as u8],
                (None, Some(0)) => parent_rgb.unwrap_or([0, 0, 0]),
                (None, Some(c)) if (1..=255).contains(&c.abs()) => vecio::aci_rgb(c.unsigned_abs() as u16),
                _ => self.layers.get(&layer_name).map(|l| l.rgb).or(parent_rgb).unwrap_or([0, 0, 0]),
            };
            let key = (rgb, vecio::layer_from_name(&layer_name));
            // the entity's own plane facing down: mirror its x
            let ex = if e.mirrored() { Xform::scale(-1.0, 1.0).then(x) } else { x };
            let mut segs: Vec<Seg> = Vec::new();
            match e.kind.as_str() {
                "LINE" => segs.push(Seg::line(e.p(10), e.p(11))),
                "CIRCLE" => segs.extend(vecio::circle(e.p(10), e.fd(40, 0.0))),
                "ARC" => {
                    let (a0, mut a1) = (e.fd(50, 0.0).to_radians(), e.fd(51, 360.0).to_radians());
                    if a1 <= a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    let r = e.fd(40, 0.0);
                    segs.extend(vecio::ellipse_arc(e.p(10), p2(r, 0.0), p2(0.0, r), a0, a1));
                }
                "ELLIPSE" => {
                    let u = e.p(11);
                    let v = p2(-u.y, u.x) * e.fd(40, 1.0);
                    let (t0, mut t1) = (e.fd(41, 0.0), e.fd(42, std::f64::consts::TAU));
                    if t1 <= t0 {
                        t1 += std::f64::consts::TAU;
                    }
                    let s: Vec<Seg> = vecio::ellipse_arc(e.p(10), u, v, t0, t1);
                    // an ellipse's centre and axis are already in drawing coordinates (no mirroring)
                    groups.entry(key).or_default().extend(s.into_iter().map(|s| s.map(&|p| x.apply(p))));
                    continue;
                }
                "LWPOLYLINE" => {
                    let mut pts: Vec<(P2, f64)> = Vec::new();
                    for (c, v) in &e.codes {
                        let n: f64 = v.trim().parse().unwrap_or(0.0);
                        match c {
                            10 => pts.push((p2(n, 0.0), 0.0)),
                            20 => {
                                if let Some(l) = pts.last_mut() {
                                    l.0.y = n;
                                }
                            }
                            42 => {
                                if let Some(l) = pts.last_mut() {
                                    l.1 = n;
                                }
                            }
                            _ => {}
                        }
                    }
                    segs.extend(poly_segs(&pts, e.i(70).unwrap_or(0) & 1 != 0));
                }
                "POLYLINE" => {
                    let flags = e.i(70).unwrap_or(0);
                    let mut pts: Vec<(P2, f64)> = Vec::new();
                    while i < ents.len() && ents[i].kind == "VERTEX" {
                        let v = &ents[i];
                        // spline frame points aren't on the curve
                        if v.i(70).unwrap_or(0) & 16 == 0 {
                            pts.push((v.p(10), v.fd(42, 0.0)));
                        }
                        i += 1;
                    }
                    if i < ents.len() && ents[i].kind == "SEQEND" {
                        i += 1;
                    }
                    if flags & (16 | 64) != 0 {
                        // 3D meshes: their edges as lines
                        *skipped.other.entry("3D mesh".into()).or_default() += 1;
                        continue;
                    }
                    segs.extend(poly_segs(&pts, flags & 1 != 0));
                }
                "SPLINE" => segs.extend(spline(e)),
                "INSERT" => {
                    let name = e.s(2).unwrap_or("");
                    let Some(b) = self.blocks.get(name) else {
                        if !name.starts_with('*') {
                            *skipped.other.entry("XREF".into()).or_default() += 1;
                        }
                        continue;
                    };
                    let sx = e.fd(41, 1.0);
                    let sy = e.fd(42, 1.0);
                    let rot = e.fd(50, 0.0).to_radians();
                    let (cols, rows) = (e.i(70).unwrap_or(1).max(1), e.i(71).unwrap_or(1).max(1));
                    let (cs, rs) = (e.fd(44, 0.0), e.fd(45, 0.0));
                    let ins = e.p(10);
                    for r in 0..rows.min(1000) {
                        for c in 0..cols.min(1000) {
                            // block → (scale about its base) → grid step → rotate → insertion point → parent
                            let local = Xform::translate(-b.base.x, -b.base.y).then(Xform::scale(sx, sy)).then(Xform::translate(c as f64 * cs, r as f64 * rs)).then(Xform::rotate(rot)).then(Xform::translate(ins.x, ins.y));
                            self.draw(&b.ents, local.then(ex), Some(rgb), Some(&layer_name), depth + 1, groups, skipped);
                        }
                    }
                    continue;
                }
                k if TEXTISH.contains(&k) => skipped.text += 1,
                "VERTEX" | "SEQEND" | "POINT" | "VIEWPORT" | "ATTRIB" => {}
                k => *skipped.other.entry(k.to_string()).or_default() += 1,
            }
            groups.entry(key).or_default().extend(segs.into_iter().map(|s| s.map(&|p| ex.apply(p))));
        }
    }
}

/// A polyline's pieces: straight, or arcs where a vertex has a bulge.
fn poly_segs(pts: &[(P2, f64)], closed: bool) -> Vec<Seg> {
    let mut out = Vec::new();
    let n = pts.len();
    if n < 2 {
        return out;
    }
    let last = if closed { n } else { n - 1 };
    for k in 0..last {
        let (a, bulge) = pts[k];
        let b = pts[(k + 1) % n].0;
        out.extend(vecio::bulge_arc(a, b, bulge));
    }
    out
}

/// A spline, drawn finely as short lines (B-spline or NURBS from its control points, else through its fit
/// points).
fn spline(e: &Ent) -> Vec<Seg> {
    let deg = e.i(71).unwrap_or(3).clamp(1, 11) as usize;
    let xs = e.all(10);
    let ys = e.all(20);
    let ctrl: Vec<P2> = xs.iter().zip(&ys).map(|(x, y)| p2(*x, *y)).collect();
    let fit: Vec<P2> = e.all(11).iter().zip(e.all(21).iter()).map(|(x, y)| p2(*x, *y)).collect();
    let closed = e.i(70).unwrap_or(0) & 1 != 0;
    let pts: Vec<P2> = if ctrl.len() > deg {
        let n = ctrl.len();
        let mut knots = e.all(40);
        if knots.len() != n + deg + 1 {
            // clamped and uniform
            knots = (0..n + deg + 1).map(|k| (k.saturating_sub(deg)).min(n - deg) as f64).collect();
        }
        let mut w = e.all(41);
        if w.len() != n {
            w = vec![1.0; n];
        }
        let (t0, t1) = (knots[deg], knots[n]);
        let samples = (n * 24).clamp(64, 6000);
        (0..=samples).map(|s| de_boor(deg, &knots, &ctrl, &w, t0 + (t1 - t0) * s as f64 / samples as f64)).collect()
    } else if fit.len() >= 2 {
        fit
    } else {
        return Vec::new();
    };
    let mut out: Vec<Seg> = pts.windows(2).map(|p| Seg::line(p[0], p[1])).collect();
    if closed && pts.len() > 2 && pts[0].dist(*pts.last().unwrap()) > 1e-9 {
        out.push(Seg::line(*pts.last().unwrap(), pts[0]));
    }
    out
}

/// A point on a (rational) B-spline.
fn de_boor(p: usize, knots: &[f64], ctrl: &[P2], w: &[f64], t: f64) -> P2 {
    let n = ctrl.len();
    // the knot span holding t
    let mut k = p;
    while k + 1 < n && knots[k + 1] <= t {
        k += 1;
    }
    let mut d: Vec<(f64, f64, f64)> = (0..=p).map(|j| {
        let i = (j + k).saturating_sub(p).min(n - 1);
        (ctrl[i].x * w[i], ctrl[i].y * w[i], w[i])
    }).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den.abs() < 1e-12 { 0.0 } else { (t - knots[i]) / den };
            d[j] = ((1.0 - a) * d[j - 1].0 + a * d[j].0, (1.0 - a) * d[j - 1].1 + a * d[j].1, (1.0 - a) * d[j - 1].2 + a * d[j].2);
        }
    }
    let (x, y, ww) = d[p];
    if ww.abs() < 1e-12 { p2(x, y) } else { p2(x / ww, y / ww) }
}

// ------------------------------------------------------------------------------------------------- export

/// The shapes as a DXF drawing (R12, millimetres; each BurnCraft layer a DXF layer in its colour). Pictures
/// can't go in a DXF: returns how many were left out.
pub fn export(shapes: &[Shape]) -> (String, usize) {
    let mut ents = String::new();
    let mut used: Vec<usize> = Vec::new();
    let mut pictures = 0;
    let mut bb = BBox::EMPTY;
    for s in shapes {
        write_shape(&mut ents, s, Xform::IDENTITY, &mut used, &mut pictures, &mut bb);
    }
    used.sort();
    used.dedup();
    if bb.is_empty() {
        bb = BBox { min: p2(0.0, 0.0), max: p2(0.0, 0.0) };
    }
    let mut out = String::new();
    let mut put = |c: i32, v: &str| {
        out.push_str(&format!("{c}\n{v}\n"));
    };
    put(999, "BurnCraft");
    put(0, "SECTION");
    put(2, "HEADER");
    for (k, v) in [("$ACADVER", "AC1009"), ("$INSUNITS", "4"), ("$MEASUREMENT", "1")] {
        put(9, k);
        put(if k == "$ACADVER" { 1 } else { 70 }, v);
    }
    put(9, "$EXTMIN");
    put(10, &n(bb.min.x));
    put(20, &n(bb.min.y));
    put(9, "$EXTMAX");
    put(10, &n(bb.max.x));
    put(20, &n(bb.max.y));
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "TABLES");
    put(0, "TABLE");
    put(2, "LTYPE");
    put(70, "1");
    put(0, "LTYPE");
    put(2, "CONTINUOUS");
    put(70, "0");
    put(3, "Solid line");
    put(72, "65");
    put(73, "0");
    put(40, "0.0");
    put(0, "ENDTAB");
    put(0, "TABLE");
    put(2, "LAYER");
    put(70, &used.len().to_string());
    for l in &used {
        put(0, "LAYER");
        put(2, &layer_name(*l));
        put(70, "0");
        put(62, &vecio::nearest_aci(layer_rgb(*l)).to_string());
        put(6, "CONTINUOUS");
    }
    put(0, "ENDTAB");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    out.push_str(&ents);
    let mut put = |c: i32, v: &str| {
        out.push_str(&format!("{c}\n{v}\n"));
    };
    put(0, "ENDSEC");
    put(0, "EOF");
    (out, pictures)
}

fn n(v: f64) -> String {
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

fn write_shape(out: &mut String, s: &Shape, parent: Xform, used: &mut Vec<usize>, pictures: &mut usize, bb: &mut BBox) {
    // the shape's own placement first, then its group's (as Shape::world_outline does)
    let x = parent.then_after(s.xform);
    let shape = Shape { xform: x, ..s.clone() };
    match &s.kind {
        ShapeKind::Group(children) => {
            for c in children {
                write_shape(out, c, x, used, pictures, bb);
            }
            return;
        }
        ShapeKind::Bitmap { .. } => {
            *pictures += 1;
            return;
        }
        _ => {}
    }
    let layer = layer_name(s.layer);
    used.push(s.layer);
    *bb = bb.union(&shape.bbox());
    let mut put = |c: i32, v: &str| {
        out.push_str(&format!("{c}\n{v}\n"));
    };
    // circles stay exact circles
    if let Some((c, r)) = crate::ops::circle_of(&shape) {
        put(0, "CIRCLE");
        put(8, &layer);
        put(10, &n(c.x));
        put(20, &n(c.y));
        put(30, "0");
        put(40, &n(r));
        return;
    }
    for l in shape.world_outline(0.01) {
        if l.len() < 2 {
            continue;
        }
        let closed = is_closed(&l);
        let pts = if closed { &l[..l.len() - 1] } else { &l[..] };
        if pts.len() == 2 && !closed {
            put(0, "LINE");
            put(8, &layer);
            put(10, &n(pts[0].x));
            put(20, &n(pts[0].y));
            put(30, "0");
            put(11, &n(pts[1].x));
            put(21, &n(pts[1].y));
            put(31, "0");
            continue;
        }
        put(0, "POLYLINE");
        put(8, &layer);
        put(66, "1");
        put(10, "0");
        put(20, "0");
        put(30, "0");
        put(70, if closed { "1" } else { "0" });
        for p in pts {
            put(0, "VERTEX");
            put(8, &layer);
            put(10, &n(p.x));
            put(20, &n(p.y));
            put(30, "0");
        }
        put(0, "SEQEND");
        put(8, &layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(header: &str, tables: &str, blocks: &str, ents: &str) -> String {
        format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n{tables}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n")
    }

    fn bounds(pd: &PathData) -> BBox {
        let mut b = BBox::EMPTY;
        for p in pd.flatten(0.001).into_iter().flatten() {
            b.add(p);
        }
        b
    }

    fn all_bounds(im: &crate::svg::Imported) -> BBox {
        im.paths.iter().map(|(pd, _)| bounds(pd)).chain(im.on_layers.iter().map(|(pd, _)| bounds(pd))).fold(BBox::EMPTY, |a, b| a.union(&b))
    }

    #[test]
    fn blocks_that_multiply_stop_instead_of_freezing() {
        // three levels of 1000 × 1000 grids of a point: 10^18 copies from a tiny file
        let p0 = "0\nBLOCK\n2\nb0\n10\n0\n20\n0\n0\nPOINT\n10\n0\n20\n0\n0\nENDBLK\n";
        let grid = |name: &str, of: &str| format!("0\nBLOCK\n2\n{name}\n10\n0\n20\n0\n0\nINSERT\n2\n{of}\n10\n0\n20\n0\n70\n1000\n71\n1000\n44\n1\n45\n1\n0\nENDBLK\n");
        let blocks = format!("{p0}{}{}{}", grid("b1", "b0"), grid("b2", "b1"), grid("b3", "b2"));
        let ents = "0\nCIRCLE\n10\n5\n20\n5\n40\n5\n0\nINSERT\n2\nb3\n10\n0\n20\n0\n";
        let started = std::time::Instant::now();
        let im = import(dxf("", "", &blocks, ents).as_bytes()).unwrap();
        assert!(started.elapsed().as_secs() < 30, "it stopped early");
        assert_eq!(im.paths.len(), 1, "the circle drawn before the blocks came in");
        assert!(im.notes.iter().any(|n| n.contains("repeats its blocks")), "and it says so: {:?}", im.notes);
    }

    #[test]
    fn units_lines_join_and_text_is_left_out() {
        // inches: a 1 × 1 in square from four separate lines, on a red layer; and a text
        let ents = "0\nLINE\n8\nCut\n10\n0\n20\n0\n11\n1\n21\n0\n0\nLINE\n8\nCut\n10\n1\n20\n1\n11\n1\n21\n0\n0\nLINE\n8\nCut\n10\n1\n20\n1\n11\n0\n21\n1\n0\nLINE\n8\nCut\n10\n0\n20\n0\n11\n0\n21\n1\n0\nTEXT\n8\nCut\n1\nhello\n";
        let d = dxf("9\n$INSUNITS\n70\n1\n", "0\nLAYER\n2\nCut\n62\n1\n", "", ents);
        let im = import(d.as_bytes()).unwrap();
        assert_eq!(im.paths.len(), 1);
        assert_eq!(im.paths[0].1, [255, 0, 0], "the layer's colour");
        let lines = im.paths[0].0.flatten(0.01);
        assert_eq!(lines.len(), 1, "four lines joined into one outline");
        assert!(is_closed(&lines[0]));
        let b = bounds(&im.paths[0].0);
        assert!((b.width() - 25.4).abs() < 1e-6 && (b.height() - 25.4).abs() < 1e-6);
        assert_eq!(im.skipped_text, 1);
        assert!(im.notes.is_empty(), "units were given");
        // no units: millimetres, and it says so
        let d = dxf("", "", "", "0\nCIRCLE\n10\n5\n20\n5\n40\n5\n");
        let im = import(d.as_bytes()).unwrap();
        assert!((all_bounds(&im).width() - 10.0).abs() < 1e-6);
        assert!(im.notes[0].contains("millimetres"));
        assert!(import(b"hello").is_err());
        assert!(import(dxf("", "", "", "0\nTEXT\n1\nonly text\n").as_bytes()).is_err());
    }

    #[test]
    fn bulges_arcs_ellipses_and_splines() {
        // a 20 × 10 rectangle with rounded right end (a half-circle bulge), closed
        let lw = "0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n20\n20\n0\n42\n1\n10\n20\n20\n10\n10\n0\n20\n10\n";
        let arc = "0\nARC\n10\n100\n20\n0\n40\n10\n50\n0\n51\n90\n";
        let ell = "0\nELLIPSE\n10\n200\n20\n0\n11\n20\n21\n0\n40\n0.5\n";
        // a clamped cubic B-spline through (300,0) … (330,0) with control points up at y=10
        let spl = "0\nSPLINE\n71\n3\n72\n8\n73\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n10\n300\n20\n0\n10\n310\n20\n10\n10\n320\n20\n10\n10\n330\n20\n0\n";
        let im = import(dxf("9\n$INSUNITS\n70\n4\n", "", "", &format!("{lw}{arc}{ell}{spl}")).as_bytes()).unwrap();
        let lines = im.paths[0].0.flatten(0.01);
        let find = |x0: f64, x1: f64| lines.iter().filter(|l| l.iter().all(|p| p.x >= x0 && p.x <= x1)).flat_map(|l| l.iter().copied()).fold(BBox::EMPTY, |mut b, p| {
            b.add(p);
            b
        });
        let r = find(-1.0, 40.0);
        assert!((r.max.x - 25.0).abs() < 1e-3, "the bulge reaches 5 past x = 20: {r:?}");
        let a = find(90.0, 120.0);
        assert!((a.max.x - 110.0).abs() < 1e-3 && (a.max.y - 10.0).abs() < 1e-3 && a.min.y.abs() < 1e-3);
        let e = find(170.0, 230.0);
        assert!((e.width() - 40.0).abs() < 1e-3 && (e.height() - 20.0).abs() < 1e-3);
        let s = find(299.0, 331.0);
        assert!((s.width() - 30.0).abs() < 1e-6 && s.max.y > 6.0 && s.max.y < 7.6, "{s:?}");
        assert!(im.paths[0].0.prims.iter().any(|p| p.kind == PrimKind::Bezier), "arcs stay curves");
    }

    #[test]
    fn blocks_nest_with_their_placement_and_colours() {
        // block "sq": a 10 × 10 square around its base point (5,5); block "two": "sq" twice, the second moved
        // 20 right; the drawing inserts "two" at (100, 0), turned 90° and doubled
        let sq = "0\nBLOCK\n2\nsq\n10\n5\n20\n5\n0\nLWPOLYLINE\n8\n0\n62\n0\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n10\n0\nENDBLK\n";
        let two = "0\nBLOCK\n2\ntwo\n10\n0\n20\n0\n0\nINSERT\n2\nsq\n10\n0\n20\n0\n0\nINSERT\n2\nsq\n10\n20\n20\n0\n0\nENDBLK\n";
        let ents = "0\nINSERT\n2\ntwo\n8\nC03\n62\n5\n10\n100\n20\n0\n41\n2\n42\n2\n50\n90\n";
        let im = import(dxf("9\n$INSUNITS\n70\n4\n", "", &format!("{sq}{two}"), ents).as_bytes()).unwrap();
        assert!(im.paths.is_empty());
        assert_eq!(im.on_layers.len(), 1, "a DXF layer named C03 goes back on C03");
        assert_eq!(im.on_layers[0].1, 3);
        let lines = im.on_layers[0].0.flatten(0.01);
        assert_eq!(lines.len(), 2);
        let b = bounds(&im.on_layers[0].0);
        // each square 20 × 20 (doubled), centred on its place: the first on (100, 0); the second was 20 to the
        // right in the block, which doubled and turned 90° puts it 40 up, on (100, 40)
        assert!((b.min.x - 90.0).abs() < 1e-6 && (b.max.x - 110.0).abs() < 1e-6, "{b:?}");
        assert!((b.min.y + 10.0).abs() < 1e-6 && (b.max.y - 50.0).abs() < 1e-6, "{b:?}");
        // a block that names another file isn't followed, and a missing block is noted
        let x = "0\nBLOCK\n2\nother\n70\n4\n1\nC:\\elsewhere.dwg\n0\nENDBLK\n";
        let im = import(dxf("", "", x, &format!("0\nINSERT\n2\nother\n10\n0\n20\n0\n{}", "0\nCIRCLE\n10\n0\n20\n0\n40\n1\n")).as_bytes()).unwrap();
        assert!(im.notes.iter().any(|n| n.contains("other drawings")));
    }

    #[test]
    fn exported_dxf_reads_back_the_same() {
        let mut shapes = vec![Shape::new(ShapeKind::Rect { w: 30.0, h: 20.0, cr: 4.0 }, Xform::translate(40.0, 50.0), 2)];
        shapes.push(Shape::new(ShapeKind::Ellipse { rx: 6.0, ry: 6.0 }, Xform::translate(90.0, 50.0), 5));
        shapes.push(Shape::new(ShapeKind::Polygon { n: 6, rx: 10.0, ry: 10.0 }, Xform::translate(130.0, 50.0).then_after(Xform::IDENTITY), 2));
        let (text, pictures) = export(&shapes);
        assert_eq!(pictures, 0);
        let im = import(text.as_bytes()).unwrap();
        assert!(im.notes.is_empty(), "{:?}", im.notes);
        assert!(im.paths.is_empty(), "every piece back on its own layer");
        let on = |l: usize| im.on_layers.iter().filter(|(_, k)| *k == l).map(|(pd, _)| bounds(pd)).fold(BBox::EMPTY, |a, b| a.union(&b));
        let want2 = shapes[0].bbox().union(&shapes[2].bbox());
        let got2 = on(2);
        for (g, w) in [(got2.min, want2.min), (got2.max, want2.max), (on(5).min, shapes[1].bbox().min), (on(5).max, shapes[1].bbox().max)] {
            // (a shape's own bounds come from a slightly coarse outline: within 0.05 mm)
            assert!(g.dist(w) < 0.05, "{g:?} vs {w:?}");
        }
        // closed shapes stay closed
        assert!(im.on_layers.iter().all(|(pd, _)| pd.flatten(0.01).iter().all(|l| is_closed(l))));
    }
}
