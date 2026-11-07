//! EZCad2 project files (.ezd): import the shapes, text, images and pen settings.
//!
//! The file layout follows the open-source reader in MeerK40t (`meerk40t/extra/ezd.py`, MIT licence):
//! a UTF-16 "EZCADUNI" header, a seek table, 256 pens, and a Huffman-compressed stream of objects (curves,
//! rectangles, circles, ellipses, polygons, text, images, groups and hatches). EZCad2 measures in mm from the
//! middle of the field with Y up; BurnCraft puts that middle in the middle of the work area.
//!
//! Exporting to .ezd isn't offered: the format is undocumented and EZCad2 is strict about what it opens.

use crate::geom::*;
use crate::model::*;

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.b.len() {
            return Err("the file ends early".into());
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn peek_i32(&self) -> Option<i32> {
        self.b.get(self.pos..self.pos + 4).map(|s| i32::from_le_bytes(s.try_into().unwrap()))
    }
    /// A count, then that many (length, bytes) items.
    fn items(&mut self) -> Result<Vec<&'a [u8]>, String> {
        let n = self.i32()?;
        if !(0..=100_000).contains(&n) {
            return Err("unexpected data".into());
        }
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let len = self.i32()?;
            if len == -1 {
                break;
            }
            if !(0..=64 * 1024 * 1024).contains(&len) {
                return Err("unexpected data".into());
            }
            out.push(self.take(len as usize)?);
        }
        Ok(out)
    }
}

fn int(v: &[&[u8]], i: usize) -> i64 {
    match v.get(i) {
        Some(b) if b.len() == 4 => i32::from_le_bytes((*b).try_into().unwrap()) as i64,
        Some(b) if b.len() == 2 => u16::from_le_bytes((*b).try_into().unwrap()) as i64,
        _ => 0,
    }
}
fn float(v: &[&[u8]], i: usize) -> f64 {
    match v.get(i) {
        Some(b) if b.len() == 8 => f64::from_le_bytes((*b).try_into().unwrap()),
        Some(b) if b.len() == 4 => i32::from_le_bytes((*b).try_into().unwrap()) as f64,
        _ => 0.0,
    }
}
fn point(v: &[&[u8]], i: usize) -> P2 {
    match v.get(i) {
        Some(b) if b.len() == 16 => p2(f64::from_le_bytes(b[0..8].try_into().unwrap()), f64::from_le_bytes(b[8..16].try_into().unwrap())),
        _ => p2(0.0, 0.0),
    }
}
fn text(v: &[&[u8]], i: usize) -> String {
    match v.get(i) {
        Some(b) => {
            let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&u).trim_end_matches('\0').to_string()
        }
        None => String::new(),
    }
}
/// EZCad's 3×3 matrix as an affine transform: x' = m0 x + m3 y + m6, y' = m1 x + m4 y + m7.
fn matrix(v: &[&[u8]], i: usize) -> Xform {
    match v.get(i) {
        Some(b) if b.len() == 72 => {
            let m: Vec<f64> = b.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
            Xform { a: m[0], b: m[1], c: m[3], d: m[4], e: m[6], f: m[7] }
        }
        _ => Xform::IDENTITY,
    }
}

/// One laser setting ("pen").
#[derive(Clone, Debug, Default)]
pub struct Pen {
    pub color: [u8; 3],
    pub name: String,
    pub passes: u32,
    /// mm/s
    pub speed: f64,
    /// percent
    pub power: f64,
    /// kHz
    pub frequency: f64,
}

#[derive(Debug)]
enum Obj {
    Curve { pen: i64, label: String, position: P2, contours: Vec<(u8, Vec<P2>)>, closed: bool },
    Rect { pen: i64, min: P2, max: P2, round: f64, m: Xform },
    Circle { pen: i64, center: P2, r: f64, m: Xform },
    Ellipse { pen: i64, c0: P2, c1: P2, m: Xform },
    Polygon { pen: i64, c0: P2, c1: P2, sides: u32, m: Xform },
    Text { pen: i64, text: String, font: String, height: f64, pos: P2 },
    Image { pen: i64, bmp: Vec<u8>, pos: P2, w: f64, h: f64 },
    Group(Vec<Obj>),
    Hatch { pen: i64, children: Vec<Obj>, line_space: f64, angle: f64, outlines: bool },
    Other,
}

