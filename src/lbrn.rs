//! Reading and writing LightBurn projects (.lbrn2 and the older .lbrn).

use crate::geom::*;
use crate::model::*;
use crate::xml::{self, Node};
use base64::Engine;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct Shared {
    verts: HashMap<String, Vec<Vert>>,
    prims: HashMap<String, PrimSpec>,
}

#[derive(Clone)]
enum PrimSpec {
    Open,
    Closed,
    List(Vec<Prim>),
}

pub struct Loaded {
    pub doc: Document,
    pub warnings: Vec<String>,
    /// Root children BurnCraft doesn't model (UIPrefs, VariableText...), written back unchanged.
    pub kept: Vec<Node>,
}

pub fn load_str(src: &str) -> Result<Loaded, String> {
    let root = xml::parse(src)?;
    if !root.name.ends_with("Project") {
        return Err(format!("not a LightBurn project (root element is <{}>)", root.name));
    }
    let mut doc = Document::default();
    let mut warnings = Vec::new();
    let mut kept = Vec::new();
    doc.app_version = root.attr("AppVersion").unwrap_or("").to_string();
    doc.format_version = root.attr("FormatVersion").unwrap_or("0").to_string();
    doc.material_height = root.attr_f64("MaterialHeight").unwrap_or(0.0);
    doc.mirror_x = root.attr_bool("MirrorX").unwrap_or(false);
    doc.mirror_y = root.attr_bool("MirrorY").unwrap_or(false);
    for (k, v) in &root.attrs {
        if !matches!(k.as_str(), "AppVersion" | "FormatVersion" | "MaterialHeight" | "MirrorX" | "MirrorY" | "BcRelations" | "BcOffsets" | "BcArrays" | "BcVarText") {
            doc.extra_root.push((k.clone(), v.clone()));
        }
    }
    let mut shared = Shared::default();
    for n in &root.children {
        match n.name.as_str() {
            "CutSetting" | "CutSetting_Img" => {
                let cs = parse_cut(n);
                doc.cuts.retain(|c| c.index != cs.index);
                doc.cuts.push(cs);
            }
            "Shape" => match parse_shape(n, &mut shared, &mut warnings) {
                Some(s) => doc.shapes.push(s),
                None => {}
            },
            "Notes" => {
                doc.notes = n.attr("Notes").map(|s| s.to_string()).unwrap_or_else(|| n.text.clone());
            }
            "Thumbnail" => {}
            _ => kept.push(n.clone()),
        }
    }
    doc.cuts.sort_by_key(|c| c.index);
    crate::relations::from_file(&mut doc, root.attr("BcRelations"), root.attr("BcOffsets"), root.attr("BcArrays"));
    if let Some(v) = root.attr("BcVarText").and_then(|t| serde_json::from_str(t).ok()) {
        doc.vartext = v;
    }
    Ok(Loaded { doc, warnings, kept })
}

fn value_of(n: &Node) -> Option<&str> {
    n.attr("Value").or(if n.text.is_empty() { None } else { Some(n.text.as_str()) })
}

