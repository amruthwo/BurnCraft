//! Document model: shapes, layers (cut settings) and the project.

use crate::geom::*;
use std::sync::Arc;

/// The 30 layer colours from LightBurn's palette.json (ARGB without alpha).
pub const PALETTE: [u32; 30] = [
    0x000000, 0x0000FF, 0xFF0000, 0x00E000, 0xD0D000, 0xFF8000, 0x00E0E0, 0xFF00FF, 0xB4B4B4, 0x0000A0, 0xA00000, 0x00A000, 0xA0A000, 0xC08000, 0x00A0FF,
    0xA000A0, 0x808080, 0x7D87B9, 0xBB7784, 0x4A6FE3, 0xD33F6A, 0x8CD78C, 0xF0B98D, 0xF6C4E1, 0xFA9ED4, 0x500A78, 0xB45A00, 0x004754, 0x86FA88, 0xFFDB66,
];
/// Colour layers C00..C29; index 30 and 31 are the tool layers T1 and T2.
pub const COLOR_LAYERS: usize = 30;
/// Construction lines live on tool layer T2: shown dashed, used for sketching, snaps and relations, never burned.
pub const CONSTRUCTION_LAYER: usize = COLOR_LAYERS + 1;

/// Layer settings the user saved as defaults for new projects.
pub static LAYER_DEFAULTS: std::sync::RwLock<Vec<CutSetting>> = std::sync::RwLock::new(Vec::new());

