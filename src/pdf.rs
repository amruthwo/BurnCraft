//! PDF in and out, with `lopdf` (MIT) for the file structure; the drawing is read and written here.
//!
//! Import reads one page's vector drawing: paths (lines, curves, rectangles) through their transforms and
//! nested forms, each painted colour to the nearest layer, and embedded pictures (JPEG, or 8-bit RGB / grey /
//! CMYK). Text is left out with a note (convert it to outlines first). Decompression is capped, and nothing
//! outside the file is ever read. Export writes a one-page vector PDF at real size, with curves, layer
//! colours and pictures.

use crate::geom::*;
use crate::model::*;
use crate::vecio::{self, Seg};
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashMap;

/// One PDF point in mm (1/72 inch).
pub const PT_MM: f64 = 25.4 / 72.0;
/// The most a PDF may decompress to while being read (a guard against "zip bombs").
const MAX_BYTES: usize = 256 * 1024 * 1024;

fn load(data: &[u8]) -> Result<Document, String> {
    let opts = lopdf::LoadOptions { max_decompressed_size: Some(MAX_BYTES), ..Default::default() };
    Document::load_mem_with_options(data, opts).map_err(|e| match e {
        lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword => "This PDF is password-protected. Save an unprotected copy, then import that.".to_string(),
        e => format!("This PDF can't be read: {e}"),
    })
}

/// How many pages a PDF has.
pub fn page_count(data: &[u8]) -> Result<usize, String> {
    Ok(load(data)?.get_pages().len())
}

fn num(o: &Object) -> f64 {
    match o {
        Object::Integer(i) => *i as f64,
        Object::Real(r) => *r as f64,
        _ => 0.0,
    }
}

/// The page's visible box (MediaBox, looked up through its parents).
fn media_box(doc: &Document, page: ObjectId) -> BBox {
    let mut id = Some(page);
    let mut depth = 0;
    while let (Some(i), true) = (id, depth < 32) {
        let Ok(d) = doc.get_dictionary(i) else { break };
        if let Ok(a) = d.get(b"MediaBox").and_then(|o| doc.dereference(o).map(|(_, o)| o)).and_then(|o| o.as_array()) {
            if a.len() == 4 {
                let v: Vec<f64> = a.iter().map(|o| doc.dereference(o).map(|(_, o)| num(o)).unwrap_or(0.0)).collect();
                return BBox { min: p2(v[0].min(v[2]), v[1].min(v[3])), max: p2(v[0].max(v[2]), v[1].max(v[3])) };
            }
        }
        id = d.get(b"Parent").and_then(|o| o.as_reference()).ok();
        depth += 1;
    }
    BBox { min: p2(0.0, 0.0), max: p2(612.0, 792.0) }
}

#[derive(Clone)]
struct Gs {
    ctm: Xform,
    stroke: [u8; 3],
    fill: [u8; 3],
}

struct Reader<'a> {
    doc: &'a Document,
    /// PDF points → BurnCraft mm (page top-left to the bed's top-left)
    to_mm: Xform,
    groups: HashMap<[u8; 3], Vec<Seg>>,
    images: Vec<(BitmapData, f64, f64, Xform)>,
    text: usize,
    unread_images: usize,
    /// Drawing operations still allowed: Forms that draw other Forms many times over can multiply a small
    /// file into billions of operations, so reading stops here instead of freezing BurnCraft.
    ops_left: u64,
}

/// The most drawing operations one PDF page import runs, counting every repeat of a Form.
const MAX_OPS: u64 = 5_000_000;

fn colour(ops: &[Object]) -> [u8; 3] {
    let v: Vec<f64> = ops.iter().filter(|o| matches!(o, Object::Integer(_) | Object::Real(_))).map(num).collect();
    let c = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    match v.len() {
        1 => [c(v[0]), c(v[0]), c(v[0])],
        3 => [c(v[0]), c(v[1]), c(v[2])],
        4 => {
            let k = 1.0 - v[3];
            [c((1.0 - v[0]) * k), c((1.0 - v[1]) * k), c((1.0 - v[2]) * k)]
        }
        _ => [0, 0, 0],
    }
}

