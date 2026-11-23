//! The "BurnCraft Colony": collectible tiles that record settings that worked (with the ant mark, a serial
//! number and a QR code holding the settings themselves), and the "BurnAnt" benchmark card.

use crate::geom::*;
use crate::materials::Material;
use crate::model::*;

/// The ant mark (drawn by the project's owner): an ant inside a flame. Engrave it; as a cut-out only the
/// outer flame holds together.
const ANT_SVG: &[u8] = include_bytes!("../assets/ant-mark.svg");

/// The layers tiles use.
pub const CUT: usize = 0;
pub const ENGRAVE: usize = 1;
pub const SAMPLE: usize = 2;
pub const SCORE: usize = 3;

/// The ant mark's outline (its own coordinates) and its box.
fn ant_path() -> Option<&'static (PathData, BBox)> {
    static ANT: std::sync::OnceLock<Option<(PathData, BBox)>> = std::sync::OnceLock::new();
    ANT.get_or_init(|| {
        let im = crate::svg::import(ANT_SVG, 0.0).ok()?;
        let mut pd = PathData::default();
        for (p, _) in im.paths {
            let base = pd.verts.len();
            pd.verts.extend(p.verts);
            pd.prims.extend(p.prims.into_iter().map(|q| Prim { a: q.a + base, b: q.b + base, ..q }));
        }
        let b = Shape::new(ShapeKind::Path(pd.clone()), Xform::IDENTITY, 0).bbox();
        (!b.is_empty()).then_some((pd, b))
    })
    .as_ref()
}

/// The ant mark `height` mm tall, centred on `c`.
pub fn ant_mark(height: f64, c: P2, layer: usize) -> Option<Shape> {
    let (pd, b) = ant_path()?;
    let k = height / b.height();
    let x = Xform::translate(-b.center().x, -b.center().y).then(Xform::scale(k, k)).then(Xform::translate(c.x, c.y));
    Some(Shape::new(ShapeKind::Path(pd.clone()), x, layer))
}

/// The ant mark as a shaded picture (dark at the bottom, lighter at the top), `height` mm tall, for the
/// dithered-ant part of the benchmark.
fn ant_picture(height: f64, c: P2, layer: usize) -> Option<Shape> {
    let (pd, b) = ant_path()?;
    let (w_mm, h_mm) = (height * b.width() / b.height(), height);
    let (pw, ph) = ((w_mm * 10.0).round().max(8.0) as usize, (h_mm * 10.0).round().max(8.0) as usize);
    // the outline in pixels (y down)
    let rings: Vec<Vec<P2>> = pd.flatten(0.05).into_iter().map(|l| l.into_iter().map(|p| p2((p.x - b.min.x) / b.width() * pw as f64, (b.max.y - p.y) / b.height() * ph as f64)).collect()).collect();
    let mut rgba = vec![255u8; pw * ph * 4];
    crate::imaging::mask_pixels(&mut rgba, pw, ph, &rings, true);
    for y in 0..ph {
        let g = (40.0 + 150.0 * (1.0 - y as f64 / ph as f64)) as u8;
        for x in 0..pw {
            let i = (y * pw + x) * 4;
            let v = if rgba[i + 3] == 255 { g } else { 255 };
            rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    let png = crate::platform::png_bytes(pw as u32, ph as u32, rgba)?;
    let data = crate::lbrn::decode_bitmap(png, "")?;
    Some(Shape::new(ShapeKind::Bitmap { w: w_mm, h: h_mm, data, adjust: ImageAdjust { dither: Dither::Jarvis, ..Default::default() } }, Xform::translate(c.x, c.y), layer))
}

/// Text `height` mm tall with its left end at `at` (baseline), or centred on `at.x`.
fn text(doc: &mut Document, s: &str, at: P2, height: f64, centred: bool, layer: usize) {
    doc.shapes.push(crate::calibrate::text_shape(s, height, at, centred, layer));
}

/// A QR code `size` mm wide centred on `c` (no quiet zone: the tile around it is blank).
fn qr(text: &str, size: f64, c: P2, layer: usize) -> Option<Shape> {
    use crate::barcode::{encode_qr_compact, outlines, BarcodeState, Kind};
    let m = encode_qr_compact(text).ok()?;
    let st = BarcodeState { kind: Kind::Qr, text: text.into(), width: size, quiet_zone: false, invert: false, human_text: false, ..Default::default() };
    let rings: Vec<Vec<P2>> = outlines(&st, &m).into_iter().map(|r| r.into_iter().map(|p| p + c).collect()).collect();
    Some(Shape::new(ShapeKind::Path(PathData::from_polylines(&rings)), Xform::IDENTITY, layer))
}

/// Today's local date as YYYY-MM-DD (UTC where the time zone can't be read: Windows; the windows that make
/// tiles let the date be changed).
pub fn today() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new_0();
        return format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date());
    }
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        // SAFETY: localtime_r only writes the struct we give it
        let t = crate::autosave::wall_secs() as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday);
        }
    }
    #[allow(unreachable_code)]
    date_of((crate::autosave::wall_secs() / 86_400.0).floor() as i64)
}