pub fn parse_cut(n: &Node) -> CutSetting {
    let mut cs = CutSetting::new(0);
    if n.name == "CutSetting_Img" {
        cs.mode = CutMode::Image;
    }
    if let Some(t) = n.attr("type") {
        cs.mode = CutMode::from_file_name(t);
    }
    // Values may come as child elements (<speed Value="100"/>) or, in old files, as attributes.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (k, v) in &n.attrs {
        if k != "type" {
            pairs.push((k.clone(), v.clone()));
        }
    }
    for c in &n.children {
        if let Some(v) = value_of(c) {
            pairs.push((c.name.clone(), v.to_string()));
        }
    }
    let mut named = false;
    for (k, v) in pairs {
        let f = v.trim().parse::<f64>().ok();
        let b = matches!(v.trim(), "1" | "True" | "true");
        match k.as_str() {
            "index" => cs.index = f.unwrap_or(0.0) as usize,
            "name" => {
                cs.name = v;
                named = true
            }
            "speed" => cs.speed = f.unwrap_or(cs.speed),
            "maxPower" => cs.max_power = f.unwrap_or(cs.max_power),
            "minPower" => cs.min_power = f.unwrap_or(cs.min_power),
            "numPasses" => cs.passes = f.unwrap_or(1.0).max(1.0) as u32,
            "interval" => cs.interval = f.unwrap_or(cs.interval),
            "angle" => cs.angle = f.unwrap_or(0.0),
            "crossHatch" => cs.cross_hatch = b,
            "bidir" => cs.bidir = b,
            "overscanPercent" => cs.overscan_percent = f.unwrap_or(cs.overscan_percent),
            "priority" => cs.priority = f.unwrap_or(0.0) as i32,
            "doOutput" => cs.output = b,
            "forceConstantPower" => cs.force_constant_power = b,
            "hide" => cs.hide = b,
            "runBlower" => cs.air_assist = b,
            "zOffset" => cs.z_offset = f.unwrap_or(0.0),
            "kerf" => cs.kerf = f.unwrap_or(0.0),
            "frequency" => cs.frequency = f.unwrap_or(cs.frequency),
            "floodFill" => cs.fill_individually = b,
            _ => cs.extra.push((k, v)),
        }
    }
    if !named {
        cs.name = layer_name(cs.index);
    }
    cs
}

fn parse_vertlist(s: &str) -> Vec<Vert> {
    // V<x> <y>[c0x<x>c0y<y>|c0x1][c1x<x>c1y<y>|c1x1] ...
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize| -> Option<f64> {
        while *i < b.len() && b[*i] == b' ' {
            *i += 1;
        }
        let st = *i;
        while *i < b.len() && (b[*i].is_ascii_digit() || matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E')) {
            // stop at an 'e' that isn't followed by a digit/sign (exponent)
            if (b[*i] == b'e' || b[*i] == b'E') && !(*i + 1 < b.len() && (b[*i + 1].is_ascii_digit() || b[*i + 1] == b'-' || b[*i + 1] == b'+')) {
                break;
            }
            *i += 1;
        }
        s[st..*i].parse().ok()
    };
    while i < b.len() {
        if b[i] == b'V' {
            i += 1;
            let x = num(&mut i).unwrap_or(0.0);
            let y = num(&mut i).unwrap_or(0.0);
            let mut v = Vert::new(p2(x, y));
            loop {
                if s[i..].starts_with("c0x") || s[i..].starts_with("c1x") {
                    let which = b[i + 1];
                    i += 3;
                    let cx = num(&mut i).unwrap_or(0.0);
                    let tag = if which == b'0' { "c0y" } else { "c1y" };
                    if s[i..].starts_with(tag) {
                        i += 3;
                        let cy = num(&mut i).unwrap_or(0.0);
                        if which == b'0' {
                            v.c0 = Some(p2(cx, cy));
                        } else {
                            v.c1 = Some(p2(cx, cy));
                        }
                    }
                } else {
                    break;
                }
            }
            out.push(v);
        } else {
            i += 1;
        }
    }
    out
}

fn parse_primlist(s: &str) -> PrimSpec {
    let t = s.trim();
    if t == "LineOpen" {
        return PrimSpec::Open;
    }
    if t == "LineClosed" {
        return PrimSpec::Closed;
    }
    let mut prims = Vec::new();
    let mut it = t.split(|c: char| c == ' ' || c == 'L' || c == 'B').peekable();
    let _ = &mut it;
    // tokenise manually so we keep the kind letters
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let kind = match b[i] {
            b'L' => Some(PrimKind::Line),
            b'B' => Some(PrimKind::Bezier),
            _ => None,
        };
        i += 1;
        if let Some(kind) = kind {
            let mut nums = [0usize; 2];
            for n in nums.iter_mut() {
                while i < b.len() && b[i] == b' ' {
                    i += 1;
                }
                let st = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                *n = t[st..i].parse().unwrap_or(0);
            }
            prims.push(Prim { kind, a: nums[0], b: nums[1] });
        }
    }
    PrimSpec::List(prims)
}

