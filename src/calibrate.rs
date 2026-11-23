//! Test & Calibrate: labelled test designs, ready to burn. Each cell of a test burns with its own settings
//! (a `CutOverride` on the shape), so one layer can hold a whole grid of speeds and powers.

use crate::geom::*;
use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TestKind {
    #[default]
    PowerSpeed,
    CutThrough,
    Interval,
    Focus,
    KerfFit,
    Dither,
    Color,
    Detail,
}

impl TestKind {
    pub const ALL: [TestKind; 8] = [TestKind::PowerSpeed, TestKind::CutThrough, TestKind::Interval, TestKind::Focus, TestKind::KerfFit, TestKind::Dither, TestKind::Color, TestKind::Detail];
    pub fn label(self) -> &'static str {
        match self {
            TestKind::PowerSpeed => "Power × speed grid",
            TestKind::CutThrough => "Cut-through ladder",
            TestKind::Interval => "Line interval (LPI)",
            TestKind::Focus => "Focus ramp",
            TestKind::KerfFit => "Kerf and fit",
            TestKind::Dither => "Picture modes strip",
            TestKind::Color => "Colour on metal (MOPA fiber)",
            TestKind::Detail => "Fine detail ladder",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            TestKind::PowerSpeed => "A grid of squares, each at its own power (across) and speed (down). Burn it, pick the square you like best, select it and press Save as material.",
            TestKind::CutThrough => "Squares cut at different speeds (across) and numbers of passes (down). The squares that drop out cut through: the fastest of those is the one to use.",
            TestKind::Interval => "Filled squares at different line intervals. Pick the lowest number of lines per inch that still looks solid: it burns fastest.",
            TestKind::Focus => "A long line with a scale. Prop one end of the material up (a few mm), burn, and find where the line is thinnest: that height is the focus.",
            TestKind::KerfFit => "A kerf gauge (10 pieces in a frame) and a row of slots for your material's thickness. Measure the gap in the gauge and enter it below to get the kerf offset; the slot that fits snugly shows the allowance for joints.",
            TestKind::Dither => "The same gradient in every picture mode, to compare them on your material.",
            TestKind::Color => "Filled squares at different frequencies (across) and pulse widths (down), for colours on stainless steel and titanium with a MOPA fiber laser.",
            TestKind::Detail => "Text from 0.5 to 4 mm tall, holes from 0.2 to 2 mm and line pairs with gaps from 0.05 to 0.5 mm: the smallest you can still read or see is your laser's detail limit.",
        }
    }
}

/// A range of values from `min` to `max` in `steps` (at least 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub steps: u32,
}