/// The civil date of a day count since 1970-01-01.
pub fn date_of(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// The settings as compact text for the QR code: readable by any phone, no website needed.
/// e.g. `Burn Colony #7|Birch plywood 3mm|My diode|Line 8mm/s 85% x2 kerf0.08 air|2026-11-23` (interval,
/// kerf and Z in mm)
pub fn settings_code(m: &Material, serial: u32, date: &str) -> String {
    let f = |v: f64| fmt_num((v * 100.0).round() / 100.0);
    let mode = CutMode::from_file_name(&m.mode);
    let mut s = format!("{} {}mm/s {}%", mode.label(), f(m.speed), f(m.max_power));
    if m.min_power != m.max_power && mode == CutMode::Image {
        s.push_str(&format!(" min{}%", f(m.min_power)));
    }
    if m.passes > 1 {
        s.push_str(&format!(" x{}", m.passes));
    }
    if mode != CutMode::Line {
        s.push_str(&format!(" int{}", f(m.interval)));
    }
    if m.kerf != 0.0 {
        s.push_str(&format!(" kerf{}", f(m.kerf)));
    }
    if let Some(p) = m.pulse_width {
        s.push_str(&format!(" {}kHz {p}ns", f(m.frequency / 1000.0)));
    }
    if m.z_offset != 0.0 {
        s.push_str(&format!(" z{}", f(m.z_offset)));
    }
    if m.air_assist {
        s.push_str(" air");
    }
    let thick = if m.thickness > 0.0 { format!(" {}mm", f(m.thickness)) } else { String::new() };
    // long names are shortened in the code (the tile's text has them in full), so its dots stay big
    let short = |t: &str, n: usize| t.chars().take(n).collect::<String>().trim().to_string();
    let laser = if m.laser.is_empty() { String::new() } else { format!("|{}", short(&m.laser, 16)) };
    format!("Burn Colony #{serial}|{}{thick}{laser}|{s}|{date}", if m.name.is_empty() { "Material".to_string() } else { short(&m.name, 24) })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TileSize {
    /// 80 × 50 mm, the standard tile.
    #[default]
    Card,
    /// 120 × 80 mm.
    LargeCard,
    /// A 60 mm round coin.
    Coin,
}

impl TileSize {
    pub const ALL: [TileSize; 3] = [TileSize::Card, TileSize::LargeCard, TileSize::Coin];
    pub fn label(self) -> &'static str {
        match self {
            TileSize::Card => "Card 80 × 50 mm",
            TileSize::LargeCard => "Large card 120 × 80 mm",
            TileSize::Coin => "Coin 60 mm",
        }
    }
}

fn cut_rect(doc: &mut Document, c: P2, w: f64, h: f64, r: f64) {
    doc.shapes.push(Shape::new(ShapeKind::Rect { w, h, cr: r }, Xform::translate(c.x, c.y), CUT));
}
fn cut_circle(doc: &mut Document, c: P2, d: f64) {
    doc.shapes.push(Shape::new(ShapeKind::Ellipse { rx: d / 2.0, ry: d / 2.0 }, Xform::translate(c.x, c.y), CUT));
}

/// Usual settings for the tile's own layers (to check before burning): the outline is cut, text engraved.
fn tile_layers(doc: &mut Document, m: Option<&Material>) {
    doc.sync_cuts();
    fresh_layers(doc, &[CUT, ENGRAVE, SAMPLE, SCORE]);
    let e = doc.cut_mut(ENGRAVE);
    (e.mode, e.speed, e.max_power, e.min_power, e.interval, e.name) = (CutMode::Fill, 150.0, 30.0, 30.0, 0.1, "Engrave (check)".into());
    let c = doc.cut_mut(CUT);
    (c.mode, c.speed, c.max_power, c.min_power, c.passes, c.name) = (CutMode::Line, 10.0, 80.0, 80.0, 1, "Cut out (check)".into());
    // a tile of the material that was tested: cut it with the winning settings when they're for cutting
    if let Some(m) = m.filter(|m| CutMode::from_file_name(&m.mode) == CutMode::Line) {
        m.apply(doc.cut_mut(CUT));
        doc.cut_mut(CUT).name = "Cut out".into();
    }
    if doc.cut(SCORE).is_some() {
        let s = doc.cut_mut(SCORE);
        (s.mode, s.speed, s.max_power, s.min_power, s.name) = (CutMode::Line, 200.0, 20.0, 20.0, "Score (check)".into());
    }
    // everything is engraved before the tile is cut free (it could shift once it's loose)
    doc.cut_mut(CUT).priority = 100;
}

/// Start these layers from plain settings, not the user's saved layer defaults (which may add passes, kerf
/// or a Z offset the tile doesn't expect).
pub fn fresh_layers(doc: &mut Document, layers: &[usize]) {
    for l in layers {
        if let Some(c) = doc.cuts.iter_mut().find(|c| c.index == *l) {
            let image = c.mode == CutMode::Image;
            *c = CutSetting::new(*l);
            if image {
                c.mode = CutMode::Image;
            }
        }
    }
}

/// A Colony tile for a material's winning settings: the ant mark, the serial number, the material, the laser,
/// the settings, the date, a QR code with all of it, and a sample burned with those settings.
pub fn material_tile(m: &Material, size: TileSize, serial: u32, date: &str) -> Document {
    let mut doc = Document::default();
    let code = settings_code(m, serial, date);
    let f = |v: f64| fmt_num((v * 100.0).round() / 100.0);
    let title = format!("{}{}", if m.name.is_empty() { "Material" } else { &m.name }, if m.thickness > 0.0 { format!(" {} mm", f(m.thickness)) } else { String::new() });
    let settings = m.summary();
    let clip = |s: &str, n: usize| if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() };
    match size {
        TileSize::Card | TileSize::LargeCard => {
            cut_rect(&mut doc, p2(40.0, 25.0), 80.0, 50.0, 4.0);
            cut_circle(&mut doc, p2(5.5, 44.5), 4.0);
            doc.shapes.extend(ant_mark(12.0, p2(15.0, 40.0), ENGRAVE));
            text(&mut doc, "BURN COLONY", p2(22.0, 42.5), 3.5, false, ENGRAVE);
            text(&mut doc, &format!("Ant #{serial}"), p2(22.0, 35.5), 5.0, false, ENGRAVE);
            doc.shapes.extend(qr(&code, 17.0, p2(68.5, 38.5), ENGRAVE));
            let lines = [clip(&title, 40), clip(if m.laser.is_empty() { "any laser" } else { &m.laser }, 40), clip(&settings, 46), date.to_string()];
            for (k, l) in lines.iter().enumerate() {
                text(&mut doc, l, p2(4.0, 27.5 - k as f64 * 4.0), 2.2, false, ENGRAVE);
            }
            // a sample burned with the settings
            let c = p2(68.5, 15.0);
            if CutMode::from_file_name(&m.mode) == CutMode::Image {
                if let Some(s) = crate::calibrate::gradient_shape(16.0, 9.0, ImageAdjust::default(), c, SAMPLE) {
                    doc.shapes.push(s);
                }
            } else if CutMode::from_file_name(&m.mode) == CutMode::Line {
                // a cut: a slot open at both ends, so nothing drops out of the tile
                doc.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&[vec![p2(c.x - 8.0, c.y), p2(c.x + 8.0, c.y)]])), Xform::IDENTITY, SAMPLE));
            } else {
                doc.shapes.push(Shape::new(ShapeKind::Rect { w: 16.0, h: 9.0, cr: 0.0 }, Xform::translate(c.x, c.y), SAMPLE));
            }
            text(&mut doc, "sample", p2(68.5, 7.0), 1.6, true, ENGRAVE);
            if size == TileSize::LargeCard {
                // the same layout half as big again (120 × 75), in the middle of a 120 × 80 card
                for s in doc.shapes.iter_mut().skip(1) {
                    s.xform = s.xform.then(Xform::scale(1.5, 1.5)).then(Xform::translate(0.0, 2.5));
                }
                doc.shapes[0] = Shape::new(ShapeKind::Rect { w: 120.0, h: 80.0, cr: 6.0 }, Xform::translate(60.0, 40.0), CUT);
            }
        }
        TileSize::Coin => {
            cut_circle(&mut doc, p2(30.0, 30.0), 60.0);
            cut_circle(&mut doc, p2(30.0, 55.5), 4.0);
            doc.shapes.extend(ant_mark(10.0, p2(30.0, 45.5), ENGRAVE));
            text(&mut doc, &format!("BURN COLONY · Ant #{serial}"), p2(30.0, 36.5), 2.4, true, ENGRAVE);
            doc.shapes.extend(qr(&code, 16.0, p2(30.0, 25.0), ENGRAVE));
            text(&mut doc, &clip(&title, 26), p2(30.0, 13.5), 1.8, true, ENGRAVE);
            text(&mut doc, &clip(&settings, 30), p2(30.0, 10.5), 1.6, true, ENGRAVE);
            text(&mut doc, date, p2(30.0, 7.5), 1.6, true, ENGRAVE);
        }
    }
    tile_layers(&mut doc, Some(m));
    if doc.cut(SAMPLE).is_some() {
        let mut c = doc.cut(SAMPLE).cloned().unwrap_or_else(|| CutSetting::new(SAMPLE));
        m.apply(&mut c);
        c.name = "Sample (the saved settings)".into();
        *doc.cut_mut(SAMPLE) = c;
    }
    doc.title = Some(format!("Colony tile - Ant #{serial}"));
    doc
}