fn resolve_prims(spec: &PrimSpec, n: usize) -> Vec<Prim> {
    match spec {
        PrimSpec::Open => (0..n.saturating_sub(1)).map(|i| Prim { kind: PrimKind::Line, a: i, b: i + 1 }).collect(),
        PrimSpec::Closed => {
            let mut v: Vec<Prim> = (0..n.saturating_sub(1)).map(|i| Prim { kind: PrimKind::Line, a: i, b: i + 1 }).collect();
            if n > 1 {
                v.push(Prim { kind: PrimKind::Line, a: n - 1, b: 0 });
            }
            v
        }
        PrimSpec::List(l) => l.iter().copied().filter(|p| p.a < n && p.b < n).collect(),
    }
}

fn parse_path_data(n: &Node, shared: &mut Shared) -> Option<PathData> {
    let vid = n.attr("VertID").map(|s| s.to_string());
    let pid = n.attr("PrimID").map(|s| s.to_string());
    let verts = if let Some(vl) = n.child("VertList") {
        let v = parse_vertlist(&vl.text);
        if let Some(id) = &vid {
            shared.verts.insert(id.clone(), v.clone());
        }
        Some(v)
    } else if n.children.iter().any(|c| c.name == "V") {
        Some(
            n.children
                .iter()
                .filter(|c| c.name == "V")
                .map(|c| {
                    let p = p2(c.attr_f64("vx").unwrap_or(0.0), c.attr_f64("vy").unwrap_or(0.0));
                    let c0 = c.attr_f64("c0x").zip(c.attr_f64("c0y")).map(|(x, y)| p2(x, y));
                    let c1 = c.attr_f64("c1x").zip(c.attr_f64("c1y")).map(|(x, y)| p2(x, y));
                    Vert { p, c0, c1 }
                })
                .collect(),
        )
    } else {
        vid.as_ref().and_then(|id| shared.verts.get(id).cloned())
    }?;
    let spec = if let Some(pl) = n.child("PrimList") {
        let sp = parse_primlist(&pl.text);
        if let Some(id) = &pid {
            shared.prims.insert(id.clone(), sp.clone());
        }
        sp
    } else if n.children.iter().any(|c| c.name == "P") {
        PrimSpec::List(
            n.children
                .iter()
                .filter(|c| c.name == "P")
                .map(|c| Prim {
                    kind: if c.attr("T") == Some("B") { PrimKind::Bezier } else { PrimKind::Line },
                    a: c.attr_f64("p0").unwrap_or(0.0) as usize,
                    b: c.attr_f64("p1").unwrap_or(0.0) as usize,
                })
                .collect(),
        )
    } else if let Some(sp) = pid.as_ref().and_then(|id| shared.prims.get(id).cloned()) {
        sp
    } else {
        PrimSpec::Closed
    };
    let prims = resolve_prims(&spec, verts.len());
    Some(PathData { verts, prims })
}

const KNOWN_ATTRS: &[&str] = &["Type", "CutIndex", "Locked", "W", "H", "Cr", "Rx", "Ry", "N", "Str", "Font", "Data", "File", "VertID", "PrimID", "Gamma", "Contrast", "Brightness", "Name", "HasBackupPath", "ForceCase", "Weld", "LS", "BcCase"];

