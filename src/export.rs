//! Export: the design (or the selection) as SVG, DXF, PDF or PNG.

use crate::geom::*;
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Svg,
    Dxf,
    Pdf,
    Png,
    Gcode,
}

impl Format {
    pub const ALL: [Format; 5] = [Format::Svg, Format::Dxf, Format::Pdf, Format::Png, Format::Gcode];
    pub fn label(self) -> &'static str {
        match self {
            Format::Svg => "SVG",
            Format::Dxf => "DXF",
            Format::Pdf => "PDF",
            Format::Png => "PNG",
            Format::Gcode => "G-code",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Dxf => "dxf",
            Format::Pdf => "pdf",
            Format::Png => "png",
            Format::Gcode => "gcode",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Format::Svg => "For drawing programs and other laser software: real size, curves, layer colours, pictures.",
            Format::Dxf => "For CAD programs: millimetres, each layer a DXF layer, curves as fine lines. Pictures can't go in a DXF.",
            Format::Pdf => "Vector PDF at real size, for printing or sharing: curves, layer colours, pictures.",
            Format::Png => "A picture of the design, for documents, chats and websites.",
            Format::Gcode => "The laser job file (the same as Laser → Save G-code).",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub format: Format,
    /// Only the selected shapes.
    pub selection: bool,
    /// Also layers that don't burn, and construction lines.
    pub everything: bool,
    /// The page is the design's own bounds (otherwise the work area).
    pub fit: bool,
    pub dpi: f64,
    /// White background (otherwise see-through).
    pub white: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { format: Format::Svg, selection: false, everything: false, fit: false, dpi: 300.0, white: true }
    }
}

/// The shapes to export: the design or the selection, without layers that don't burn and construction lines
/// unless asked for.
pub fn gather(doc: &Document, sel: &[usize], o: &Options) -> Vec<Shape> {
    let burns = |l: usize| l < COLOR_LAYERS && doc.cut(l).map(|c| c.output).unwrap_or(true);
    let pick: Vec<usize> = if o.selection { sel.to_vec() } else { (0..doc.shapes.len()).collect() };
    pick.into_iter().filter_map(|i| doc.shapes.get(i)).filter(|s| o.everything || matches!(s.kind, ShapeKind::Group(_)) || burns(s.layer)).cloned().collect()
}

/// The page for SVG, PDF and PNG: the work area, or the shapes' own bounds (with a small margin).
pub fn page(shapes: &[Shape], bed: BBox, o: &Options) -> BBox {
    if !o.fit {
        return bed;
    }
    let b = shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
    if b.is_empty() {
        bed
    } else {
        BBox { min: b.min - p2(1.0, 1.0), max: b.max + p2(1.0, 1.0) }
    }
}

