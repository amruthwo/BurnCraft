//! SVG in and out: import and paste with `usvg` (sizes and units, viewBox, transforms, every shape kind,
//! colours and embedded pictures), and copy as SVG text for other programs.
//!
//! SVG measures in pixels at 96 per inch and its Y runs down; BurnCraft uses mm with Y up. On the way in, the
//! page's top-left corner goes to the bed's top-left; on the way out, the given frame becomes the page.

use crate::geom::*;
use crate::model::*;

/// One SVG pixel in mm (SVG's 96 pixels per inch).
pub const PX_MM: f64 = 25.4 / 96.0;

/// What an SVG brought in.
pub struct Imported {
    /// Outlines, one path per colour (curves kept), with the RGB colour they were drawn in.
    pub paths: Vec<(PathData, [u8; 3])>,
    /// Embedded pictures: bitmap, width and height in mm, and where it sits (local → world).
    pub images: Vec<(BitmapData, f64, f64, Xform)>,
    /// Text that wasn't converted to outlines (it can't be read without fonts).
    pub skipped_text: usize,
    /// Outlines that say which BurnCraft layer they belong on (a DXF layer named C00…C29, T1, T2).
    pub on_layers: Vec<(PathData, usize)>,
    /// Other things left out or assumed, to tell the user (one line each).
    pub notes: Vec<String>,
}

/// Read an SVG document. `page_top` is where the page's top edge goes, in mm (the bed's height on import).
pub fn import(data: &[u8], page_top: f64) -> Result<Imported, String> {
    let mut opt = usvg::Options::default();
    // only pictures embedded in the SVG (data: URIs) come in: an `href` naming a file is ignored, so an SVG
    // from elsewhere can't pull files from this computer into the design
    opt.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = usvg::Tree::from_data(data, &opt).or_else(|e| {
        // pasted SVG without the namespace is still SVG
        let text = std::str::from_utf8(data).map_err(|_| e.to_string())?;
        if text.contains("xmlns=") {
            return Err(e.to_string());
        }
        usvg::Tree::from_str(&text.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1), &opt).map_err(|e| e.to_string())
    });
    let tree = tree.map_err(|e| format!("This SVG can't be read: {e}"))?;
    // text is left out (it needs fonts to become outlines): count it in the file to say so
    let src = String::from_utf8_lossy(data);
    let skipped_text = src.match_indices("<text").filter(|(i, _)| src[i + 5..].starts_with(|c: char| c == '>' || c.is_whitespace())).count();
    let mut out = Imported { paths: Vec::new(), images: Vec::new(), skipped_text, on_layers: Vec::new(), notes: Vec::new() };
    let world = |t: &usvg::Transform, x: f32, y: f32| -> P2 {
        let px = t.sx as f64 * x as f64 + t.kx as f64 * y as f64 + t.tx as f64;
        let py = t.ky as f64 * x as f64 + t.sy as f64 * y as f64 + t.ty as f64;
        p2(px * PX_MM, page_top - py * PX_MM)
    };
    let mut by_colour: Vec<([u8; 3], PathData)> = Vec::new();
    walk(tree.root(), &mut |node| match node {
        usvg::Node::Path(p) => {
            if !p.is_visible() {
                return;
            }
            // the outline's colour: its stroke, or else its fill
            let paint = p.stroke().map(|s| s.paint()).or_else(|| p.fill().map(|f| f.paint()));
            let rgb = match paint {
                Some(usvg::Paint::Color(c)) => [c.red, c.green, c.blue],
                _ => [0, 0, 0],
            };
            let t = p.abs_transform();
            let pd = match by_colour.iter_mut().find(|(c, _)| *c == rgb) {
                Some((_, pd)) => pd,
                None => {
                    by_colour.push((rgb, PathData::default()));
                    &mut by_colour.last_mut().unwrap().1
                }
            };
            append_segments(pd, p.data(), &|x, y| world(&t, x, y));
        }
        usvg::Node::Image(im) => {
            if !im.is_visible() {
                return;
            }
            let bytes = match im.kind() {
                usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) | usvg::ImageKind::GIF(b) | usvg::ImageKind::WEBP(b) => b.as_ref().clone(),
                usvg::ImageKind::SVG(_) => return,
            };
            let Some(bd) = crate::lbrn::decode_bitmap(bytes, "svg image") else { return };
            let t = im.abs_transform();
            let s = im.size();
            let (tl, tr, bl) = (world(&t, 0.0, 0.0), world(&t, s.width(), 0.0), world(&t, 0.0, s.height()));
            let (w, h) = (tl.dist(tr), tl.dist(bl));
            if w < 1e-6 || h < 1e-6 {
                return;
            }
            // the bitmap's own frame: x along its top edge, y up its left side, centred
            let ux = (tr - tl) * (1.0 / w);
            let uy = (tl - bl) * (1.0 / h);
            let c = (tr + bl) * 0.5;
            out.images.push((bd, w, h, Xform { a: ux.x, b: ux.y, c: uy.x, d: uy.y, e: c.x, f: c.y }));
        }
        usvg::Node::Text(_) => {}
        usvg::Node::Group(_) => {}
    });
    out.paths = by_colour.into_iter().filter(|(_, pd)| !pd.prims.is_empty()).map(|(c, pd)| (pd, c)).collect();
    if out.paths.is_empty() && out.images.is_empty() {
        return Err(if out.skipped_text > 0 { "This SVG only has text. Convert the text to paths (outlines) in your drawing program and save it again.".into() } else { "This SVG has no shapes BurnCraft can use.".into() });
    }
    Ok(out)
}