struct Parsed {
    pens: Vec<Pen>,
    objects: Vec<Obj>,
}

fn huffman(r: &mut Reader, want: usize) -> Result<Vec<u8>, String> {
    let n = r.u16()? as usize;
    // (code bits, length) → byte
    let mut table: std::collections::HashMap<(u32, u16), u8> = std::collections::HashMap::new();
    let mut max_len = 0;
    for _ in 0..n {
        let ch = r.take(1)?[0];
        let bits = r.u32()?;
        let len = r.u16()?;
        if len == 0 || len > 32 {
            continue;
        }
        let code = if len == 32 { bits } else { bits & ((1u32 << len) - 1) };
        table.insert((code, len), ch);
        max_len = max_len.max(len);
    }
    let data = &r.b[r.pos..];
    let mut out = Vec::with_capacity(want);
    let (mut code, mut len) = (0u32, 0u16);
    'bytes: for &byte in data {
        for k in (0..8).rev() {
            code = (code << 1) | ((byte >> k) & 1) as u32;
            len += 1;
            if let Some(&c) = table.get(&(code, len)) {
                out.push(c);
                code = 0;
                len = 0;
                if out.len() >= want {
                    break 'bytes;
                }
            } else if len > max_len {
                return Err("the drawing data is damaged".into());
            }
        }
    }
    Ok(out)
}

/// The 15-item header every object starts with; returns (pen, label, position).
fn header(r: &mut Reader) -> Result<(i64, String, P2), String> {
    let h = r.items()?;
    Ok((int(&h, 0), text(&h, 3), point(&h, 13)))
}

fn children(r: &mut Reader) -> Result<Vec<Obj>, String> {
    let n = r.i32()?;
    let mut out = Vec::new();
    if !(0..=100_000).contains(&n) {
        return Ok(out);
    }
    for _ in 0..n {
        match object(r)? {
            Some(o) => out.push(o),
            None => break,
        }
    }
    Ok(out)
}

/// A group written without its type number (the cached result inside hatches and spirals).
fn bare_group(r: &mut Reader) -> Result<Vec<Obj>, String> {
    header(r)?;
    children(r)
}