/// The file's bytes, and a note to show (e.g. pictures left out of a DXF).
pub fn bytes(doc: &Document, shapes: &[Shape], bed: BBox, o: &Options) -> Result<(Vec<u8>, Option<String>), String> {
    let pg = page(shapes, bed, o);
    match o.format {
        Format::Svg => Ok((crate::svg::export(shapes, pg).into_bytes(), None)),
        Format::Dxf => {
            let (t, pics) = crate::dxf::export(shapes);
            Ok((t.into_bytes(), (pics > 0).then(|| format!("{pics} picture(s) left out: a DXF can't hold pictures"))))
        }
        Format::Pdf => crate::pdf::export(shapes, pg).map(|b| (b, None)),
        Format::Png => {
            let fill = |l: usize| doc.cut(l).is_some_and(|c| matches!(c.mode, CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill));
            let (w, h, rgba) = crate::raster::render(shapes, pg, o.dpi, o.white, &fill)?;
            crate::platform::png_bytes(w, h, rgba).map(|b| (b, None)).ok_or_else(|| "The picture couldn't be saved.".into())
        }
        Format::Gcode => Err("G-code is saved by Save G-code".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A design with a turned, moved group (shapes inside groups must come out where they're drawn), a
    /// circle and a curve.
    fn design() -> Document {
        let mut doc = Document::default();
        let child = Shape::new(ShapeKind::Rect { w: 20.0, h: 10.0, cr: 0.0 }, Xform::translate(10.0, 0.0), 1);
        let mut group = Shape::new(ShapeKind::Group(vec![child]), Xform::translate(60.0, 40.0).then_after(Xform::rotate(std::f64::consts::FRAC_PI_2)), 1);
        group.layer = 1;
        doc.shapes.push(group);
        doc.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 8.0, ry: 8.0 }, Xform::translate(120.0, 60.0), 2));
        let mut pd = PathData::default();
        crate::sketch::append_path(&mut pd, &crate::sketch::circle_path(p2(0.0, 0.0), 5.0));
        doc.shapes.push(Shape::new(ShapeKind::Path(pd), Xform::translate(150.0, 30.0), 3));
        // a construction line, left out by default
        doc.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(200.0, 0.0)], false)), Xform::IDENTITY, CONSTRUCTION_LAYER));
        doc.sync_cuts();
        doc
    }

    fn outlines(im: &crate::svg::Imported) -> BBox {
        let mut b = BBox::EMPTY;
        for (pd, _) in im.paths.iter().chain(im.on_layers.iter().map(|(p, _)| p).map(|p| (p.clone(), [0u8; 3])).collect::<Vec<_>>().iter()) {
            for p in pd.flatten(0.001).into_iter().flatten() {
                b.add(p);
            }
        }
        b
    }

    #[test]
    fn svg_dxf_and_pdf_come_back_the_same_size_and_place() {
        let doc = design();
        let bed = BBox { min: p2(0.0, 0.0), max: p2(200.0, 100.0) };
        let o = Options::default();
        let shapes = gather(&doc, &[], &o);
        assert_eq!(shapes.len(), 3, "the construction line is left out");
        assert_eq!(gather(&doc, &[], &Options { everything: true, ..o }).len(), 4);
        assert_eq!(gather(&doc, &[1], &Options { selection: true, ..o }).len(), 1);
        let want = shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        // the turned group: its 20 × 10 rectangle stands upright, centred 10 above (60, 40)
        let g = shapes[0].bbox();
        assert!((g.width() - 10.0).abs() < 0.05 && (g.height() - 20.0).abs() < 0.05 && (g.center().y - 50.0).abs() < 0.05, "{g:?}");
        for format in [Format::Svg, Format::Dxf, Format::Pdf] {
            let (b, _) = bytes(&doc, &shapes, bed, &Options { format, ..o }).unwrap();
            let im = match format {
                Format::Svg => crate::svg::import(&b, bed.max.y).unwrap(),
                Format::Dxf => crate::dxf::import(&b).unwrap(),
                _ => crate::pdf::import(&b, 1, bed.max.y).unwrap(),
            };
            let got = outlines(&im);
            assert!(got.min.dist(want.min) < 0.05 && got.max.dist(want.max) < 0.05, "{format:?}: {got:?} vs {want:?}");
        }
        // fitted to the design: same sizes, the page moves
        let fit = Options { fit: true, ..o };
        let pg = page(&shapes, bed, &fit);
        assert!((pg.width() - (want.width() + 2.0)).abs() < 1e-9);
    }

    #[test]
    fn png_at_the_chosen_resolution() {
        let doc = design();
        let bed = BBox { min: p2(0.0, 0.0), max: p2(200.0, 100.0) };
        let o = Options { format: Format::Png, dpi: 50.8, ..Default::default() };
        let (b, _) = bytes(&doc, &gather(&doc, &[], &o), bed, &o).unwrap();
        let img = image::load_from_memory(&b).unwrap();
        assert_eq!((img.width(), img.height()), (400, 200), "2 pixels per mm");
    }
}
