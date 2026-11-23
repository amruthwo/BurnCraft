//! BurnCraft's own project file, `.burn`: a zip holding
//!
//! - `burncraft.json`: the format name and version, the BurnCraft that wrote it, and the whole project
//!   (shapes, layers and their settings, and everything BurnCraft-only: relations, editable offsets and arrays,
//!   variable text, notes). Fields this version doesn't know are kept and written back.
//! - `images/…`: the pictures as real PNG / JPEG files (exactly as they came in).
//! - `thumbnail.png`: a small picture of the design.
//!
//! Fonts are never embedded, only their names. Reading is careful: only these names are read, nothing outside
//! the file is followed, odd paths are refused, and sizes are limited.

use crate::geom::*;
use crate::model::*;
use crate::xml::Node;
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::sync::Arc;

pub const FORMAT: &str = "BurnCraft project";
/// The format version this BurnCraft writes (and the newest it fully understands).
pub const VERSION: u64 = 1;
/// The file type, for file managers.
#[allow(dead_code)]
pub const MIME: &str = "application/x-burncraft-project";
/// Largest single entry, and all entries together, when reading (uncompressed).
const MAX_ENTRY: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
/// Fields a `.burn` had that this BurnCraft doesn't know are kept in the "extra" slots under this prefix.
pub const KEEP: &str = "burn:";

/// A `.burn` file read back.
pub struct Loaded {
    pub doc: Document,
    /// Parts of a LightBurn project BurnCraft doesn't edit (kept so `.lbrn2` saves stay complete).
    pub kept: Vec<Node>,
    pub warnings: Vec<String>,
}

/// Is this a `.burn` file (a zip with `burncraft.json`)?
pub fn looks_like(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04") && data.windows(14).any(|w| w == b"burncraft.json")
}

// ----------------------------------------------------------------------------------------------- writing

/// The project as a `.burn` file.
pub fn save(doc: &Document, kept: &[Node]) -> Result<Vec<u8>, String> {
    let mut images = Images::default();
    let mut project = Map::new();
    if let Some(t) = &doc.title {
        project.insert("title".into(), json!(t));
    }
    project.insert("notes".into(), json!(doc.notes));
    project.insert("material_height".into(), json!(doc.material_height));
    project.insert("mirror_x".into(), json!(doc.mirror_x));
    project.insert("mirror_y".into(), json!(doc.mirror_y));
    let (root_extra, root_kept) = split_extra(&doc.extra_root);
    if !root_extra.is_empty() {
        project.insert("lbrn_attributes".into(), Value::Object(root_extra));
    }
    if !kept.is_empty() {
        let xml: Vec<Value> = kept
            .iter()
            .map(|n| {
                let mut s = String::new();
                n.write(&mut s, 0);
                json!(s)
            })
            .collect();
        project.insert("lbrn_other".into(), Value::Array(xml));
    }
    project.insert("layers".into(), Value::Array(doc.cuts.iter().map(cut_json).collect()));
    project.insert("shapes".into(), Value::Array(doc.shapes.iter().map(|s| shape_json(s, &mut images)).collect()));
    for (k, v) in crate::relations::to_file(doc) {
        let key = match k {
            "BcRelations" => "relations",
            "BcOffsets" => "offsets",
            "BcArrays" => "arrays",
            _ => continue,
        };
        if let Ok(v) = serde_json::from_str::<Value>(&v) {
            project.insert(key.into(), v);
        }
    }
    let has_variable = doc.shapes.iter().any(|s| matches!(&s.kind, ShapeKind::Text { style, .. } if style.variable));
    if has_variable || !doc.vartext.rows.is_empty() {
        if let Ok(v) = serde_json::to_value(&doc.vartext) {
            project.insert("variable_text".into(), v);
        }
    }
    // fields from a newer BurnCraft, written back as they were
    let mut file = Map::new();
    for (k, v) in root_kept {
        match k.strip_prefix("^") {
            Some(top) => file.insert(top.to_string(), v),
            None => project.insert(k, v),
        };
    }
    file.insert("format".into(), json!(FORMAT));
    file.insert("format_version".into(), json!(VERSION));
    file.insert("written_by".into(), json!(format!("BurnCraft {}", env!("CARGO_PKG_VERSION"))));
    file.insert("project".into(), Value::Object(project));
    let text = serde_json::to_string_pretty(&Value::Object(file)).map_err(|e| e.to_string())?;

    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut out);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("burncraft.json", deflate).map_err(|e| e.to_string())?;
        z.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        // pictures are already compressed: stored as they are
        for (name, bytes) in &images.files {
            z.start_file(format!("images/{name}"), stored).map_err(|e| e.to_string())?;
            z.write_all(bytes).map_err(|e| e.to_string())?;
        }
        if let Some(png) = thumbnail(doc) {
            z.start_file("thumbnail.png", stored).map_err(|e| e.to_string())?;
            z.write_all(&png).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}