fn parse_shape(n: &Node, shared: &mut Shared, warnings: &mut Vec<String>) -> Option<Shape> {
    let ty = n.attr("Type").unwrap_or("Path");
    let layer = n.attr_f64("CutIndex").unwrap_or(0.0).max(0.0) as usize;
    let xform = n.child("XForm").and_then(|x| Xform::parse(&x.text)).unwrap_or_default();
    let f = |k: &str, d: f64| n.attr_f64(k).unwrap_or(d);
    let kind = match ty {
        "Rect" => ShapeKind::Rect { w: f("W", 10.0), h: f("H", 10.0), cr: f("Cr", 0.0) },
        "Ellipse" => ShapeKind::Ellipse { rx: f("Rx", 5.0), ry: f("Ry", 5.0) },
        "Polygon" => {
            if n.child("VertList").is_some() {
                ShapeKind::Path(parse_path_data(n, shared)?)
            } else {
                ShapeKind::Polygon { n: f("N", 6.0) as u32, rx: f("Rx", f("W", 10.0) / 2.0), ry: f("Ry", f("H", 10.0) / 2.0) }
            }
        }
        "Text" => {
            let backup = n.child("BackupPath").and_then(|bp| {
                let pd = parse_path_data(bp, shared).or_else(|| {
                    // backup may itself be a group of paths
                    let mut lines: Vec<Vec<P2>> = Vec::new();
                    if let Some(ch) = bp.child("Children") {
                        for c in ch.children.iter().filter(|c| c.name == "Shape") {
                            if let Some(s) = parse_shape(c, shared, warnings) {
                                lines.extend(s.world_outline(0.02));
                            }
                        }
                    }
                    (!lines.is_empty()).then(|| PathData::from_polylines(&lines))
                })?;
                // The backup path carries its own (absolute) transform; bring it into the text's local space.
                let bx = bp.child("XForm").and_then(|x| Xform::parse(&x.text)).unwrap_or(xform);
                let to_local = xform.inverse().unwrap_or_default().then_after(bx);
                Some(PathData {
                    verts: pd
                        .verts
                        .iter()
                        .map(|v| Vert { p: to_local.apply(v.p), c0: v.c0.map(|c| to_local.apply(c)), c1: v.c1.map(|c| to_local.apply(c)) })
                        .collect(),
                    prims: pd.prims,
                })
            });
            let case = match n.attr_f64("BcCase") {
                Some(c) => TextCase::from_code(c as u8),
                None => TextCase::from_code(f("ForceCase", 0.0) as u8),
            };
            let style = TextStyle { case, weld: n.attr_bool("Weld").unwrap_or(false), letter_spacing: f("LS", 0.0), variable: n.attr_bool("BcVar").unwrap_or(false) };
            ShapeKind::Text { text: n.attr("Str").unwrap_or("").to_string(), font: n.attr("Font").unwrap_or("").to_string(), height: f("H", 10.0), style, backup }
        }
        "Bitmap" => {
            let data_b64 = n.attr("Data").unwrap_or("");
            let bytes = base64::engine::general_purpose::STANDARD.decode(data_b64.trim()).unwrap_or_default();
            let Some(bd) = decode_bitmap(bytes, n.attr("File").unwrap_or("")) else {
                warnings.push("An image in the project could not be decoded and was skipped.".into());
                return None;
            };
            let adjust = ImageAdjust {
                gamma: f("Gamma", 1.0),
                contrast: f("Contrast", 0.0),
                brightness: f("Brightness", 0.0),
                ..Default::default()
            };
            ShapeKind::Bitmap { w: f("W", bd.width as f64 * 0.1), h: f("H", bd.height as f64 * 0.1), data: bd, adjust }
        }
        "Group" => {
            let mut ch = Vec::new();
            if let Some(c) = n.child("Children") {
                for s in c.children.iter().filter(|c| c.name == "Shape") {
                    if let Some(sh) = parse_shape(s, shared, warnings) {
                        ch.push(sh);
                    }
                }
            }
            ShapeKind::Group(ch)
        }
        "Path" => ShapeKind::Path(parse_path_data(n, shared)?),
        other => {
            // Unknown types: use their vector data if they have any.
            match parse_path_data(n, shared) {
                Some(pd) => ShapeKind::Path(pd),
                None => {
                    warnings.push(format!("Skipped a \u{201c}{other}\u{201d} shape BurnCraft can't show yet."));
                    return None;
                }
            }
        }
    };
    let mut s = Shape::new(kind, xform, layer);
    s.locked = n.attr_bool("Locked").unwrap_or(false);
    s.name = n.attr("Name").unwrap_or("").to_string();
    for (k, v) in &n.attrs {
        if !KNOWN_ATTRS.contains(&k.as_str()) {
            s.extra_attrs.push((k.clone(), v.clone()));
        }
    }
    Some(s)
}