impl Range {
    pub const fn new(min: f64, max: f64, steps: u32) -> Range {
        Range { min, max, steps }
    }
    pub fn values(&self) -> Vec<f64> {
        let n = self.steps.clamp(1, 50);
        if n == 1 {
            return vec![self.min];
        }
        (0..n).map(|i| self.min + (self.max - self.min) * i as f64 / (n - 1) as f64).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestParams {
    pub kind: TestKind,
    /// Power × speed: how the cells burn (Line, Fill or Image).
    pub mode: CutMode,
    pub power: Range,
    pub speed: Range,
    pub passes: Range,
    pub interval: Range,
    /// kHz and ns for the colour test.
    pub frequency: Range,
    pub pulse: Range,
    /// Power for tests that keep it fixed (cut-through, interval, focus, kerf, detail, colour).
    pub fixed_power: f64,
    pub fixed_speed: f64,
    /// Material thickness, mm (kerf and fit slots).
    pub thickness: f64,
    /// Cell size and the gap between cells, mm.
    pub cell: f64,
    pub gap: f64,
}

impl Default for TestParams {
    fn default() -> Self {
        TestParams {
            kind: TestKind::PowerSpeed,
            mode: CutMode::Fill,
            power: Range::new(10.0, 100.0, 5),
            speed: Range::new(50.0, 250.0, 5),
            passes: Range::new(1.0, 4.0, 4),
            interval: Range::new(0.05, 0.3, 6),
            frequency: Range::new(20.0, 200.0, 6),
            pulse: Range::new(4.0, 200.0, 6),
            fixed_power: 80.0,
            fixed_speed: 100.0,
            thickness: 3.0,
            cell: 10.0,
            gap: 3.0,
        }
    }
}

/// The layers a test uses: the cells, labels (engraved lightly), and pictures.
pub const CELLS: usize = 0;
pub const LABELS: usize = 1;
pub const PICTURES: usize = 2;
const LABEL_HEIGHT: f64 = 2.5;

fn label(doc: &mut Document, text: impl Into<String>, at: P2, height: f64) {
    doc.shapes.push(Shape::new(ShapeKind::Text { text: text.into(), font: crate::fonts::DEFAULT_FONT.into(), height, style: Default::default(), backup: None }, Xform::translate(at.x, at.y), LABELS));
}

/// A rectangle whose centre is at `c`.
fn rect(c: P2, w: f64, h: f64, layer: usize, cut: CutOverride) -> Shape {
    let mut s = Shape::new(ShapeKind::Rect { w, h, cr: 0.0 }, Xform::translate(c.x, c.y), layer);
    s.cut = cut;
    s
}

fn line(a: P2, b: P2, layer: usize) -> Shape {
    Shape::new(ShapeKind::Path(PathData::from_polylines(&[vec![a, b]])), Xform::IDENTITY, layer)
}

/// A left-to-right black-to-white gradient picture.
fn gradient(w_mm: f64, h_mm: f64, adjust: ImageAdjust, at: P2) -> Option<Shape> {
    let (pw, ph) = (256u32, 24u32);
    let mut rgba = Vec::with_capacity((pw * ph * 4) as usize);
    for _ in 0..ph {
        for x in 0..pw {
            let g = (x * 255 / (pw - 1)) as u8;
            rgba.extend_from_slice(&[g, g, g, 255]);
        }
    }
    let png = crate::platform::png_bytes(pw, ph, rgba)?;
    let data = crate::lbrn::decode_bitmap(png, "")?;
    Some(Shape::new(ShapeKind::Bitmap { w: w_mm, h: h_mm, data, adjust }, Xform::translate(at.x, at.y), PICTURES))
}

fn f(v: f64) -> String {
    fmt_num((v * 100.0).round() / 100.0)
}

/// Make the test design, centred on the work area of `dev`.
pub fn generate(p: &TestParams, dev: &crate::gcode::Device) -> Document {
    let mut doc = Document::default();
    let (cell, gap) = (p.cell.clamp(2.0, 100.0), p.gap.clamp(0.5, 50.0));
    let pitch = cell + gap;
    let lh = LABEL_HEIGHT;
    let title = |doc: &mut Document, text: String, y: f64| label(doc, text, p2(0.0, y), lh * 1.4);
    match p.kind {
        TestKind::PowerSpeed => {
            let (powers, speeds) = (p.power.values(), p.speed.values());
            let img = p.mode == CutMode::Image;
            for (j, sp) in speeds.iter().enumerate() {
                let y = (speeds.len() - 1 - j) as f64 * pitch + cell / 2.0;
                label(&mut doc, f(*sp), p2(-gap - lh * 4.0, y - lh / 2.0), lh);
                for (i, pw) in powers.iter().enumerate() {
                    let c = p2(i as f64 * pitch + cell / 2.0, y);
                    let cut = CutOverride { speed: Some(*sp), max_power: Some(*pw), min_power: Some(if img { 0.0 } else { *pw }), ..Default::default() };
                    if img {
                        if let Some(mut s) = gradient(cell, cell, ImageAdjust { dither: Dither::Grayscale, ..Default::default() }, c) {
                            s.layer = CELLS;
                            s.cut = cut;
                            doc.shapes.push(s);
                        }
                    } else {
                        doc.shapes.push(rect(c, cell, cell, CELLS, cut));
                    }
                }
            }
            let top = speeds.len() as f64 * pitch;
            for (i, pw) in powers.iter().enumerate() {
                label(&mut doc, format!("{}%", f(*pw)), p2(i as f64 * pitch, top), lh);
            }
            label(&mut doc, "Power →", p2(0.0, top + lh * 1.8), lh);
            label(&mut doc, "Speed mm/s ↓", p2(-gap - lh * 4.0, top + lh * 1.8), lh);
            title(&mut doc, format!("Power × speed · {}", p.mode.label()), top + lh * 4.2);
            let c = doc.cut_mut(CELLS);
            c.mode = p.mode;
            c.interval = 0.1;
        }
        TestKind::CutThrough => {
            let (speeds, passes) = (p.speed.values(), p.passes.values());
            for (j, ps) in passes.iter().enumerate() {
                let y = (passes.len() - 1 - j) as f64 * pitch + cell / 2.0;
                let n = ps.round().max(1.0) as u32;
                label(&mut doc, format!("{n}×"), p2(-gap - lh * 2.5, y - lh / 2.0), lh);
                for (i, sp) in speeds.iter().enumerate() {
                    let cut = CutOverride { speed: Some(*sp), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), passes: Some(n), ..Default::default() };
                    doc.shapes.push(rect(p2(i as f64 * pitch + cell / 2.0, y), cell, cell, CELLS, cut));
                }
            }
            let top = passes.len() as f64 * pitch;
            for (i, sp) in speeds.iter().enumerate() {
                label(&mut doc, f(*sp), p2(i as f64 * pitch, top), lh);
            }
            label(&mut doc, "Speed mm/s →   passes ↓", p2(0.0, top + lh * 1.8), lh);
            title(&mut doc, format!("Cut-through · {}% power", f(p.fixed_power)), top + lh * 4.2);
            doc.cut_mut(CELLS).mode = CutMode::Line;
        }
        TestKind::Interval => {
            let ivs = p.interval.values();
            for (i, iv) in ivs.iter().enumerate() {
                let x = i as f64 * pitch;
                let cut = CutOverride { interval: Some(*iv), speed: Some(p.fixed_speed), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), ..Default::default() };
                doc.shapes.push(rect(p2(x + cell / 2.0, cell / 2.0), cell, cell, CELLS, cut));
                label(&mut doc, f(*iv), p2(x, -lh * 1.6), lh * 0.8);
                label(&mut doc, format!("{:.0}", 25.4 / iv.max(0.001)), p2(x, -lh * 3.0), lh * 0.8);
            }
            label(&mut doc, "mm / lines per inch", p2(0.0, -lh * 4.8), lh * 0.8);
            title(&mut doc, format!("Line interval · {} mm/s · {}%", f(p.fixed_speed), f(p.fixed_power)), cell + lh);
            doc.cut_mut(CELLS).mode = CutMode::Fill;
        }
        TestKind::Focus => {
            let len = 100.0;
            let cut = CutOverride { speed: Some(p.fixed_speed), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), ..Default::default() };
            let mut l = line(p2(0.0, 0.0), p2(len, 0.0), CELLS);
            l.cut = cut;
            doc.shapes.push(l);
            for k in 0..=20 {
                let x = k as f64 * 5.0;
                let tall = if k % 2 == 0 { 4.0 } else { 2.0 };
                let mut t = line(p2(x, -1.0), p2(x, -1.0 - tall), CELLS);
                t.cut = cut;
                doc.shapes.push(t);
                if k % 2 == 0 {
                    label(&mut doc, format!("{}", k * 5), p2(x - 1.0, -5.0 - lh * 1.4), lh * 0.8);
                }
            }
            label(&mut doc, "Prop up the right end. The thinnest part of the line is in focus.", p2(0.0, 4.0), lh * 0.8);
            title(&mut doc, "Focus ramp".into(), 4.0 + lh * 2.0);
            doc.cut_mut(CELLS).mode = CutMode::Line;
        }
        TestKind::KerfFit => {
            let cut = CutOverride { speed: Some(p.fixed_speed), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), ..Default::default() };
            // kerf gauge: a 100 × 20 mm opening cut into 10 strips, inside a frame
            let add = |doc: &mut Document, mut s: Shape| {
                s.cut = cut;
                doc.shapes.push(s);
            };
            add(&mut doc, rect(p2(50.0, 10.0), 100.0, 20.0, CELLS, cut));
            for k in 1..10 {
                add(&mut doc, line(p2(k as f64 * 10.0, 0.0), p2(k as f64 * 10.0, 20.0), CELLS));
            }
            add(&mut doc, rect(p2(50.0, 10.0), 116.0, 36.0, CELLS, cut));
            label(&mut doc, "Kerf: push the strips to one side, measure the gap, kerf offset = gap ÷ 22", p2(-8.0, 22.0 + lh * 2.6), lh * 0.8);
            // fit comb: slots for the material's thickness, from 0.2 mm tighter to 0.2 mm looser
            let t = p.thickness.clamp(0.2, 50.0);
            let y0 = -30.0;
            let offs: Vec<f64> = (0..9).map(|k| -0.2 + k as f64 * 0.05).collect();
            let slot_pitch = (t + 6.0).max(8.0);
            let w = slot_pitch * offs.len() as f64 + 6.0;
            add(&mut doc, rect(p2(w / 2.0 - 3.0, y0), w, 24.0, CELLS, cut));
            for (k, o) in offs.iter().enumerate().filter(|(_, o)| t + **o >= 0.1) {
                let x = k as f64 * slot_pitch + slot_pitch / 2.0;
                add(&mut doc, rect(p2(x, y0 + 3.0), t + o, 10.0, CELLS, cut));
                label(&mut doc, format!("{}{}", if *o > 0.0 { "+" } else { "" }, f(*o)), p2(x - 3.0, y0 - 8.0), lh * 0.7);
            }
            label(&mut doc, format!("Fit: slots for {} mm material, ± mm", f(t)), p2(-3.0, y0 + 12.0 + lh), lh * 0.8);
            title(&mut doc, "Kerf and fit".into(), 22.0 + lh * 5.0);
            doc.cut_mut(CELLS).mode = CutMode::Line;
        }
        TestKind::Dither => {
            let modes = Dither::ALL;
            let (w, h) = (60.0, 8.0);
            for (k, d) in modes.iter().enumerate() {
                let y = (modes.len() - 1 - k) as f64 * (h + gap);
                let adjust = ImageAdjust { dither: *d, ..Default::default() };
                if let Some(s) = gradient(w, h, adjust, p2(30.0 + w / 2.0, y + h / 2.0)) {
                    doc.shapes.push(s);
                }
                label(&mut doc, d.label(), p2(0.0, y + h / 2.0 - lh / 2.0), lh * 0.9);
            }
            title(&mut doc, "Picture modes".into(), modes.len() as f64 * (h + gap) + lh);
            let c = doc.cut_mut(PICTURES);
            (c.mode, c.speed, c.max_power, c.min_power, c.interval) = (CutMode::Image, p.fixed_speed, p.fixed_power, 0.0, 0.1);
        }
        TestKind::Color => {
            let (freqs, pulses) = (p.frequency.values(), p.pulse.values());
            for (j, pu) in pulses.iter().enumerate() {
                let y = (pulses.len() - 1 - j) as f64 * pitch + cell / 2.0;
                let ns = pu.round().clamp(1.0, 65535.0) as u16;
                label(&mut doc, format!("{ns}"), p2(-gap - lh * 3.0, y - lh / 2.0), lh);
                for (i, fr) in freqs.iter().enumerate() {
                    let cut = CutOverride { frequency: Some(fr * 1000.0), pulse_width: Some(ns), speed: Some(p.fixed_speed), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), ..Default::default() };
                    doc.shapes.push(rect(p2(i as f64 * pitch + cell / 2.0, y), cell, cell, CELLS, cut));
                }
            }
            let top = pulses.len() as f64 * pitch;
            for (i, fr) in freqs.iter().enumerate() {
                label(&mut doc, f(*fr), p2(i as f64 * pitch, top), lh);
            }
            label(&mut doc, "Frequency kHz →   pulse width ns ↓", p2(0.0, top + lh * 1.8), lh);
            title(&mut doc, format!("Colour · {} mm/s · {}%", f(p.fixed_speed), f(p.fixed_power)), top + lh * 4.2);
            let c = doc.cut_mut(CELLS);
            (c.mode, c.interval) = (CutMode::Fill, 0.01);
        }
        TestKind::Detail => {
            let cut = CutOverride { speed: Some(p.fixed_speed), max_power: Some(p.fixed_power), min_power: Some(p.fixed_power), ..Default::default() };
            // text sizes (on the labels layer, engraved with the cell settings)
            let mut y = 0.0;
            for hgt in [4.0, 3.0, 2.0, 1.5, 1.0, 0.75, 0.5] {
                let mut s = Shape::new(ShapeKind::Text { text: format!("{} mm BurnCraft 0123", f(hgt)), font: crate::fonts::DEFAULT_FONT.into(), height: hgt, style: Default::default(), backup: None }, Xform::translate(0.0, y), CELLS);
                s.cut = cut;
                doc.shapes.push(s);
                y -= hgt + 2.0;
            }
            // holes
            let hy = y - 4.0;
            let mut x = 0.0;
            for d in [2.0, 1.5, 1.0, 0.8, 0.6, 0.5, 0.4, 0.3, 0.2] {
                let mut s = Shape::new(ShapeKind::Ellipse { rx: d / 2.0, ry: d / 2.0 }, Xform::translate(x + d / 2.0, hy), CELLS);
                s.cut = cut;
                doc.shapes.push(s);
                label(&mut doc, f(d), p2(x - 0.5, hy - 4.5), 1.5);
                x += d + 5.0;
            }
            // line pairs
            let ly = hy - 12.0;
            let mut x = 0.0;
            for g in [0.5, 0.4, 0.3, 0.2, 0.15, 0.1, 0.05] {
                for dx in [0.0, g] {
                    let mut l = line(p2(x + dx, ly), p2(x + dx, ly - 8.0), CELLS);
                    l.cut = cut;
                    doc.shapes.push(l);
                }
                label(&mut doc, f(g), p2(x - 1.0, ly - 12.0), 1.5);
                x += 7.0;
            }
            label(&mut doc, "holes (mm)          line gaps (mm)", p2(0.0, ly + 3.0), 1.5);
            title(&mut doc, "Fine detail".into(), 6.0);
            // filled text and dots, and the open line pairs burned as lines
            doc.cut_mut(CELLS).mode = CutMode::FillLine;
        }
    }
    doc.sync_cuts();
    // labels: a light engrave
    if doc.cut(LABELS).is_some() {
        let c = doc.cut_mut(LABELS);
        (c.mode, c.speed, c.max_power, c.min_power, c.name) = (CutMode::Fill, 150.0, 20.0, 20.0, "Labels".into());
    }
    if doc.cut(CELLS).is_some() {
        doc.cut_mut(CELLS).name = "Test".into();
    }
    // centred on the work area
    let b = doc.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
    if !b.is_empty() {
        let d = p2(dev.width / 2.0, dev.height / 2.0) - b.center();
        for s in &mut doc.shapes {
            s.xform = s.xform.then(Xform::translate(d.x, d.y));
        }
    }
    doc.title = Some(format!("Test - {}", p.kind.label()));
    doc
}