/// Pictures collected while writing: each distinct picture once.
#[derive(Default)]
struct Images {
    files: Vec<(String, Arc<Vec<u8>>)>,
}

impl Images {
    fn add(&mut self, data: &BitmapData) -> String {
        let bytes: Arc<Vec<u8>> = if data.encoded.is_empty() { Arc::new(crate::platform::png_bytes(data.width, data.height, data.rgba.as_ref().clone()).unwrap_or_default()) } else { data.encoded.clone() };
        if let Some((n, _)) = self.files.iter().find(|(_, b)| Arc::ptr_eq(b, &bytes) || **b == *bytes) {
            return n.clone();
        }
        let name = format!("{}.{}", self.files.len() + 1, image_ext(&bytes));
        self.files.push((name.clone(), bytes));
        name
    }
}

fn image_ext(b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") {
        "png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if b.starts_with(b"GIF8") {
        "gif"
    } else if b.len() > 12 && &b[8..12] == b"WEBP" {
        "webp"
    } else if b.starts_with(b"BM") {
        "bmp"
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        "tif"
    } else {
        "img"
    }
}

/// Attributes kept for LightBurn, and fields kept from a newer `.burn` (prefixed `burn:`, values as JSON).
fn split_extra(extra: &[(String, String)]) -> (Map<String, Value>, Vec<(String, Value)>) {
    let mut lbrn = Map::new();
    let mut kept = Vec::new();
    for (k, v) in extra {
        match k.strip_prefix(KEEP) {
            Some(key) => kept.push((key.to_string(), serde_json::from_str(v).unwrap_or(Value::String(v.clone())))),
            None => {
                lbrn.insert(k.clone(), json!(v));
            }
        }
    }
    (lbrn, kept)
}

fn xform_json(x: &Xform) -> Value {
    json!([x.a, x.b, x.c, x.d, x.e, x.f])
}

fn path_json(pd: &PathData) -> Value {
    let pts: Vec<Value> = pd
        .verts
        .iter()
        .map(|v| {
            let mut o = Map::new();
            o.insert("x".into(), json!(v.p.x));
            o.insert("y".into(), json!(v.p.y));
            if let Some(c) = v.c0 {
                o.insert("out".into(), json!([c.x, c.y]));
            }
            if let Some(c) = v.c1 {
                o.insert("in".into(), json!([c.x, c.y]));
            }
            Value::Object(o)
        })
        .collect();
    let segs: Vec<Value> = pd.prims.iter().map(|p| json!([if p.kind == PrimKind::Bezier { "curve" } else { "line" }, p.a, p.b])).collect();
    json!({ "points": pts, "segments": segs })
}

fn shape_json(s: &Shape, images: &mut Images) -> Value {
    let mut o = Map::new();
    let (ty, fields): (&str, Value) = match &s.kind {
        ShapeKind::Path(pd) => ("path", path_json(pd)),
        ShapeKind::Rect { w, h, cr } => ("rectangle", json!({ "width": w, "height": h, "corner_radius": cr })),
        ShapeKind::Ellipse { rx, ry } => ("ellipse", json!({ "radius_x": rx, "radius_y": ry })),
        ShapeKind::Polygon { n, rx, ry } => ("polygon", json!({ "sides": n, "radius_x": rx, "radius_y": ry })),
        ShapeKind::Text { text, font, height, style, backup } => (
            "text",
            json!({ "text": text, "font": font, "height": height, "case": format!("{:?}", style.case), "weld": style.weld, "letter_spacing": style.letter_spacing, "variable": style.variable, "outline": backup.as_ref().map(path_json) }),
        ),
        ShapeKind::Bitmap { w, h, data, adjust } => (
            "picture",
            json!({ "width": w, "height": h, "file": format!("images/{}", images.add(data)), "source_file": data.source_file, "brightness": adjust.brightness, "contrast": adjust.contrast, "gamma": adjust.gamma, "invert": adjust.invert, "dither": format!("{:?}", adjust.dither), "halftone_lpi": adjust.halftone_lpi, "halftone_angle": adjust.halftone_angle, "slices": adjust.slices }),
        ),
        ShapeKind::Group(ch) => ("group", json!({ "children": ch.iter().map(|c| shape_json(c, images)).collect::<Vec<_>>() })),
    };
    o.insert("type".into(), json!(ty));
    if let Value::Object(f) = fields {
        o.extend(f);
    }
    o.insert("layer".into(), json!(s.layer));
    o.insert("transform".into(), xform_json(&s.xform));
    if s.locked {
        o.insert("locked".into(), json!(true));
    }
    if !s.name.is_empty() {
        o.insert("name".into(), json!(s.name));
    }
    if !s.tabs.is_empty() {
        o.insert("tabs".into(), json!(s.tabs.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()));
    }
    if !s.cut.is_empty() {
        o.insert("settings".into(), serde_json::to_value(s.cut).unwrap_or(Value::Null));
    }
    let (lbrn, kept) = split_extra(&s.extra_attrs);
    if !lbrn.is_empty() {
        o.insert("lbrn_attributes".into(), Value::Object(lbrn));
    }
    for (k, v) in kept {
        o.insert(k, v);
    }
    Value::Object(o)
}