pub fn decode_bitmap(bytes: Vec<u8>, file: &str) -> Option<BitmapData> {
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(BitmapData { width: img.width(), height: img.height(), rgba: Arc::new(img.into_raw()), encoded: Arc::new(bytes), source_file: file.to_string() })
}

// ---------------------------------------------------------------- writing

fn vertlist(verts: &[Vert]) -> String {
    let mut s = String::new();
    for v in verts {
        s.push('V');
        s.push_str(&fmt_num(v.p.x));
        s.push(' ');
        s.push_str(&fmt_num(v.p.y));
        match v.c0 {
            Some(c) => s.push_str(&format!("c0x{}c0y{}", fmt_num(c.x), fmt_num(c.y))),
            None => s.push_str("c0x1"),
        }
        match v.c1 {
            Some(c) => s.push_str(&format!("c1x{}c1y{}", fmt_num(c.x), fmt_num(c.y))),
            None => s.push_str("c1x1"),
        }
    }
    s
}

fn primlist(prims: &[Prim]) -> String {
    let mut s = String::new();
    for p in prims {
        s.push(if p.kind == PrimKind::Bezier { 'B' } else { 'L' });
        s.push_str(&format!("{} {}", p.a, p.b));
    }
    s
}

fn shape_node(s: &Shape) -> Node {
    let mut n = Node::new("Shape");
    n.set("Type", s.type_name()).set("CutIndex", s.layer.to_string());
    if s.locked {
        n.set("Locked", "1");
    }
    if !s.name.is_empty() {
        n.set("Name", s.name.clone());
    }
    match &s.kind {
        ShapeKind::Rect { w, h, cr } => {
            n.set("W", fmt_num(*w)).set("H", fmt_num(*h)).set("Cr", fmt_num(*cr));
        }
        ShapeKind::Ellipse { rx, ry } => {
            n.set("Rx", fmt_num(*rx)).set("Ry", fmt_num(*ry));
        }
        ShapeKind::Polygon { n: sides, rx, ry } => {
            n.set("N", sides.to_string()).set("Rx", fmt_num(*rx)).set("Ry", fmt_num(*ry));
        }
        ShapeKind::Text { text, font, height, style, .. } => {
            n.set("Font", if font.is_empty() { "Sans,-1,100,5,50,0,0,0,0,0".to_string() } else { font.clone() })
                .set("Str", text.clone())
                .set("H", fmt_num(*height))
                .set("LS", fmt_num(style.letter_spacing))
                .set("ForceCase", if style.case == TextCase::Upper { "1" } else { "0" })
                .set("BcCase", style.case.code().to_string())
                .set("Weld", if style.weld { "1" } else { "0" })
                .set("HasBackupPath", "1");
            if style.variable {
                n.set("BcVar", "1");
            }
        }
        ShapeKind::Bitmap { w, h, data, adjust } => {
            n.set("W", fmt_num(*w)).set("H", fmt_num(*h)).set("Gamma", fmt_num(adjust.gamma)).set("Contrast", fmt_num(adjust.contrast)).set("Brightness", fmt_num(adjust.brightness));
            if !data.source_file.is_empty() {
                n.set("File", data.source_file.clone());
            }
            n.set("Data", base64::engine::general_purpose::STANDARD.encode(data.encoded.as_slice()));
        }
        _ => {}
    }
    for (k, v) in &s.extra_attrs {
        n.set(k, v.clone());
    }
    n.push(Node::new("XForm").with_text(s.xform.to_lbrn()));
    match &s.kind {
        ShapeKind::Path(pd) => {
            n.push(Node::new("VertList").with_text(vertlist(&pd.verts)));
            n.push(Node::new("PrimList").with_text(primlist(&pd.prims)));
        }
        ShapeKind::Group(ch) => {
            let mut c = Node::new("Children");
            for k in ch {
                c.push(shape_node(k));
            }
            n.push(c);
        }
        ShapeKind::Text { .. } => {
            // Save the outline too so LightBurn can show the text even without the same font.
            let lines = s.local_outline(0.02);
            let pd = PathData::from_polylines(&lines);
            let mut bp = Node::new("BackupPath");
            bp.set("Type", "Path").set("CutIndex", s.layer.to_string());
            bp.push(Node::new("XForm").with_text(s.xform.to_lbrn()));
            bp.push(Node::new("VertList").with_text(vertlist(&pd.verts)));
            bp.push(Node::new("PrimList").with_text(primlist(&pd.prims)));
            n.push(bp);
        }
        _ => {}
    }
    n
}