fn walk(g: &usvg::Group, f: &mut dyn FnMut(&usvg::Node)) {
    for n in g.children() {
        f(n);
        if let usvg::Node::Group(c) = n {
            walk(c, f);
        }
    }
}

/// Add a usvg path's segments to `pd`, keeping curves (quadratics become cubics).
fn append_segments(pd: &mut PathData, path: &usvg::tiny_skia_path::Path, map: &dyn Fn(f32, f32) -> P2) {
    use usvg::tiny_skia_path::PathSegment as S;
    let mut start: Option<usize> = None;
    let mut last: Option<usize> = None;
    let add = |pd: &mut PathData, p: P2| -> usize {
        pd.verts.push(Vert::new(p));
        pd.verts.len() - 1
    };
    for seg in path.segments() {
        match seg {
            S::MoveTo(p) => {
                let i = add(pd, map(p.x, p.y));
                start = Some(i);
                last = Some(i);
            }
            S::LineTo(p) => {
                let Some(a) = last else { continue };
                let b = add(pd, map(p.x, p.y));
                pd.prims.push(Prim { kind: PrimKind::Line, a, b });
                last = Some(b);
            }
            S::QuadTo(q, p) => {
                let Some(a) = last else { continue };
                let (p0, q, p1) = (pd.verts[a].p, map(q.x, q.y), map(p.x, p.y));
                let c0 = p0 + (q - p0) * (2.0 / 3.0);
                let c1 = p1 + (q - p1) * (2.0 / 3.0);
                let b = add(pd, p1);
                pd.verts[a].c0 = Some(c0);
                pd.verts[b].c1 = Some(c1);
                pd.prims.push(Prim { kind: PrimKind::Bezier, a, b });
                last = Some(b);
            }
            S::CubicTo(c0, c1, p) => {
                let Some(a) = last else { continue };
                let b = add(pd, map(p.x, p.y));
                pd.verts[a].c0 = Some(map(c0.x, c0.y));
                pd.verts[b].c1 = Some(map(c1.x, c1.y));
                pd.prims.push(Prim { kind: PrimKind::Bezier, a, b });
                last = Some(b);
            }
            S::Close => {
                // join back to the start: the last point becomes the start point (so the outline is one loop)
                if let (Some(s), Some(l)) = (start, last) {
                    if l != s {
                        if pd.verts[l].p.dist(pd.verts[s].p) < 1e-9 && pd.verts[l].c0.is_none() {
                            // already back at the start: reuse the start vertex
                            if let Some(pr) = pd.prims.iter_mut().rev().find(|pr| pr.b == l) {
                                pr.b = s;
                                pd.verts[s].c1 = pd.verts[l].c1;
                            }
                        } else {
                            pd.prims.push(Prim { kind: PrimKind::Line, a: l, b: s });
                        }
                    }
                }
                last = start;
            }
        }
    }
}