/// The standard card sizes a test can sit on (outer width × height, mm).
const CARDS: [(f64, f64); 3] = [(80.0, 50.0), (120.0, 80.0), (160.0, 100.0)];

/// Put a test design on a Colony tile: the smallest standard card it fits (with a header: the ant mark, the
/// serial number, the test's name, the date and a QR code with the test's settings), or a larger one when it
/// doesn't fit any. Returns the tile and whether it's a standard size.
pub fn on_tile(test: Document, name: &str, code: &str, serial: u32, date: &str) -> (Document, bool) {
    let b = test.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
    let (margin, header) = (5.0, 21.0);
    let need = (b.width() + margin * 2.0, b.height() + margin * 2.0 + header);
    let fit = CARDS.iter().find(|(w, h)| *w >= need.0 && *h >= need.1).copied();
    let (w, h) = fit.unwrap_or(((need.0 / 10.0).ceil() * 10.0, (need.1 / 10.0).ceil() * 10.0));
    let mut doc = Document::default();
    cut_rect(&mut doc, p2(w / 2.0, h / 2.0), w, h, 4.0);
    // (the tile's own shapes are moved to layers 3 and 4 below)
    cut_circle(&mut doc, p2(5.5, h - 5.5), 4.0);
    doc.shapes.extend(ant_mark(11.0, p2(15.0, h - 10.0), ENGRAVE));
    text(&mut doc, &format!("BURN COLONY · Ant #{serial}"), p2(22.0, h - 9.0), 2.8, false, ENGRAVE);
    text(&mut doc, &format!("{name} · {date}"), p2(22.0, h - 13.5), 1.8, false, ENGRAVE);
    doc.shapes.extend(qr(code, 17.0, p2(w - 12.0, h - 11.5), ENGRAVE));
    // the test below the header, its layers kept
    let area_c = p2(w / 2.0, (h - header) / 2.0);
    let d = area_c - b.center();
    let mut test = test;
    for s in &mut test.shapes {
        s.xform = s.xform.then(Xform::translate(d.x, d.y));
    }
    let cuts = test.cuts.clone();
    // the test uses layers 0–2 for its cells, labels and pictures: the tile's own go on 3 (cut) and 4 (text)
    for s in doc.shapes.iter_mut() {
        s.layer = if s.layer == CUT { 3 } else { 4 };
    }
    doc.shapes.extend(test.shapes);
    doc.cuts = cuts;
    doc.sync_cuts();
    fresh_layers(&mut doc, &[3, 4]);
    let c = doc.cut_mut(3);
    (c.mode, c.speed, c.max_power, c.min_power, c.name) = (CutMode::Line, 10.0, 80.0, 80.0, "Tile cut out (check)".into());
    let e = doc.cut_mut(4);
    (e.mode, e.speed, e.max_power, e.min_power, e.interval, e.name) = (CutMode::Fill, 150.0, 30.0, 30.0, 0.1, "Tile engrave (check)".into());
    // cut the tile out last
    doc.cut_mut(3).priority = 100;
    doc.title = Some(format!("Colony tile - {name}"));
    (doc, fit.is_some())
}