fn cut_json(c: &CutSetting) -> Value {
    let mut o = json!({
        "index": c.index, "name": c.name, "mode": format!("{:?}", c.mode), "speed": c.speed, "min_power": c.min_power,
        "max_power": c.max_power, "passes": c.passes, "interval": c.interval, "angle": c.angle, "cross_hatch": c.cross_hatch,
        "bidirectional": c.bidir, "overscan_percent": c.overscan_percent, "priority": c.priority, "output": c.output,
        "hide": c.hide, "air_assist": c.air_assist, "z_offset": c.z_offset, "kerf": c.kerf, "frequency": c.frequency,
        "fill_individually": c.fill_individually, "constant_power": c.force_constant_power,
        "tabs": serde_json::to_value(c.tabs).unwrap_or(Value::Null),
    });
    if let Value::Object(m) = &mut o {
        if !c.material.is_empty() {
            m.insert("material".into(), json!(c.material));
        }
        if let Some(p) = c.pulse_width {
            m.insert("pulse_width".into(), json!(p));
        }
    }
    let (lbrn, kept) = split_extra(&c.extra);
    if let Value::Object(m) = &mut o {
        if !lbrn.is_empty() {
            m.insert("lbrn_attributes".into(), Value::Object(lbrn));
        }
        for (k, v) in kept {
            m.insert(k, v);
        }
    }
    o
}

/// A small picture of the design (at most 512 pixels across).
fn thumbnail(doc: &Document) -> Option<Vec<u8>> {
    let b = doc.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
    if b.is_empty() {
        return None;
    }
    let frame = BBox { min: b.min - p2(2.0, 2.0), max: b.max + p2(2.0, 2.0) };
    let dpi = (512.0 * 25.4 / frame.width().max(frame.height())).min(300.0);
    let fill = |l: usize| doc.cut(l).is_some_and(|c| matches!(c.mode, CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill));
    let (w, h, rgba) = crate::raster::render(&doc.shapes, frame, dpi, true, &fill).ok()?;
    crate::platform::png_bytes(w, h, rgba)
}

// ----------------------------------------------------------------------------------------------- reading