fn object(r: &mut Reader) -> Result<Option<Obj>, String> {
    let kind = match r.i32() {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    if kind == 0 {
        return Ok(None);
    }
    let o = match kind {
        1 => {
            let (pen, label, position) = header(r)?;
            let count = r.u32()?;
            let closed = r.u32()? != 0;
            let mut contours = Vec::new();
            for _ in 0..count.min(1_000_000) {
                let head = r.take(6)?;
                let t = head[1];
                if t == 0 {
                    r.take(40)?;
                    continue;
                }
                let n = r.i32()?.clamp(0, 10_000_000) as usize;
                let mut pts = Vec::with_capacity(n);
                for _ in 0..n {
                    let x = r.f64()?;
                    let y = r.f64()?;
                    pts.push(p2(x, y));
                }
                contours.push((t, pts));
            }
            Obj::Curve { pen, label, position, contours, closed }
        }
        3 => {
            let (pen, ..) = header(r)?;
            let a = r.items()?;
            Obj::Rect { pen, min: point(&a, 0), max: point(&a, 1), round: float(&a, 2), m: matrix(&a, 7) }
        }
        4 => {
            let (pen, ..) = header(r)?;
            let a = r.items()?;
            Obj::Circle { pen, center: point(&a, 0), r: float(&a, 1), m: matrix(&a, 5) }
        }
        5 => {
            let (pen, ..) = header(r)?;
            let a = r.items()?;
            Obj::Ellipse { pen, c0: point(&a, 1), c1: point(&a, 2), m: matrix(&a, 6) }
        }
        6 => {
            let (pen, ..) = header(r)?;
            let a = r.items()?;
            Obj::Polygon { pen, c0: point(&a, 1), c1: point(&a, 2), sides: int(&a, 7).clamp(3, 1000) as u32, m: matrix(&a, 9) }
        }
        0x800 => {
            let (pen, _, position) = header(r)?;
            let a = r.items()?;
            let n = r.i32()?;
            for _ in 0..n.clamp(0, 10_000) {
                r.u16()?;
                r.items()?;
                r.items()?;
            }
            r.i32()?;
            let pos = point(&a, 7);
            Obj::Text { pen, text: text(&a, 10), font: text(&a, 18), height: float(&a, 1), pos: if pos == p2(0.0, 0.0) { position } else { pos } }
        }
        0x40 => {
            let (pen, _, position) = header(r)?;
            let a = r.items()?;
            let start = r.pos;
            r.take(2)?;
            let size = r.i32()?.max(6) as usize;
            r.pos = start;
            let bmp = r.take(size)?.to_vec();
            Obj::Image { pen, bmp, pos: position, w: float(&a, 5), h: float(&a, 4) }
        }
        0x10 | 0x30 | 0x50 => {
            header(r)?;
            let ch = children(r)?;
            if kind == 0x50 {
                r.items()?;
            }
            Obj::Group(ch)
        }
        0x20 => {
            let (pen, ..) = header(r)?;
            let ch = children(r)?;
            let a = r.items()?;
            let (line_space, angle, outlines, hatch_pen) = if a.len() >= 42 { (float(&a, 5), float(&a, 8), int(&a, 0) != 0, int(&a, 2)) } else { (0.1, 0.0, true, pen) };
            if r.peek_i32() == Some(15) {
                bare_group(r)?; // the hatch lines EZCad made; BurnCraft makes its own fill
            }
            Obj::Hatch { pen: hatch_pen, children: ch, line_space, angle, outlines }
        }
        0x60 => {
            header(r)?;
            let ch = children(r)?;
            r.items()?;
            bare_group(r)?;
            Obj::Group(ch)
        }
        0x2000 | 0x3000 | 0x4000 | 0x5000 | 0x6000 => {
            header(r)?;
            r.items()?;
            Obj::Other
        }
        _ => return Err(format!("unknown object type {kind:#x}")),
    };
    Ok(Some(o))
}

fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    let mut r = Reader { b: bytes, pos: 0 };
    let magic = r.take(16).map_err(|_| "not an EZCad2 file".to_string())?;
    let m: Vec<u16> = magic.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    if String::from_utf16_lossy(&m) != "EZCADUNI" {
        return Err("not an EZCad2 file (no EZCADUNI header)".into());
    }
    r.take(4 + 4 + 60 * 3 + 140)?;
    let mut seek = [0i32; 7];
    for s in seek.iter_mut() {
        *s = r.i32()?;
    }
    let (pens_at, vectors_at) = (seek[2], seek[5]);
    let mut pens = Vec::new();
    if pens_at > 0 {
        r.pos = pens_at as usize;
        let count = r.i32()?;
        let at = r.i32()?;
        r.pos = at.max(0) as usize;
        for _ in 0..count.clamp(0, 256) {
            let a = r.items()?;
            let bgr = int(&a, 0) as u32;
            pens.push(Pen {
                color: [(bgr & 255) as u8, ((bgr >> 8) & 255) as u8, ((bgr >> 16) & 255) as u8],
                name: text(&a, 1),
                passes: int(&a, 4).max(1) as u32,
                speed: float(&a, 5),
                power: float(&a, 6),
                frequency: float(&a, 7) / 1000.0,
            });
        }
    }
    let mut objects = Vec::new();
    if vectors_at > 0 {
        r.pos = vectors_at as usize;
        let want = r.u32()? as usize;
        r.take(16)?;
        let data = huffman(&mut r, want.min(512 * 1024 * 1024))?;
        let mut v = Reader { b: &data, pos: 0 };
        while v.pos + 4 <= data.len() {
            match object(&mut v) {
                Ok(Some(o)) => objects.push(o),
                Ok(None) => {} // end of a section: carry on with the next
                Err(_) => break,
            }
        }
    }
    Ok(Parsed { pens, objects })
}