/// The layer whose colour is closest to `rgb` (BurnCraft's colour layers).
pub fn nearest_layer(rgb: [u8; 3]) -> usize {
    let d = |a: [u8; 3]| (0..3).map(|k| (a[k] as i32 - rgb[k] as i32).pow(2)).sum::<i32>();
    (0..COLOR_LAYERS).min_by_key(|&l| d(layer_rgb(l))).unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------- export

/// The shapes as an SVG document (mm), each in its layer's colour. `frame` (world mm) becomes the page.
pub fn export(shapes: &[Shape], frame: BBox) -> String {
    let (w, h) = (frame.width().max(0.01), frame.height().max(0.01));
    let to = |p: P2| p2(p.x - frame.min.x, frame.max.y - p.y);
    let mut body = String::new();
    for s in shapes {
        write_shape(&mut body, s, Xform::IDENTITY, &to);
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {} -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">\n{}</svg>\n",
        crate::platform::CLIP_MARKER,
        n(w),
        n(h),
        n(w),
        n(h),
        body
    )
}

fn n(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn hex(layer: usize) -> String {
    let [r, g, b] = layer_rgb(layer);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn write_shape(out: &mut String, s: &Shape, parent: Xform, to: &dyn Fn(P2) -> P2) {
    // the shape's own placement first, then its group's (as Shape::world_outline does)
    let x = parent.then_after(s.xform);
    match &s.kind {
        ShapeKind::Group(children) => {
            for c in children {
                write_shape(out, c, x, to);
            }
        }
        ShapeKind::Bitmap { w, h, data, .. } => {
            use base64::Engine;
            let bytes: Vec<u8> = if data.encoded.is_empty() { crate::platform::png_bytes(data.width, data.height, data.rgba.as_ref().clone()).unwrap_or_default() } else { data.encoded.as_ref().clone() };
            if bytes.is_empty() {
                return;
            }
            let mime = if bytes.starts_with(&[0xFF, 0xD8]) { "image/jpeg" } else { "image/png" };
            // the picture's top-left, top-right and bottom-left corners on the page
            let (tl, tr, bl) = (to(x.apply(p2(-w / 2.0, h / 2.0))), to(x.apply(p2(w / 2.0, h / 2.0))), to(x.apply(p2(-w / 2.0, -h / 2.0))));
            let (ux, uy) = ((tr - tl) * (1.0 / w), (bl - tl) * (1.0 / h));
            out.push_str(&format!(
                "<image width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {})\" xlink:href=\"data:{mime};base64,{}\"/>\n",
                n(*w),
                n(*h),
                n(ux.x),
                n(ux.y),
                n(uy.x),
                n(uy.y),
                n(tl.x),
                n(tl.y),
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ));
        }
        ShapeKind::Path(pd) => {
            let d = path_d(pd, &|p| to(x.apply(p)));
            if !d.is_empty() {
                out.push_str(&format!("<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.1\"/>\n", hex(s.layer)));
            }
        }
        _ => {
            // everything else as its outline
            let shape = Shape { xform: x, ..s.clone() };
            let mut d = String::new();
            for l in shape.world_outline(0.02) {
                for (i, p) in l.iter().enumerate() {
                    let q = to(*p);
                    d.push_str(&format!("{}{} {} ", if i == 0 { "M" } else { "L" }, n(q.x), n(q.y)));
                }
                if is_closed(&l) {
                    d.push_str("Z ");
                }
            }
            if !d.is_empty() {
                out.push_str(&format!("<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.1\"/>\n", d.trim_end(), hex(s.layer)));
            }
        }
    }
}

/// SVG path data for a path, keeping its curves.
fn path_d(pd: &PathData, map: &dyn Fn(P2) -> P2) -> String {
    let mut d = String::new();
    let mut last_b: Option<usize> = None;
    let mut start: Option<usize> = None;
    for pr in &pd.prims {
        if pr.a >= pd.verts.len() || pr.b >= pd.verts.len() {
            continue;
        }
        if last_b != Some(pr.a) {
            let p = map(pd.verts[pr.a].p);
            d.push_str(&format!("M{} {} ", n(p.x), n(p.y)));
            start = Some(pr.a);
        }
        let (va, vb) = (pd.verts[pr.a], pd.verts[pr.b]);
        let p = map(vb.p);
        match pr.kind {
            PrimKind::Line => d.push_str(&format!("L{} {} ", n(p.x), n(p.y))),
            PrimKind::Bezier => {
                let (c0, c1) = (map(va.c0.unwrap_or(va.p)), map(vb.c1.unwrap_or(vb.p)));
                d.push_str(&format!("C{} {} {} {} {} {} ", n(c0.x), n(c0.y), n(c1.x), n(c1.y), n(p.x), n(p.y)));
            }
        }
        if Some(pr.b) == start {
            d.push_str("Z ");
        }
        last_b = Some(pr.b);
    }
    d.trim_end().to_string()
}

/// The shapes from an SVG made by BurnCraft's own Copy (so a paste can use the exact shapes it remembers).
pub fn is_ours(text: &str) -> bool {
    text.contains(crate::platform::CLIP_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_viewbox_transforms_shapes_and_colours() {
        // a 100 × 50 mm page; a red 20 × 10 mm rectangle at (10, 5) from the top-left, a blue circle moved by a
        // transform, and a quadratic curve
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
            <rect x="10" y="5" width="20" height="10" fill="none" stroke="#ff0000"/>
            <g transform="translate(60 25)"><circle r="5" fill="#0000ff"/></g>
            <path d="M0 50 Q 50 0 100 50" stroke="#000" fill="none"/>
            <text x="5" y="45">hello</text>
        </svg>"##;
        let im = import(svg.as_bytes(), 50.0).unwrap();
        assert_eq!(im.paths.len(), 3, "one path per colour");
        assert_eq!(im.skipped_text, 1);
        let bb = |pd: &PathData| {
            let mut b = BBox::EMPTY;
            for l in pd.flatten(0.01) {
                for p in l {
                    b.add(p);
                }
            }
            b
        };
        let red = &im.paths.iter().find(|(_, c)| *c == [255, 0, 0]).unwrap().0;
        let b = bb(red);
        // 10 mm from the left, top 5 mm below the page top (y up: 45), 20 × 10 mm
        assert!((b.min.x - 10.0).abs() < 1e-3 && (b.max.y - 45.0).abs() < 1e-3, "{b:?}");
        assert!((b.width() - 20.0).abs() < 1e-3 && (b.height() - 10.0).abs() < 1e-3);
        let flat = red.flatten(0.01);
        assert_eq!(flat.len(), 1);
        assert!(is_closed(&flat[0]), "the rectangle is one closed outline");
        let blue = bb(&im.paths.iter().find(|(_, c)| *c == [0, 0, 255]).unwrap().0);
        assert!((blue.center().x - 60.0).abs() < 1e-3 && (blue.center().y - 25.0).abs() < 1e-3 && (blue.width() - 10.0).abs() < 1e-2);
        // curves stay curves
        assert!(im.paths.iter().any(|(pd, _)| pd.prims.iter().any(|p| p.kind == PrimKind::Bezier)));
        assert_eq!(nearest_layer([255, 0, 0]), (0..COLOR_LAYERS).find(|&l| layer_rgb(l) == [255, 0, 0]).unwrap_or(nearest_layer([255, 0, 0])));
        // pixels without units: 96 per inch
        let px = import(br#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="48"/></svg>"#, 25.4).unwrap();
        let b = bb(&px.paths[0].0);
        assert!((b.width() - 25.4).abs() < 1e-3 && (b.height() - 12.7).abs() < 1e-3, "{b:?}");
        assert!(import(b"not svg", 0.0).is_err());
        assert!(import(br#"<svg xmlns="http://www.w3.org/2000/svg"><text>only text</text></svg>"#, 0.0).is_err());
    }

    #[test]
    fn copied_svg_reads_back_the_same() {
        let mut shapes = vec![Shape::new(ShapeKind::Rect { w: 20.0, h: 10.0, cr: 0.0 }, Xform::translate(30.0, 40.0), 1)];
        let mut pd = PathData::default();
        crate::sketch::append_path(&mut pd, &crate::sketch::circle_path(p2(0.0, 0.0), 5.0));
        shapes.push(Shape::new(ShapeKind::Path(pd), Xform::translate(60.0, 40.0), 2));
        let frame = shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        let text = export(&shapes, frame);
        assert!(is_ours(&text) && crate::app::is_svg_text(&text));
        let back = import(text.as_bytes(), frame.height()).unwrap();
        assert_eq!(back.paths.len(), 2);
        let mut b = BBox::EMPTY;
        for (pd, _) in &back.paths {
            for p in pd.flatten(0.01).into_iter().flatten() {
                b.add(p);
            }
        }
        // same size, and the same place relative to the frame's bottom-left
        assert!((b.width() - frame.width()).abs() < 0.01 && (b.height() - frame.height()).abs() < 0.01, "{b:?} vs {frame:?}");
        assert!(b.min.x.abs() < 0.01 && b.min.y.abs() < 0.01);
        // each shape on its layer's colour
        assert!(back.paths.iter().any(|(_, c)| *c == layer_rgb(1)) && back.paths.iter().any(|(_, c)| *c == layer_rgb(2)));
    }

    #[test]
    fn pictures_only_come_from_inside_the_svg() {
        use base64::Engine;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        // a real picture file on this computer, named by an SVG from elsewhere: it must not be read
        let file = std::env::temp_dir().join(format!("burncraft-svg-href-{}.png", std::process::id()));
        std::fs::write(&file, &png).unwrap();
        let linked = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10mm" height="10mm" viewBox="0 0 10 10"><rect width="5" height="5" stroke="#000" fill="none"/><image href="{0}" width="4" height="4"/><image xlink:href="{0}" width="4" height="4"/></svg>"##,
            file.display()
        );
        let im = import(linked.as_bytes(), 10.0);
        let _ = std::fs::remove_file(&file);
        let im = im.unwrap();
        assert!(im.images.is_empty(), "a picture named by its file path must not come in");
        assert_eq!(im.paths.len(), 1, "the rest of the drawing still comes in");
        // the same picture embedded in the SVG does come in
        let embedded = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><image href="data:image/png;base64,{}" width="4" height="4"/></svg>"#,
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        assert_eq!(import(embedded.as_bytes(), 10.0).unwrap().images.len(), 1);
    }
}