/// Read a `.burn` file.
pub fn load(data: &[u8]) -> Result<Loaded, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| "This isn't a BurnCraft project (.burn) file, or it's damaged.".to_string())?;
    if zip.len() > MAX_ENTRIES {
        return Err("This .burn file has too many parts to be a BurnCraft project.".into());
    }
    let mut json_text: Option<String> = None;
    let mut images: std::collections::HashMap<String, Vec<u8>> = Default::default();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("This .burn file is damaged: {e}"))?;
        let name = f.name().map(|n| n.to_string()).map_err(|_| "This .burn file has a part with an unreadable name, so it wasn't opened.".to_string())?;
        // refuse odd names (paths that climb out or start at the root), and only read the expected parts
        if f.enclosed_name().is_none() || name.contains('\\') || name.starts_with('/') || name.split('/').any(|p| p == "..") {
            return Err(format!("This .burn file has a part with an unsafe name ({name}), so it wasn't opened."));
        }
        let wanted = name == "burncraft.json" || name.strip_prefix("images/").is_some_and(safe_image_name);
        if !wanted {
            continue;
        }
        if f.size() > MAX_ENTRY {
            return Err(format!("A part of this .burn file is too big ({name})."));
        }
        let mut buf = Vec::new();
        // the size in the header can lie: never read more than the limit
        (&mut f).take(MAX_ENTRY + 1).read_to_end(&mut buf).map_err(|e| format!("This .burn file is damaged: {e}"))?;
        if buf.len() as u64 > MAX_ENTRY {
            return Err(format!("A part of this .burn file is too big ({name})."));
        }
        total += buf.len() as u64;
        if total > MAX_TOTAL {
            return Err("This .burn file unpacks to more than 1 GB, so it wasn't opened.".into());
        }
        if name == "burncraft.json" {
            json_text = Some(String::from_utf8(buf).map_err(|_| "This .burn file's project data is damaged.".to_string())?);
        } else {
            images.insert(name, buf);
        }
    }
    let text = json_text.ok_or("This .burn file has no project in it.")?;
    let file: Value = serde_json::from_str(&text).map_err(|e| format!("This .burn file's project data is damaged: {e}"))?;
    if file.get("format").and_then(|v| v.as_str()) != Some(FORMAT) {
        return Err("This isn't a BurnCraft project (.burn) file.".into());
    }
    let mut warnings = Vec::new();
    let version = file.get("format_version").and_then(|v| v.as_u64()).unwrap_or(1);
    if version > VERSION {
        let by = file.get("written_by").and_then(|v| v.as_str()).unwrap_or("a newer BurnCraft");
        warnings.push(format!("This project was made by a newer BurnCraft ({by}). Everything this version understands was opened; update BurnCraft to see the rest."));
    }
    let p = file.get("project").and_then(|v| v.as_object()).ok_or("This .burn file has no project in it.")?;
    let mut doc = Document::default();
    let mut ctx = ReadCtx { images: &images, warnings: &mut warnings };
    doc.title = p.get("title").and_then(|v| v.as_str()).map(String::from);
    doc.notes = s(p, "notes");
    doc.material_height = f(p, "material_height", 0.0);
    doc.mirror_x = b(p, "mirror_x", false);
    doc.mirror_y = b(p, "mirror_y", false);
    if let Some(Value::Object(m)) = p.get("lbrn_attributes") {
        doc.extra_root = m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect();
    }
    let mut kept = Vec::new();
    if let Some(Value::Array(a)) = p.get("lbrn_other") {
        for x in a.iter().filter_map(|v| v.as_str()) {
            if let Ok(n) = crate::xml::parse(x) {
                kept.push(n);
            }
        }
    }
    if let Some(Value::Array(a)) = p.get("layers") {
        for c in a.iter().filter_map(|v| v.as_object()) {
            let cs = cut_from(c);
            doc.cuts.retain(|x| x.index != cs.index);
            doc.cuts.push(cs);
        }
    }
    doc.cuts.sort_by_key(|c| c.index);
    if let Some(Value::Array(a)) = p.get("shapes") {
        doc.shapes = a.iter().filter_map(|v| v.as_object()).filter_map(|o| ctx.shape(o, 0)).collect();
    }
    let link = |k: &str| p.get(k).map(|v| v.to_string());
    crate::relations::from_file(&mut doc, link("relations").as_deref(), link("offsets").as_deref(), link("arrays").as_deref());
    if let Some(v) = p.get("variable_text").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        doc.vartext = v;
    }
    // fields this version doesn't know: kept, to be written back
    const KNOWN: &[&str] = &["title", "notes", "material_height", "mirror_x", "mirror_y", "lbrn_attributes", "lbrn_other", "layers", "shapes", "relations", "offsets", "arrays", "variable_text"];
    for (k, v) in p.iter().filter(|(k, _)| !KNOWN.contains(&k.as_str())) {
        doc.extra_root.push((format!("{KEEP}{k}"), v.to_string()));
    }
    for (k, v) in file.as_object().into_iter().flatten().filter(|(k, _)| !["format", "format_version", "written_by", "project"].contains(&k.as_str())) {
        doc.extra_root.push((format!("{KEEP}^{k}"), v.to_string()));
    }
    Ok(Loaded { doc, kept, warnings })
}

fn safe_image_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 100 && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) && !n.starts_with('.')
}

fn s(o: &Map<String, Value>, k: &str) -> String {
    o.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}