/// Contours (line, quadratic or cubic pieces, each starting at its start point) → path data.
fn curve_path(contours: &[(u8, Vec<P2>)], closed: bool) -> PathData {
    let mut pd = PathData::default();
    let mut sub_start = 0;
    let mut last: Option<P2> = None;
    for (t, pts) in contours {
        if pts.len() < 2 {
            continue;
        }
        let near = |a: P2, b: P2| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
        if last.map(|l| !near(l, pts[0])).unwrap_or(true) {
            sub_start = pd.verts.len();
            pd.verts.push(Vert::new(pts[0]));
        }
        let mut cur = pd.verts.len() - 1;
        match t {
            2 => {
                for q in pts[1..].chunks_exact(2) {
                    let (p0, c, p1) = (pd.verts[cur].p, q[0], q[1]);
                    pd.verts[cur].c0 = Some(p2(p0.x + (c.x - p0.x) * 2.0 / 3.0, p0.y + (c.y - p0.y) * 2.0 / 3.0));
                    pd.verts.push(Vert { p: p1, c0: None, c1: Some(p2(p1.x + (c.x - p1.x) * 2.0 / 3.0, p1.y + (c.y - p1.y) * 2.0 / 3.0)) });
                    pd.prims.push(Prim { kind: PrimKind::Bezier, a: cur, b: cur + 1 });
                    cur += 1;
                }
            }
            3 => {
                for q in pts[1..].chunks_exact(3) {
                    pd.verts[cur].c0 = Some(q[0]);
                    pd.verts.push(Vert { p: q[2], c0: None, c1: Some(q[1]) });
                    pd.prims.push(Prim { kind: PrimKind::Bezier, a: cur, b: cur + 1 });
                    cur += 1;
                }
            }
            _ => {
                for p in &pts[1..] {
                    pd.verts.push(Vert::new(*p));
                    pd.prims.push(Prim { kind: PrimKind::Line, a: cur, b: cur + 1 });
                    cur += 1;
                }
            }
        }
        last = Some(pd.verts[cur].p);
    }
    // join the final point back onto the start when the curve is closed (or ends where it began)
    if let (Some(l), true) = (last, pd.verts.len() > sub_start + 1) {
        let s = pd.verts[sub_start].p;
        let at_start = (l.x - s.x).abs() < 1e-6 && (l.y - s.y).abs() < 1e-6;
        if at_start || closed {
            if let Some(pr) = pd.prims.last_mut() {
                if at_start && pr.b == pd.verts.len() - 1 {
                    let c1 = pd.verts[pr.b].c1;
                    pd.verts.pop();
                    pr.b = sub_start;
                    pd.verts[sub_start].c1 = c1;
                } else if closed && !at_start {
                    let n = pd.verts.len() - 1;
                    pd.prims.push(Prim { kind: PrimKind::Line, a: n, b: sub_start });
                }
            }
        }
    }
    pd
}

/// What an import produces: shapes (in BurnCraft bed coordinates), the pens used (pen → layer), and notes.
pub struct Import {
    pub shapes: Vec<Shape>,
    pub pens: Vec<(usize, Pen)>,
    /// Layers that should fill (EZCad hatches): layer, line spacing, angle.
    pub fills: Vec<(usize, f64, f64)>,
    pub notes: Vec<String>,
}

/// Pen number → BurnCraft colour layer (pens 0–29 keep their number).
pub fn pen_layer(pen: i64) -> usize {
    (pen.max(0) as usize) % COLOR_LAYERS
}