/// The kerf offset to use from the gap measured in the kerf gauge: the gap is 11 cut widths (both ends of the
/// opening and the 9 cuts between the strips), and the offset is half a cut width.
pub fn kerf_from_gap(gap_mm: f64) -> f64 {
    gap_mm.max(0.0) / 22.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_test_makes_a_design_that_fits_and_burns_as_labelled() {
        let dev = crate::gcode::Device::default();
        for kind in TestKind::ALL {
            let p = TestParams { kind, ..Default::default() };
            let doc = generate(&p, &dev);
            assert!(!doc.shapes.is_empty(), "{kind:?}");
            let b = doc.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
            assert!(crate::gcode::bed_box(&dev).contains_box(&b), "{kind:?} fits the bed: {b:?}");
            let job = crate::gcode::build_job(&doc, &dev, None);
            assert!(job.moves.iter().any(|m| m.power > 0.0), "{kind:?} burns something");
            assert_eq!(job.skipped_outside, 0, "{kind:?}");
        }
    }

    #[test]
    fn the_grid_cells_burn_at_their_own_settings() {
        let dev = crate::gcode::Device::default();
        let p = TestParams { power: Range::new(20.0, 60.0, 3), speed: Range::new(100.0, 300.0, 3), mode: CutMode::Line, ..Default::default() };
        let doc = generate(&p, &dev);
        let cells: Vec<&Shape> = doc.shapes.iter().filter(|s| s.layer == CELLS).collect();
        assert_eq!(cells.len(), 9);
        let job = crate::gcode::build_job(&doc, &dev, None);
        let mut seen: Vec<(i64, i64)> = job.moves.iter().filter(|m| m.power > 0.0 && m.layer == CELLS).map(|m| ((m.power * 100.0).round() as i64, m.speed.round() as i64)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen, vec![(20, 100), (20, 200), (20, 300), (40, 100), (40, 200), (40, 300), (60, 100), (60, 200), (60, 300)]);
        // the cut-through ladder's passes
        let p = TestParams { kind: TestKind::CutThrough, speed: Range::new(10.0, 10.0, 1), passes: Range::new(1.0, 3.0, 3), cell: 10.0, ..Default::default() };
        let job = crate::gcode::build_job(&generate(&p, &dev), &dev, None);
        let cut: f64 = job.moves.iter().filter(|m| m.power > 0.0 && m.layer == CELLS).map(|m| m.from.dist(m.to)).sum();
        assert!((cut - 40.0 * (1.0 + 2.0 + 3.0)).abs() < 1e-6, "{cut}");
    }

    #[test]
    fn colour_cells_carry_frequency_and_pulse_and_kerf_is_worked_out() {
        let dev = crate::gcode::Device::default();
        let p = TestParams { kind: TestKind::Color, frequency: Range::new(30.0, 60.0, 2), pulse: Range::new(10.0, 10.0, 1), ..Default::default() };
        let job = crate::gcode::build_job(&generate(&p, &dev), &dev, None);
        let mut f: Vec<(i64, Option<u16>)> = job.moves.iter().filter(|m| m.power > 0.0 && m.layer == CELLS).map(|m| (m.frequency.round() as i64, m.pulse_width)).collect();
        f.dedup();
        assert_eq!(f, vec![(30000, Some(10)), (60000, Some(10))]);
        assert!((kerf_from_gap(2.2) - 0.1).abs() < 1e-12);
        assert_eq!(Range::new(1.0, 2.0, 1).values(), vec![1.0]);
        assert_eq!(Range::new(0.0, 1.0, 3).values(), vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn cell_settings_and_material_links_are_saved() {
        let dev = crate::gcode::Device::default();
        let p = TestParams { kind: TestKind::Color, ..Default::default() };
        let mut doc = generate(&p, &dev);
        doc.cut_mut(CELLS).material = "m123".into();
        let own = |d: &Document| d.shapes.iter().map(|s| s.cut).collect::<Vec<_>>();
        let l = crate::lbrn::load_str(&crate::lbrn::save_string(&doc, &[])).unwrap();
        assert_eq!(own(&l.doc), own(&doc));
        assert_eq!(l.doc.cut(CELLS).unwrap().material, "m123");
        let b = crate::burn::load(&crate::burn::save(&doc, &[]).unwrap()).unwrap();
        assert_eq!(own(&b.doc), own(&doc));
        assert_eq!(b.doc.cut(CELLS).unwrap().material, "m123");
        assert!(own(&doc).iter().any(|c| c.pulse_width.is_some()));
    }

    #[test]
    fn colour_settings_carry_their_pulse_width_and_mixed_jobs_are_refused() {
        let dev = crate::gcode::Device::default();
        let p = TestParams { kind: TestKind::Color, frequency: Range::new(60.0, 60.0, 1), pulse: Range::new(10.0, 10.0, 1), ..Default::default() };
        let mut doc = generate(&p, &dev);
        // a colour square saved as a material and put on another layer burns at its pulse width
        let cell = doc.shapes.iter().find(|s| !s.cut.is_empty()).unwrap().clone();
        let m = crate::materials::Material::from_cut(&cell.cut.apply(doc.cut(CELLS).unwrap()), "Steel blue", 0.0, "");
        assert_eq!(m.pulse_width, Some(10));
        let mut sq = Shape::new(ShapeKind::Rect { w: 5.0, h: 5.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 5);
        sq.cut = CutOverride::default();
        doc.shapes.push(sq);
        doc.sync_cuts();
        m.apply(doc.cut_mut(5));
        let job = crate::gcode::build_job(&doc, &dev, None);
        assert!(job.moves.iter().filter(|m| m.power > 0.0 && m.layer == 5).all(|m| m.pulse_width == Some(10) && (m.frequency - 60_000.0).abs() < 1e-6));
        // the labels have no pulse width: on a MOPA machine without its own setting, that job is refused
        let mut g = crate::galvo::GalvoSettings::default();
        g.source = crate::galvo::Source::Fiber;
        g.pulse_width = None;
        assert!(crate::galvo::pulse_width_problem(&job, &g).is_some());
        g.pulse_width = Some(200);
        assert!(crate::galvo::pulse_width_problem(&job, &g).is_none());
        // the fit comb never makes a slot too narrow to cut
        let doc = generate(&TestParams { kind: TestKind::KerfFit, thickness: 0.2, ..Default::default() }, &dev);
        assert!(doc.shapes.iter().all(|s| !matches!(s.kind, ShapeKind::Rect { w, .. } if w < 0.1)));
    }
}