/// The BurnAnt benchmark: one standard 80 × 50 mm card that tests a gradient, fine text, small holes, inner
/// and outer cuts and a dithered ant, with boxes to tick and lines to note the settings.
pub fn burnant() -> Document {
    let mut doc = Document::default();
    cut_rect(&mut doc, p2(40.0, 25.0), 80.0, 50.0, 4.0);
    cut_circle(&mut doc, p2(5.5, 44.5), 4.0);
    doc.shapes.extend(ant_mark(11.0, p2(15.0, 40.5), ENGRAVE));
    text(&mut doc, "BurnAnt", p2(22.0, 40.5), 5.0, false, ENGRAVE);
    text(&mut doc, "BurnCraft benchmark v1", p2(22.0, 36.5), 2.0, false, ENGRAVE);
    // the dithered ant
    doc.shapes.extend(ant_picture(20.0, p2(68.0, 37.0), SAMPLE));
    // gradient
    doc.shapes.extend(crate::calibrate::gradient_shape(36.0, 5.0, ImageAdjust { dither: Dither::Grayscale, ..Default::default() }, p2(22.0, 30.0), SAMPLE));
    // fine text
    let mut y = 23.0;
    for h in [2.0, 1.5, 1.0, 0.7, 0.5] {
        text(&mut doc, &format!("BurnAnt {} mm", fmt_num(h)), p2(4.0, y), h, false, ENGRAVE);
        y -= h + 1.4;
    }
    // small holes, cut
    let mut x = 5.0;
    for d in [2.0, 1.5, 1.0, 0.7, 0.5] {
        cut_circle(&mut doc, p2(x + d / 2.0, 7.5), d);
        x += d + 2.0;
    }
    text(&mut doc, "holes", p2(4.0, 3.5), 1.4, false, ENGRAVE);
    // inner and outer cuts: a washer that drops out when both are through (the inside is cut first)
    cut_circle(&mut doc, p2(30.0, 12.0), 11.0);
    cut_circle(&mut doc, p2(30.0, 12.0), 5.0);
    // places to note the results: tick boxes and lines to write on
    for (k, what) in ["gradient", "ant", "text", "holes", "cut"].iter().enumerate() {
        let y = 23.5 - k as f64 * 4.2;
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 2.6, h: 2.6, cr: 0.0 }, Xform::translate(41.0, y), SCORE));
        text(&mut doc, what, p2(43.5, y - 0.8), 1.7, false, ENGRAVE);
    }
    for (k, what) in ["speed", "power", "date"].iter().enumerate() {
        let y = 21.0 - k as f64 * 6.0;
        text(&mut doc, what, p2(57.0, y + 0.8), 1.5, false, ENGRAVE);
        doc.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&[vec![p2(57.0, y), p2(76.0, y)]])), Xform::IDENTITY, SCORE));
    }
    tile_layers(&mut doc, None);
    let s = doc.cut_mut(SAMPLE);
    (s.mode, s.speed, s.max_power, s.min_power, s.interval, s.name) = (CutMode::Image, 150.0, 40.0, 0.0, 0.1, "Pictures (check)".into());
    // inner shapes before outer, the card last
    doc.cut_mut(CUT).priority = 100;
    doc.title = Some("BurnAnt benchmark".into());
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(doc: &Document) -> BBox {
        doc.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()))
    }

    #[test]
    fn the_ant_mark_loads_at_any_size() {
        let s = ant_mark(15.0, p2(10.0, 20.0), ENGRAVE).expect("the ant mark is built in");
        let b = s.bbox();
        assert!((b.height() - 15.0).abs() < 0.05, "{b:?}");
        assert!((b.width() / b.height() - 161.368 / 214.021).abs() < 0.02, "keeps its shape");
        assert!(b.center().dist(p2(10.0, 20.0)) < 0.05);
        assert!(s.world_outline(0.05).iter().all(|l| is_closed(l)), "one closed outline");
    }

    #[test]
    fn dates_and_codes() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(20_415), "2025-11-23");
        assert_eq!(date_of(11_016), "2000-02-29");
        let mut c = CutSetting::new(0);
        (c.speed, c.max_power, c.passes, c.kerf) = (8.0, 85.0, 2, 0.08);
        let m = Material::from_cut(&c, "Birch plywood", 3.0, "My diode");
        assert_eq!(settings_code(&m, 7, "2026-11-23"), "Burn Colony #7|Birch plywood 3mm|My diode|Line 8mm/s 85% x2 kerf0.08 air|2026-11-23");
    }

    #[test]
    fn tiles_and_the_benchmark_fit_their_size_and_burn() {
        let dev = crate::gcode::Device::default();
        let mut c = CutSetting::new(0);
        (c.mode, c.speed, c.max_power) = (CutMode::Fill, 200.0, 40.0);
        let m = Material::from_cut(&c, "Slate", 0.0, "My diode");
        for (size, w, h) in [(TileSize::Card, 80.0, 50.0), (TileSize::LargeCard, 120.0, 80.0), (TileSize::Coin, 60.0, 60.0)] {
            let doc = material_tile(&m, size, 3, "2026-11-23");
            let b = bounds(&doc);
            assert!((b.width() - w).abs() < 0.1 && (b.height() - h).abs() < 0.1, "{size:?}: {b:?}");
            assert!(doc.shapes.iter().any(|s| s.layer == ENGRAVE && matches!(s.kind, ShapeKind::Path(_)) && s.bbox().width() > 10.0), "{size:?} has the QR code");
            let job = crate::gcode::build_job(&doc, &dev, None);
            assert!(job.moves.iter().any(|m| m.power > 0.0));
        }
        // the sample burns with the saved settings
        let doc = material_tile(&m, TileSize::Card, 3, "2026-11-23");
        let s = doc.cut(SAMPLE).unwrap();
        assert_eq!((s.mode, s.speed, s.max_power), (CutMode::Fill, 200.0, 40.0));
        let b = burnant();
        let bb = bounds(&b);
        assert!((bb.width() - 80.0).abs() < 0.01 && (bb.height() - 50.0).abs() < 0.01, "{bb:?}");
        let job = crate::gcode::build_job(&b, &dev, None);
        assert!(job.moves.iter().any(|m| m.power > 0.0 && m.layer == SAMPLE), "the pictures burn");
        // the card's outline is cut last
        let last_cut = job.moves.iter().rposition(|m| m.power > 0.0).unwrap();
        assert_eq!(job.moves[last_cut].layer, CUT);
    }

    #[test]
    fn tests_go_on_the_smallest_card_that_fits() {
        let dev = crate::gcode::Device::default();
        let small = crate::calibrate::TestParams { power: crate::calibrate::Range::new(20.0, 60.0, 3), speed: crate::calibrate::Range::new(100.0, 300.0, 3), cell: 6.0, gap: 2.0, ..Default::default() };
        let (doc, standard) = on_tile(crate::calibrate::generate(&small, &dev), "Power × speed", "BC test", 9, "2026-11-23");
        assert!(standard);
        let b = bounds(&doc);
        assert!(CARDS.iter().any(|(w, h)| (b.width() - w).abs() < 0.01 && (b.height() - h).abs() < 0.01), "{b:?}");
        // the test keeps its own layers and settings
        assert!(doc.shapes.iter().any(|s| s.layer == crate::calibrate::CELLS && !s.cut.is_empty()));
        assert_eq!(doc.shapes.iter().filter(|s| s.layer == 3).count(), 2, "the tile's outline and hole");
        let job = crate::gcode::build_job(&doc, &dev, None);
        assert_eq!(job.skipped_outside, 0);
    }

    #[test]
    fn qr_dots_are_big_enough_to_burn() {
        // the longest usual codes: a material with everything set, and a test's settings
        let mut c = CutSetting::new(0);
        (c.mode, c.speed, c.max_power, c.min_power, c.passes, c.kerf, c.z_offset, c.pulse_width) = (CutMode::Image, 123.45, 67.8, 12.3, 3, 0.08, -1.5, Some(200));
        let m = Material::from_cut(&c, "Walnut veneer on MDF", 12.5, "Workshop CO2 60W");
        let p = crate::calibrate::TestParams::default();
        for (code, size) in [(settings_code(&m, 1234, "2026-11-23"), 17.0), (settings_code(&m, 1234, "2026-11-23"), 16.0), (crate::calibrate::params_code(&p, 1234, "2026-11-23"), 17.0)] {
            let q = crate::barcode::encode_qr_compact(&code).unwrap();
            let module = size / q.w as f64;
            assert!(module >= 0.35, "{} modules at {size} mm: {module:.2} mm dots for {code}", q.w);
        }
        // a usual material: bigger dots
        let mut c = CutSetting::new(0);
        (c.speed, c.max_power, c.passes) = (8.0, 85.0, 2);
        let usual = settings_code(&Material::from_cut(&c, "Birch plywood", 3.0, "My diode"), 12, "2026-11-23");
        let q = crate::barcode::encode_qr_compact(&usual).unwrap();
        assert!(16.0 / q.w as f64 >= 0.45, "{} modules", q.w);
    }
}