fn f(o: &Map<String, Value>, k: &str, d: f64) -> f64 {
    o.get(k).and_then(|v| v.as_f64()).unwrap_or(d)
}
fn b(o: &Map<String, Value>, k: &str, d: bool) -> bool {
    o.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
}
fn pt(v: &Value) -> Option<P2> {
    let a = v.as_array()?;
    Some(p2(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

fn cut_from(c: &Map<String, Value>) -> CutSetting {
    let index = c.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let d = CutSetting::new(index);
    let mode = c.get("mode").and_then(|v| v.as_str()).and_then(|m| CutMode::ALL.into_iter().find(|x| format!("{x:?}") == m)).unwrap_or(d.mode);
    const KNOWN: &[&str] = &["index", "name", "mode", "speed", "min_power", "max_power", "passes", "interval", "angle", "cross_hatch", "bidirectional", "overscan_percent", "priority", "output", "hide", "air_assist", "z_offset", "kerf", "frequency", "fill_individually", "constant_power", "tabs", "material", "pulse_width", "lbrn_attributes"];
    let mut extra: Vec<(String, String)> = match c.get("lbrn_attributes") {
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect(),
        _ => Vec::new(),
    };
    extra.extend(c.iter().filter(|(k, _)| !KNOWN.contains(&k.as_str())).map(|(k, v)| (format!("{KEEP}{k}"), v.to_string())));
    CutSetting {
        index,
        name: c.get("name").and_then(|v| v.as_str()).map(String::from).unwrap_or(d.name.clone()),
        mode,
        speed: f(c, "speed", d.speed),
        min_power: f(c, "min_power", d.min_power),
        max_power: f(c, "max_power", d.max_power),
        passes: c.get("passes").and_then(|v| v.as_u64()).unwrap_or(d.passes as u64) as u32,
        interval: f(c, "interval", d.interval),
        angle: f(c, "angle", d.angle),
        cross_hatch: b(c, "cross_hatch", d.cross_hatch),
        bidir: b(c, "bidirectional", d.bidir),
        overscan_percent: f(c, "overscan_percent", d.overscan_percent),
        priority: c.get("priority").and_then(|v| v.as_i64()).unwrap_or(d.priority as i64) as i32,
        output: b(c, "output", d.output),
        hide: b(c, "hide", d.hide),
        air_assist: b(c, "air_assist", d.air_assist),
        z_offset: f(c, "z_offset", d.z_offset),
        kerf: f(c, "kerf", d.kerf),
        frequency: f(c, "frequency", d.frequency),
        fill_individually: b(c, "fill_individually", d.fill_individually),
        force_constant_power: b(c, "constant_power", d.force_constant_power),
        tabs: c.get("tabs").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        pulse_width: c.get("pulse_width").and_then(|v| v.as_u64()).map(|v| v.clamp(1, 65535) as u16),
        material: c.get("material").and_then(|v| v.as_str()).unwrap_or_default().chars().take(200).collect(),
        extra,
    }
}

struct ReadCtx<'a> {
    images: &'a std::collections::HashMap<String, Vec<u8>>,
    warnings: &'a mut Vec<String>,
}

impl ReadCtx<'_> {
    fn path(&self, v: &Value) -> Option<PathData> {
        let o = v.as_object()?;
        let verts: Vec<Vert> = o
            .get("points")?
            .as_array()?
            .iter()
            .map(|p| Vert { p: p2(p.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0), p.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0)), c0: p.get("out").and_then(pt), c1: p.get("in").and_then(pt) })
            .collect();
        let n = verts.len();
        let prims: Vec<Prim> = o
            .get("segments")?
            .as_array()?
            .iter()
            .filter_map(|s| {
                let a = s.as_array()?;
                let kind = if a.first()?.as_str()? == "curve" { PrimKind::Bezier } else { PrimKind::Line };
                let (pa, pb) = (a.get(1)?.as_u64()? as usize, a.get(2)?.as_u64()? as usize);
                (pa < n && pb < n).then_some(Prim { kind, a: pa, b: pb })
            })
            .collect();
        Some(PathData { verts, prims })
    }

    fn shape(&mut self, o: &Map<String, Value>, depth: usize) -> Option<Shape> {
        if depth > 64 {
            return None;
        }
        let kind = match o.get("type")?.as_str()? {
            "path" => ShapeKind::Path(self.path(&Value::Object(o.clone()))?),
            "rectangle" => ShapeKind::Rect { w: f(o, "width", 10.0), h: f(o, "height", 10.0), cr: f(o, "corner_radius", 0.0) },
            "ellipse" => ShapeKind::Ellipse { rx: f(o, "radius_x", 5.0), ry: f(o, "radius_y", 5.0) },
            "polygon" => ShapeKind::Polygon { n: o.get("sides").and_then(|v| v.as_u64()).unwrap_or(6).clamp(3, 1000) as u32, rx: f(o, "radius_x", 5.0), ry: f(o, "radius_y", 5.0) },
            "text" => {
                let case = o.get("case").and_then(|v| v.as_str()).and_then(|c| TextCase::ALL.into_iter().find(|x| format!("{x:?}") == c)).unwrap_or_default();
                let style = TextStyle { case, weld: b(o, "weld", false), letter_spacing: f(o, "letter_spacing", 0.0), variable: b(o, "variable", false) };
                ShapeKind::Text { text: s(o, "text"), font: s(o, "font"), height: f(o, "height", 10.0), style, backup: o.get("outline").and_then(|v| self.path(v)) }
            }
            "picture" => {
                let file = s(o, "file");
                let Some(bytes) = self.images.get(&file) else {
                    self.warnings.push(format!("A picture ({file}) is missing from this .burn file."));
                    return None;
                };
                let Some(mut data) = crate::lbrn::decode_bitmap(bytes.clone(), &s(o, "source_file")) else {
                    self.warnings.push(format!("A picture ({file}) in this .burn file couldn't be read."));
                    return None;
                };
                data.source_file = s(o, "source_file");
                let dither = o.get("dither").and_then(|v| v.as_str()).and_then(|d| Dither::ALL.into_iter().find(|x| format!("{x:?}") == d)).unwrap_or(Dither::Grayscale);
                let adjust = ImageAdjust {
                    brightness: f(o, "brightness", 0.0),
                    contrast: f(o, "contrast", 0.0),
                    gamma: f(o, "gamma", 1.0),
                    invert: b(o, "invert", false),
                    dither,
                    halftone_lpi: f(o, "halftone_lpi", ImageAdjust::DEFAULT_LPI).clamp(1.0, 300.0),
                    halftone_angle: f(o, "halftone_angle", ImageAdjust::DEFAULT_ANGLE),
                    slices: o.get("slices").and_then(|v| v.as_u64()).unwrap_or(ImageAdjust::DEFAULT_SLICES as u64).clamp(1, 255) as u32,
                };
                ShapeKind::Bitmap { w: f(o, "width", 10.0), h: f(o, "height", 10.0), data, adjust }
            }
            "group" => {
                let ch: Vec<Shape> = o.get("children").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|c| c.as_object()).filter_map(|c| self.shape(c, depth + 1)).collect();
                ShapeKind::Group(ch)
            }
            other => {
                self.warnings.push(format!("A kind of shape this BurnCraft doesn't know (\u{201c}{other}\u{201d}) was left out."));
                return None;
            }
        };
        let t: Vec<f64> = o.get("transform").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
        let xform = if t.len() == 6 { Xform { a: t[0], b: t[1], c: t[2], d: t[3], e: t[4], f: t[5] } } else { Xform::IDENTITY };
        let mut sh = Shape::new(kind, xform, o.get("layer").and_then(|v| v.as_u64()).unwrap_or(0) as usize);
        sh.locked = b(o, "locked", false);
        sh.name = s(o, "name");
        sh.cut = o.get("settings").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        sh.tabs = o.get("tabs").and_then(|v| v.as_array()).into_iter().flatten().filter_map(pt).filter(|p| p.x.is_finite() && p.y.is_finite()).take(10_000).collect();
        if let Some(Value::Object(m)) = o.get("lbrn_attributes") {
            sh.extra_attrs = m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect();
        }
        const KNOWN: &[&str] = &[
            "type", "layer", "transform", "locked", "name", "tabs", "settings", "lbrn_attributes", "points", "segments", "width", "height", "corner_radius", "radius_x", "radius_y", "sides", "text", "font", "case", "weld", "letter_spacing", "variable", "outline", "file", "source_file", "brightness", "contrast", "gamma", "invert", "dither", "halftone_lpi", "halftone_angle", "slices", "children",
        ];
        for (k, v) in o.iter().filter(|(k, _)| !KNOWN.contains(&k.as_str())) {
            sh.extra_attrs.push((format!("{KEEP}{k}"), v.to_string()));
        }
        Some(sh)
    }
}