impl Reader<'_> {
    /// An XObject by name from a resources dictionary.
    fn xobject<'b>(&'b self, res: &'b Dictionary, name: &[u8]) -> Option<&'b Stream> {
        let xo = res.get(b"XObject").ok().and_then(|o| self.doc.dereference(o).ok()).and_then(|(_, o)| o.as_dict().ok())?;
        let o = xo.get(name).ok()?;
        self.doc.dereference(o).ok().and_then(|(_, o)| o.as_stream().ok())
    }

    fn run(&mut self, content: &[u8], res: &Dictionary, gs0: Gs, depth: usize) {
        if depth > 12 {
            return;
        }
        let Ok(content) = Content::decode(content) else { return };
        let mut gs = gs0;
        let mut stack: Vec<Gs> = Vec::new();
        let mut path: Vec<Seg> = Vec::new();
        let (mut cur, mut start) = (p2(0.0, 0.0), p2(0.0, 0.0));
        let mut in_text = false;
        let mut text_shown = false;
        for Operation { operator, operands: o } in &content.operations {
            if self.ops_left == 0 {
                return;
            }
            self.ops_left -= 1;
            let f = |k: usize| o.get(k).map(num).unwrap_or(0.0);
            let dev = |gs: &Gs, x: f64, y: f64| gs.ctm.apply(p2(x, y));
            match operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => {
                    if let Some(g) = stack.pop() {
                        gs = g;
                    }
                }
                "cm" => {
                    let m = Xform { a: f(0), b: f(1), c: f(2), d: f(3), e: f(4), f: f(5) };
                    gs.ctm = gs.ctm.then_after(m);
                }
                "m" => {
                    cur = dev(&gs, f(0), f(1));
                    start = cur;
                }
                "l" => {
                    let p = dev(&gs, f(0), f(1));
                    path.push(Seg::line(cur, p));
                    cur = p;
                }
                "c" => {
                    let (c0, c1, p) = (dev(&gs, f(0), f(1)), dev(&gs, f(2), f(3)), dev(&gs, f(4), f(5)));
                    path.push(Seg::curve(cur, c0, c1, p));
                    cur = p;
                }
                "v" => {
                    let (c1, p) = (dev(&gs, f(0), f(1)), dev(&gs, f(2), f(3)));
                    path.push(Seg::curve(cur, cur, c1, p));
                    cur = p;
                }
                "y" => {
                    let (c0, p) = (dev(&gs, f(0), f(1)), dev(&gs, f(2), f(3)));
                    path.push(Seg::curve(cur, c0, p, p));
                    cur = p;
                }
                "h" => {
                    if cur.dist(start) > 1e-9 {
                        path.push(Seg::line(cur, start));
                    }
                    cur = start;
                }
                "re" => {
                    let (x, y, w, h) = (f(0), f(1), f(2), f(3));
                    let c = [dev(&gs, x, y), dev(&gs, x + w, y), dev(&gs, x + w, y + h), dev(&gs, x, y + h)];
                    for k in 0..4 {
                        path.push(Seg::line(c[k], c[(k + 1) % 4]));
                    }
                    cur = c[0];
                    start = c[0];
                }
                // painting: strokes take the stroke colour, fills the fill colour
                "S" | "s" | "B" | "B*" | "b" | "b*" | "f" | "F" | "f*" => {
                    if matches!(operator.as_str(), "s" | "b" | "b*") && cur.dist(start) > 1e-9 {
                        path.push(Seg::line(cur, start));
                    }
                    let rgb = if matches!(operator.as_str(), "f" | "F" | "f*") { gs.fill } else { gs.stroke };
                    let to = self.to_mm;
                    self.groups.entry(rgb).or_default().extend(path.drain(..).map(|s| s.map(&|p| to.apply(p))));
                }
                "n" => path.clear(),
                "G" | "RG" | "K" | "SC" | "SCN" => gs.stroke = colour(o),
                "g" | "rg" | "k" | "sc" | "scn" => gs.fill = colour(o),
                "BT" => {
                    in_text = true;
                    text_shown = false;
                }
                "Tj" | "TJ" | "'" | "\"" if in_text && !text_shown => {
                    text_shown = true;
                    self.text += 1;
                }
                "ET" => in_text = false,
                "Do" => {
                    let Some(name) = o.first().and_then(|n| n.as_name().ok()) else { continue };
                    let Some(xo) = self.xobject(res, name) else { continue };
                    match xo.dict.get(b"Subtype").and_then(|s| s.as_name()) {
                        Ok(b"Form") => {
                            let mut g = gs.clone();
                            if let Ok(m) = xo.dict.get(b"Matrix").and_then(|m| m.as_array()) {
                                if m.len() == 6 {
                                    let v: Vec<f64> = m.iter().map(num).collect();
                                    g.ctm = g.ctm.then_after(Xform { a: v[0], b: v[1], c: v[2], d: v[3], e: v[4], f: v[5] });
                                }
                            }
                            let res2 = xo.dict.get(b"Resources").ok().and_then(|o| self.doc.dereference(o).ok()).and_then(|(_, o)| o.as_dict().ok()).unwrap_or(res);
                            if let Ok(bytes) = xo.decompressed_content_with_limit(MAX_BYTES) {
                                let res2 = res2.clone();
                                self.run(&bytes, &res2, g, depth + 1);
                            }
                        }
                        Ok(b"Image") => match self.image(xo) {
                            Some(bd) => {
                                // the picture fills the unit square of its transform; (0, 1) is its top-left
                                let m = |x: f64, y: f64| self.to_mm.apply(gs.ctm.apply(p2(x, y)));
                                let (tl, tr, bl) = (m(0.0, 1.0), m(1.0, 1.0), m(0.0, 0.0));
                                let (w, h) = (tl.dist(tr), tl.dist(bl));
                                if w > 1e-6 && h > 1e-6 {
                                    let ux = (tr - tl) * (1.0 / w);
                                    let uy = (tl - bl) * (1.0 / h);
                                    let c = (tr + bl) * 0.5;
                                    self.images.push((bd, w, h, Xform { a: ux.x, b: ux.y, c: uy.x, d: uy.y, e: c.x, f: c.y }));
                                }
                            }
                            None => self.unread_images += 1,
                        },
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// A picture's pixels: JPEG as it is, or 8-bit grey / RGB / CMYK samples.
    fn image(&self, s: &Stream) -> Option<BitmapData> {
        let d = &s.dict;
        let filters: Vec<Vec<u8>> = match d.get(b"Filter") {
            Ok(Object::Name(n)) => vec![n.clone()],
            Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok().map(|n| n.to_vec())).collect(),
            _ => Vec::new(),
        };
        if filters.last().map(|f| f.as_slice()) == Some(b"DCTDecode") {
            let bytes = if filters.len() == 1 { s.content.clone() } else { s.decompressed_content_with_limit(MAX_BYTES).ok()? };
            return crate::lbrn::decode_bitmap(bytes, "pdf image");
        }
        let w = d.get(b"Width").ok().map(num)? as u32;
        let h = d.get(b"Height").ok().map(num)? as u32;
        if d.get(b"BitsPerComponent").ok().map(num).unwrap_or(8.0) as u32 != 8 || w == 0 || h == 0 || w as u64 * h as u64 > 100_000_000 {
            return None;
        }
        let cs = d.get(b"ColorSpace").ok().and_then(|o| self.doc.dereference(o).ok()).map(|(_, o)| o);
        let comps = match cs {
            Some(Object::Name(n)) if n == b"DeviceGray" => 1,
            Some(Object::Name(n)) if n == b"DeviceRGB" => 3,
            Some(Object::Name(n)) if n == b"DeviceCMYK" => 4,
            Some(Object::Array(a)) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => a.get(1).and_then(|r| self.doc.dereference(r).ok()).and_then(|(_, o)| o.as_stream().ok()).and_then(|st| st.dict.get(b"N").ok().map(num)).unwrap_or(3.0) as usize,
            _ => return None,
        };
        let raw = if filters.is_empty() { s.content.clone() } else { s.decompressed_content_with_limit(MAX_BYTES).ok()? };
        let n = (w * h) as usize;
        if raw.len() < n * comps {
            return None;
        }
        let mut rgba = Vec::with_capacity(n * 4);
        for k in 0..n {
            let p = &raw[k * comps..k * comps + comps];
            let rgb = match comps {
                1 => [p[0], p[0], p[0]],
                3 => [p[0], p[1], p[2]],
                4 => {
                    let kk = 255 - p[3] as u32;
                    [((255 - p[0] as u32) * kk / 255) as u8, ((255 - p[1] as u32) * kk / 255) as u8, ((255 - p[2] as u32) * kk / 255) as u8]
                }
                _ => return None,
            };
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        let png = crate::platform::png_bytes(w, h, rgba)?;
        crate::lbrn::decode_bitmap(png, "pdf image")
    }
}

/// Read page `page` (1 = first) of a PDF. The page's top-left corner goes to `page_top` on the left edge.
pub fn import(data: &[u8], page: usize, page_top: f64) -> Result<crate::svg::Imported, String> {
    let doc = load(data)?;
    let pages = doc.get_pages();
    let Some(&page_id) = pages.get(&(page.max(1) as u32)) else { return Err(format!("This PDF has no page {page}.")) };
    let mb = media_box(&doc, page_id);
    let k = PT_MM;
    let to_mm = Xform { a: k, b: 0.0, c: 0.0, d: k, e: -mb.min.x * k, f: page_top - mb.max.y * k };
    let content = doc.get_page_content_with_limit(page_id, MAX_BYTES).map_err(|e| format!("This PDF page can't be read: {e}"))?;
    let (direct, ids) = doc.get_page_resources(page_id).map_err(|e| e.to_string())?;
    // the page's resources: the dictionary written in the page, or the first one it refers to
    let res: Dictionary = match direct {
        Some(d) => d.clone(),
        None => ids.first().and_then(|id| doc.get_dictionary(*id).ok()).cloned().unwrap_or_default(),
    };
    let mut r = Reader { doc: &doc, to_mm, groups: HashMap::new(), images: Vec::new(), text: 0, unread_images: 0, ops_left: MAX_OPS };
    r.run(&content, &res, Gs { ctm: Xform::IDENTITY, stroke: [0, 0, 0], fill: [0, 0, 0] }, 0);
    let mut out = crate::svg::Imported { paths: Vec::new(), images: std::mem::take(&mut r.images), skipped_text: r.text, on_layers: Vec::new(), notes: Vec::new() };
    let mut keys: Vec<[u8; 3]> = r.groups.keys().copied().collect();
    keys.sort();
    for rgb in keys {
        let pd = vecio::build_paths(&r.groups[&rgb], 0.001);
        if !pd.prims.is_empty() {
            out.paths.push((pd, rgb));
        }
    }
    if r.ops_left == 0 {
        out.notes.push(format!("this page repeats its drawing more than {MAX_OPS} times over, so only the first part came in"));
    }
    if r.unread_images > 0 {
        out.notes.push(format!("{} picture(s) in a format BurnCraft can't read were left out", r.unread_images));
    }
    if out.paths.is_empty() && !out.images.is_empty() {
        out.notes.push("this page has no drawn shapes, only pictures: they came in as pictures (use Trace Image to get outlines)".into());
    }
    if out.paths.is_empty() && out.images.is_empty() {
        return Err(if out.skipped_text > 0 { "This PDF page only has text. Convert the text to outlines (curves) in the program that made it, then import it again.".into() } else { "This PDF page has no shapes or pictures BurnCraft can use.".into() });
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------- export

/// The shapes as a one-page PDF. `page` (world mm) is the page; shapes keep their real size.
pub fn export(shapes: &[Shape], page: BBox) -> Result<Vec<u8>, String> {
    let k = 1.0 / PT_MM;
    let to = |p: P2| p2((p.x - page.min.x) * k, (p.y - page.min.y) * k);
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut ops: Vec<Operation> = vec![Operation::new("w", vec![(0.1 * k).into()]), Operation::new("J", vec![1.into()]), Operation::new("j", vec![1.into()])];
    let mut xobjects = Dictionary::new();
    for s in shapes {
        write_shape(&mut doc, &mut ops, &mut xobjects, s, Xform::IDENTITY, &to);
    }
    let content = Content { operations: ops };
    let mut stream = Stream::new(Dictionary::new(), content.encode().map_err(|e| e.to_string())?);
    let _ = stream.compress();
    let content_id = doc.add_object(stream);
    let mut resources = Dictionary::new();
    resources.set("XObject", Object::Dictionary(xobjects));
    let (w, h) = (page.width().max(1.0) * k, page.height().max(1.0) * k);
    let mut pg = Dictionary::new();
    pg.set("Type", Object::Name(b"Page".to_vec()));
    pg.set("Parent", Object::Reference(pages_id));
    pg.set("MediaBox", Object::Array(vec![0.into(), 0.into(), (w as f32).into(), (h as f32).into()]));
    pg.set("Contents", Object::Reference(content_id));
    pg.set("Resources", Object::Dictionary(resources));
    let page_id = doc.add_object(pg);
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    pages.set("Count", Object::Integer(1));
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let mut cat = Dictionary::new();
    cat.set("Type", Object::Name(b"Catalog".to_vec()));
    cat.set("Pages", Object::Reference(pages_id));
    let cat_id = doc.add_object(cat);
    doc.trailer.set("Root", Object::Reference(cat_id));
    let mut info = Dictionary::new();
    info.set("Producer", Object::string_literal("BurnCraft"));
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", Object::Reference(info_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn f(v: f64) -> Object {
    Object::Real(v as f32)
}

fn write_shape(doc: &mut Document, ops: &mut Vec<Operation>, xobjects: &mut Dictionary, s: &Shape, parent: Xform, to: &dyn Fn(P2) -> P2) {
    // the shape's own placement first, then its group's
    let x = parent.then_after(s.xform);
    match &s.kind {
        ShapeKind::Group(children) => {
            for c in children {
                write_shape(doc, ops, xobjects, c, x, to);
            }
        }
        ShapeKind::Bitmap { w, h, data, .. } => {
            let n = (data.width * data.height) as usize;
            if data.rgba.len() < n * 4 || n == 0 {
                return;
            }
            let mut rgb = Vec::with_capacity(n * 3);
            let mut alpha = Vec::with_capacity(n);
            for px in data.rgba.chunks_exact(4) {
                rgb.extend_from_slice(&px[..3]);
                alpha.push(px[3]);
            }
            let img = |bytes: Vec<u8>, cs: &str| {
                let mut d = Dictionary::new();
                d.set("Type", Object::Name(b"XObject".to_vec()));
                d.set("Subtype", Object::Name(b"Image".to_vec()));
                d.set("Width", Object::Integer(data.width as i64));
                d.set("Height", Object::Integer(data.height as i64));
                d.set("ColorSpace", Object::Name(cs.as_bytes().to_vec()));
                d.set("BitsPerComponent", Object::Integer(8));
                let mut st = Stream::new(d, bytes);
                let _ = st.compress();
                st
            };
            let mut main = img(rgb, "DeviceRGB");
            if alpha.iter().any(|a| *a < 255) {
                let mask_id = doc.add_object(img(alpha, "DeviceGray"));
                main.dict.set("SMask", Object::Reference(mask_id));
            }
            let id = doc.add_object(main);
            let name = format!("Im{}", xobjects.len() + 1);
            xobjects.set(name.as_bytes().to_vec(), Object::Reference(id));
            // the unit square onto the picture: (0,0) its bottom-left, (1,0) bottom-right, (0,1) top-left
            let (bl, br, tl) = (to(x.apply(p2(-w / 2.0, -h / 2.0))), to(x.apply(p2(w / 2.0, -h / 2.0))), to(x.apply(p2(-w / 2.0, h / 2.0))));
            let (ux, uy) = (br - bl, tl - bl);
            ops.push(Operation::new("q", vec![]));
            ops.push(Operation::new("cm", vec![f(ux.x), f(ux.y), f(uy.x), f(uy.y), f(bl.x), f(bl.y)]));
            ops.push(Operation::new("Do", vec![Object::Name(name.into_bytes())]));
            ops.push(Operation::new("Q", vec![]));
        }
        _ => {
            let [r, g, b] = layer_rgb(s.layer);
            ops.push(Operation::new("RG", vec![f(r as f64 / 255.0), f(g as f64 / 255.0), f(b as f64 / 255.0)]));
            let mut any = false;
            if let ShapeKind::Path(pd) = &s.kind {
                // keep the curves
                let mut last_b: Option<usize> = None;
                let mut start: Option<usize> = None;
                for pr in &pd.prims {
                    if pr.a >= pd.verts.len() || pr.b >= pd.verts.len() {
                        continue;
                    }
                    if last_b != Some(pr.a) {
                        let p = to(x.apply(pd.verts[pr.a].p));
                        ops.push(Operation::new("m", vec![f(p.x), f(p.y)]));
                        start = Some(pr.a);
                    }
                    let (va, vb) = (pd.verts[pr.a], pd.verts[pr.b]);
                    let p = to(x.apply(vb.p));
                    match pr.kind {
                        PrimKind::Line => ops.push(Operation::new("l", vec![f(p.x), f(p.y)])),
                        PrimKind::Bezier => {
                            let (c0, c1) = (to(x.apply(va.c0.unwrap_or(va.p))), to(x.apply(vb.c1.unwrap_or(vb.p))));
                            ops.push(Operation::new("c", vec![f(c0.x), f(c0.y), f(c1.x), f(c1.y), f(p.x), f(p.y)]));
                        }
                    }
                    if Some(pr.b) == start {
                        ops.push(Operation::new("h", vec![]));
                    }
                    last_b = Some(pr.b);
                    any = true;
                }
            } else {
                let shape = Shape { xform: x, ..s.clone() };
                for l in shape.world_outline(0.01) {
                    for (i, p) in l.iter().enumerate() {
                        let q = to(*p);
                        ops.push(Operation::new(if i == 0 { "m" } else { "l" }, vec![f(q.x), f(q.y)]));
                    }
                    if is_closed(&l) {
                        ops.push(Operation::new("h", vec![]));
                    }
                    any = true;
                }
            }
            if any {
                ops.push(Operation::new("S", vec![]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(im: &crate::svg::Imported) -> BBox {
        let mut b = BBox::EMPTY;
        for (pd, _) in &im.paths {
            for p in pd.flatten(0.001).into_iter().flatten() {
                b.add(p);
            }
        }
        b
    }

    #[test]
    fn exported_pdf_reads_back_the_same_with_curves_colours_and_pictures() {
        let mut shapes = vec![Shape::new(ShapeKind::Rect { w: 30.0, h: 20.0, cr: 0.0 }, Xform::translate(40.0, 50.0), 1)];
        shapes.push(Shape::new(ShapeKind::Ellipse { rx: 6.0, ry: 6.0 }, Xform::translate(90.0, 50.0), 2));
        let mut pd = PathData::default();
        crate::sketch::append_path(&mut pd, &crate::sketch::circle_path(p2(0.0, 0.0), 5.0));
        shapes.push(Shape::new(ShapeKind::Path(pd), Xform::translate(120.0, 50.0), 2));
        // a 2 × 1 pixel picture, 10 × 5 mm
        let png = crate::platform::png_bytes(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
        let bd = crate::lbrn::decode_bitmap(png, "t").unwrap();
        shapes.push(Shape::new(ShapeKind::Bitmap { w: 10.0, h: 5.0, data: bd, adjust: Default::default() }, Xform::translate(150.0, 50.0), 3));
        let page = BBox { min: p2(0.0, 0.0), max: p2(200.0, 100.0) };
        let bytes = export(&shapes, page).unwrap();
        assert_eq!(page_count(&bytes).unwrap(), 1);
        let im = import(&bytes, 1, 100.0).unwrap();
        assert_eq!(im.paths.len(), 2, "one per layer colour");
        assert!(im.paths.iter().any(|(_, c)| *c == layer_rgb(1)) && im.paths.iter().any(|(_, c)| *c == layer_rgb(2)));
        assert!(im.paths.iter().any(|(pd, _)| pd.prims.iter().any(|p| p.kind == PrimKind::Bezier)), "curves stay curves");
        let b = bounds(&im);
        let want = shapes[..3].iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        assert!(b.min.dist(want.min) < 0.05 && b.max.dist(want.max) < 0.05, "{b:?} vs {want:?}");
        assert_eq!(im.images.len(), 1);
        let (bd, w, h, x) = &im.images[0];
        assert_eq!((bd.width, bd.height), (2, 1));
        assert!((w - 10.0).abs() < 1e-3 && (h - 5.0).abs() < 1e-3);
        assert!(x.apply(p2(0.0, 0.0)).dist(p2(150.0, 50.0)) < 1e-3);
        assert!(bd.rgba[0] < 10 && bd.rgba[4] > 245, "the pixels come back");
    }

    #[test]
    fn transforms_forms_text_and_rejects() {
        // a hand-made page: a red square drawn through a scale transform, a blue line inside a form moved by
        // its matrix, and some text
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let form_ops = Content { operations: vec![Operation::new("0 0 1 RG", vec![]), Operation::new("m", vec![0.into(), 0.into()]), Operation::new("l", vec![72.into(), 0.into()]), Operation::new("S", vec![])] };
        let mut fd = Dictionary::new();
        fd.set("Type", Object::Name(b"XObject".to_vec()));
        fd.set("Subtype", Object::Name(b"Form".to_vec()));
        fd.set("BBox", Object::Array(vec![0.into(), 0.into(), 100.into(), 100.into()]));
        fd.set("Matrix", Object::Array(vec![1.into(), 0.into(), 0.into(), 1.into(), 72.into(), 144.into()]));
        // RG needs its numbers as operands
        let form_ops = Content { operations: vec![Operation::new("RG", vec![0.into(), 0.into(), 1.into()]), form_ops.operations[1].clone(), form_ops.operations[2].clone(), form_ops.operations[3].clone()] };
        let form_id = doc.add_object(Stream::new(fd, form_ops.encode().unwrap()));
        let ops = vec![
            Operation::new("q", vec![]),
            Operation::new("cm", vec![2.into(), 0.into(), 0.into(), 2.into(), 0.into(), 0.into()]),
            Operation::new("RG", vec![1.into(), 0.into(), 0.into()]),
            Operation::new("re", vec![36.into(), 36.into(), 36.into(), 36.into()]),
            Operation::new("S", vec![]),
            Operation::new("Q", vec![]),
            Operation::new("Do", vec![Object::Name(b"F1".to_vec())]),
            Operation::new("BT", vec![]),
            Operation::new("Tj", vec![Object::string_literal("hi")]),
            Operation::new("ET", vec![]),
        ];
        let content_id = doc.add_object(Stream::new(Dictionary::new(), Content { operations: ops }.encode().unwrap()));
        let mut xo = Dictionary::new();
        xo.set("F1", Object::Reference(form_id));
        let mut res = Dictionary::new();
        res.set("XObject", Object::Dictionary(xo));
        let mut pg = Dictionary::new();
        pg.set("Type", Object::Name(b"Page".to_vec()));
        pg.set("Parent", Object::Reference(pages_id));
        pg.set("MediaBox", Object::Array(vec![0.into(), 0.into(), 288.into(), 288.into()]));
        pg.set("Contents", Object::Reference(content_id));
        pg.set("Resources", Object::Dictionary(res));
        let page_id = doc.add_object(pg);
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages.set("Count", Object::Integer(1));
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let mut cat = Dictionary::new();
        cat.set("Type", Object::Name(b"Catalog".to_vec()));
        cat.set("Pages", Object::Reference(pages_id));
        let cat_id = doc.add_object(cat);
        doc.trailer.set("Root", Object::Reference(cat_id));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        // the page is 4 × 4 in (101.6 mm); put its top at 101.6 so page and bed line up
        let im = import(&bytes, 1, 101.6).unwrap();
        assert_eq!(im.skipped_text, 1);
        let red = im.paths.iter().find(|(_, c)| *c == [255, 0, 0]).unwrap();
        let mut b = BBox::EMPTY;
        for p in red.0.flatten(0.01).into_iter().flatten() {
            b.add(p);
        }
        // a 36 pt square at (36, 36), doubled: 72 pt = 1 in = 25.4 mm wide, from 1 in to 2 in
        assert!((b.min.x - 25.4).abs() < 1e-3 && (b.width() - 25.4).abs() < 1e-3 && (b.min.y - 25.4).abs() < 1e-3, "{b:?}");
        let blue = im.paths.iter().find(|(_, c)| *c == [0, 0, 255]).unwrap();
        let l = &blue.0.flatten(0.01)[0];
        assert!(l[0].dist(p2(25.4, 50.8)) < 1e-3 && l[1].dist(p2(50.8, 50.8)) < 1e-3, "{l:?}");
        assert!(import(b"not a pdf", 1, 0.0).is_err());
        assert!(import(&bytes, 2, 0.0).is_err());
    }
}