pub fn import(bytes: &[u8], bed_w: f64, bed_h: f64) -> Result<Import, String> {
    let parsed = parse(bytes)?;
    // EZCad (0, 0) is the middle of the field, Y up
    let to_bed = Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: bed_w / 2.0, f: bed_h / 2.0 };
    let mut out = Import { shapes: Vec::new(), pens: Vec::new(), fills: Vec::new(), notes: Vec::new() };
    let mut used = std::collections::BTreeSet::new();
    let mut skipped = 0;
    fn walk(o: &Obj, fill: Option<i64>, to_bed: Xform, out: &mut Vec<Shape>, used: &mut std::collections::BTreeSet<i64>, fills: &mut Vec<(usize, f64, f64)>, skipped: &mut usize) {
        let mut add = |kind: ShapeKind, x: Xform, pen: i64| {
            let pen = fill.unwrap_or(pen);
            used.insert(pen);
            out.push(Shape::new(kind, to_bed.then_after(x), pen_layer(pen)));
        };
        match o {
            Obj::Curve { pen, contours, closed, label, position } => {
                // EZCad's own hatch lines (pen 0, no name, at the origin) are left out; BurnCraft fills instead
                if *pen == 0 && label.is_empty() && *position == p2(0.0, 0.0) && fill.is_some() {
                    return;
                }
                let pd = curve_path(contours, *closed);
                if !pd.prims.is_empty() {
                    add(ShapeKind::Path(pd), Xform::IDENTITY, *pen);
                }
            }
            Obj::Rect { pen, min, max, round, m } => {
                let (w, h) = ((max.x - min.x).abs(), (max.y - min.y).abs());
                let c = p2((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
                add(ShapeKind::Rect { w, h, cr: round.max(0.0).min(w.min(h) / 2.0) }, m.then_after(Xform::translate(c.x, c.y)), *pen);
            }
            Obj::Circle { pen, center, r, m } => add(ShapeKind::Ellipse { rx: *r, ry: *r }, m.then_after(Xform::translate(center.x, center.y)), *pen),
            Obj::Ellipse { pen, c0, c1, m } => {
                let c = p2((c0.x + c1.x) / 2.0, (c0.y + c1.y) / 2.0);
                add(ShapeKind::Ellipse { rx: (c1.x - c0.x).abs() / 2.0, ry: (c1.y - c0.y).abs() / 2.0 }, m.then_after(Xform::translate(c.x, c.y)), *pen);
            }
            Obj::Polygon { pen, c0, c1, sides, m } => {
                let c = p2((c0.x + c1.x) / 2.0, (c0.y + c1.y) / 2.0);
                let (rx, ry) = ((c1.x - c0.x).abs() / 2.0, (c1.y - c0.y).abs() / 2.0);
                let step = std::f64::consts::TAU / *sides as f64;
                let mut pts: Vec<P2> = (0..*sides).map(|i| p2(c.x + (step / 2.0 + i as f64 * step).cos() * rx, c.y + (step / 2.0 + i as f64 * step).sin() * ry)).collect();
                pts.push(pts[0]);
                add(ShapeKind::Path(PathData::polyline(&pts[..pts.len() - 1], true)), *m, *pen);
            }
            Obj::Text { pen, text, font, height, pos } => {
                if !text.trim().is_empty() {
                    let font = if font.is_empty() { String::new() } else { format!("{font},-1,100,5,50,0,0,0,0,0") };
                    add(ShapeKind::Text { text: text.clone(), font, height: height.max(0.5), style: Default::default(), backup: None }, Xform::translate(pos.x, pos.y), *pen);
                }
            }
            Obj::Image { pen, bmp, pos, w, h } => match crate::lbrn::decode_bitmap(bmp.clone(), "ezcad.bmp") {
                Some(data) if *w > 0.0 && *h > 0.0 => add(ShapeKind::Bitmap { w: *w, h: *h, data, adjust: Default::default() }, Xform::translate(pos.x, pos.y), *pen),
                _ => *skipped += 1,
            },
            Obj::Group(ch) => {
                for c in ch {
                    walk(c, fill, to_bed, out, used, fills, skipped);
                }
            }
            Obj::Hatch { pen, children, line_space, angle, .. } => {
                let layer = pen_layer(*pen);
                if !fills.iter().any(|f| f.0 == layer) {
                    fills.push((layer, *line_space, *angle));
                }
                for c in children {
                    walk(c, Some(*pen), to_bed, out, used, fills, skipped);
                }
            }
            Obj::Other => *skipped += 1,
        }
    }
    for o in &parsed.objects {
        walk(o, None, to_bed, &mut out.shapes, &mut used, &mut out.fills, &mut skipped);
    }
    for p in used {
        if let Some(pen) = parsed.pens.get(p.max(0) as usize) {
            out.pens.push((pen_layer(p), pen.clone()));
        }
    }
    if skipped > 0 {
        out.notes.push(format!("{skipped} EZCad item(s) (timers, inputs/outputs, axis moves or unreadable images) aren't drawings and were left out."));
    }
    if out.shapes.is_empty() {
        return Err("no drawings found in this EZCad2 file".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny .ezd: header, seek table, one pen, and a Huffman-coded object stream (fixed 8-bit codes).
    fn sample() -> Vec<u8> {
        let utf16 = |s: &str, n: usize| {
            let mut v: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            v.resize(n, 0);
            v
        };
        let item = |b: &[u8]| {
            let mut v = (b.len() as i32).to_le_bytes().to_vec();
            v.extend_from_slice(b);
            v
        };
        let items = |list: &[Vec<u8>]| {
            let mut v = (list.len() as i32).to_le_bytes().to_vec();
            for i in list {
                v.extend(item(i));
            }
            v
        };
        let pt = |x: f64, y: f64| [x.to_le_bytes(), y.to_le_bytes()].concat();
        let header = |pen: i32| {
            let mut h: Vec<Vec<u8>> = vec![vec![0u8; 4]; 15];
            h[0] = pen.to_le_bytes().to_vec();
            h[3] = utf16("", 60);
            h[13] = pt(0.0, 0.0);
            items(&h)
        };
        // objects: a circle (pen 1) and a closed triangle curve (pen 2)
        let mut objs = Vec::new();
        objs.extend(4i32.to_le_bytes());
        objs.extend(header(1));
        let mut ident = Vec::new();
        for v in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
            ident.extend(v.to_le_bytes());
        }
        objs.extend(items(&[pt(10.0, 5.0), 7.5f64.to_le_bytes().to_vec(), vec![0; 8], vec![0; 4], vec![0; 4], ident]));
        objs.extend(1i32.to_le_bytes());
        objs.extend(header(2));
        objs.extend(1u32.to_le_bytes());
        objs.extend(1u32.to_le_bytes());
        objs.extend([0u8, 1, 0, 0, 0, 0]);
        objs.extend(4i32.to_le_bytes());
        for (x, y) in [(0.0, 0.0), (20.0, 0.0), (10.0, 15.0), (0.0, 0.0)] {
            objs.extend(pt(x, y));
        }
        objs.extend(0i32.to_le_bytes());
        // Huffman table: every byte value gets its own 8-bit code
        let mut vec_section = (objs.len() as u32).to_le_bytes().to_vec();
        vec_section.extend([0u8; 16]);
        vec_section.extend(256u16.to_le_bytes());
        for c in 0..=255u8 {
            vec_section.push(c);
            vec_section.extend((c as u32).to_le_bytes());
            vec_section.extend(8u16.to_le_bytes());
        }
        vec_section.extend(&objs);
        let mut f = utf16("EZCADUNI", 16);
        f.extend(0i32.to_le_bytes());
        f.extend(2001i32.to_le_bytes());
        f.extend(vec![0u8; 60 * 3 + 140]);
        let seek_at = f.len();
        f.extend(vec![0u8; 28 + 96]);
        // pens: count, then where they start
        let pens_at = f.len();
        f.extend(3i32.to_le_bytes());
        f.extend(((pens_at + 8) as i32).to_le_bytes());
        for (i, name) in ["Default", "Red", "Blue"].iter().enumerate() {
            let mut a: Vec<Vec<u8>> = vec![vec![0u8; 4]; 8];
            a[0] = (0x0000FF_i32 << (8 * i as i32)).to_le_bytes().to_vec();
            a[1] = utf16(name, 60);
            a[4] = 2i32.to_le_bytes().to_vec();
            a[5] = (100.0 * (i + 1) as f64).to_le_bytes().to_vec();
            a[6] = 55.0f64.to_le_bytes().to_vec();
            a[7] = 20000.0f64.to_le_bytes().to_vec();
            f.extend(items(&a));
        }
        let vectors_at = f.len();
        f.extend(vec_section);
        f[seek_at + 8..seek_at + 12].copy_from_slice(&(pens_at as i32).to_le_bytes());
        f[seek_at + 20..seek_at + 24].copy_from_slice(&(vectors_at as i32).to_le_bytes());
        f
    }

    #[test]
    fn reads_shapes_and_pens() {
        let imp = import(&sample(), 110.0, 110.0).unwrap();
        assert_eq!(imp.shapes.len(), 2);
        // the circle: radius 7.5 at (10, 5) from the middle of a 110 mm field
        let b = imp.shapes[0].bbox();
        assert!((b.center().x - 65.0).abs() < 0.01 && (b.center().y - 60.0).abs() < 0.01);
        assert!((b.width() - 15.0).abs() < 0.05);
        assert_eq!(imp.shapes[0].layer, 1);
        // the triangle is one closed path
        assert!(imp.shapes[1].is_closed());
        assert_eq!(imp.shapes[1].layer, 2);
        let pen1 = &imp.pens.iter().find(|p| p.0 == 1).unwrap().1;
        assert_eq!(pen1.name, "Red");
        assert_eq!(pen1.speed, 200.0);
        assert_eq!(pen1.power, 55.0);
        assert_eq!(pen1.passes, 2);
        assert!((pen1.frequency - 20.0).abs() < 1e-9);
        assert!(import(b"not an ezd file at all, really not", 100.0, 100.0).is_err());
    }
}