/// How many fields from a newer BurnCraft are in the project (they can't go into a `.lbrn2`).
pub fn newer_fields(doc: &Document) -> usize {
    fn count(s: &Shape) -> usize {
        s.extra_attrs.iter().filter(|(k, _)| k.starts_with(KEEP)).count() + if let ShapeKind::Group(ch) = &s.kind { ch.iter().map(count).sum() } else { 0 }
    }
    doc.extra_root.iter().filter(|(k, _)| k.starts_with(KEEP)).count() + doc.cuts.iter().map(|c| c.extra.iter().filter(|(k, _)| k.starts_with(KEEP)).count()).sum::<usize>() + doc.shapes.iter().map(count).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project with one of everything.
    fn project() -> Document {
        let mut doc = Document::default();
        doc.title = Some("Coasters".into());
        doc.notes = "oak, 3 mm".into();
        doc.material_height = 3.0;
        let mut pd = PathData::default();
        crate::sketch::append_path(&mut pd, &crate::sketch::circle_path(p2(0.0, 0.0), 5.0));
        pd.verts.push(Vert::new(p2(20.0, 0.0)));
        pd.verts.push(Vert::new(p2(30.0, 5.0)));
        let n = pd.verts.len();
        pd.prims.push(Prim { kind: PrimKind::Line, a: n - 2, b: n - 1 });
        let mut path = Shape::new(ShapeKind::Path(pd), Xform::translate(50.0, 50.0), 1);
        path.name = "logo".into();
        path.locked = true;
        doc.shapes.push(path);
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 30.0, h: 20.0, cr: 3.0 }, Xform::translate(100.0, 50.0).then_after(Xform::rotate(0.3)), 2));
        let text_style = TextStyle { case: TextCase::ALL[1], weld: true, letter_spacing: 1.5, variable: true };
        doc.shapes.push(Shape::new(ShapeKind::Text { text: "Hi {Name}".into(), font: "Sans,-1,100,5,50,0,0,0,0,0".into(), height: 8.0, style: text_style, backup: None }, Xform::translate(20.0, 20.0), 0));
        let png = crate::platform::png_bytes(3, 2, (0..24).map(|i| (i * 10) as u8).collect()).unwrap();
        let bd = crate::lbrn::decode_bitmap(png, "photo.png").unwrap();
        let adjust = ImageAdjust { brightness: 10.0, contrast: -5.0, gamma: 1.2, invert: true, dither: Dither::Jarvis, ..Default::default() };
        doc.shapes.push(Shape::new(ShapeKind::Bitmap { w: 30.0, h: 20.0, data: bd.clone(), adjust: adjust.clone() }, Xform::translate(150.0, 50.0), 4));
        // the same picture twice is stored once
        doc.shapes.push(Shape::new(ShapeKind::Bitmap { w: 15.0, h: 10.0, data: bd, adjust }, Xform::translate(150.0, 90.0), 4));
        let child = Shape::new(ShapeKind::Ellipse { rx: 4.0, ry: 2.0 }, Xform::translate(5.0, 0.0), 3);
        doc.shapes.push(Shape::new(ShapeKind::Group(vec![child]), Xform::translate(10.0, 80.0), 3));
        doc.sync_cuts();
        doc.cut_mut(2).speed = 42.0;
        doc.cut_mut(2).mode = CutMode::Fill;
        doc.cut_mut(2).extra.push(("someLightBurnKey".into(), "7".into()));
        doc.vartext.rows = vec![vec!["Ada".into()]];
        let a = doc.shapes[0].id;
        let b = doc.shapes[1].id;
        doc.relations.push(crate::relations::Relation { kind: crate::relations::RelKind::Horizontal, ents: vec![crate::relations::Ent::Line { shape: a, a: 0, b: 1 }], value: 0.0 });
        let _ = b;
        doc
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        for (n, b) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn a_round_trip_keeps_everything() {
        let doc = project();
        let bytes = save(&doc, &[]).unwrap();
        assert!(looks_like(&bytes));
        let back = load(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let d = &back.doc;
        assert_eq!(d.title.as_deref(), Some("Coasters"));
        assert_eq!(d.notes, "oak, 3 mm");
        assert_eq!(d.shapes.len(), doc.shapes.len());
        for (a, b) in doc.shapes.iter().zip(&d.shapes) {
            assert_eq!(a.xform, b.xform);
            assert_eq!((a.layer, a.locked, &a.name), (b.layer, b.locked, &b.name));
            assert_eq!(format!("{:?}", strip(&a.kind)), format!("{:?}", strip(&b.kind)));
        }
        // pictures byte-for-byte, stored once
        if let (ShapeKind::Bitmap { data: a, .. }, ShapeKind::Bitmap { data: b, adjust, .. }) = (&doc.shapes[3].kind, &d.shapes[3].kind) {
            assert_eq!(a.encoded, b.encoded);
            assert_eq!(adjust.dither, Dither::Jarvis);
            assert!(adjust.invert);
        } else {
            panic!("a picture");
        }
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(z.file_names().filter(|n| n.as_ref().is_ok_and(|n| n.starts_with("images/"))).count(), 1);
        assert!(z.by_name("thumbnail.png").is_ok());
        // layers and their settings, including LightBurn keys BurnCraft doesn't edit
        let c = d.cut(2).unwrap();
        assert_eq!((c.speed, c.mode), (42.0, CutMode::Fill));
        assert!(c.extra.contains(&("someLightBurnKey".into(), "7".into())));
        // BurnCraft-only things
        assert_eq!(d.vartext.rows, doc.vartext.rows);
        assert_eq!(d.relations.len(), 1);
        assert_eq!(d.relations[0].ents, vec![crate::relations::Ent::Line { shape: d.shapes[0].id, a: 0, b: 1 }]);
        // and once more gives the same file content
        let again = save(d, &back.kept).unwrap();
        let j = |b: &[u8]| {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
            let mut s = String::new();
            z.by_name("burncraft.json").unwrap().read_to_string(&mut s).unwrap();
            s
        };
        assert_eq!(j(&bytes), j(&again));
    }

    /// Text shapes keep a cached outline that isn't saved; compare without it.
    fn strip(k: &ShapeKind) -> ShapeKind {
        match k {
            ShapeKind::Bitmap { w, h, .. } => ShapeKind::Rect { w: *w, h: *h, cr: -1.0 },
            ShapeKind::Group(ch) => ShapeKind::Group(ch.iter().map(|c| Shape { id: 0, kind: strip(&c.kind), ..c.clone() }).collect()),
            k => k.clone(),
        }
    }

    #[test]
    fn unknown_fields_survive_and_a_newer_version_warns() {
        let bytes = save(&project(), &[]).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let mut s = String::new();
        z.by_name("burncraft.json").unwrap().read_to_string(&mut s).unwrap();
        let mut v: Value = serde_json::from_str(&s).unwrap();
        // a newer BurnCraft added things at every level
        v["format_version"] = json!(2);
        v["future_top"] = json!({ "x": 1 });
        v["project"]["material_library"] = json!(["oak"]);
        v["project"]["shapes"][1]["engrave_depth"] = json!([0.5, 0.25]);
        v["project"]["layers"][0]["coolant"] = json!("oak 3 mm");
        let img = {
            let mut b = Vec::new();
            z.by_name("images/1.png").unwrap().read_to_end(&mut b).unwrap();
            b
        };
        let newer = zip_with(&[("burncraft.json", v.to_string().as_bytes()), ("images/1.png", &img)]);
        let l = load(&newer).unwrap();
        assert!(l.warnings.iter().any(|w| w.contains("newer BurnCraft")));
        assert_eq!(newer_fields(&l.doc), 4);
        let resaved = save(&l.doc, &l.kept).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&resaved)).unwrap();
        let mut s = String::new();
        z.by_name("burncraft.json").unwrap().read_to_string(&mut s).unwrap();
        let w: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(w["future_top"], json!({ "x": 1 }));
        assert_eq!(w["project"]["material_library"], json!(["oak"]));
        assert_eq!(w["project"]["shapes"][1]["engrave_depth"], json!([0.5, 0.25]));
        assert_eq!(w["project"]["layers"][0]["coolant"], json!("oak 3 mm"));
        // a LightBurn save leaves them out (and says so: see newer_fields), but stays readable
        let lbrn = crate::lbrn::save_string(&l.doc, &l.kept);
        assert!(!lbrn.contains("burn:"));
        assert!(crate::lbrn::load_str(&lbrn).is_ok());
    }

    #[test]
    fn unsafe_or_oversized_files_are_refused() {
        let ok = br#"{"format":"BurnCraft project","format_version":1,"project":{"shapes":[]}}"#;
        assert!(load(&zip_with(&[("burncraft.json", ok)])).is_ok());
        assert!(load(&zip_with(&[("burncraft.json", ok), ("../evil.png", b"x")])).is_err(), "a path climbing out");
        assert!(load(&zip_with(&[("burncraft.json", ok), ("images/../../x", b"x")])).is_err());
        assert!(load(&zip_with(&[("/etc/passwd", b"x"), ("burncraft.json", ok)])).is_err(), "an absolute path");
        // unexpected parts are never read
        assert!(load(&zip_with(&[("burncraft.json", ok), ("other/thing.bin", b"x")])).is_ok());
        // a picture the project names but that isn't there: said, the rest opens
        let missing = br#"{"format":"BurnCraft project","format_version":1,"project":{"shapes":[{"type":"picture","file":"images/9.png","transform":[1,0,0,1,0,0]},{"type":"ellipse","radius_x":2,"radius_y":2}]}}"#;
        let l = load(&zip_with(&[("burncraft.json", missing)])).unwrap();
        assert_eq!(l.doc.shapes.len(), 1);
        assert!(l.warnings[0].contains("missing"));
        // too big: the part says it's bigger than the limit
        let mut out = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut out);
            z.start_file("burncraft.json", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true)).unwrap();
            let zeros = vec![b' '; 1 << 20];
            for _ in 0..(MAX_ENTRY / (1 << 20) + 1) {
                z.write_all(&zeros).unwrap();
            }
            z.finish().unwrap();
        }
        let e = load(&out.into_inner()).err().unwrap();
        assert!(e.contains("too big"), "{e}");
        // not a .burn at all
        assert!(load(b"hello").is_err());
        assert!(load(&zip_with(&[("burncraft.json", br#"{"format":"something else"}"#)])).is_err());
    }

    #[test]
    fn lbrn2_to_burn_to_lbrn2() {
        let doc = project();
        let lbrn = crate::lbrn::save_string(&doc, &[]);
        let l1 = crate::lbrn::load_str(&lbrn).unwrap();
        let burn = save(&l1.doc, &l1.kept).unwrap();
        let l2 = load(&burn).unwrap();
        let lbrn2 = crate::lbrn::save_string(&l2.doc, &l2.kept);
        let l3 = crate::lbrn::load_str(&lbrn2).unwrap();
        assert_eq!(l3.doc.shapes.len(), doc.shapes.len());
        for (a, b) in l1.doc.shapes.iter().zip(&l3.doc.shapes) {
            assert!(a.bbox().min.dist(b.bbox().min) < 1e-6 && a.bbox().max.dist(b.bbox().max) < 1e-6);
        }
        assert_eq!(l3.doc.cut(2).map(|c| c.speed), Some(42.0));
    }
}