pub fn layer_rgb(index: usize) -> [u8; 3] {
    let c = if index < COLOR_LAYERS {
        PALETTE[index]
    } else if index == COLOR_LAYERS {
        0xFF6600
    } else {
        0x66CC00
    };
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

pub fn layer_name(index: usize) -> String {
    if index < COLOR_LAYERS {
        format!("C{:02}", index)
    } else {
        format!("T{}", index - COLOR_LAYERS + 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CutMode {
    Line,
    Fill,
    FillLine,
    OffsetFill,
    Image,
}

impl CutMode {
    pub const ALL: [CutMode; 5] = [CutMode::Line, CutMode::Fill, CutMode::FillLine, CutMode::OffsetFill, CutMode::Image];
    pub fn label(self) -> &'static str {
        match self {
            CutMode::Line => "Line",
            CutMode::Fill => "Fill",
            CutMode::FillLine => "Fill + Line",
            CutMode::OffsetFill => "Offset Fill",
            CutMode::Image => "Image",
        }
    }
    /// Name used in the `type` attribute of `<CutSetting>`.
    pub fn file_name(self) -> &'static str {
        match self {
            CutMode::Line => "Cut",
            CutMode::Fill => "Scan",
            CutMode::FillLine => "Scan+Cut",
            CutMode::OffsetFill => "Offset Fill",
            CutMode::Image => "Image",
        }
    }
    pub fn from_file_name(s: &str) -> CutMode {
        match s {
            "Scan" | "Fill" => CutMode::Fill,
            "Scan+Cut" | "Fill+Line" => CutMode::FillLine,
            "Offset Fill" | "OffsetFill" => CutMode::OffsetFill,
            "Image" => CutMode::Image,
            _ => CutMode::Line,
        }
    }
}

/// One layer's laser settings. Defaults match a new layer in LightBurn.
#[derive(Clone, Debug)]
pub struct CutSetting {
    pub index: usize,
    pub name: String,
    pub mode: CutMode,
    pub speed: f64,
    pub min_power: f64,
    pub max_power: f64,
    pub passes: u32,
    pub interval: f64,
    pub angle: f64,
    pub cross_hatch: bool,
    pub bidir: bool,
    pub overscan_percent: f64,
    pub priority: i32,
    pub output: bool,
    pub hide: bool,
    pub air_assist: bool,
    pub z_offset: f64,
    pub kerf: f64,
    /// PWM / pulse frequency for fiber and diode modules that support it.
    pub frequency: f64,
    /// Fill each closed shape on its own instead of sweeping across the whole layer (LightBurn "Flood Fill").
    pub fill_individually: bool,
    /// LightBurn's "Constant Power Mode" (file key `forceConstantPower`): this layer burns with M3 (constant power)
    /// even on a laser set to dynamic M4.
    pub force_constant_power: bool,
    /// Keys BurnCraft doesn't edit, kept so files round-trip.
    pub extra: Vec<(String, String)>,
}

impl CutSetting {
    pub const DEFAULT_SPEED: f64 = 100.0;
    pub const DEFAULT_POWER: f64 = 20.0;
    pub const DEFAULT_INTERVAL: f64 = 0.1;
    pub const DEFAULT_FREQUENCY: f64 = 20000.0;

    pub fn new(index: usize) -> Self {
        CutSetting {
            index,
            name: layer_name(index),
            mode: CutMode::Line,
            speed: Self::DEFAULT_SPEED,
            min_power: Self::DEFAULT_POWER,
            max_power: Self::DEFAULT_POWER,
            passes: 1,
            interval: Self::DEFAULT_INTERVAL,
            angle: 0.0,
            cross_hatch: false,
            bidir: true,
            overscan_percent: 2.5,
            priority: index as i32,
            output: index < COLOR_LAYERS,
            hide: false,
            air_assist: true,
            z_offset: 0.0,
            kerf: 0.0,
            frequency: Self::DEFAULT_FREQUENCY,
            fill_individually: false,
            force_constant_power: false,
            extra: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
    pub p: P2,
    /// Outgoing control point (towards the next vertex).
    pub c0: Option<P2>,
    /// Incoming control point (from the previous vertex).
    pub c1: Option<P2>,
}

impl Vert {
    pub fn new(p: P2) -> Self {
        Vert { p, c0: None, c1: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimKind {
    Line,
    Bezier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prim {
    pub kind: PrimKind,
    pub a: usize,
    pub b: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathData {
    pub verts: Vec<Vert>,
    pub prims: Vec<Prim>,
}

impl PathData {
    pub fn polyline(points: &[P2], closed: bool) -> PathData {
        let verts: Vec<Vert> = points.iter().map(|p| Vert::new(*p)).collect();
        let n = verts.len();
        let mut prims: Vec<Prim> = (0..n.saturating_sub(1)).map(|i| Prim { kind: PrimKind::Line, a: i, b: i + 1 }).collect();
        if closed && n > 2 {
            prims.push(Prim { kind: PrimKind::Line, a: n - 1, b: 0 });
        }
        PathData { verts, prims }
    }

    /// Append several polylines (each its own sub-path).
    pub fn from_polylines(lines: &[Vec<P2>]) -> PathData {
        let mut pd = PathData::default();
        for l in lines {
            let closed = is_closed(l);
            let pts = if closed { &l[..l.len() - 1] } else { &l[..] };
            if pts.len() < 2 {
                continue;
            }
            let base = pd.verts.len();
            pd.verts.extend(pts.iter().map(|p| Vert::new(*p)));
            for i in 0..pts.len() - 1 {
                pd.prims.push(Prim { kind: PrimKind::Line, a: base + i, b: base + i + 1 });
            }
            if closed {
                pd.prims.push(Prim { kind: PrimKind::Line, a: base + pts.len() - 1, b: base });
            }
        }
        pd
    }

    /// Chain primitives into polylines in local coordinates.
    pub fn flatten(&self, tol: f64) -> Vec<Vec<P2>> {
        let mut out: Vec<Vec<P2>> = Vec::new();
        let mut cur: Vec<P2> = Vec::new();
        let mut last_b: Option<usize> = None;
        for pr in &self.prims {
            if pr.a >= self.verts.len() || pr.b >= self.verts.len() {
                continue;
            }
            if last_b != Some(pr.a) {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push(self.verts[pr.a].p);
            }
            let va = self.verts[pr.a];
            let vb = self.verts[pr.b];
            match pr.kind {
                PrimKind::Line => cur.push(vb.p),
                PrimKind::Bezier => {
                    let c0 = va.c0.unwrap_or(va.p);
                    let c1 = vb.c1.unwrap_or(vb.p);
                    flatten_cubic(va.p, c0, c1, vb.p, tol, &mut cur);
                }
            }
            last_b = Some(pr.b);
        }
        if cur.len() > 1 {
            out.push(cur);
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct BitmapData {
    /// Original encoded bytes (PNG/JPEG...) kept for saving.
    pub encoded: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Decoded RGBA pixels.
    pub rgba: Arc<Vec<u8>>,
    pub source_file: String,
}

#[derive(Clone, Debug)]
pub struct ImageAdjust {
    pub brightness: f64,
    pub contrast: f64,
    pub gamma: f64,
    pub invert: bool,
    pub dither: Dither,
}

impl Default for ImageAdjust {
    fn default() -> Self {
        ImageAdjust { brightness: 0.0, contrast: 0.0, gamma: 1.0, invert: false, dither: Dither::Grayscale }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dither {
    Threshold,
    Grayscale,
    FloydSteinberg,
    Jarvis,
}

impl Dither {
    pub const ALL: [Dither; 4] = [Dither::Threshold, Dither::Grayscale, Dither::FloydSteinberg, Dither::Jarvis];
    pub fn label(self) -> &'static str {
        match self {
            Dither::Threshold => "Threshold",
            Dither::Grayscale => "Grayscale",
            Dither::FloydSteinberg => "Floyd–Steinberg",
            Dither::Jarvis => "Jarvis",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextCase {
    #[default]
    AsTyped,
    Upper,
    Lower,
    Title,
}

impl TextCase {
    pub const ALL: [TextCase; 4] = [TextCase::AsTyped, TextCase::Upper, TextCase::Lower, TextCase::Title];
    pub fn label(self) -> &'static str {
        match self {
            TextCase::AsTyped => "As typed",
            TextCase::Upper => "UPPER CASE",
            TextCase::Lower => "lower case",
            TextCase::Title => "Title Case",
        }
    }
    /// BurnCraft's case code (saved as `BcCase`). LightBurn itself only has an "Upper Case" checkbox,
    /// saved as `ForceCase` = 1.
    pub fn code(self) -> u8 {
        match self {
            TextCase::AsTyped => 0,
            TextCase::Upper => 1,
            TextCase::Lower => 2,
            TextCase::Title => 3,
        }
    }
    pub fn from_code(c: u8) -> TextCase {
        match c {
            1 => TextCase::Upper,
            2 => TextCase::Lower,
            3 => TextCase::Title,
            _ => TextCase::AsTyped,
        }
    }
    pub fn apply(self, s: &str) -> String {
        match self {
            TextCase::AsTyped => s.to_string(),
            TextCase::Upper => s.to_uppercase(),
            TextCase::Lower => s.to_lowercase(),
            TextCase::Title => {
                let mut out = String::with_capacity(s.len());
                let mut start = true;
                for c in s.chars() {
                    if start && c.is_alphabetic() {
                        out.extend(c.to_uppercase());
                        start = false;
                    } else {
                        out.extend(c.to_lowercase());
                    }
                    if c.is_whitespace() {
                        start = true;
                    }
                }
                out
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextStyle {
    pub case: TextCase,
    /// Merge overlapping letters into one outline (LightBurn `Weld`).
    pub weld: bool,
    /// Extra space between letters in mm (LightBurn `LS`).
    pub letter_spacing: f64,
}

#[derive(Clone, Debug)]
pub enum ShapeKind {
    Path(PathData),
    /// Centred on the local origin.
    Rect { w: f64, h: f64, cr: f64 },
    Ellipse { rx: f64, ry: f64 },
    Polygon { n: u32, rx: f64, ry: f64 },
    Text { text: String, font: String, height: f64, style: TextStyle, backup: Option<PathData> },
    /// Centred on the local origin, w × h in mm.
    Bitmap { w: f64, h: f64, data: BitmapData, adjust: ImageAdjust },
    Group(Vec<Shape>),
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub id: u64,
    pub kind: ShapeKind,
    pub xform: Xform,
    pub layer: usize,
    pub locked: bool,
    pub name: String,
    /// Attributes we don't model, kept for round-trip.
    pub extra_attrs: Vec<(String, String)>,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Shape {
    pub fn new(kind: ShapeKind, xform: Xform, layer: usize) -> Shape {
        Shape { id: new_id(), kind, xform, layer, locked: false, name: String::new(), extra_attrs: Vec::new() }
    }

    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            ShapeKind::Path(_) => "Path",
            ShapeKind::Rect { .. } => "Rect",
            ShapeKind::Ellipse { .. } => "Ellipse",
            ShapeKind::Polygon { .. } => "Polygon",
            ShapeKind::Text { .. } => "Text",
            ShapeKind::Bitmap { .. } => "Bitmap",
            ShapeKind::Group(_) => "Group",
        }
    }

    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        match &self.kind {
            ShapeKind::Text { text, .. } => format!("Text \u{201c}{}\u{201d}", text.chars().take(16).collect::<String>()),
            ShapeKind::Group(c) => format!("Group ({} items)", c.len()),
            ShapeKind::Polygon { n, .. } => format!("Polygon ({n} sides)"),
            _ => self.type_name().to_string(),
        }
    }

    /// Give this shape and all children fresh ids (after copy/paste).
    pub fn renew_ids(&mut self) {
        self.id = new_id();
        if let ShapeKind::Group(ch) = &mut self.kind {
            for c in ch {
                c.renew_ids();
            }
        }
    }

    pub fn is_bitmap(&self) -> bool {
        matches!(self.kind, ShapeKind::Bitmap { .. })
    }

    /// Outline of the shape as local-space polylines.
    pub fn local_outline(&self, tol: f64) -> Vec<Vec<P2>> {
        match &self.kind {
            ShapeKind::Path(pd) => pd.flatten(tol),
            ShapeKind::Rect { w, h, cr } => vec![rect_outline(*w, *h, *cr, tol)],
            ShapeKind::Ellipse { rx, ry } => vec![ellipse_outline(*rx, *ry, tol)],
            ShapeKind::Polygon { n, rx, ry } => {
                let n = (*n).max(3) as usize;
                let mut pts: Vec<P2> = (0..n)
                    .map(|i| {
                        let a = std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / n as f64;
                        p2(rx * a.cos(), ry * a.sin())
                    })
                    .collect();
                pts.push(pts[0]);
                vec![pts]
            }
            ShapeKind::Text { backup, text, height, font, style } => match backup {
                Some(b) => b.flatten(tol),
                None => crate::text::text_outline(text, font, *height, style, tol),
            },
            ShapeKind::Bitmap { w, h, .. } => {
                let (hw, hh) = (w / 2.0, h / 2.0);
                vec![vec![p2(-hw, -hh), p2(hw, -hh), p2(hw, hh), p2(-hw, hh), p2(-hw, -hh)]]
            }
            ShapeKind::Group(ch) => {
                // each child's outline comes back in group-local space
                ch.iter().flat_map(|c| c.world_outline(tol)).collect()
            }
        }
    }

    /// Outline in the parent's coordinate space (this shape's xform applied).
    pub fn world_outline(&self, tol: f64) -> Vec<Vec<P2>> {
        let s = self.xform.mean_scale();
        self.local_outline(tol / s).into_iter().map(|l| l.into_iter().map(|p| self.xform.apply(p)).collect()).collect()
    }

    pub fn bbox(&self) -> BBox {
        let mut b = BBox::EMPTY;
        for l in self.world_outline(0.05) {
            for p in l {
                b.add(p);
            }
        }
        b
    }

    /// Flattened leaf shapes with their accumulated transforms (groups expanded).
    pub fn leaves(&self, parent: Xform, out: &mut Vec<(Shape, Xform)>) {
        let x = parent.then_after(self.xform);
        match &self.kind {
            ShapeKind::Group(ch) => {
                for c in ch {
                    c.leaves(x, out);
                }
            }
            _ => out.push((self.clone(), x)),
        }
    }

    pub fn is_closed(&self) -> bool {
        match &self.kind {
            ShapeKind::Path(_) | ShapeKind::Text { .. } => self.local_outline(0.1).iter().all(|l| is_closed(l)),
            ShapeKind::Group(ch) => ch.iter().all(|c| c.is_closed()),
            _ => true,
        }
    }

    /// Distance from a parent-space point to the outline, plus whether it is inside a closed outline.
    pub fn hit(&self, p: P2, tol: f64) -> bool {
        let outline = self.world_outline(tol * 0.25);
        for l in &outline {
            for w in l.windows(2) {
                if dist_to_segment(p, w[0], w[1]) <= tol {
                    return true;
                }
            }
        }
        if self.is_bitmap() {
            return self.bbox().contains(p);
        }
        // filled hit inside closed outlines
        let closed: Vec<Vec<P2>> = outline.into_iter().filter(|l| is_closed(l)).collect();
        !closed.is_empty() && point_in_rings(p, &closed) && self.filled_hint()
    }

    fn filled_hint(&self) -> bool {
        matches!(self.kind, ShapeKind::Text { .. } | ShapeKind::Group(_) | ShapeKind::Rect { .. } | ShapeKind::Ellipse { .. } | ShapeKind::Polygon { .. } | ShapeKind::Path(_))
    }

    /// Convert to an editable path (keeps transform).
    pub fn to_path(&self) -> Option<Shape> {
        let pd = match &self.kind {
            ShapeKind::Path(_) => return None,
            ShapeKind::Bitmap { .. } => return None,
            ShapeKind::Group(_) => return None,
            ShapeKind::Rect { w, h, cr } => rect_path(*w, *h, *cr),
            ShapeKind::Ellipse { rx, ry } => ellipse_path(*rx, *ry),
            _ => PathData::from_polylines(&self.local_outline(0.02)),
        };
        let mut s = self.clone();
        s.kind = ShapeKind::Path(pd);
        Some(s)
    }

    /// Set every leaf's layer.
    pub fn set_layer(&mut self, layer: usize) {
        self.layer = layer;
        if let ShapeKind::Group(ch) = &mut self.kind {
            for c in ch {
                c.set_layer(layer);
            }
        }
    }
}

/// Magic constant for approximating a quarter circle with a cubic bezier.
const KAPPA: f64 = 0.552_284_749_830_793_4;

pub fn ellipse_path(rx: f64, ry: f64) -> PathData {
    let pts = [p2(rx, 0.0), p2(0.0, ry), p2(-rx, 0.0), p2(0.0, -ry)];
    let tangents = [p2(0.0, ry), p2(-rx, 0.0), p2(0.0, -ry), p2(rx, 0.0)];
    let verts: Vec<Vert> = (0..4).map(|i| Vert { p: pts[i], c0: Some(pts[i] + tangents[i] * KAPPA), c1: Some(pts[i] - tangents[i] * KAPPA) }).collect();
    let prims = (0..4).map(|i| Prim { kind: PrimKind::Bezier, a: i, b: (i + 1) % 4 }).collect();
    PathData { verts, prims }
}

pub fn rect_path(w: f64, h: f64, cr: f64) -> PathData {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let r = cr.min(hw).min(hh).max(0.0);
    if r <= 1e-9 {
        return PathData::polyline(&[p2(-hw, -hh), p2(hw, -hh), p2(hw, hh), p2(-hw, hh)], true);
    }
    let k = r * KAPPA;
    let mut verts = Vec::new();
    let mut prims = Vec::new();
    // corners counter-clockwise starting bottom-right
    let corners = [(p2(hw, -hh), p2(-1.0, 0.0), p2(0.0, 1.0)), (p2(hw, hh), p2(0.0, -1.0), p2(-1.0, 0.0)), (p2(-hw, hh), p2(1.0, 0.0), p2(0.0, -1.0)), (p2(-hw, -hh), p2(0.0, 1.0), p2(1.0, 0.0))];
    for (c, din, dout) in corners {
        let a = c + din * r; // arc start (coming in)
        let b = c + dout * r; // arc end
        let ia = verts.len();
        verts.push(Vert { p: a, c0: Some(a - din * k), c1: None });
        verts.push(Vert { p: b, c0: None, c1: Some(b - dout * k) });
        prims.push(Prim { kind: PrimKind::Bezier, a: ia, b: ia + 1 });
    }
    for i in 0..4 {
        let b = i * 2 + 1;
        let next = (i * 2 + 2) % 8;
        prims.insert(i * 2 + 1, Prim { kind: PrimKind::Line, a: b, b: next });
    }
    PathData { verts, prims }
}

pub fn rect_outline(w: f64, h: f64, cr: f64, tol: f64) -> Vec<P2> {
    let pd = rect_path(w, h, cr);
    pd.flatten(tol).into_iter().next().unwrap_or_default()
}

pub fn ellipse_outline(rx: f64, ry: f64, tol: f64) -> Vec<P2> {
    let r = rx.abs().max(ry.abs()).max(1e-6);
    let n = ((std::f64::consts::TAU * r / (8.0 * tol.max(1e-4)).sqrt().max(0.01)).sqrt() * 4.0).clamp(24.0, 720.0) as usize;
    let mut pts: Vec<P2> = (0..n).map(|i| {
        let a = i as f64 * std::f64::consts::TAU / n as f64;
        p2(rx * a.cos(), ry * a.sin())
    }).collect();
    pts.push(pts[0]);
    pts
}

#[derive(Clone, Debug)]
pub struct Document {
    pub shapes: Vec<Shape>,
    pub cuts: Vec<CutSetting>,
    pub notes: String,
    pub app_version: String,
    pub format_version: String,
    pub material_height: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub extra_root: Vec<(String, String)>,
    /// Sketch relations (see relations.rs).
    pub relations: Vec<crate::relations::Relation>,
    /// Offset results and the shape they were made from (see relations.rs).
    pub offsets: Vec<crate::relations::OffsetLink>,
    /// Arrays that can still be edited (see relations.rs).
    pub arrays: Vec<crate::relations::ArrayLink>,
    /// Name given to the project tab (right-click → Rename); the file name is used otherwise.
    pub title: Option<String>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            shapes: Vec::new(),
            cuts: Vec::new(),
            notes: String::new(),
            app_version: format!("BurnCraft {}", env!("CARGO_PKG_VERSION")),
            format_version: "1".into(),
            material_height: 0.0,
            mirror_x: false,
            mirror_y: false,
            extra_root: Vec::new(),
            relations: Vec::new(),
            offsets: Vec::new(),
            arrays: Vec::new(),
            title: None,
        }
    }
}

impl Document {
    pub fn cut(&self, index: usize) -> Option<&CutSetting> {
        self.cuts.iter().find(|c| c.index == index)
    }
    pub fn cut_mut(&mut self, index: usize) -> &mut CutSetting {
        if let Some(i) = self.cuts.iter().position(|c| c.index == index) {
            return &mut self.cuts[i];
        }
        let mut cs = LAYER_DEFAULTS.read().ok().and_then(|d| d.iter().find(|c| c.index == index).cloned()).unwrap_or_else(|| CutSetting::new(index));
        if self.shapes_on_layer_is_image(index) {
            cs.mode = CutMode::Image;
        }
        self.cuts.push(cs);
        self.cuts.sort_by_key(|c| c.index);
        let i = self.cuts.iter().position(|c| c.index == index).unwrap();
        &mut self.cuts[i]
    }
    fn shapes_on_layer_is_image(&self, index: usize) -> bool {
        fn walk(s: &Shape, index: usize, any: &mut bool, all_img: &mut bool) {
            match &s.kind {
                ShapeKind::Group(ch) => ch.iter().for_each(|c| walk(c, index, any, all_img)),
                _ if s.layer == index => {
                    *any = true;
                    *all_img &= s.is_bitmap();
                }
                _ => {}
            }
        }
        let (mut any, mut all_img) = (false, true);
        for s in &self.shapes {
            walk(s, index, &mut any, &mut all_img);
        }
        any && all_img
    }
    /// Make sure every layer used by a shape has a cut setting, and drop unused ones with default values.
    pub fn sync_cuts(&mut self) {
        // walk the shapes without copying them (big projects have tens of thousands of paths)
        fn walk(s: &Shape, used: &mut std::collections::BTreeSet<usize>) {
            match &s.kind {
                ShapeKind::Group(ch) => ch.iter().for_each(|c| walk(c, used)),
                _ => {
                    used.insert(s.layer);
                }
            }
        }
        let mut used = std::collections::BTreeSet::new();
        for s in &self.shapes {
            walk(s, &mut used);
        }
        for l in &used {
            if self.cut(*l).is_none() {
                self.cut_mut(*l);
            }
        }
        self.cuts.retain(|c| used.contains(&c.index));
    }
    pub fn bbox(&self) -> BBox {
        self.shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rect_path_is_closed() {
        let pd = rect_path(10.0, 6.0, 2.0);
        let f = pd.flatten(0.01);
        assert_eq!(f.len(), 1);
        assert!(is_closed(&f[0]));
    }
}
