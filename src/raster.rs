//! Drawing shapes into a picture (with `tiny-skia`): for PNG export and Copy as Picture. Outlines are drawn
//! in their layer colours, shapes on fill layers are filled, and pictures are drawn as they are.

use crate::geom::*;
use crate::model::*;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

/// The largest picture made, in pixels per side.
pub const MAX_SIDE: u32 = 16000;

/// Draw `shapes` with `frame` (world mm) as the picture, at `dpi`. `fill(layer)` says which layers are filled.
/// Returns width, height and RGBA pixels (not premultiplied).
pub fn render(shapes: &[Shape], frame: BBox, dpi: f64, white: bool, fill: &dyn Fn(usize) -> bool) -> Result<(u32, u32, Vec<u8>), String> {
    if frame.is_empty() {
        return Err("Nothing to draw.".into());
    }
    let s = dpi / 25.4; // pixels per mm
    let (w, h) = ((frame.width() * s).ceil().max(1.0), (frame.height() * s).ceil().max(1.0));
    if w > MAX_SIDE as f64 || h > MAX_SIDE as f64 {
        return Err(format!("That would be a {} × {} pixel picture: lower the DPI.", w as u64, h as u64));
    }
    let mut pm = Pixmap::new(w as u32, h as u32).ok_or("The picture couldn't be made.")?;
    if white {
        pm.fill(Color::WHITE);
    }
    let to = |p: P2| p2((p.x - frame.min.x) * s, (frame.max.y - p.y) * s);
    let line_w = (0.2 * s).max(1.0) as f32;
    for sh in shapes {
        draw(&mut pm, sh, Xform::IDENTITY, &to, s, line_w, fill);
    }
    // back to straight (not premultiplied) colours
    let mut rgba = pm.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok((w as u32, h as u32, rgba))
}

fn draw(pm: &mut Pixmap, s: &Shape, parent: Xform, to: &dyn Fn(P2) -> P2, scale: f64, line_w: f32, fill: &dyn Fn(usize) -> bool) {
    let x = parent.then_after(s.xform);
    match &s.kind {
        ShapeKind::Group(ch) => {
            for c in ch {
                draw(pm, c, x, to, scale, line_w, fill);
            }
        }
        ShapeKind::Bitmap { w, h, data, .. } => {
            let (iw, ih) = (data.width, data.height);
            if iw == 0 || ih == 0 || data.rgba.len() < (iw * ih * 4) as usize {
                return;
            }
            let mut px = data.rgba.as_ref().clone();
            for p in px.chunks_exact_mut(4) {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = (*c as u32 * a / 255) as u8;
                }
            }
            let Some(img) = tiny_skia::IntSize::from_wh(iw, ih).and_then(|sz| Pixmap::from_vec(px, sz)) else { return };
            // picture pixel (u, v) → its place on the page
            let at = |u: f64, v: f64| to(x.apply(p2(-w / 2.0 + u * w / iw as f64, h / 2.0 - v * h / ih as f64)));
            let (o, ux, vy) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
            let t = Transform::from_row((ux.x - o.x) as f32, (ux.y - o.y) as f32, (vy.x - o.x) as f32, (vy.y - o.y) as f32, o.x as f32, o.y as f32);
            pm.draw_pixmap(0, 0, img.as_ref(), &PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() }, t, None);
        }
        _ => {
            let mut pb = PathBuilder::new();
            let mut any = false;
            if let ShapeKind::Path(pd) = &s.kind {
                let mut last_b: Option<usize> = None;
                let mut start: Option<usize> = None;
                for pr in &pd.prims {
                    if pr.a >= pd.verts.len() || pr.b >= pd.verts.len() {
                        continue;
                    }
                    if last_b != Some(pr.a) {
                        let p = to(x.apply(pd.verts[pr.a].p));
                        pb.move_to(p.x as f32, p.y as f32);
                        start = Some(pr.a);
                    }
                    let (va, vb) = (pd.verts[pr.a], pd.verts[pr.b]);
                    let p = to(x.apply(vb.p));
                    match pr.kind {
                        PrimKind::Line => pb.line_to(p.x as f32, p.y as f32),
                        PrimKind::Bezier => {
                            let (c0, c1) = (to(x.apply(va.c0.unwrap_or(va.p))), to(x.apply(vb.c1.unwrap_or(vb.p))));
                            pb.cubic_to(c0.x as f32, c0.y as f32, c1.x as f32, c1.y as f32, p.x as f32, p.y as f32);
                        }
                    }
                    if Some(pr.b) == start {
                        pb.close();
                    }
                    last_b = Some(pr.b);
                    any = true;
                }
            } else {
                let shape = Shape { xform: x, ..s.clone() };
                for l in shape.world_outline((0.5 / scale).max(0.005)) {
                    for (i, p) in l.iter().enumerate() {
                        let q = to(*p);
                        if i == 0 {
                            pb.move_to(q.x as f32, q.y as f32);
                        } else {
                            pb.line_to(q.x as f32, q.y as f32);
                        }
                    }
                    if is_closed(&l) {
                        pb.close();
                    }
                    any = true;
                }
            }
            let Some(path) = pb.finish().filter(|_| any) else { return };
            let [r, g, b] = layer_rgb(s.layer);
            let mut paint = Paint::default();
            paint.set_color_rgba8(r, g, b, 255);
            paint.anti_alias = true;
            if fill(s.layer) {
                pm.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
            }
            pm.stroke_path(&path, &paint, &Stroke { width: line_w, ..Default::default() }, Transform::identity(), None);
        }
    }
}

/// A short fingerprint of a picture (size and a sample of its colours), to recognise our own on the
/// clipboard even if the system changed it slightly on the way.
pub fn fingerprint(w: usize, h: usize, rgba: &[u8]) -> u64 {
    let mut hsh: u64 = (w as u64) << 32 ^ h as u64;
    let n = w * h;
    let step = (n / 997).max(1);
    let mut i = 0;
    while i < n && i * 4 + 2 < rgba.len() {
        for c in 0..3 {
            // coarse, so small colour changes on the way don't matter
            hsh = hsh.wrapping_mul(1_099_511_628_211) ^ (rgba[i * 4 + c] >> 4) as u64;
        }
        i += step;
    }
    hsh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_outlines_fills_and_pictures_in_place() {
        let shapes = vec![
            // a red-layer outline square 10 × 10 mm at (5, 5)..(15, 15)
            Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(10.0, 10.0), 1),
            // a filled square on layer 2 at (25..35, 5..15)
            Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(30.0, 10.0), 2),
        ];
        let frame = BBox { min: p2(0.0, 0.0), max: p2(40.0, 20.0) };
        let (w, h, px) = render(&shapes, frame, 25.4, true, &|l| l == 2).unwrap();
        assert_eq!((w, h), (40, 20), "1 pixel per mm at 25.4 DPI");
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 3) as usize];
        assert_eq!(at(10, 10), [255, 255, 255], "inside the outline-only square: white");
        assert_eq!(at(1, 1), [255, 255, 255]);
        assert_eq!(at(30, 10), layer_rgb(2), "inside the filled square: its colour");
        let edge = at(5, 10);
        assert!(edge != [255, 255, 255], "the outline is drawn");
        // transparent background
        let (_, _, px) = render(&shapes, frame, 25.4, false, &|_| false).unwrap();
        assert_eq!(px[3], 0);
        assert!(render(&shapes, frame, 100000.0, true, &|_| false).is_err(), "too big");
        assert_eq!(fingerprint(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255]), fingerprint(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255]));
    }
}