pub fn cut_node(c: &CutSetting) -> Node {
    let mut n = Node::new(if c.mode == CutMode::Image { "CutSetting_Img" } else { "CutSetting" });
    n.set("type", c.mode.file_name());
    let mut v = |k: &str, val: String| {
        let mut e = Node::new(k);
        e.set("Value", val);
        n.push(e);
    };
    let b = |x: bool| if x { "1".to_string() } else { "0".to_string() };
    v("index", c.index.to_string());
    v("name", c.name.clone());
    v("minPower", fmt_num(c.min_power));
    v("maxPower", fmt_num(c.max_power));
    v("speed", fmt_num(c.speed));
    v("numPasses", c.passes.to_string());
    v("interval", fmt_num(c.interval));
    v("angle", fmt_num(c.angle));
    v("crossHatch", b(c.cross_hatch));
    v("bidir", b(c.bidir));
    v("overscanPercent", fmt_num(c.overscan_percent));
    v("priority", c.priority.to_string());
    v("doOutput", b(c.output));
    if c.force_constant_power {
        v("forceConstantPower", b(true));
    }
    v("hide", b(c.hide));
    v("runBlower", b(c.air_assist));
    if c.z_offset != 0.0 {
        v("zOffset", fmt_num(c.z_offset));
    }
    if c.kerf != 0.0 {
        v("kerf", fmt_num(c.kerf));
    }
    if c.fill_individually {
        v("floodFill", "1".into());
    }
    if c.frequency != CutSetting::DEFAULT_FREQUENCY {
        v("frequency", fmt_num(c.frequency));
    }
    for (k, val) in &c.extra {
        v(k, val.clone());
    }
    n
}

pub fn save_string(doc: &Document, kept: &[Node]) -> String {
    let mut root = Node::new("LightBurnProject");
    root.set("AppVersion", format!("BurnCraft {}", env!("CARGO_PKG_VERSION")))
        .set("FormatVersion", "1")
        .set("MaterialHeight", fmt_num(doc.material_height))
        .set("MirrorX", if doc.mirror_x { "True" } else { "False" })
        .set("MirrorY", if doc.mirror_y { "True" } else { "False" });
    for (k, v) in &doc.extra_root {
        root.set(k, v.clone());
    }
    for (k, v) in crate::relations::to_file(doc) {
        root.set(k, v);
    }
    let has_variable = doc.shapes.iter().any(|s| matches!(&s.kind, ShapeKind::Text { style, .. } if style.variable));
    if has_variable || !doc.vartext.rows.is_empty() {
        if let Ok(j) = serde_json::to_string(&doc.vartext) {
            root.set("BcVarText", j);
        }
    }
    for k in kept {
        root.push(k.clone());
    }
    for c in &doc.cuts {
        root.push(cut_node(c));
    }
    for s in &doc.shapes {
        root.push(shape_node(s));
    }
    let mut notes = Node::new("Notes");
    notes.set("ShowOnLoad", "0").set("Notes", doc.notes.clone());
    root.push(notes);
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    root.write(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<LightBurnProject AppVersion="1.5.00" FormatVersion="1" MaterialHeight="0" MirrorX="False" MirrorY="False">
    <CutSetting type="Scan">
        <index Value="1"/>
        <name Value="C01"/>
        <maxPower Value="35"/>
        <speed Value="250"/>
    </CutSetting>
    <Shape Type="Group" CutIndex="0">
        <XForm>1 0 0 1 0 0</XForm>
        <Children>
            <Shape Type="Path" CutIndex="0" VertID="3" PrimID="2">
                <XForm>1 0 0 1 0 0</XForm>
                <VertList>V10 20c0x1c1x1V10 0c0x1c1x1</VertList>
                <PrimList>L0 1</PrimList>
            </Shape>
            <Shape Type="Path" CutIndex="0" VertID="7" PrimID="2">
                <XForm>1 0 0 1 0 0</XForm>
                <VertList>V0 5c0x1c1x1V20 5c0x1c1x1</VertList>
            </Shape>
        </Children>
    </Shape>
    <Shape Type="Path" CutIndex="1">
        <XForm>2 0 0 2 5 5</XForm>
        <VertList>V0 0c0x3c0y0c1x1V10 0c0x1c1x7c1y0V10 10c0x1c1x1</VertList>
        <PrimList>B0 1L1 2</PrimList>
    </Shape>
    <Shape Type="Path" CutIndex="1" VertID="3" PrimID="9"><XForm>1 0 0 1 100 0</XForm><PrimList>LineClosed</PrimList></Shape>
    <Shape Type="Rect" CutIndex="2" W="30" H="20" Cr="0"><XForm>1 0 0 1 50 50</XForm></Shape>
</LightBurnProject>"#;

    #[test]
    fn loads_sample() {
        let l = load_str(SAMPLE).unwrap();
        assert_eq!(l.doc.shapes.len(), 4);
        let c = l.doc.cut(1).unwrap();
        assert_eq!(c.mode, CutMode::Fill);
        assert_eq!(c.speed, 250.0);
        assert_eq!(c.max_power, 35.0);
        // shared PrimID reuse
        if let ShapeKind::Group(ch) = &l.doc.shapes[0].kind {
            if let ShapeKind::Path(pd) = &ch[1].kind {
                assert_eq!(pd.prims.len(), 1);
            } else {
                panic!()
            }
        }
        // bezier control points
        if let ShapeKind::Path(pd) = &l.doc.shapes[1].kind {
            assert_eq!(pd.verts[0].c0, Some(p2(3.0, 0.0)));
            assert_eq!(pd.verts[1].c1, Some(p2(7.0, 0.0)));
            assert_eq!(pd.prims[0].kind, PrimKind::Bezier);
        }
        // VertID reuse with LineClosed
        if let ShapeKind::Path(pd) = &l.doc.shapes[2].kind {
            assert_eq!(pd.verts.len(), 2);
        } else {
            panic!()
        }
        let b = l.doc.shapes[3].bbox();
        assert!((b.width() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn roundtrip() {
        let l = load_str(SAMPLE).unwrap();
        let s = save_string(&l.doc, &l.kept);
        let l2 = load_str(&s).unwrap();
        assert_eq!(l2.doc.shapes.len(), l.doc.shapes.len());
        assert_eq!(l2.doc.cut(1).unwrap().speed, 250.0);
        let (a, b) = (l.doc.bbox(), l2.doc.bbox());
        assert!(a.min.dist(b.min) < 1e-4 && a.max.dist(b.max) < 1e-4);
    }
}
