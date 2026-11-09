//! The design canvas: view navigation, drawing tools, selection, node editing, image selections and previews.

use crate::actions::Action;
use crate::app::{App, ArcMode, SideTab, Tool};
use crate::geom::*;
use crate::model::*;
use crate::theme::{self, ACCENT, SELECT};
use crate::wand::SelMode;
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape as EShape, Stroke, StrokeKind, Ui};

#[derive(Clone, Debug)]
pub enum Drag {
    Pan,
    BoxSelect { start: P2 },
    Move { start: P2, orig: Vec<(usize, Xform)> },
    Scale { handle: usize, anchor: P2, start: P2, orig: Vec<(usize, Xform)> },
    Rotate { center: P2, start_angle: f64, orig: Vec<(usize, Xform)> },
    Create { start: P2, cur: P2 },
    /// Dragging a control handle (part 1 = outgoing, 2 = incoming) of one node.
    Handle { shape: usize, vert: usize, part: u8, last_local: P2 },
    /// Moving all selected nodes.
    NodeMove { shape: usize, last_local: P2 },
    NodeBox { start: P2 },
    NodeLasso,
    PenHandle { idx: usize },
    ImageLasso { shape_id: u64 },
    Measure { start: P2, cur: P2 },
}

#[derive(Default)]
pub struct CanvasState {
    pub drag: Option<Drag>,
    pub line_pts: Vec<P2>,
    pub pen: Vec<Vert>,
    pub arc_pts: Vec<P2>,
    pub arc_tangent: Option<P2>,
    pub lasso_pts: Vec<P2>,
    pub pending_fit: Option<BBox>,
    pub zoom_step: Option<f64>,
    /// Selected node indices of the first selected path (node tool).
    pub nodes: Vec<usize>,
    pub adjust_drag: bool,
    pub hover_world: Option<P2>,
    pub snap_mark: Option<P2>,
    pub last_measure: Option<(P2, P2)>,
    pub rect: Option<Rect>,
    pub focus_text: bool,
    pub prop_drag: bool,
    pub font_list_open: bool,
    pub font_filter: String,
    pub font_hover: Option<String>,
    pub font_preview_cache: Option<(String, u64, Vec<Vec<P2>>)>,
    pub bool_preview: Option<(u64, Action, Vec<Vec<P2>>)>,
    /// Smart guides: what the cursor snapped to this frame, points/lines the user rested on.
    pub inference: crate::snap::Inference,
    pub acquired: Vec<P2>,
    pub ref_seg: Option<(P2, P2)>,
    pub dwell: Option<(P2, f64)>,
    pub seg_dwell: Option<((P2, P2), f64)>,
    pub guided: bool,
    /// Line tool: per segment, Some(tangent) when that segment is a tangent arc.
    pub line_arcs: Vec<Option<P2>>,
    /// Line tool: the next segment will be an arc (toggled by resting on the last point).
    pub line_arc_mode: bool,
    pub arc_hover: Option<f64>,
    pub arc_armed: bool,
    /// Dimension tool: label rectangles from the last frame, and the value being typed.
    pub dim_hits: Vec<(Rect, crate::dims::Dim)>,
    pub dim_edit: Option<(crate::dims::Dim, String, Pos2)>,
    pub pointer: Option<Pos2>,
    /// The mouse in mm while it is over the canvas (paste goes there, like LightBurn).
    pub pointer_world: Option<P2>,
    /// Relations tool: picked items, and the relation tags drawn last frame (screen rect, relation index).
    pub rel_picks: Vec<crate::relations::Ent>,
    pub rel_hits: Vec<(Rect, usize)>,
    /// The "Edit array" button under a selected array member (screen).
    pub array_chip: Option<Rect>,
    /// Off-bed warning outlines: (document revision, bed, merged boxes, shape count).
    pub off_bed: Option<(u64, BBox, Vec<BBox>, usize)>,
    /// Line tool: the smart-guide result at each placed point (turned into relations when the line is done).
    pub line_snaps: Vec<crate::snap::Inference>,
    pub last_size: Option<egui::Vec2>,
    /// Re-frame the bed when the canvas resizes, until the user pans or zooms.
    pub auto_fit: bool,
}

impl CanvasState {
    pub fn cancel(&mut self) {
        self.drag = None;
        self.line_pts.clear();
        self.pen.clear();
        self.arc_pts.clear();
        self.arc_tangent = None;
        self.lasso_pts.clear();
        self.acquired.clear();
        self.ref_seg = None;
        self.line_arcs.clear();
        self.line_snaps.clear();
        self.line_arc_mode = false;
        self.arc_hover = None;
        self.dim_edit = None;
        self.dwell = None;
        self.seg_dwell = None;
    }
    pub fn busy(&self) -> bool {
        self.drag.is_some() || !self.line_pts.is_empty() || !self.pen.is_empty() || !self.arc_pts.is_empty()
    }
}

struct Tf {
    rect: Rect,
    center: P2,
    zoom: f64,
}

impl Tf {
    fn s(&self, p: P2) -> Pos2 {
        pos2(self.rect.center().x + ((p.x - self.center.x) * self.zoom) as f32, self.rect.center().y - ((p.y - self.center.y) * self.zoom) as f32)
    }
    fn w(&self, p: Pos2) -> P2 {
        p2(self.center.x + (p.x - self.rect.center().x) as f64 / self.zoom, self.center.y - (p.y - self.rect.center().y) as f64 / self.zoom)
    }
}

/// Layer colour as drawn on the bed. Only colours that would vanish against the bed are swapped
/// (black on a dark bed, white on a light bed); every other layer keeps its own colour.
pub fn display_color(layer: usize, dark_bed: bool) -> Color32 {
    let [r, g, b] = layer_rgb(layer);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if dark_bed && max < 50 {
        Color32::from_rgb(232, 232, 236)
    } else if !dark_bed && min > 225 {
        Color32::from_rgb(150, 150, 150)
    } else if dark_bed && (r as u32 + g as u32 + b as u32) < 200 {
        // deep blues/purples: lift them a little so they read on a dark bed, keeping the hue
        let lift = |c: u8| (c as u32 + (255 - c as u32) / 4).min(255) as u8;
        Color32::from_rgb(lift(r), lift(g), lift(b).max(150))
    } else {
        Color32::from_rgb(r, g, b)
    }
}

/// Shift lock from `from`: horizontal, vertical or 45° (see `snap::axis_snap`).
pub fn angle_snap(from: P2, p: P2) -> P2 {
    crate::snap::axis_snap(from, p)
}

impl App {
    /// World outlines and bounds of every shape, cached. Big projects are slow to flatten, so when only whole
    /// shapes were moved, scaled or rotated (`touch_xform`) the cached outlines are transformed instead of rebuilt.
    pub fn ensure_outlines(&mut self) {
        let n = self.doc.shapes.len();
        if self.outline_cache.0 == self.rev && self.outline_cache.1.len() == n && self.bbox_cache.len() == n {
            return;
        }
        let build = |s: &Shape| -> Vec<Vec<P2>> {
            let mut leaves = Vec::new();
            s.leaves(Xform::IDENTITY, &mut leaves);
            let mut lines: Vec<Vec<P2>> = Vec::new();
            for (leaf, x) in leaves {
                if leaf.is_bitmap() {
                    continue;
                }
                for l in leaf.local_outline(0.05 / x.mean_scale()) {
                    let mut v: Vec<P2> = Vec::with_capacity(l.len() + 1);
                    // first point holds the layer index in x (unpacked when drawing)
                    v.push(p2(leaf.layer as f64, f64::NAN));
                    v.extend(l.into_iter().map(|p| x.apply(p)));
                    lines.push(v);
                }
            }
            lines
        };
        let bounds = |s: &Shape, lines: &[Vec<P2>]| -> BBox {
            let mut b = BBox::EMPTY;
            for l in lines {
                for p in &l[1..] {
                    b.add(*p);
                }
            }
            if b.is_empty() || matches!(s.kind, ShapeKind::Bitmap { .. }) || matches!(&s.kind, ShapeKind::Group(ch) if ch.iter().any(|c| c.is_bitmap())) {
                b = b.union(&s.bbox());
            }
            b
        };
        let reuse = self.cached_content_rev == self.content_rev && self.outline_cache.1.len() == n && self.outline_meta.len() == n && self.bbox_cache.len() == n;
        let mut lines_all = std::mem::take(&mut self.outline_cache.1);
        if !reuse {
            lines_all = self.doc.shapes.iter().map(build).collect();
            self.outline_meta = self.doc.shapes.iter().map(|s| (s.id, s.xform)).collect();
            self.bbox_cache = self.doc.shapes.iter().zip(&lines_all).map(|(s, l)| bounds(s, l)).collect();
        } else {
            for (i, s) in self.doc.shapes.iter().enumerate() {
                let (id, old) = self.outline_meta[i];
                if id != s.id {
                    lines_all[i] = build(s);
                } else if old != s.xform {
                    match old.inverse() {
                        Some(inv) => {
                            let d = s.xform.then_after(inv);
                            for l in lines_all[i].iter_mut() {
                                for p in l[1..].iter_mut() {
                                    *p = d.apply(*p);
                                }
                            }
                        }
                        None => lines_all[i] = build(s),
                    }
                } else {
                    continue;
                }
                self.outline_meta[i] = (s.id, s.xform);
                self.bbox_cache[i] = bounds(s, &lines_all[i]);
            }
        }
        self.outline_cache = (self.rev, lines_all);
        self.cached_content_rev = self.content_rev;
    }

    /// Tie the Relations tool's picked items with `kind`; the first picked item stays where it is.
    pub fn add_relation(&mut self, kind: crate::relations::RelKind) {
        let picks = std::mem::take(&mut self.canvas.rel_picks);
        if picks.is_empty() {
            return;
        }
        self.checkpoint();
        let held = crate::relations::hold(&picks[0]);
        self.doc.relations.push(crate::relations::make(kind, &picks));
        let ids: Vec<u64> = picks.iter().map(|e| e.shape()).collect();
        let err = crate::relations::solve_connected(&mut self.doc, &ids, &held);
        if kind == crate::relations::RelKind::Coincident {
            // two loose ends made coincident: join them into one continuous (closed, if it closes) path
            let idx: Vec<usize> = picks.iter().filter_map(|e| self.doc.shapes.iter().position(|s| s.id == e.shape())).collect();
            let joined = crate::join::join(&mut self.doc, &idx, 1e-6);
            if !joined.is_empty() {
                self.sel = joined;
            }
        }
        self.touch();
        if err > 1e-3 {
            self.warnings.push(format!("The {} relation conflicts with other relations, so nothing was moved. Remove it (Relations tool: click its tag) or another relation on those shapes.", kind.label().to_lowercase()));
        }
    }

    /// Delete the selected nodes of the path being edited.
    pub fn delete_node(&mut self) {
        let Some(&si) = self.sel.first() else { return };
        let Some(ShapeKind::Path(mut pd)) = self.doc.shapes.get(si).map(|s| s.kind.clone()) else { return };
        let nodes = std::mem::take(&mut self.canvas.nodes);
        self.checkpoint();
        if crate::nodes::delete(&mut pd, &nodes) {
            self.doc.shapes[si].kind = ShapeKind::Path(pd);
        } else {
            // removing all but one point removes the shape
            crate::ops::delete(&mut self.doc, &[si]);
            self.sel.clear();
        }
    }

    /// Snap targets (endpoints, midpoints, centres, quadrants, straight segments, outlines) near `w`.
    fn scene_near(&self, w: P2, tol: f64, exclude_sel: bool) -> crate::snap::Scene {
        use crate::snap::PointKind::*;
        let mut sc = crate::snap::Scene::default();
        let mut selected = vec![false; self.doc.shapes.len()];
        for &i in &self.sel {
            if let Some(m) = selected.get_mut(i) {
                *m = true;
            }
        }
        for (i, lines) in self.outline_cache.1.iter().enumerate() {
            if exclude_sel && selected.get(i).copied().unwrap_or(false) {
                continue;
            }
            let s = &self.doc.shapes[i];
            let b = self.shape_box(i);
            if !b.expand(tol).contains(w) || self.layer_hidden_shape(s) {
                continue;
            }
            sc.outlines.extend(lines.iter().map(|l| l[1..].to_vec()));
            let x = s.xform;
            match &s.kind {
                ShapeKind::Path(pd) => {
                    // The centre of a path's bounds is offered too; a star's
                    // centre is its own centre (the bounds of an odd star aren't centred on it)
                    let c = if crate::dims::star_params(s).is_some() { x.apply(p2(0.0, 0.0)) } else { b.center() };
                    sc.points.push((c, Center));
                    add_path_feats(&mut sc, pd, x)
                }
                ShapeKind::Ellipse { rx, ry } => {
                    sc.points.push((x.apply(p2(0.0, 0.0)), Center));
                    for q in [p2(*rx, 0.0), p2(-rx, 0.0), p2(0.0, *ry), p2(0.0, -ry)] {
                        sc.points.push((x.apply(q), Quadrant));
                    }
                    // and the four 45° points, cos(π/4)·(±rx, ±ry)
                    let k = crate::snap::ELLIPSE_DIAGONAL_RAD.cos();
                    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                        sc.points.push((x.apply(p2(sx * k * rx, sy * k * ry)), Diagonal));
                    }
                }
                ShapeKind::Rect { .. } | ShapeKind::Polygon { .. } => {
                    sc.points.push((x.apply(p2(0.0, 0.0)), Center));
                    if let Some(Shape { kind: ShapeKind::Path(pd), .. }) = s.to_path() {
                        add_path_feats(&mut sc, &pd, x);
                    }
                }
                _ => {
                    sc.points.push((b.center(), Center));
                    for c in [b.min, b.max, p2(b.min.x, b.max.y), p2(b.max.x, b.min.y)] {
                        sc.points.push((c, Endpoint));
                    }
                }
            }
        }
        sc
    }

    /// Smart-guide inference for the cursor, plus the scene it used.
    fn infer(&self, w: P2, zoom: f64, exclude_sel: bool, anchor: Option<P2>) -> (crate::snap::Inference, crate::snap::Scene) {
        let px = 1.0 / zoom;
        let tol = crate::snap::OBJ_SNAP_PX * px;
        let g = (self.settings.grid_step / 10.0).max(0.1);
        if !self.settings.snap_points {
            let p = if self.settings.snap_grid { p2((w.x / g).round() * g, (w.y / g).round() * g) } else { w };
            return (crate::snap::Inference { p, ..Default::default() }, Default::default());
        }
        let mut sc = self.scene_near(w, tol, exclude_sel);
        {
            use crate::snap::PointKind::*;
            let live: Vec<P2> = self.canvas.line_pts.iter().copied().chain(self.canvas.pen.iter().map(|v| v.p)).chain(self.canvas.arc_pts.iter().copied()).collect();
            for p in &live {
                sc.points.push((*p, Endpoint));
            }
            for w2 in self.canvas.line_pts.windows(2) {
                sc.points.push((w2[0].lerp(w2[1], 0.5), Midpoint));
                sc.segments.push((w2[0], w2[1]));
                sc.outlines.push(vec![w2[0], w2[1]]);
            }
        }
        let inf = crate::snap::infer(w, px, &sc, anchor, &self.canvas.acquired, self.canvas.ref_seg, self.settings.snap_grid.then_some(g));
        (inf, sc)
    }

    fn snap(&self, w: P2, zoom: f64, exclude_sel: bool) -> (P2, Option<P2>) {
        let (i, _) = self.infer(w, zoom, exclude_sel, None);
        (i.p, i.mark.map(|m| m.0))
    }

    fn hit_test(&self, w: P2, zoom: f64) -> Option<usize> {
        let tol = 6.0 / zoom;
        let cached = self.outline_cache.0 == self.rev && self.outline_cache.1.len() == self.doc.shapes.len();
        for i in (0..self.doc.shapes.len()).rev() {
            let s = &self.doc.shapes[i];
            if !self.shape_box(i).expand(tol).contains(w) || self.layer_hidden_shape(s) {
                continue;
            }
            if !cached || s.is_bitmap() || matches!(&s.kind, ShapeKind::Group(ch) if ch.iter().any(|c| c.is_bitmap())) {
                if s.hit(w, tol) {
                    return Some(i);
                }
                continue;
            }
            // the cached world outlines: near a line, or inside a closed outline
            let lines = &self.outline_cache.1[i];
            if lines.iter().any(|l| l[1..].windows(2).any(|sg| dist_to_segment(w, sg[0], sg[1]) <= tol)) {
                return Some(i);
            }
            let closed: Vec<Vec<P2>> = lines.iter().filter(|l| is_closed(&l[1..])).map(|l| l[1..].to_vec()).collect();
            if !closed.is_empty() && point_in_rings(w, &closed) {
                return Some(i);
            }
        }
        None
    }

    fn layer_hidden_shape(&self, s: &Shape) -> bool {
        self.doc.cut(s.layer).map(|c| c.hide).unwrap_or(false)
    }

    /// Topmost image under a point, with the pixel position.
    fn image_at(&self, w: P2) -> Option<(usize, P2)> {
        for i in (0..self.doc.shapes.len()).rev() {
            let s = &self.doc.shapes[i];
            if let ShapeKind::Bitmap { w: bw, h: bh, data, .. } = &s.kind {
                let px = world_to_pixel(s, *bw, *bh, data, w)?;
                if px.x >= 0.0 && px.y >= 0.0 && px.x < data.width as f64 && px.y < data.height as f64 {
                    return Some((i, px));
                }
            }
        }
        None
    }

    fn texture_for(&mut self, ctx: &egui::Context, s: &Shape) -> Option<egui::TextureId> {
        let ShapeKind::Bitmap { data, adjust, .. } = &s.kind else { return None };
        let mode_image = self.doc.cut(s.layer).map(|c| c.mode == CutMode::Image).unwrap_or(true);
        let invert = self.invert_bed;
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (adjust.brightness.to_bits(), adjust.contrast.to_bits(), adjust.gamma.to_bits(), adjust.invert, adjust.dither as u8, mode_image, invert).hash(&mut h);
            (std::sync::Arc::as_ptr(&data.rgba) as usize).hash(&mut h);
            h.finish()
        };
        if let Some((k, t)) = self.textures.get(&s.id) {
            if *k == key {
                return Some(t.id());
            }
        }
        let gray = crate::imaging::adjusted_gray(data, adjust);
        let mut shown = if mode_image { crate::imaging::dither(&gray, data.width as usize, data.height as usize, adjust.dither) } else { gray };
        if invert {
            shown.iter_mut().for_each(|v| *v = 255 - *v);
        }
        let img = egui::ColorImage::from_gray([data.width as usize, data.height as usize], &shown);
        let tex = ctx.load_texture(format!("bmp{}", s.id), img, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.textures.insert(s.id, (key, tex));
        Some(id)
    }
}

fn world_to_pixel(s: &Shape, bw: f64, bh: f64, data: &BitmapData, w: P2) -> Option<P2> {
    let l = s.xform.inverse()?.apply(w);
    Some(p2((l.x + bw / 2.0) / bw * data.width as f64, (bh / 2.0 - l.y) / bh * data.height as f64))
}

fn pixel_to_world(s: &Shape, p: P2) -> P2 {
    if let ShapeKind::Bitmap { w, h, data, .. } = &s.kind {
        s.xform.apply(p2(p.x / data.width as f64 * w - w / 2.0, h / 2.0 - p.y / data.height as f64 * h))
    } else {
        p
    }
}

fn selection_handles(r: Rect) -> [Pos2; 8] {
    [r.left_top(), r.center_top(), r.right_top(), r.right_center(), r.right_bottom(), r.center_bottom(), r.left_bottom(), r.left_center()]
}

/// The path data of a shape as it would be edited (paths as-is, other shapes converted).
fn editable_path(s: &Shape) -> Option<PathData> {
    match &s.kind {
        ShapeKind::Path(pd) => Some(pd.clone()),
        _ => match s.to_path()?.kind {
            ShapeKind::Path(pd) => Some(pd),
            _ => None,
        },
    }
}

/// Nearest corner vertex (with segments on both sides) of shape `si`, within `tol`.
fn corner_near(app: &App, si: usize, w: P2, tol: f64) -> Option<(usize, P2)> {
    let s = app.doc.shapes.get(si)?;
    let pd = editable_path(s)?;
    let mut best: Option<(f64, usize, P2)> = None;
    for (vi, v) in pd.verts.iter().enumerate() {
        if !pd.prims.iter().any(|p| p.b == vi) || !pd.prims.iter().any(|p| p.a == vi) {
            continue;
        }
        let wp = s.xform.apply(v.p);
        let d = wp.dist(w);
        if d < tol && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
            best = Some((d, vi, wp));
        }
    }
    best.map(|(_, v, p)| (v, p))
}

/// What the trim tool would cut at `w`: (shape index, line index in its outline, kept pieces, removed piece).
fn trim_candidate(app: &App, w: P2, zoom: f64) -> Option<(usize, usize, Vec<Vec<P2>>, Vec<P2>)> {
    let tol = 8.0 / zoom;
    let si = app.hit_test(w, zoom)?;
    let s = &app.doc.shapes[si];
    if s.is_bitmap() || matches!(s.kind, ShapeKind::Group(_)) {
        return None;
    }
    let lines = s.world_outline(0.02);
    let (li, _) = lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.windows(2).map(|q| dist_to_segment(w, q[0], q[1])).fold(f64::INFINITY, f64::min)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
    let target = &lines[li];
    let tb = BBox::from_points(target).expand(tol);
    let mut cutters: Vec<Vec<P2>> = lines.iter().enumerate().filter(|(i, _)| *i != li).map(|(_, l)| l.clone()).collect();
    for (oi, other) in app.outline_cache.1.iter().enumerate() {
        if oi == si || !app.doc.shapes[oi].bbox().intersects(&tb) {
            continue;
        }
        cutters.extend(other.iter().map(|l| l[1..].to_vec()));
    }
    let (kept, removed) = crate::sketch::trim_split(target, w, &cutters);
    Some((si, li, kept, removed))
}

pub fn canvas_ui(app: &mut App, ui: &mut Ui) -> Vec<Action> {
    let mut run = Vec::new();
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    app.canvas.rect = Some(rect);
    let ctx = ui.ctx().clone();
    let now = ui.input(|i| i.time);
    // ---- view fitting
    // keep the bed framed while the window and panels settle, until the user moves the view
    let size_changed = app.canvas.last_size.map(|s| (s - rect.size()).length() > 2.0).unwrap_or(true);
    app.canvas.last_size = Some(rect.size());
    if !app.view.fitted || (app.canvas.auto_fit && size_changed) {
        let d = &app.settings.device;
        let mut b = BBox { min: p2(0.0, 0.0), max: p2(d.width, d.height) };
        if !app.view.fitted || app.canvas.auto_fit {
            b = (0..app.doc.shapes.len()).fold(b, |b, i| b.union(&app.shape_box(i)));
        }
        app.canvas.pending_fit.get_or_insert(b);
        if !app.view.fitted {
            app.canvas.auto_fit = true;
        }
        app.view.fitted = true;
    }
    if let Some(b) = app.canvas.pending_fit.take() {
        if !b.is_empty() && rect.width() > 10.0 {
            app.view.zoom = ((rect.width() as f64 / b.width().max(1.0)).min(rect.height() as f64 / b.height().max(1.0)) * 0.9).clamp(0.02, 2000.0);
            app.view.center = b.center();
        }
    }
    if let Some(k) = app.canvas.zoom_step.take() {
        app.view.zoom = (app.view.zoom * k).clamp(0.02, 2000.0);
    }
    // ---- navigation (CAD style)
    let hover = resp.hover_pos();
    let (scroll, pinch, touch_pan, mods, space) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.multi_touch().map(|t| t.translation_delta), i.modifiers, i.key_down(egui::Key::Space)));
    app.ensure_outlines();
    // Inkscape-style: scrolling over a selected node grows / shrinks the node selection
    let mut scroll_used = false;
    if app.tool == Tool::Nodes && scroll.y.abs() > 0.5 && !app.canvas.nodes.is_empty() {
        if let (Some(hp), Some(&si)) = (hover, app.sel.first()) {
            let tf = Tf { rect, center: app.view.center, zoom: app.view.zoom };
            if let Some(s) = app.doc.shapes.get(si) {
                if let ShapeKind::Path(pd) = &s.kind {
                    let near = app.canvas.nodes.iter().any(|v| pd.verts.get(*v).map(|vt| tf.s(s.xform.apply(vt.p)).distance(hp) < 14.0).unwrap_or(false));
                    if near {
                        let acc = ui.ctx().data_mut(|d| {
                            let a = d.get_temp_mut_or_default::<f32>(egui::Id::unique("node_grow"));
                            *a += scroll.y;
                            let v = *a;
                            if v.abs() > 40.0 {
                                *a = 0.0;
                            }
                            v
                        });
                        if acc > 40.0 {
                            app.canvas.nodes = crate::nodes::grow(pd, &app.canvas.nodes);
                        } else if acc < -40.0 {
                            app.canvas.nodes = crate::nodes::shrink(pd, &app.canvas.nodes);
                        }
                        scroll_used = true;
                    }
                }
            }
        }
    }
    if let Some(hp) = hover {
        let tf = Tf { rect, center: app.view.center, zoom: app.view.zoom };
        let before = tf.w(hp);
        let mut factor = pinch as f64;
        if scroll.y.abs() > 0.0 && !scroll_used {
            factor *= (scroll.y as f64 * 0.0025).exp();
        }
        if (factor - 1.0).abs() > 1e-6 {
            app.view.zoom = (app.view.zoom * factor).clamp(0.02, 2000.0);
            let tf2 = Tf { rect, center: app.view.center, zoom: app.view.zoom };
            let after = tf2.w(hp);
            app.view.center = app.view.center + (before - after);
        }
        if scroll.x.abs() > 0.0 {
            app.view.center.x -= scroll.x as f64 / app.view.zoom;
        }
    }
    if let Some(t) = touch_pan {
        app.view.center = app.view.center + p2(-(t.x as f64), t.y as f64) * (1.0 / app.view.zoom);
    }
    let pan_drag = resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) || (resp.dragged_by(egui::PointerButton::Primary) && (app.tool == Tool::Pan || space));
    if pan_drag || scroll.length() > 0.0 || (pinch - 1.0).abs() > 1e-4 {
        app.canvas.auto_fit = false;
    }
    if pan_drag {
        let d = resp.drag_delta();
        if mods.shift && resp.dragged_by(egui::PointerButton::Middle) {
            app.view.zoom = (app.view.zoom * (-(d.y as f64) * 0.01).exp()).clamp(0.02, 2000.0);
        } else {
            app.view.center = app.view.center + p2(-(d.x as f64), d.y as f64) * (1.0 / app.view.zoom);
        }
        if app.canvas.drag.is_none() {
            app.canvas.drag = Some(Drag::Pan);
        }
    }
    let tf = Tf { rect, center: app.view.center, zoom: app.view.zoom };
    let zoom = app.view.zoom;

    // ---- pointer world position + snapping
    let raw_world = hover.map(|h| tf.w(h));
    app.canvas.hover_world = raw_world;
    let moving_sel = matches!(app.canvas.drag, Some(Drag::Move { .. }));
    let snaps = !matches!(app.tool, Tool::Select | Tool::Wand | Tool::Lasso | Tool::Trim) || moving_sel;
    // the point the new segment starts from, for horizontal / vertical / parallel inference
    let anchor = match (&app.canvas.drag, app.tool) {
        (Some(Drag::Create { start, .. }), _) | (Some(Drag::Measure { start, .. }), _) => Some(*start),
        (Some(Drag::PenHandle { .. }), _) => None,
        (_, Tool::Line) => app.canvas.line_pts.last().copied(),
        (_, Tool::Pen) => app.canvas.pen.last().map(|v| v.p),
        (_, Tool::Arc) => app.canvas.arc_pts.last().copied(),
        _ => None,
    };
    let (inference, scene) = match raw_world {
        Some(w) if snaps => app.infer(w, zoom, moving_sel, anchor),
        Some(w) => (crate::snap::Inference { p: w, ..Default::default() }, Default::default()),
        None => Default::default(),
    };
    let snapped = inference.p;
    app.canvas.snap_mark = inference.mark.map(|m| m.0);
    app.canvas.guided = inference.mark.is_some() || !inference.guides.is_empty();
    // rest on a point (0.25 s) to use it as an alignment reference; rest on a line (0.35 s) for parallel/perpendicular.
    // Only while drawing something: moving nodes or shapes doesn't collect references.
    let drawing = matches!(app.tool, Tool::Line | Tool::Pen | Tool::Arc | Tool::Rect | Tool::Ellipse | Tool::Polygon | Tool::Star | Tool::Measure) && app.canvas.drag.as_ref().map(|d| matches!(d, Drag::Create { .. } | Drag::Measure { .. } | Drag::PenHandle { .. })).unwrap_or(true);
    if !drawing {
        app.canvas.acquired.clear();
        app.canvas.ref_seg = None;
        app.canvas.dwell = None;
        app.canvas.seg_dwell = None;
    }
    match inference.mark.filter(|_| drawing) {
        Some((p, k)) if !matches!(k, crate::snap::PointKind::OnPath | crate::snap::PointKind::Grid) => match app.canvas.dwell {
            Some((q, t)) if q.dist(p) < 1e-9 => {
                if now - t > 0.25 && !app.canvas.acquired.iter().any(|a| a.dist(p) < 1e-9) {
                    app.canvas.acquired.push(p);
                    if app.canvas.acquired.len() > 6 {
                        app.canvas.acquired.remove(0);
                    }
                }
            }
            _ => app.canvas.dwell = Some((p, now)),
        },
        _ => app.canvas.dwell = None,
    }
    if let (Some(w), true) = (raw_world, snaps && drawing) {
        match crate::snap::segment_near(w, 6.0 / zoom, &scene) {
            Some(seg) => match app.canvas.seg_dwell {
                Some((s, t)) if s == seg => {
                    if now - t > 0.35 {
                        app.canvas.ref_seg = Some(seg);
                    }
                }
                _ => app.canvas.seg_dwell = Some((seg, now)),
            },
            None => app.canvas.seg_dwell = None,
        }
    }
    app.canvas.inference = inference;
    app.canvas.pointer = hover;
    app.canvas.pointer_world = hover.map(|h| tf.w(h));
    // Line tool: rest on the last point to switch the next segment between straight and tangent arc
    if app.tool == Tool::Line && app.canvas.line_pts.len() >= 2 {
        if let (Some(h), Some(last)) = (hover, app.canvas.line_pts.last().copied()) {
            let d = tf.s(last).distance(h);
            if d > 14.0 {
                app.canvas.arc_armed = true;
                app.canvas.arc_hover = None;
            } else if d < 8.0 && app.canvas.arc_armed {
                match app.canvas.arc_hover {
                    None => app.canvas.arc_hover = Some(now),
                    Some(t) if now - t > LINE_ARC_DWELL => {
                        app.canvas.line_arc_mode = !app.canvas.line_arc_mode;
                        app.canvas.arc_armed = false;
                        app.canvas.arc_hover = None;
                    }
                    _ => {}
                }
                ctx.request_repaint();
            }
        }
    }
    // Shift on line-type tools: lock the segment to horizontal, vertical or 45° from the previous point
    let last_pt = match app.tool {
        Tool::Line => app.canvas.line_pts.last().copied(),
        Tool::Pen if !matches!(app.canvas.drag, Some(Drag::PenHandle { .. })) => app.canvas.pen.last().map(|v| v.p),
        Tool::Arc => app.canvas.arc_pts.first().copied(),
        _ => None,
    };
    let snapped = match (mods.shift, last_pt) {
        (true, Some(last)) if raw_world.is_some() && !(app.tool == Tool::Line && app.canvas.line_arc_mode) => angle_snap(last, snapped),
        _ => snapped,
    };

    // ---- primary interaction
    let primary_pan = app.tool == Tool::Pan || space;
    let sel_box = ops_bbox(app);
    let sel_rect = if sel_box.is_empty() { None } else { Some(Rect::from_two_pos(tf.s(sel_box.min), tf.s(sel_box.max))) };
    if !primary_pan {
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(press) = ui.input(|i| i.pointer.press_origin()) {
                let pw = tf.w(press);
                let (psnap, _) = if snaps { app.snap(pw, zoom, false) } else { (pw, None) };
                let psnap = match (mods.shift, last_pt) {
                    (true, Some(last)) => angle_snap(last, psnap),
                    _ => psnap,
                };
                start_drag(app, press, pw, psnap, sel_rect, sel_box, zoom, mods);
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(w) = raw_world {
                update_drag(app, w, snapped, mods);
            }
        }
        if resp.drag_stopped() {
            finish_drag(app, raw_world.unwrap_or_default(), mods, zoom);
        }
        if resp.clicked() {
            if let Some(w) = raw_world {
                click(app, w, snapped, mods, zoom, now);
            }
        }
        if resp.double_clicked() {
            if let Some(w) = raw_world {
                double_click(app, w, zoom);
            }
        }
    }
    if resp.drag_stopped() && matches!(app.canvas.drag, Some(Drag::Pan)) {
        app.canvas.drag = None;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.egui_wants_keyboard_input() {
        match app.tool {
            Tool::Line if !app.canvas.line_pts.is_empty() => finish_line(app, false),
            Tool::Pen if !app.canvas.pen.is_empty() => finish_pen(app, false),
            _ => {}
        }
    }

    // ---- context menu
    resp.context_menu(|ui| {
        for a in [Action::Undo, Action::Redo] {
            if ui.add_enabled(app.enabled(a), egui::Button::new(a.label())).clicked() {
                run.push(a);
                ui.close();
            }
        }
        ui.separator();
        let mut items = vec![Action::Cut, Action::Copy, Action::Paste, Action::Duplicate, Action::Delete];
        if app.tool == Tool::Nodes {
            items = vec![Action::NodeSmooth, Action::NodeCorner, Action::NodeLine, Action::NodeCurve, Action::NodeInsert, Action::NodeBreak, Action::Delete];
        }
        for a in items {
            if ui.add_enabled(app.enabled(a), egui::Button::new(a.label()).shortcut_text(app.settings.keymap.label(a))).clicked() {
                run.push(a);
                ui.close();
            }
        }
        ui.separator();
        for a in [Action::Group, Action::Ungroup, Action::ConvertToPath, Action::FlipH, Action::FlipV, Action::PageCenter, Action::TraceImage, Action::SelectionToPath] {
            if app.enabled(a) && ui.add(egui::Button::new(a.label()).shortcut_text(app.settings.keymap.label(a))).clicked() {
                run.push(a);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Fit bed").clicked() {
            run.push(Action::ZoomPage);
            ui.close();
        }
    });

    paint(app, ui, &ctx, rect, &tf, &resp, raw_world, snapped, mods, now, primary_pan);

    if app.doc.shapes.is_empty() && app.screenshot.is_none() && !app.welcome_off {
        welcome(ui, app, rect, &mut run);
        if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("hide_welcome"))).unwrap_or(false) {
            app.welcome_off = true;
        }
    }
    run
}

#[allow(clippy::too_many_arguments)]
fn paint(app: &mut App, ui: &mut Ui, ctx: &egui::Context, rect: Rect, tf: &Tf, resp: &egui::Response, raw_world: Option<P2>, snapped: P2, mods: egui::Modifiers, now: f64, primary_pan: bool) {
    let dark = ui.visuals().dark_mode;
    let col = theme::colors(dark);
    let zoom = tf.zoom;
    let hover = resp.hover_pos();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, col.canvas);
    let dev = app.settings.device.clone();
    // the bed: dark or light, optionally inverted to check designs for dark materials
    let dark_bed = dark != app.invert_bed;
    let bed_fill = if app.invert_bed { if dark { Color32::from_rgb(238, 236, 230) } else { Color32::from_rgb(38, 36, 34) } } else { col.bed };
    let (grid_minor, grid_major) = if app.invert_bed {
        if dark {
            (Color32::from_rgb(220, 218, 212), Color32::from_rgb(196, 194, 188))
        } else {
            (Color32::from_rgb(52, 50, 48), Color32::from_rgb(72, 70, 66))
        }
    } else {
        (col.grid_minor, col.grid_major)
    };
    let bed = Rect::from_two_pos(tf.s(p2(0.0, 0.0)), tf.s(p2(dev.width, dev.height)));
    painter.rect_filled(bed.expand(3.0), 4.0, Color32::from_black_alpha(if dark { 70 } else { 25 }));
    painter.rect_filled(bed, 2.0, bed_fill);
    if app.settings.show_grid {
        let major = app.settings.grid_step.max(1.0);
        let minor = major / 10.0;
        if minor * zoom > 6.0 {
            draw_grid(&painter, tf, &dev, minor, grid_minor);
        }
        draw_grid(&painter, tf, &dev, major, grid_major);
        let mut x = 0.0;
        let step = nice_step(80.0 / zoom);
        while x <= dev.width + 1e-6 {
            painter.text(tf.s(p2(x, 0.0)) + vec2(2.0, 3.0), Align2::LEFT_TOP, fmt_num(x * app.settings.units.factor()), FontId::proportional(10.0), col.weak);
            x += step;
        }
        let mut y = step;
        while y <= dev.height + 1e-6 {
            painter.text(tf.s(p2(0.0, y)) + vec2(-4.0, 0.0), Align2::RIGHT_CENTER, fmt_num(y * app.settings.units.factor()), FontId::proportional(10.0), col.weak);
            y += step;
        }
    }
    // the camera's picture of the bed, warped into place with the 4-point alignment
    if app.settings.camera.show {
        let aspect = app.camera_tex.as_ref().map(|(_, t)| (t.size_vec2().x / t.size_vec2().y.max(1.0)) as f64).unwrap_or(1.0);
        if let (Some((_, tex)), Some(h)) = (&app.camera_tex, crate::camera::bed_to_image(&app.settings.camera, dev.width, dev.height, aspect)) {
            let quad = app.settings.camera.bed(dev.width, dev.height);
            let n = 16usize;
            let mut mesh = egui::Mesh::with_texture(tex.id());
            let alpha = (app.settings.camera.opacity.clamp(0.05, 1.0) * 255.0) as u8;
            let bil = |s: f64, t: f64| {
                let a = p2(quad[0].x + (quad[1].x - quad[0].x) * s, quad[0].y + (quad[1].y - quad[0].y) * s);
                let b = p2(quad[3].x + (quad[2].x - quad[3].x) * s, quad[3].y + (quad[2].y - quad[3].y) * s);
                p2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
            };
            for j in 0..=n {
                for i in 0..=n {
                    let bp = bil(i as f64 / n as f64, j as f64 / n as f64);
                    let uv = h.apply(bp);
                    mesh.vertices.push(egui::epaint::Vertex { pos: tf.s(bp), uv: pos2(uv.x as f32, uv.y as f32), color: Color32::from_white_alpha(alpha) });
                }
            }
            for j in 0..n {
                for i in 0..n {
                    let k = (j * (n + 1) + i) as u32;
                    let w = (n + 1) as u32;
                    mesh.indices.extend([k, k + 1, k + w + 1, k, k + w + 1, k + w]);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    // rotary: the band one turn around the object covers, centred on the design
    if app.rotary_on() {
        let r = &app.settings.device.rotary;
        let y0 = crate::rotary::centre_line(&app.doc, dev.height);
        let half = r.circumference() / 2.0;
        let band = Color32::from_rgba_unmultiplied(230, 150, 60, 200);
        for y in [y0 - half, y0 + half] {
            let a = tf.s(p2(0.0, y));
            let b = tf.s(p2(dev.width, y));
            painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, band), 8.0, 5.0));
        }
        painter.text(tf.s(p2(dev.width, y0 + half)) + vec2(-4.0, -4.0), Align2::RIGHT_BOTTOM, format!("rotary: one turn, {:.1} mm", r.circumference()), FontId::proportional(11.0), band);
        painter.line_segment([tf.s(p2(0.0, y0)), tf.s(p2(dev.width, y0))], Stroke::new(1.0, band.gamma_multiply(0.5)));
    }
    let o = tf.s(p2(0.0, 0.0));
    painter.circle_filled(o, 4.0, ACCENT);
    painter.line_segment([o, o + vec2(22.0, 0.0)], Stroke::new(2.0, Color32::from_rgb(230, 80, 80)));
    painter.line_segment([o, o - vec2(0.0, 22.0)], Stroke::new(2.0, Color32::from_rgb(90, 200, 90)));

    // bitmaps first (they sit under vectors, like LightBurn)
    let shapes_snapshot: Vec<(usize, Shape)> = app.doc.shapes.iter().enumerate().filter(|(_, s)| s.is_bitmap()).map(|(i, s)| (i, s.clone())).collect();
    for (_, s) in &shapes_snapshot {
        if app.layer_hidden_shape(s) {
            continue;
        }
        if let (ShapeKind::Bitmap { w, h, .. }, Some(tex)) = (&s.kind, app.texture_for(ctx, s)) {
            let corners = [p2(-w / 2.0, h / 2.0), p2(w / 2.0, h / 2.0), p2(w / 2.0, -h / 2.0), p2(-w / 2.0, -h / 2.0)];
            let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
            let mut mesh = egui::Mesh::with_texture(tex);
            for k in 0..4 {
                mesh.vertices.push(egui::epaint::Vertex { pos: tf.s(s.xform.apply(corners[k])), uv: uvs[k], color: Color32::WHITE });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
            painter.add(EShape::mesh(mesh));
        }
    }
    // filled view: each Fill layer is filled as a whole with the even-odd rule, the same as the laser
    // (a shape inside a filled shape is a hole, a shape inside that is filled again, and so on)
    if app.settings.filled_view {
        // fill spacing in whole powers of two, so zooming doesn't refill the view at every step
        let step = 2f64.powf((2.5 / zoom).max(0.05).log2().round()).max(0.05);
        let transforming = matches!(app.canvas.drag, Some(Drag::Move { .. }) | Some(Drag::Scale { .. }) | Some(Drag::Rotate { .. }));
        if !transforming && (app.fill_cache.0 != app.rev || (app.fill_cache.1 - step).abs() > step * 0.2) {
            let mut per_layer: std::collections::BTreeMap<usize, Vec<Vec<P2>>> = Default::default();
            let fill_layer = |layer: usize| matches!(app.doc.cut(layer).map(|c| c.mode).unwrap_or(CutMode::Line), CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill);
            for lines in &app.outline_cache.1 {
                for l in lines {
                    let layer = l[0].x as usize;
                    if fill_layer(layer) && is_closed(&l[1..]) {
                        per_layer.entry(layer).or_default().push(l[1..].to_vec());
                    }
                }
            }
            let mut fills = Vec::new();
            for (layer, rings) in per_layer {
                let segs: Vec<(P2, P2)> = crate::gcode::fill_segments(&rings, step, 0.0).into_iter().flatten().collect();
                fills.push((layer, segs));
            }
            app.fill_cache = (app.rev, step, fills);
        }
        for (layer, segs) in &app.fill_cache.2 {
            let c = display_color(*layer, dark_bed).gamma_multiply(0.55);
            for (a, b) in segs.iter().take(80_000) {
                painter.line_segment([tf.s(*a), tf.s(*b)], Stroke::new(1.0, c));
            }
        }
    }
    // vector outlines
    let view_box = BBox { min: tf.w(rect.left_bottom()), max: tf.w(rect.right_top()) };
    let bed_box = crate::gcode::bed_box(&dev);
    let mut sel_mask = vec![false; app.doc.shapes.len()];
    for &i in &app.sel {
        if let Some(m) = sel_mask.get_mut(i) {
            *m = true;
        }
    }
    let hidden: std::collections::HashSet<usize> = app.doc.cuts.iter().filter(|c| c.hide).map(|c| c.index).collect();
    for (i, lines) in app.outline_cache.1.iter().enumerate() {
        let selected = sel_mask[i];
        let s = &app.doc.shapes[i];
        let sb = app.bbox_cache.get(i).copied().unwrap_or_else(|| s.bbox());
        if !sb.intersects(&view_box) {
            continue;
        }
        // a whole shape no bigger than a pixel each way: one dot at its centre
        let tiny = sb.width() * zoom <= DOT_SIZE_PX && sb.height() * zoom <= DOT_SIZE_PX;
        for l in lines {
            let layer = l[0].x as usize;
            if hidden.contains(&layer) {
                continue;
            }
            if tiny {
                painter.rect_filled(Rect::from_center_size(tf.s(sb.center()), vec2(1.0, 1.0)), 0.0, display_color(layer, dark_bed));
                break;
            }
            // a point is only drawn when it is at least half a pixel
            // (|dx| + |dy|) from the last one drawn
            let mut pts: Vec<Pos2> = Vec::with_capacity(l.len());
            for p in &l[1..] {
                let q = tf.s(*p);
                if pts.last().map(|lp: &Pos2| ((q.x - lp.x).abs() + (q.y - lp.y).abs()) as f64 >= MIN_SCREEN_STEP).unwrap_or(true) {
                    pts.push(q);
                }
            }
            if let Some(last) = l.last() {
                let q = tf.s(*last);
                if pts.last() != Some(&q) {
                    pts.push(q);
                }
            }
            if pts.len() < 2 {
                continue;
            }
            if selected {
                painter.add(EShape::line(pts.clone(), Stroke::new(4.5, SELECT.gamma_multiply(0.45))));
            }
            if layer == crate::model::CONSTRUCTION_LAYER {
                // construction geometry: thin dashes, never burned
                let c = if dark_bed { Color32::from_rgb(150, 170, 200) } else { Color32::from_rgb(90, 110, 140) };
                painter.add(EShape::dashed_line(&pts, Stroke::new(if selected { 1.4 } else { 1.0 }, c), 7.0, 4.0));
                continue;
            }
            painter.add(EShape::line(pts, Stroke::new(if selected { 1.8 } else { 1.4 }, display_color(layer, dark_bed))));
        }
        if s.locked && selected {
            painter.text(tf.s(p2(sb.max.x, sb.max.y)), Align2::LEFT_BOTTOM, "locked", FontId::proportional(10.0), col.weak);
        }
    }
    // shapes off the bed: only while previewing or burning, when it matters. Nearby shapes are merged into
    // one tinted outline, and the count is said once.
    if app.preview_on || app.laser.progress.1 > 0 {
        draw_off_bed(app, &painter, tf, bed_box, rect);
    }
    for (i, s) in &shapes_snapshot {
        if app.sel.contains(i) {
            if let ShapeKind::Bitmap { w, h, .. } = &s.kind {
                let pts: Vec<Pos2> = [p2(-w / 2.0, h / 2.0), p2(w / 2.0, h / 2.0), p2(w / 2.0, -h / 2.0), p2(-w / 2.0, -h / 2.0), p2(-w / 2.0, h / 2.0)].iter().map(|c| tf.s(s.xform.apply(*c))).collect();
                painter.add(EShape::line(pts, Stroke::new(1.5, SELECT)));
            }
        }
    }

    // image selection ("marching ants")
    if let Some(sel) = &app.image_sel {
        if let Some(i) = app.index_of_id(sel.shape_id) {
            let s = &app.doc.shapes[i];
            let phase = ((now * 8.0) % 8.0) as f32;
            for l in &sel.outline {
                let pts: Vec<Pos2> = l.iter().map(|p| tf.s(pixel_to_world(s, *p))).collect();
                painter.add(EShape::line(pts.clone(), Stroke::new(1.6, Color32::WHITE)));
                painter.extend(EShape::dashed_line_with_offset(&pts, Stroke::new(1.6, Color32::BLACK), &[4.0], &[4.0], phase));
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    // font preview while pointing at fonts in the list
    if let (Some(spec), [si]) = (app.canvas.font_hover.clone(), app.sel.as_slice()) {
        if let Some(s) = app.doc.shapes.get(*si) {
            if let ShapeKind::Text { text, height, style, .. } = &s.kind {
                let key = format!("{spec}|{text}|{height}|{:?}", style);
                let fresh = !matches!(&app.canvas.font_preview_cache, Some((k, id, _)) if *k == key && *id == s.id);
                if fresh {
                    let lines = crate::text::text_outline(text, &spec, *height, style, 0.05);
                    app.canvas.font_preview_cache = Some((key, s.id, lines));
                }
                if let Some((_, _, lines)) = &app.canvas.font_preview_cache {
                    let b = s.bbox();
                    painter.rect_filled(Rect::from_two_pos(tf.s(b.min), tf.s(b.max)).expand(6.0), 4.0, bed_fill.gamma_multiply(0.92));
                    for l in lines {
                        painter.add(EShape::line(l.iter().map(|p| tf.s(s.xform.apply(*p))).collect(), Stroke::new(1.6, ACCENT)));
                    }
                }
            }
        }
    }

    // trace / AI previews (green), offset preview (orange)
    let trace_lines: Vec<Vec<P2>> = if app.win_trace {
        app.trace.preview.clone()
    } else if let crate::app::McpState::Done { lines, .. } = &app.mcp {
        lines.clone()
    } else {
        Vec::new()
    };
    for l in &trace_lines {
        painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.6, Color32::from_rgb(40, 200, 120))));
    }
    for l in &app.offset_preview {
        painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.8, Color32::from_rgb(255, 150, 40))));
    }
    // combine preview while pointing at Weld / Union / Subtract / Intersect
    if let Some(a @ (Action::Weld | Action::BoolUnion | Action::BoolSubtract | Action::BoolIntersect)) = app.hover_action {
        if app.sel.len() >= 2 {
            let fresh = !matches!(&app.canvas.bool_preview, Some((r, pa, _)) if *r == app.rev && *pa == a);
            if fresh {
                let op = match a {
                    Action::BoolSubtract => crate::ops::BoolOp::Subtract,
                    Action::BoolIntersect => crate::ops::BoolOp::Intersect,
                    _ => crate::ops::BoolOp::Union,
                };
                let mut d = app.doc.clone();
                let lines = match crate::ops::boolean(&mut d, &app.sel, op) {
                    Ok(i) => d.shapes[i].world_outline(0.05),
                    Err(_) => Vec::new(),
                };
                app.canvas.bool_preview = Some((app.rev, a, lines));
            }
            if let Some((_, _, lines)) = &app.canvas.bool_preview {
                let b = ops_bbox(app);
                painter.rect_filled(Rect::from_two_pos(tf.s(b.min), tf.s(b.max)).expand(8.0), 6.0, bed_fill.gamma_multiply(0.85));
                let fill_step = (3.0 / zoom).max(0.05);
                for (pa, pb) in crate::gcode::fill_segments(lines, fill_step, 45.0).into_iter().flatten() {
                    painter.line_segment([tf.s(pa), tf.s(pb)], Stroke::new(1.0, ACCENT.gamma_multiply(0.5)));
                }
                for l in lines {
                    painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(2.0, ACCENT)));
                }
                let msg = if lines.is_empty() { "Can't combine these (shapes must be closed and overlap)".to_string() } else { format!("Result of {}", a.label()) };
                painter.text(tf.s(p2(b.min.x, b.max.y)) - vec2(0.0, 12.0), Align2::LEFT_BOTTOM, msg, FontId::proportional(12.0), ACCENT);
            }
        }
    }
    if app.win_circular {
        let c = tf.s(app.circular.3);
        painter.line_segment([c - vec2(10.0, 0.0), c + vec2(10.0, 0.0)], Stroke::new(2.0, ACCENT));
        painter.line_segment([c - vec2(0.0, 10.0), c + vec2(0.0, 10.0)], Stroke::new(2.0, ACCENT));
        let b = ops_bbox(app);
        if !b.is_empty() {
            let r = app.circular.3.dist(b.center()) * zoom;
            painter.circle_stroke(c, r as f32, Stroke::new(1.0, ACCENT.gamma_multiply(0.6)));
        }
    }

    // job preview
    if app.preview_on {
        if let Some(job) = &app.job {
            let n = ((job.moves.len() as f32) * app.preview_t) as usize;
            painter.rect_filled(bed, 2.0, Color32::from_black_alpha(if dark_bed { 120 } else { 40 }));
            let stride = (n / 150_000).max(1);
            for m in job.moves[..n].iter().step_by(stride) {
                if m.power <= 0.0 {
                    painter.add(EShape::dashed_line(&[tf.s(m.from), tf.s(m.to)], Stroke::new(0.7, col.weak.gamma_multiply(0.5)), 4.0, 4.0));
                } else {
                    let c = display_color(m.layer, true);
                    painter.line_segment([tf.s(m.from), tf.s(m.to)], Stroke::new(1.4, c.gamma_multiply(0.35 + 0.65 * m.power.min(1.0))));
                }
            }
            if n > 0 {
                let head = tf.s(job.moves[n - 1].to);
                painter.circle_filled(head, 5.0, Color32::from_rgb(255, 60, 40));
                if app.preview_playing {
                    painter.circle_stroke(head, 9.0, Stroke::new(1.5, Color32::from_rgb(255, 60, 40).gamma_multiply(0.6)));
                }
            }
        }
    }

    // selection box + handles
    let sel_box = ops_bbox(app);
    if !sel_box.is_empty() && app.tool == Tool::Select && !app.preview_on {
        let r = Rect::from_two_pos(tf.s(sel_box.min), tf.s(sel_box.max));
        painter.add(EShape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.0, SELECT), 6.0, 4.0));
        for h in selection_handles(r) {
            let hr = Rect::from_center_size(h, vec2(9.0, 9.0));
            painter.rect_filled(hr, 2.0, col.bed);
            painter.rect_stroke(hr, 2.0, Stroke::new(1.5, SELECT), StrokeKind::Middle);
        }
        let rot = r.center_top() - vec2(0.0, 22.0);
        painter.line_segment([r.center_top(), rot], Stroke::new(1.0, SELECT));
        painter.circle_filled(rot, 5.5, SELECT);
        let u = app.settings.units;
        painter.text(r.center_bottom() + vec2(0.0, 8.0), Align2::CENTER_TOP, format!("{} × {} {}", fmt_num((sel_box.width() * u.factor() * 100.0).round() / 100.0), fmt_num((sel_box.height() * u.factor() * 100.0).round() / 100.0), u.suffix()), FontId::proportional(11.0), SELECT);
        // part of an array: a button to change it, right under the selection
        app.canvas.array_chip = None;
        if app.enabled(Action::EditArray) {
            let g = painter.layout_no_wrap("Edit array".into(), FontId::proportional(11.5), Color32::WHITE);
            let chip = Rect::from_center_size(r.center_bottom() + vec2(0.0, 34.0), g.size() + vec2(28.0, 8.0));
            let hovered = app.canvas.pointer.map(|p| chip.contains(p)).unwrap_or(false);
            painter.rect_filled(chip, 5.0, if hovered { ACCENT } else { ACCENT.gamma_multiply(0.75) });
            crate::icons::paint(&painter, Rect::from_center_size(pos2(chip.left() + 12.0, chip.center().y), vec2(14.0, 14.0)), crate::icons::Icon::Array, &crate::icons::Pal { fg: Color32::WHITE, accent: Color32::WHITE, soft: Color32::TRANSPARENT });
            painter.galley(pos2(chip.left() + 22.0, chip.center().y - g.size().y / 2.0), g, Color32::WHITE);
            app.canvas.array_chip = Some(chip);
        }
    } else {
        app.canvas.array_chip = None;
    }

    // node editing overlay
    if app.tool == Tool::Nodes {
        if let Some(&si) = app.sel.first() {
            if let Some(s) = app.doc.shapes.get(si) {
                if let ShapeKind::Path(pd) = &s.kind {
                    for (vi, v) in pd.verts.iter().enumerate().take(20000) {
                        let sp = tf.s(s.xform.apply(v.p));
                        let on = app.canvas.nodes.contains(&vi);
                        if on {
                            for c in [v.c0, v.c1].into_iter().flatten() {
                                let cp = tf.s(s.xform.apply(c));
                                painter.line_segment([sp, cp], Stroke::new(1.0, col.weak));
                                painter.circle_filled(cp, 3.5, ACCENT);
                            }
                        }
                        if v.c0.is_some() && v.c1.is_some() {
                            painter.circle_filled(sp, 4.5, if on { ACCENT } else { col.bed });
                            painter.circle_stroke(sp, 4.5, Stroke::new(1.2, ACCENT));
                        } else {
                            let r = Rect::from_center_size(sp, vec2(8.0, 8.0));
                            painter.rect_filled(r, 1.0, if on { ACCENT } else { col.bed });
                            painter.rect_stroke(r, 1.0, Stroke::new(1.2, ACCENT), StrokeKind::Middle);
                        }
                    }
                    let b = s.bbox();
                    let msg = if app.canvas.nodes.is_empty() {
                        "Click points, or drag a box around them (Alt-drag = lasso)".to_string()
                    } else {
                        format!("{} point(s) selected · S smooth · C corner · L line · D delete · scroll over a point to grow", app.canvas.nodes.len())
                    };
                    painter.text(tf.s(p2(b.min.x, b.max.y)) - vec2(0.0, 10.0), Align2::LEFT_BOTTOM, msg, FontId::proportional(11.5), ACCENT);
                } else {
                    let b = s.bbox();
                    painter.text(tf.s(p2(b.center().x, b.max.y)) - vec2(0.0, 8.0), Align2::CENTER_BOTTOM, "Drag a point to convert this shape to a path", FontId::proportional(12.0), ACCENT);
                }
            }
        }
    }

    // tool hover feedback
    if let Some(w) = raw_world {
        match app.tool {
            Tool::Trim if app.canvas.drag.is_none() => {
                if let Some((_, _, _, removed)) = trim_candidate(app, w, zoom) {
                    painter.add(EShape::line(removed.iter().map(|p| tf.s(*p)).collect(), Stroke::new(4.0, Color32::from_rgb(235, 70, 60))));
                }
            }
            Tool::Fillet => {
                let target = app.hit_test(w, zoom).or(app.sel.first().copied());
                if let Some(si) = target {
                    if let Some((_, cp)) = corner_near(app, si, w, 14.0 / zoom) {
                        painter.circle_stroke(tf.s(cp), 9.0, Stroke::new(2.0, ACCENT));
                        let f = app.opts.fillet;
                        painter.text(tf.s(cp) + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, format!("{} {}", f.kind.label(), fmt_num(f.d1)), FontId::proportional(11.0), ACCENT);
                    }
                }
            }
            _ => {}
        }
    }

    // in-progress drags
    match &app.canvas.drag {
        Some(Drag::BoxSelect { start }) | Some(Drag::NodeBox { start }) => {
            if let Some(cur) = raw_world {
                let r = Rect::from_two_pos(tf.s(*start), tf.s(cur));
                let window = cur.x >= start.x || matches!(app.canvas.drag, Some(Drag::NodeBox { .. }));
                let c = if window { SELECT } else { Color32::from_rgb(60, 200, 110) };
                painter.rect_filled(r, 0.0, c.gamma_multiply(0.12));
                if window {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, c), StrokeKind::Middle);
                } else {
                    painter.add(EShape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.0, c), 5.0, 4.0));
                }
            }
        }
        Some(Drag::NodeLasso) | Some(Drag::ImageLasso { .. }) => {
            let mut pts: Vec<Pos2> = app.canvas.lasso_pts.iter().map(|p| tf.s(*p)).collect();
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            painter.add(EShape::dashed_line(&pts, Stroke::new(1.4, SELECT), 5.0, 3.0));
        }
        Some(Drag::Create { start, cur }) => {
            if let Some(shape) = creation_shape(app, *start, *cur, mods) {
                for l in shape.world_outline(0.5 / zoom) {
                    painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.5, ACCENT)));
                }
                let b = shape.bbox();
                painter.text(tf.s(b.max) + vec2(6.0, -4.0), Align2::LEFT_BOTTOM, format!("{} × {}", fmt_num((b.width() * 10.0).round() / 10.0), fmt_num((b.height() * 10.0).round() / 10.0)), FontId::proportional(11.0), ACCENT);
            }
        }
        Some(Drag::Measure { start, cur }) => draw_measure(&painter, tf, *start, *cur),
        _ => {}
    }
    if app.canvas.drag.is_none() && app.tool == Tool::Measure {
        if let Some((a, b)) = app.canvas.last_measure {
            draw_measure(&painter, tf, a, b);
        }
    }
    // line tool rubber band
    if !app.canvas.line_pts.is_empty() {
        let mut wpts = app.canvas.line_pts.clone();
        let mut arcs = app.canvas.line_arcs.clone();
        if raw_world.is_some() {
            arcs.push(if app.canvas.line_arc_mode { line_end_tangent(&wpts, &arcs) } else { None });
            wpts.push(snapped);
        }
        if wpts.len() >= 2 {
            for l in line_path(&wpts, &arcs, false).flatten(0.5 / zoom) {
                painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.5, ACCENT)));
            }
        }
        let pts: Vec<Pos2> = wpts.iter().map(|p| tf.s(*p)).collect();
        for p in &pts {
            painter.rect_filled(Rect::from_center_size(*p, vec2(6.0, 6.0)), 1.0, ACCENT);
        }
        // resting on the last point: a ring fills up, then the next segment switches line <-> arc
        if let (Some(t), Some(last)) = (app.canvas.arc_hover, app.canvas.line_pts.last()) {
            let f = ((now - t) / LINE_ARC_DWELL).clamp(0.0, 1.0) as f32;
            let c = tf.s(*last);
            let n = 24;
            let ring: Vec<Pos2> = (0..=((n as f32 * f) as usize)).map(|k| {
                let a = -std::f32::consts::FRAC_PI_2 + k as f32 / n as f32 * std::f32::consts::TAU;
                c + vec2(a.cos(), a.sin()) * 11.0
            }).collect();
            if ring.len() > 1 {
                painter.add(EShape::line(ring, Stroke::new(2.5, Color32::from_rgb(60, 200, 110))));
            }
        }
        if app.canvas.line_arc_mode && raw_world.is_some() {
            painter.text(tf.s(snapped) + vec2(14.0, -26.0), Align2::LEFT_BOTTOM, "Arc (rest on the last point to switch back)", FontId::proportional(11.0), Color32::from_rgb(60, 200, 110));
        }
        if raw_world.is_some() {
            let first = app.canvas.line_pts[0];
            if tf.s(first).distance(tf.s(snapped)) < 10.0 && app.canvas.line_pts.len() > 2 {
                painter.circle_stroke(tf.s(first), 9.0, Stroke::new(2.0, ACCENT));
            }
            segment_label(&painter, tf, *app.canvas.line_pts.last().unwrap(), snapped);
        }
    }
    // pen tool preview
    if !app.canvas.pen.is_empty() {
        let mut verts = app.canvas.pen.clone();
        if raw_world.is_some() && !matches!(app.canvas.drag, Some(Drag::PenHandle { .. })) {
            verts.push(Vert::new(snapped));
        }
        let pd = pen_path(&verts, false);
        for l in pd.flatten(0.5 / zoom) {
            painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.6, ACCENT)));
        }
        for v in &app.canvas.pen {
            let sp = tf.s(v.p);
            for c in [v.c0, v.c1].into_iter().flatten() {
                painter.line_segment([sp, tf.s(c)], Stroke::new(1.0, col.weak));
                painter.circle_filled(tf.s(c), 3.0, ACCENT);
            }
            painter.rect_filled(Rect::from_center_size(sp, vec2(6.0, 6.0)), 1.0, ACCENT);
        }
        if app.canvas.pen.len() > 2 && raw_world.is_some() && tf.s(app.canvas.pen[0].p).distance(tf.s(snapped)) < 10.0 {
            painter.circle_stroke(tf.s(app.canvas.pen[0].p), 9.0, Stroke::new(2.0, ACCENT));
        }
    }
    // arc tool preview
    if !app.canvas.arc_pts.is_empty() && raw_world.is_some() {
        if let Some(pd) = arc_preview(app, snapped) {
            for l in pd.flatten(0.5 / zoom) {
                painter.add(EShape::line(l.iter().map(|p| tf.s(*p)).collect(), Stroke::new(1.6, ACCENT)));
            }
        }
        for p in &app.canvas.arc_pts {
            painter.rect_filled(Rect::from_center_size(tf.s(*p), vec2(6.0, 6.0)), 1.0, ACCENT);
        }
    }
    draw_guides(app, &painter, tf, raw_world.is_some() && !mods.shift);
    if app.tool == Tool::Dimension {
        draw_dims(app, &painter, tf, view_box);
    } else {
        app.canvas.dim_hits.clear();
    }
    if matches!(app.tool, Tool::Relate | Tool::Dimension | Tool::MirrorLine) {
        draw_relations(app, &painter, tf, zoom);
    } else {
        app.canvas.rel_hits.clear();
    }

    // cursor
    if hover.is_some() {
        match app.tool {
            Tool::Rect | Tool::Ellipse | Tool::Polygon | Tool::Line | Tool::Text | Tool::Measure | Tool::Pen | Tool::Arc | Tool::Star | Tool::Lasso | Tool::Wand => ctx.set_cursor_icon(egui::CursorIcon::Crosshair),
            Tool::Trim | Tool::Fillet => ctx.set_cursor_icon(egui::CursorIcon::PointingHand),
            _ => {}
        }
    }
    if primary_pan || matches!(app.canvas.drag, Some(Drag::Pan)) {
        ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
}

fn segment_label(painter: &egui::Painter, tf: &Tf, from: P2, to: P2) {
    let d = from.dist(to);
    let ang = (to - from).y.atan2((to - from).x).to_degrees();
    painter.text(tf.s(to) + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, format!("{:.2} mm  {:.1}°", d, ang), FontId::proportional(11.0), ACCENT);
}

fn ops_bbox(app: &App) -> BBox {
    app.selection_box()
}

fn nice_step(raw: f64) -> f64 {
    let p = 10f64.powf(raw.max(1e-6).log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= raw {
            return m * p;
        }
    }
    10.0 * p
}

fn draw_grid(painter: &egui::Painter, tf: &Tf, dev: &crate::gcode::Device, step: f64, c: Color32) {
    let n = (dev.width / step) as usize + (dev.height / step) as usize;
    if n > 600 {
        return;
    }
    let mut x = 0.0;
    while x <= dev.width + 1e-9 {
        painter.line_segment([tf.s(p2(x, 0.0)), tf.s(p2(x, dev.height))], Stroke::new(1.0, c));
        x += step;
    }
    let mut y = 0.0;
    while y <= dev.height + 1e-9 {
        painter.line_segment([tf.s(p2(0.0, y)), tf.s(p2(dev.width, y))], Stroke::new(1.0, c));
        y += step;
    }
}

fn draw_measure(painter: &egui::Painter, tf: &Tf, a: P2, b: P2) {
    let c = Color32::from_rgb(255, 200, 40);
    painter.line_segment([tf.s(a), tf.s(b)], Stroke::new(1.5, c));
    painter.circle_filled(tf.s(a), 3.0, c);
    painter.circle_filled(tf.s(b), 3.0, c);
    let d = b - a;
    painter.text(tf.s(a.lerp(b, 0.5)) + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{:.2} mm   ΔX {:.2}  ΔY {:.2}   {:.1}°", d.len(), d.x, d.y, d.y.atan2(d.x).to_degrees()), FontId::proportional(12.0), c);
}

/// Points inside a closed polygon (even-odd).
fn inside(poly: &[P2], p: P2) -> bool {
    let mut ring = poly.to_vec();
    if ring.len() > 2 {
        ring.push(ring[0]);
    }
    point_in_rings(p, &[ring])
}

#[allow(clippy::too_many_arguments)]
fn start_drag(app: &mut App, press: Pos2, pw: P2, psnap: P2, sel_rect: Option<Rect>, sel_box: BBox, zoom: f64, mods: egui::Modifiers) {
    match app.tool {
        Tool::Select => {
            if let Some(r) = sel_rect {
                let rot = r.center_top() - vec2(0.0, 22.0);
                let orig: Vec<(usize, Xform)> = app.sel.iter().map(|i| (*i, app.doc.shapes[*i].xform)).collect();
                if rot.distance(press) < 9.0 {
                    let c = sel_box.center();
                    app.checkpoint();
                    app.canvas.drag = Some(Drag::Rotate { center: c, start_angle: (pw.y - c.y).atan2(pw.x - c.x), orig });
                    return;
                }
                for (hi, h) in selection_handles(r).iter().enumerate() {
                    if h.distance(press) < 9.0 {
                        let opp = selection_handles(r)[(hi + 4) % 8];
                        let tfp = |p: Pos2| -> P2 {
                            let fx = ((p.x - r.left()) / r.width().max(1e-3)) as f64;
                            let fy = ((p.y - r.top()) / r.height().max(1e-3)) as f64;
                            p2(sel_box.min.x + fx * sel_box.width(), sel_box.max.y - fy * sel_box.height())
                        };
                        let anchor = if mods.ctrl || mods.command { sel_box.center() } else { tfp(opp) };
                        app.checkpoint();
                        app.canvas.drag = Some(Drag::Scale { handle: hi, anchor, start: tfp(*h), orig });
                        return;
                    }
                }
            }
            let hit = app.hit_test(pw, zoom);
            let inside_sel = sel_box.expand(3.0 / zoom).contains(pw) && !app.sel.is_empty();
            if let Some(h) = hit {
                if !app.sel.contains(&h) && !inside_sel {
                    if mods.shift || mods.ctrl || mods.command {
                        app.sel.push(h);
                    } else {
                        app.sel = vec![h];
                    }
                }
            }
            if hit.is_some() || inside_sel {
                let orig: Vec<(usize, Xform)> = app.sel.iter().map(|i| (*i, app.doc.shapes[*i].xform)).collect();
                app.checkpoint();
                app.canvas.drag = Some(Drag::Move { start: pw, orig });
            } else {
                app.canvas.drag = Some(Drag::BoxSelect { start: pw });
            }
        }
        Tool::Rect | Tool::Ellipse | Tool::Polygon | Tool::Star => app.canvas.drag = Some(Drag::Create { start: psnap, cur: psnap }),
        Tool::Measure => app.canvas.drag = Some(Drag::Measure { start: psnap, cur: psnap }),
        Tool::Pen => {
            if app.canvas.pen.len() > 2 && app.canvas.pen[0].p.dist(psnap) * zoom < 10.0 {
                finish_pen(app, true);
                return;
            }
            app.canvas.pen.push(Vert::new(psnap));
            app.canvas.drag = Some(Drag::PenHandle { idx: app.canvas.pen.len() - 1 });
        }
        Tool::Lasso => {
            if let Some((i, _)) = app.image_at(pw) {
                app.canvas.lasso_pts = vec![pw];
                app.canvas.drag = Some(Drag::ImageLasso { shape_id: app.doc.shapes[i].id });
            }
        }
        Tool::Nodes => {
            let tol = 8.0 / zoom;
            let Some(&si) = app.sel.first() else {
                if let Some(h) = app.hit_test(pw, zoom) {
                    app.sel = vec![h];
                }
                return;
            };
            if !matches!(app.doc.shapes[si].kind, ShapeKind::Path(_)) {
                if app.doc.shapes[si].to_path().is_some() && app.doc.shapes[si].bbox().expand(tol).contains(pw) {
                    app.checkpoint();
                    crate::ops::convert_to_path(&mut app.doc, &[si]);
                } else {
                    app.canvas.drag = Some(Drag::NodeBox { start: pw });
                    return;
                }
            }
            let s = &app.doc.shapes[si];
            let Some(inv) = s.xform.inverse() else { return };
            if let ShapeKind::Path(pd) = &s.kind {
                let mut best: Option<(f64, usize, u8)> = None;
                for (vi, v) in pd.verts.iter().enumerate() {
                    let show_handles = app.canvas.nodes.contains(&vi);
                    for (part, p) in [(0u8, Some(v.p)), (1, v.c0.filter(|_| show_handles)), (2, v.c1.filter(|_| show_handles))] {
                        if let Some(p) = p {
                            let d = s.xform.apply(p).dist(pw);
                            if d < tol && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                                best = Some((d, vi, part));
                            }
                        }
                    }
                }
                match best {
                    Some((_, vi, 0)) => {
                        if !app.canvas.nodes.contains(&vi) {
                            if mods.shift || mods.ctrl || mods.command {
                                app.canvas.nodes.push(vi);
                            } else {
                                app.canvas.nodes = vec![vi];
                            }
                        }
                        app.checkpoint();
                        app.canvas.drag = Some(Drag::NodeMove { shape: si, last_local: inv.apply(psnap) });
                    }
                    Some((_, vi, part)) => {
                        app.checkpoint();
                        app.canvas.drag = Some(Drag::Handle { shape: si, vert: vi, part, last_local: inv.apply(psnap) });
                    }
                    None => {
                        if mods.alt {
                            app.canvas.lasso_pts = vec![pw];
                            app.canvas.drag = Some(Drag::NodeLasso);
                        } else {
                            app.canvas.drag = Some(Drag::NodeBox { start: pw });
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn update_drag(app: &mut App, w: P2, snapped: P2, mods: egui::Modifiers) {
    let drag = app.canvas.drag.clone();
    match drag {
        Some(Drag::Move { start, orig }) => {
            let mut d = w - start;
            if mods.shift {
                if d.x.abs() > d.y.abs() {
                    d.y = 0.0;
                } else {
                    d.x = 0.0;
                }
            }
            if app.canvas.guided {
                d = snapped - start;
            } else if app.settings.snap_grid {
                let g = (app.settings.grid_step / 10.0).max(0.1);
                d = p2((d.x / g).round() * g, (d.y / g).round() * g);
            }
            for (i, x) in &orig {
                if !app.doc.shapes[*i].locked {
                    app.doc.shapes[*i].xform = Xform::translate(d.x, d.y).then_after(*x);
                }
            }
            app.touch_xform();
        }
        Some(Drag::Scale { handle, anchor, start, orig }) => {
            let corner = handle % 2 == 0;
            let ds = start - anchor;
            let dc = w - anchor;
            let mut sx = if ds.x.abs() > 1e-9 { dc.x / ds.x } else { 1.0 };
            let mut sy = if ds.y.abs() > 1e-9 { dc.y / ds.y } else { 1.0 };
            match handle {
                1 | 5 => sx = 1.0,
                3 | 7 => sy = 1.0,
                _ => {}
            }
            if corner && (app.lock_aspect != mods.shift) {
                let k = sx.abs().max(sy.abs());
                sx = k * if sx < 0.0 { -1.0 } else { 1.0 };
                sy = k * if sy < 0.0 { -1.0 } else { 1.0 };
            }
            if sx.abs() < 1e-4 {
                sx = 1e-4;
            }
            if sy.abs() < 1e-4 {
                sy = 1e-4;
            }
            let t = Xform::about(anchor, Xform::scale(sx, sy));
            for (i, x) in &orig {
                if !app.doc.shapes[*i].locked {
                    app.doc.shapes[*i].xform = t.then_after(*x);
                }
            }
            app.touch_xform();
        }
        Some(Drag::Rotate { center, start_angle, orig }) => {
            let mut a = (w.y - center.y).atan2(w.x - center.x) - start_angle;
            if mods.shift {
                let step = 15f64.to_radians();
                a = (a / step).round() * step;
            }
            let t = Xform::about(center, Xform::rotate(a));
            for (i, x) in &orig {
                if !app.doc.shapes[*i].locked {
                    app.doc.shapes[*i].xform = t.then_after(*x);
                }
            }
            app.touch_xform();
        }
        Some(Drag::Create { start, .. }) => app.canvas.drag = Some(Drag::Create { start, cur: snapped }),
        Some(Drag::Measure { start, .. }) => app.canvas.drag = Some(Drag::Measure { start, cur: snapped }),
        Some(Drag::NodeLasso) | Some(Drag::ImageLasso { .. }) => {
            if app.canvas.lasso_pts.last().map(|l| l.dist(w) * app.view.zoom > 3.0).unwrap_or(true) {
                app.canvas.lasso_pts.push(w);
            }
        }
        Some(Drag::PenHandle { idx }) => {
            if let Some(v) = app.canvas.pen.get_mut(idx) {
                let d = w - v.p;
                v.c0 = Some(v.p + d);
                v.c1 = Some(v.p - d);
            }
        }
        Some(Drag::NodeMove { shape, last_local }) => {
            let s = &mut app.doc.shapes[shape];
            let Some(inv) = s.xform.inverse() else { return };
            let local = inv.apply(snapped);
            if let ShapeKind::Path(pd) = &mut s.kind {
                crate::nodes::translate(pd, &app.canvas.nodes, local - last_local);
            }
            app.canvas.drag = Some(Drag::NodeMove { shape, last_local: local });
            app.touch();
        }
        Some(Drag::Handle { shape, vert, part, last_local }) => {
            let s = &mut app.doc.shapes[shape];
            let Some(inv) = s.xform.inverse() else { return };
            let local = inv.apply(snapped);
            let d = local - last_local;
            if let ShapeKind::Path(pd) = &mut s.kind {
                if let Some(v) = pd.verts.get_mut(vert) {
                    // dragging a handle keeps a smooth node smooth unless Alt is held
                    let keep_smooth = v.c0.is_some() && v.c1.is_some() && !mods.alt;
                    if part == 1 {
                        v.c0 = v.c0.map(|c| c + d);
                        if let (true, Some(c0), Some(c1)) = (keep_smooth, v.c0, v.c1) {
                            v.c1 = Some(v.p - (c0 - v.p).normalized() * c1.dist(v.p));
                        }
                    } else {
                        v.c1 = v.c1.map(|c| c + d);
                        if let (true, Some(c0), Some(c1)) = (keep_smooth, v.c0, v.c1) {
                            v.c0 = Some(v.p - (c1 - v.p).normalized() * c0.dist(v.p));
                        }
                    }
                }
                for p in pd.prims.iter_mut() {
                    if p.a == vert || p.b == vert {
                        p.kind = PrimKind::Bezier;
                    }
                }
            }
            app.canvas.drag = Some(Drag::Handle { shape, vert, part, last_local: local });
            app.touch();
        }
        _ => {}
    }
}

fn mode_for(app: &App, mods: egui::Modifiers) -> SelMode {
    if mods.shift {
        SelMode::Add
    } else if mods.alt {
        SelMode::Subtract
    } else {
        app.opts.sel_mode
    }
}

fn apply_image_selection(app: &mut App, shape_id: u64, w: usize, h: usize, mask: Vec<bool>, mode: SelMode) {
    let old = app.image_sel.as_ref().filter(|s| s.shape_id == shape_id && s.w == w && s.h == h).map(|s| s.mask.clone());
    let mask = crate::wand::combine(old.as_deref(), mask, mode);
    let mut sel = crate::wand::Selection { shape_id, w, h, mask, outline: Vec::new() };
    if sel.count() == 0 {
        app.image_sel = None;
        return;
    }
    sel.update_outline(&app.opts.wand_refine);
    app.image_sel = Some(sel);
}

fn finish_drag(app: &mut App, w: P2, mods: egui::Modifiers, zoom: f64) {
    // what was moved stays put; the sketch relations bring everything tied to it along
    let moved: Vec<u64> = match &app.canvas.drag {
        Some(Drag::NodeMove { shape, .. }) | Some(Drag::Handle { shape, .. }) => app.doc.shapes.get(*shape).map(|s| vec![s.id]).unwrap_or_default(),
        Some(Drag::Move { orig, .. }) | Some(Drag::Scale { orig, .. }) | Some(Drag::Rotate { orig, .. }) => orig.iter().filter_map(|(i, _)| app.doc.shapes.get(*i).map(|s| s.id)).collect(),
        _ => Vec::new(),
    };
    let held: Option<Vec<crate::relations::Key>> = match &app.canvas.drag {
        Some(Drag::NodeMove { shape, .. }) => Some(crate::relations::keys_of(&app.doc, *shape, &app.canvas.nodes)),
        Some(Drag::Handle { shape, vert, .. }) => Some(crate::relations::keys_of(&app.doc, *shape, &[*vert])),
        Some(Drag::Move { orig, .. }) | Some(Drag::Scale { orig, .. }) | Some(Drag::Rotate { orig, .. }) => Some(orig.iter().flat_map(|(i, _)| crate::relations::shape_keys(&app.doc, *i)).collect()),
        _ => None,
    };
    finish_drag_inner(app, w, mods, zoom);
    if let Some(h) = held {
        if !app.doc.relations.is_empty() && !moved.is_empty() {
            crate::relations::solve_connected(&mut app.doc, &moved, &h);
            app.touch();
        }
    }
}

fn finish_drag_inner(app: &mut App, w: P2, mods: egui::Modifiers, zoom: f64) {
    let drag = app.canvas.drag.take();
    match drag {
        Some(Drag::BoxSelect { start }) => {
            let b = BBox::from_points(&[start, w]);
            let window = w.x >= start.x;
            let mut picked = Vec::new();
            for (i, s) in app.doc.shapes.iter().enumerate() {
                let sb = app.shape_box(i);
                let hit = if window { b.contains_box(&sb) } else { b.contains_box(&sb) || (sb.intersects(&b) && touches_box(&app.outline_cache.1[i], &b, s)) };
                if hit && !app.layer_hidden_shape(s) {
                    picked.push(i);
                }
            }
            if mods.shift || mods.ctrl || mods.command {
                for p in picked {
                    if !app.sel.contains(&p) {
                        app.sel.push(p);
                    }
                }
            } else {
                app.sel = picked;
            }
        }
        Some(Drag::NodeBox { start }) => {
            let b = BBox::from_points(&[start, w]);
            select_nodes(app, mods, |p| b.contains(p));
        }
        Some(Drag::NodeLasso) => {
            let poly = std::mem::take(&mut app.canvas.lasso_pts);
            if poly.len() > 2 {
                select_nodes(app, egui::Modifiers { alt: false, ..mods }, |p| inside(&poly, p));
            }
        }
        Some(Drag::ImageLasso { shape_id }) => {
            let pts = std::mem::take(&mut app.canvas.lasso_pts);
            if let Some(i) = app.index_of_id(shape_id) {
                let s = app.doc.shapes[i].clone();
                if let ShapeKind::Bitmap { w: bw, h: bh, data, adjust } = &s.kind {
                    let mut px: Vec<P2> = pts.iter().filter_map(|p| world_to_pixel(&s, *bw, *bh, data, *p)).collect();
                    if px.len() > 2 {
                        let (iw, ih) = (data.width as usize, data.height as usize);
                        if app.opts.lasso_magnetic {
                            let gray = crate::imaging::adjusted_gray(data, adjust);
                            // densify so edge snapping can follow curves between mouse samples
                            let mut dense = Vec::new();
                            for k in 0..px.len() {
                                let (a, b) = (px[k], px[(k + 1) % px.len()]);
                                let n = (a.dist(b) / 3.0).ceil().max(1.0) as usize;
                                for j in 0..n {
                                    dense.push(a.lerp(b, j as f64 / n as f64));
                                }
                            }
                            px = crate::wand::magnetic(&dense, &gray, iw, ih, 6);
                        }
                        let mask = crate::wand::polygon_mask(&px, iw, ih);
                        let mode = mode_for(app, mods);
                        apply_image_selection(app, shape_id, iw, ih, mask, mode);
                    }
                }
            }
        }
        Some(Drag::Create { start, cur }) => {
            if let Some(s) = creation_shape(app, start, cur, mods) {
                let b = s.bbox();
                if b.width() * zoom > 3.0 || b.height() * zoom > 3.0 {
                    app.add_shape(s);
                }
            }
        }
        Some(Drag::PenHandle { idx }) => {
            if let Some(v) = app.canvas.pen.get_mut(idx) {
                if v.c0.map(|c| c.dist(v.p) * zoom < 3.0).unwrap_or(true) {
                    v.c0 = None;
                    v.c1 = None;
                }
            }
        }
        Some(Drag::Measure { start, cur }) => app.canvas.last_measure = Some((start, cur)),
        Some(Drag::Move { start, .. }) => {
            if start.dist(w) * zoom < 1.0 {
                if let Some((d, s)) = app.undo.pop() {
                    app.doc = d;
                    app.sel = s;
                }
            }
        }
        _ => {}
    }
}

fn select_nodes(app: &mut App, mods: egui::Modifiers, test: impl Fn(P2) -> bool) {
    let Some(&si) = app.sel.first() else { return };
    let Some(s) = app.doc.shapes.get(si) else { return };
    let ShapeKind::Path(pd) = &s.kind else { return };
    let hits: Vec<usize> = pd.verts.iter().enumerate().filter(|(_, v)| test(s.xform.apply(v.p))).map(|(i, _)| i).collect();
    if mods.alt {
        app.canvas.nodes.retain(|n| !hits.contains(n));
    } else if mods.shift || mods.ctrl || mods.command {
        for h in hits {
            if !app.canvas.nodes.contains(&h) {
                app.canvas.nodes.push(h);
            }
        }
    } else {
        app.canvas.nodes = hits;
    }
}

fn touches_box(lines: &[Vec<P2>], b: &BBox, s: &Shape) -> bool {
    if s.is_bitmap() {
        return true;
    }
    let edges = [(b.min, p2(b.max.x, b.min.y)), (p2(b.max.x, b.min.y), b.max), (b.max, p2(b.min.x, b.max.y)), (p2(b.min.x, b.max.y), b.min)];
    for l in lines {
        let pts = &l[1..];
        if pts.iter().any(|p| b.contains(*p)) {
            return true;
        }
        for w in pts.windows(2) {
            if edges.iter().any(|(a, c)| segments_intersect(w[0], w[1], *a, *c)) {
                return true;
            }
        }
    }
    false
}

fn creation_shape(app: &App, start: P2, cur: P2, mods: egui::Modifiers) -> Option<Shape> {
    let mut d = cur - start;
    let regular = matches!(app.tool, Tool::Polygon | Tool::Star);
    let equal = if regular { !mods.shift } else { mods.shift };
    if equal {
        let m = d.x.abs().max(d.y.abs());
        d = p2(m * d.x.signum(), m * d.y.signum());
    }
    let from_center = mods.ctrl || mods.command;
    let (c, w, h) = if from_center { (start, d.x.abs() * 2.0, d.y.abs() * 2.0) } else { (start + d * 0.5, d.x.abs(), d.y.abs()) };
    if w < 1e-6 && h < 1e-6 {
        return None;
    }
    let layer = app.current_layer_for_vectors();
    let o = &app.opts;
    let kind = match app.tool {
        Tool::Rect => ShapeKind::Rect { w, h, cr: o.rect_radius.min(w.min(h) / 2.0) },
        Tool::Ellipse => ShapeKind::Ellipse { rx: w / 2.0, ry: h / 2.0 },
        Tool::Polygon if o.poly_radius > 0.0 => ShapeKind::Path(crate::sketch::round_corners(&[crate::sketch::polygon(o.poly_sides, w / 2.0, h / 2.0)], o.poly_radius)),
        Tool::Polygon => ShapeKind::Polygon { n: o.poly_sides, rx: w / 2.0, ry: h / 2.0 },
        Tool::Star => ShapeKind::Path(crate::sketch::star_shape_path(o.star_points, w / 2.0, h / 2.0, o.star_ratio, o.star_bulge, o.star_radius)),
        _ => return None,
    };
    let mut s = Shape::new(kind, Xform::translate(c.x, c.y), layer);
    if app.tool == Tool::Star {
        s.name = format!("Star ({} points)", o.star_points);
        // remember how it was made, so it can be dimensioned as a star (outer / inner radius) and has a centre
        crate::dims::set_star_params(&mut s, o.star_points, o.star_ratio, o.star_bulge, w / 2.0, h / 2.0, o.star_radius);
    }
    Some(s)
}

fn finish_line(app: &mut App, close: bool) {
    let mut pts = std::mem::take(&mut app.canvas.line_pts);
    let mut arcs = std::mem::take(&mut app.canvas.line_arcs);
    arcs.resize(pts.len().saturating_sub(1), None);
    // double-clicking leaves repeated end points behind
    while pts.len() >= 2 && pts[pts.len() - 1].dist(pts[pts.len() - 2]) < 1e-6 {
        pts.pop();
        arcs.pop();
    }
    let mode = std::mem::take(&mut app.canvas.line_arc_mode);
    if pts.len() < 2 {
        return;
    }
    if close {
        arcs.push(if mode { line_end_tangent(&pts, &arcs) } else { None });
    }
    let layer = app.current_layer_for_vectors();
    let snaps = std::mem::take(&mut app.canvas.line_snaps);
    // the drawing is done: its alignment references go too
    app.canvas.acquired.clear();
    app.canvas.ref_seg = None;
    app.add_shape(Shape::new(ShapeKind::Path(line_path(&pts, &arcs, close)), Xform::IDENTITY, layer));
    record_line_relations(app, &pts, &arcs, &snaps);
}

/// Turn the smart guides used while drawing a line into lasting relations: horizontal / vertical (to the
/// previous point or an aligned point), parallel / perpendicular, coincident with a node or centre, midpoint,
/// and on a line or circle.
fn record_line_relations(app: &mut App, pts: &[P2], arcs: &[Option<P2>], snaps: &[crate::snap::Inference]) {
    use crate::relations::{make, Ent, RelKind};
    use crate::snap::PointKind;
    let Some(s) = app.doc.shapes.last() else { return };
    let id = s.id;
    let ShapeKind::Path(pd) = &s.kind else { return };
    let vidx = |p: P2| pd.verts.iter().position(|v| v.p.dist(p) < 1e-9);
    let own_line = |a: P2, b: P2| -> Option<Ent> {
        let (ia, ib) = (vidx(a)?, vidx(b)?);
        pd.prims.iter().find(|p| p.kind == PrimKind::Line && ((p.a == ia && p.b == ib) || (p.a == ib && p.b == ia))).map(|p| Ent::Line { shape: id, a: p.a, b: p.b })
    };
    let mut rels = Vec::new();
    for (i, inf) in snaps.iter().enumerate().take(pts.len()) {
        let Some(vi) = vidx(pts[i]) else { continue };
        let me = Ent::Pt { shape: id, v: vi };
        let seg = if i > 0 && arcs.get(i - 1).copied().flatten().is_none() { own_line(pts[i - 1], pts[i]) } else { None };
        let ref_line = inf.guides.iter().find(|g| g.label.is_empty()).and_then(|g| own_line(g.a, g.b).or_else(|| crate::relations::line_at(&app.doc, g.a, Some(g.b), id)));
        for g in &inf.guides {
            let vertical = (g.a.x - g.b.x).abs() < 1e-9;
            match g.label {
                "Horizontal" | "Vertical" if i > 0 && g.a.dist(pts[i - 1]) < 1e-9 => {
                    let kind = if g.label == "Horizontal" { RelKind::Horizontal } else { RelKind::Vertical };
                    match (seg, vidx(pts[i - 1])) {
                        (Some(l), _) => rels.push(make(kind, &[l])),
                        (None, Some(pv)) => rels.push(make(kind, &[Ent::Pt { shape: id, v: pv }, me])),
                        _ => {}
                    }
                }
                "Horizontal" | "Vertical" | "Aligned" => {
                    let other = vidx(g.a).filter(|v| *v != vi).map(|v| Ent::Pt { shape: id, v }).or_else(|| crate::relations::ent_at(&app.doc, g.a, id));
                    if let Some(o) = other {
                        rels.push(make(if vertical { RelKind::Vertical } else { RelKind::Horizontal }, &[o, me]));
                    }
                }
                "Parallel" | "Perpendicular" => {
                    if let (Some(l), Some(r)) = (seg, ref_line) {
                        rels.push(make(if g.label == "Parallel" { RelKind::Parallel } else { RelKind::Perpendicular }, &[l, r]));
                    }
                }
                _ => {}
            }
        }
        if let Some((p, k)) = inf.mark {
            if p.dist(pts[i]) > 1e-9 {
                continue;
            }
            match k {
                PointKind::Endpoint | PointKind::Center => {
                    if let Some(o) = crate::relations::ent_at(&app.doc, p, id) {
                        rels.push(make(RelKind::Coincident, &[me, o]));
                    }
                }
                PointKind::Midpoint => {
                    if let Some(l) = crate::relations::line_at(&app.doc, p, None, id) {
                        rels.push(make(RelKind::Midpoint, &[me, l]));
                    }
                }
                PointKind::OnPath | PointKind::Intersection | PointKind::Quadrant | PointKind::Diagonal => {
                    for o in crate::relations::items_through(&app.doc, p, id).into_iter().take(2) {
                        rels.push(make(RelKind::Coincident, &[me, o]));
                    }
                }
                PointKind::Grid => {}
            }
        }
    }
    for r in rels {
        if !app.doc.relations.contains(&r) {
            app.doc.relations.push(r);
        }
    }
}

/// How long to rest on the last point of a line before the next segment switches between line and arc (s).
const LINE_ARC_DWELL: f64 = 0.45;

/// Direction leaving the end of the line being drawn (its last segment, straight or arc).
fn line_end_tangent(pts: &[P2], arcs: &[Option<P2>]) -> Option<P2> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    match arcs.get(n - 2).copied().flatten() {
        Some(t) => {
            let arc = crate::sketch::arc_tangent(pts[n - 2], t, pts[n - 1])?;
            let v = arc.verts.last()?;
            Some(v.p - v.c1.unwrap_or(pts[n - 2]))
        }
        None => Some(pts[n - 1] - pts[n - 2]),
    }
}

/// Path for the line tool's points; segment i is a tangent arc when `arcs[i]` holds its start direction.
fn line_path(pts: &[P2], arcs: &[Option<P2>], closed: bool) -> PathData {
    let n = pts.len();
    let mut pd = PathData { verts: vec![Vert::new(pts[0])], prims: Vec::new() };
    let segs = if closed && n > 2 { n } else { n - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let closing = i == n - 1;
        let start = pd.verts.len() - 1;
        let arc = arcs.get(i).copied().flatten().and_then(|t| crate::sketch::arc_tangent(a, t, b)).filter(|p| p.verts.len() >= 2);
        match arc {
            Some(arc) => {
                pd.verts[start].c0 = arc.verts[0].c0;
                let mut prev = start;
                let m = arc.verts.len();
                for (k, v) in arc.verts.iter().enumerate().skip(1) {
                    if k == m - 1 && closing {
                        pd.verts[0].c1 = v.c1;
                        pd.prims.push(Prim { kind: PrimKind::Bezier, a: prev, b: 0 });
                    } else {
                        pd.verts.push(*v);
                        let ni = pd.verts.len() - 1;
                        pd.prims.push(Prim { kind: PrimKind::Bezier, a: prev, b: ni });
                        prev = ni;
                    }
                }
            }
            None => {
                if closing {
                    pd.prims.push(Prim { kind: PrimKind::Line, a: start, b: 0 });
                } else {
                    pd.verts.push(Vert::new(b));
                    pd.prims.push(Prim { kind: PrimKind::Line, a: start, b: pd.verts.len() - 1 });
                }
            }
        }
    }
    pd
}

fn pen_path(verts: &[Vert], closed: bool) -> PathData {
    let n = verts.len();
    let mut prims = Vec::new();
    let mut link = |a: usize, b: usize| {
        let kind = if verts[a].c0.is_some() || verts[b].c1.is_some() { PrimKind::Bezier } else { PrimKind::Line };
        prims.push(Prim { kind, a, b });
    };
    for i in 0..n.saturating_sub(1) {
        link(i, i + 1);
    }
    if closed && n > 2 {
        link(n - 1, 0);
    }
    PathData { verts: verts.to_vec(), prims }
}

fn finish_pen(app: &mut App, close: bool) {
    let mut verts = std::mem::take(&mut app.canvas.pen);
    verts.dedup_by(|a, b| a.p.dist(b.p) < 1e-6);
    if verts.len() < 2 {
        return;
    }
    let pd = pen_path(&verts, close);
    let layer = app.current_layer_for_vectors();
    app.add_shape(Shape::new(ShapeKind::Path(pd), Xform::IDENTITY, layer));
}

/// Arc shape for the points clicked so far plus the cursor.
fn arc_preview(app: &App, cur: P2) -> Option<PathData> {
    let p = &app.canvas.arc_pts;
    match app.opts.arc_mode {
        ArcMode::ThreePoint => match p.len() {
            1 => Some(PathData::polyline(&[p[0], cur], false)),
            2 => crate::sketch::arc_3pt(p[0], cur, p[1]),
            _ => None,
        },
        ArcMode::Center => match p.len() {
            1 => Some(PathData::polyline(&[p[0], cur], false)),
            2 => Some(crate::sketch::arc_center(p[0], p[1], cur, app.opts.arc_ccw)),
            _ => None,
        },
        ArcMode::Tangent => match (app.canvas.arc_tangent, p.len()) {
            (Some(t), 1) => crate::sketch::arc_tangent(p[0], t, cur),
            (None, 1) => Some(PathData::polyline(&[p[0], cur], false)),
            (None, 2) => crate::sketch::arc_tangent(p[0], p[1] - p[0], cur),
            _ => None,
        },
    }
}

/// Direction leaving the end of a path whose end point is at `w` (for tangent arcs).
fn tangent_at_path_end(app: &App, w: P2, zoom: f64) -> Option<P2> {
    for s in app.doc.shapes.iter().rev() {
        let ShapeKind::Path(pd) = &s.kind else { continue };
        for (vi, v) in pd.verts.iter().enumerate() {
            if s.xform.apply(v.p).dist(w) * zoom > 6.0 {
                continue;
            }
            // an open end: exactly one segment touches it
            let incoming = pd.prims.iter().find(|p| p.b == vi);
            let outgoing = pd.prims.iter().find(|p| p.a == vi);
            match (incoming, outgoing) {
                (Some(p), None) => {
                    let from = pd.verts[p.a];
                    let c = v.c1.unwrap_or(from.c0.unwrap_or(from.p));
                    return Some(s.xform.apply_vec(v.p - c));
                }
                (None, Some(p)) => {
                    let to = pd.verts[p.b];
                    let c = v.c0.unwrap_or(to.c1.unwrap_or(to.p));
                    return Some(s.xform.apply_vec(v.p - c));
                }
                _ => {}
            }
        }
    }
    None
}

fn click(app: &mut App, w: P2, snapped: P2, mods: egui::Modifiers, zoom: f64, now: f64) {
    match app.tool {
        Tool::Relate => {
            let tag = app.canvas.pointer.and_then(|p| app.canvas.rel_hits.iter().find(|(r, _)| r.expand(2.0).contains(p)).map(|x| x.1));
            if let Some(ri) = tag {
                app.checkpoint();
                if ri < app.doc.relations.len() {
                    app.doc.relations.remove(ri);
                }
                app.touch();
                return;
            }
            match crate::relations::pick(&app.doc, w, 9.0 / zoom) {
                Some(e) => {
                    if let Some(i) = app.canvas.rel_picks.iter().position(|x| *x == e) {
                        app.canvas.rel_picks.remove(i);
                    } else {
                        if app.canvas.rel_picks.len() >= 2 {
                            app.canvas.rel_picks.clear();
                        }
                        app.canvas.rel_picks.push(e);
                    }
                }
                None => app.canvas.rel_picks.clear(),
            }
            return;
        }
        Tool::MirrorLine => {
            if app.sel.is_empty() {
                app.error("Select the shapes to mirror first, then pick the mirror line.", now);
                return;
            }
            match mirror_line_at(app, w, zoom) {
                Some((a, b)) => {
                    app.checkpoint();
                    let sel = app.sel.clone();
                    let made = crate::sketch::mirror_copy(&mut app.doc, &sel, a, b);
                    if !made.is_empty() {
                        app.sel = made;
                    }
                    app.touch();
                }
                None => app.error("Click on a straight edge or a construction line to mirror about it.", now),
            }
            return;
        }
        _ => {}
    }
    match app.tool {
        Tool::Select if app.canvas.array_chip.zip(app.canvas.pointer).map(|(c, p)| c.contains(p)).unwrap_or(false) => {
            app.edit_array();
        }
        Tool::Select => {
            let hit = app.hit_test(w, zoom);
            let add = mods.shift || mods.ctrl || mods.command;
            match hit {
                Some(h) if add => {
                    if let Some(p) = app.sel.iter().position(|x| *x == h) {
                        app.sel.remove(p);
                    } else {
                        app.sel.push(h);
                    }
                }
                Some(h) => app.sel = vec![h],
                None if !add => app.sel.clear(),
                None => {}
            }
            if let Some(h) = hit {
                app.current_layer = app.doc.shapes[h].layer;
            }
        }
        Tool::Line => {
            if let Some(first) = app.canvas.line_pts.first() {
                if first.dist(snapped) * zoom < 10.0 && app.canvas.line_pts.len() > 2 {
                    finish_line(app, true);
                    return;
                }
            }
            if !app.canvas.line_pts.is_empty() {
                let t = if app.canvas.line_arc_mode { line_end_tangent(&app.canvas.line_pts, &app.canvas.line_arcs) } else { None };
                app.canvas.line_arcs.push(t);
                // like CAD: after one tangent arc, the tool goes back to straight lines
                app.canvas.line_arc_mode = false;
            }
            app.canvas.line_pts.push(snapped);
            let inf = if mods.shift { crate::snap::Inference { p: snapped, ..Default::default() } } else { app.canvas.inference.clone() };
            app.canvas.line_snaps.push(inf);
            app.canvas.arc_armed = false;
            app.canvas.arc_hover = None;
        }
        Tool::Pen => {
            if app.canvas.pen.len() > 2 && app.canvas.pen[0].p.dist(snapped) * zoom < 10.0 {
                finish_pen(app, true);
                return;
            }
            app.canvas.pen.push(Vert::new(snapped));
        }
        Tool::Arc => {
            if app.canvas.arc_pts.is_empty() && app.opts.arc_mode == ArcMode::Tangent {
                app.canvas.arc_tangent = tangent_at_path_end(app, snapped, zoom);
            }
            app.canvas.arc_pts.push(snapped);
            let needed = match (app.opts.arc_mode, app.canvas.arc_tangent) {
                (ArcMode::Tangent, Some(_)) => 2,
                _ => 3,
            };
            if app.canvas.arc_pts.len() >= needed {
                let last = app.canvas.arc_pts.pop().unwrap();
                let pd = arc_preview(app, last);
                app.canvas.arc_pts.clear();
                app.canvas.arc_tangent = None;
                if let Some(pd) = pd {
                    let layer = app.current_layer_for_vectors();
                    app.add_shape(Shape::new(ShapeKind::Path(pd), Xform::IDENTITY, layer));
                }
            }
        }
        Tool::Text => {
            let layer = app.current_layer_for_vectors();
            app.add_shape(Shape::new(ShapeKind::Text { text: "Text".into(), font: crate::fonts::DEFAULT_FONT.into(), height: 10.0, style: Default::default(), backup: None }, Xform::translate(snapped.x, snapped.y), layer));
            app.side_tab = SideTab::Properties;
            app.show_right = true;
            app.canvas.focus_text = true;
            app.tool = Tool::Select;
        }
        Tool::Nodes => {
            let near_node = app.sel.first().and_then(|si| app.doc.shapes.get(*si)).and_then(|s| match &s.kind {
                ShapeKind::Path(pd) => pd.verts.iter().position(|v| s.xform.apply(v.p).dist(w) * zoom < 8.0),
                _ => None,
            });
            match near_node {
                Some(v) => {
                    if mods.shift || mods.ctrl || mods.command {
                        if let Some(p) = app.canvas.nodes.iter().position(|n| *n == v) {
                            app.canvas.nodes.remove(p);
                        } else {
                            app.canvas.nodes.push(v);
                        }
                    } else {
                        app.canvas.nodes = vec![v];
                    }
                }
                None => {
                    app.canvas.nodes.clear();
                    match app.hit_test(w, zoom) {
                        Some(h) => app.sel = vec![h],
                        None => app.sel.clear(),
                    }
                }
            }
        }
        Tool::Trim => {
            if let Some((si, li, kept, _)) = trim_candidate(app, w, zoom) {
                let s = app.doc.shapes[si].clone();
                let mut lines = s.world_outline(0.02);
                lines.remove(li);
                lines.extend(kept);
                app.checkpoint();
                if lines.iter().all(|l| l.len() < 2) {
                    crate::ops::delete(&mut app.doc, &[si]);
                    app.sel.clear();
                } else {
                    let mut ns = Shape::new(ShapeKind::Path(PathData::from_polylines(&lines)), Xform::IDENTITY, s.layer);
                    ns.name = s.name.clone();
                    app.doc.shapes[si] = ns;
                    app.sel = vec![si];
                }
            }
        }
        Tool::Fillet => {
            let target = app.hit_test(w, zoom).or(app.sel.first().copied());
            if let Some(si) = target {
                if let Some((vi, _)) = corner_near(app, si, w, 14.0 / zoom) {
                    let s = app.doc.shapes[si].clone();
                    if let Some(mut pd) = editable_path(&s) {
                        // sizes are in mm on the bed; convert to the shape's own units
                        let k = 1.0 / s.xform.mean_scale();
                        let mut f = app.opts.fillet;
                        f.d1 *= k;
                        f.d2 *= k;
                        if crate::sketch::fillet_vertex(&mut pd, vi, &f) {
                            app.checkpoint();
                            app.doc.shapes[si].kind = ShapeKind::Path(pd);
                            app.sel = vec![si];
                        } else {
                            app.error("That corner is too flat or too small for this size.", now);
                        }
                    }
                }
            }
        }
        Tool::Dimension => {
            let hit = app.canvas.pointer.and_then(|p| app.canvas.dim_hits.iter().find(|(r, _)| r.expand(3.0).contains(p)).cloned());
            match hit {
                Some((r, d)) => {
                    let f = app.settings.units.factor();
                    let f = if d.is_angle() { 1.0 } else { f };
                    app.canvas.dim_edit = Some((d.clone(), fmt_num((d.value * f * 1000.0).round() / 1000.0), r.left_bottom()));
                }
                None => {
                    app.canvas.dim_edit = None;
                    match app.hit_test(w, zoom) {
                        Some(h) if mods.shift || mods.ctrl || mods.command => {
                            if !app.sel.contains(&h) {
                                app.sel.push(h);
                            }
                        }
                        Some(h) => app.sel = vec![h],
                        None => app.sel.clear(),
                    }
                }
            }
        }
        Tool::Wand => {
            if let Some((i, px)) = app.image_at(w) {
                let s = app.doc.shapes[i].clone();
                if let ShapeKind::Bitmap { data, .. } = &s.kind {
                    let (iw, ih) = (data.width as usize, data.height as usize);
                    let mask = crate::wand::magic_wand(&data.rgba, iw, ih, px.x as usize, px.y as usize, app.opts.wand_tol, app.opts.wand_contiguous);
                    let mode = mode_for(app, mods);
                    apply_image_selection(app, s.id, iw, ih, mask, mode);
                }
            } else if !mods.shift && !mods.alt {
                app.image_sel = None;
            }
        }
        _ => {}
    }
}

/// The straight line under the cursor for mirroring: a path's straight segment (including construction
/// lines), or a straight edge of any shape's outline at least 12 px long.
fn mirror_line_at(app: &App, w: P2, zoom: f64) -> Option<(P2, P2)> {
    if let Some(crate::relations::Ent::Line { shape, a, b }) = crate::relations::pick(&app.doc, w, 8.0 / zoom) {
        return Some((crate::relations::node_pos(&app.doc, shape, a)?, crate::relations::node_pos(&app.doc, shape, b)?));
    }
    let tol = 8.0 / zoom;
    let mut best: Option<(f64, P2, P2)> = None;
    for lines in &app.outline_cache.1 {
        for l in lines {
            for s in l[1..].windows(2) {
                if s[0].dist(s[1]) * zoom < 12.0 {
                    continue;
                }
                let d = dist_to_segment(w, s[0], s[1]);
                if d < tol && best.map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, s[0], s[1]));
                }
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

/// Merge overlapping or nearby boxes (within `gap`), sweeping along x.
fn merge_boxes(mut boxes: Vec<BBox>, gap: f64) -> Vec<BBox> {
    loop {
        boxes.sort_by(|a, b| a.min.x.partial_cmp(&b.min.x).unwrap());
        let mut out: Vec<BBox> = Vec::new();
        let mut changed = false;
        for b in boxes {
            match out.iter_mut().rev().take(64).find(|o| o.expand(gap).intersects(&b)) {
                Some(o) => {
                    *o = o.union(&b);
                    changed = true;
                }
                None => out.push(b),
            }
        }
        boxes = out;
        if !changed {
            return boxes;
        }
    }
}

fn draw_off_bed(app: &mut App, painter: &egui::Painter, tf: &Tf, bed_box: BBox, canvas: Rect) {
    let fresh = !matches!(&app.canvas.off_bed, Some((rev, bed, _, _)) if *rev == app.rev && *bed == bed_box);
    if fresh {
        let burns = |layer: usize| app.doc.cut(layer).map(|c| c.output && !c.hide).unwrap_or(layer < crate::model::COLOR_LAYERS);
        let mut boxes = Vec::new();
        for (i, s) in app.doc.shapes.iter().enumerate() {
            let _ = s;
            let sb = app.shape_box(i);
            let burning = app.outline_cache.1.get(i).map(|ls| ls.iter().any(|l| burns(l[0].x as usize))).unwrap_or(false);
            if burning && !sb.is_empty() && !bed_box.contains_box(&sb) {
                boxes.push(sb);
            }
        }
        let count = boxes.len();
        app.canvas.off_bed = Some((app.rev, bed_box, merge_boxes(boxes, 3.0), count));
    }
    let Some((_, _, clusters, count)) = &app.canvas.off_bed else { return };
    if *count == 0 {
        return;
    }
    let red = Color32::from_rgb(235, 80, 70);
    let mut labelled = 0;
    for b in clusters {
        let r = Rect::from_two_pos(tf.s(b.min), tf.s(b.max)).expand(4.0);
        if !r.intersects(canvas) {
            continue;
        }
        painter.rect_filled(r, 3.0, red.gamma_multiply(0.08));
        painter.add(EShape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], Stroke::new(1.0, red.gamma_multiply(0.8)), 5.0, 4.0));
        // one small tag on the larger areas only
        if r.width() > 70.0 && labelled < 8 {
            labelled += 1;
            let g = painter.layout_no_wrap("won't burn".into(), FontId::proportional(10.0), Color32::WHITE);
            let tag = Rect::from_min_size(r.left_top() + vec2(0.0, -g.size().y - 4.0), g.size() + vec2(8.0, 3.0));
            painter.rect_filled(tag, 3.0, red.gamma_multiply(0.85));
            painter.galley(tag.min + vec2(4.0, 1.5), g, Color32::WHITE);
        }
    }
    let msg = format!("{count} shape{} outside the bed won't burn", if *count == 1 { "" } else { "s" });
    let g = painter.layout_no_wrap(msg, FontId::proportional(12.0), Color32::WHITE);
    let banner = Rect::from_min_size(canvas.left_top() + vec2(12.0, 10.0), g.size() + vec2(16.0, 8.0));
    painter.rect_filled(banner, 5.0, red.gamma_multiply(0.9));
    painter.galley(banner.min + vec2(8.0, 4.0), g, Color32::WHITE);
}

/// Relation tags next to the items they tie, the Relations tool's picks, and what is under the cursor.
fn draw_relations(app: &mut App, painter: &egui::Painter, tf: &Tf, zoom: f64) {
    use crate::relations::Ent;
    let green = Color32::from_rgb(70, 200, 120);
    let pick_c = Color32::from_rgb(255, 196, 60);
    let draw_ent = |e: &Ent, c: Color32, width: f32| match *e {
        Ent::Pt { shape, v } => {
            if let Some(p) = crate::relations::node_pos(&app.doc, shape, v) {
                painter.circle_stroke(tf.s(p), 5.5, Stroke::new(width, c));
            }
        }
        Ent::Line { shape, a, b } => {
            if let (Some(p), Some(q)) = (crate::relations::node_pos(&app.doc, shape, a), crate::relations::node_pos(&app.doc, shape, b)) {
                painter.line_segment([tf.s(p), tf.s(q)], Stroke::new(width + 2.0, c.gamma_multiply(0.7)));
            }
        }
        Ent::Center { shape } | Ent::Circle { shape } => {
            if let Some(s) = app.doc.shapes.iter().find(|s| s.id == shape) {
                if let Some((ctr, r)) = crate::ops::circle_of(s) {
                    if matches!(e, Ent::Center { .. }) {
                        painter.circle_stroke(tf.s(ctr), 5.5, Stroke::new(width, c));
                    } else {
                        painter.circle_stroke(tf.s(ctr), (r * zoom) as f32, Stroke::new(width + 1.0, c.gamma_multiply(0.7)));
                    }
                }
            }
        }
    };
    if app.tool == Tool::MirrorLine {
        if let Some((a, b)) = app.canvas.pointer.and_then(|p| mirror_line_at(app, tf.w(p), zoom)) {
            painter.line_segment([tf.s(a), tf.s(b)], Stroke::new(3.0, ACCENT));
        }
        app.canvas.rel_hits.clear();
        return;
    }
    if app.tool == Tool::Relate {
        if let Some(e) = app.canvas.pointer.and_then(|p| crate::relations::pick(&app.doc, tf.w(p), 9.0 / zoom)) {
            draw_ent(&e, green.gamma_multiply(0.8), 1.4);
        }
        for e in &app.canvas.rel_picks {
            draw_ent(e, pick_c, 2.0);
        }
    }
    let mut hits = Vec::new();
    let mut used: Vec<Pos2> = Vec::new();
    for (ri, r) in app.doc.relations.iter().enumerate() {
        if r.kind.is_dimension() {
            continue;
        }
        let Some(anchor) = r.ents.first().and_then(|e| crate::relations::anchor(&app.doc, e)) else { continue };
        let mut at = tf.s(anchor) + vec2(10.0, -12.0);
        while used.iter().any(|u| u.distance(at) < 20.0) {
            at += vec2(22.0, 0.0);
        }
        used.push(at);
        let rect = Rect::from_center_size(at, vec2(8.0 + 6.5 * r.kind.glyph().chars().count() as f32, 14.0));
        let hovered = app.canvas.pointer.map(|p| rect.expand(2.0).contains(p)).unwrap_or(false);
        if hovered {
            for e in &r.ents {
                draw_ent(e, green, 1.6);
            }
        }
        let fill = if hovered && app.tool == Tool::Relate { Color32::from_rgb(200, 70, 70) } else { green.gamma_multiply(0.85) };
        painter.rect_filled(rect, 3.0, fill);
        painter.text(rect.center(), Align2::CENTER_CENTER, r.kind.glyph(), FontId::proportional(11.0), Color32::BLACK);
        if hovered {
            let tip = if app.tool == Tool::Relate { format!("{} · click to remove", r.kind.label()) } else { r.kind.label().to_string() };
            painter.text(rect.right_top() + vec2(4.0, 0.0), Align2::LEFT_BOTTOM, tip, FontId::proportional(11.0), green);
        }
        hits.push((rect, ri));
    }
    app.canvas.rel_hits = hits;
}

fn double_click(app: &mut App, w: P2, zoom: f64) {
    match app.tool {
        Tool::Line => finish_line(app, false),
        Tool::Pen => finish_pen(app, false),
        Tool::Select if app.hit_test(w, zoom).map(|h| crate::relations::array_of(&app.doc, app.doc.shapes[h].id).is_some()).unwrap_or(false) => {
            let h = app.hit_test(w, zoom).unwrap();
            app.sel = vec![h];
            app.edit_array();
        }
        Tool::Select => {
            if let Some(h) = app.hit_test(w, zoom) {
                match app.doc.shapes[h].kind {
                    ShapeKind::Text { .. } => {
                        app.side_tab = SideTab::Properties;
                        app.canvas.focus_text = true;
                    }
                    ShapeKind::Path(_) => {
                        app.sel = vec![h];
                        app.tool = Tool::Nodes;
                        app.canvas.nodes.clear();
                    }
                    _ => app.side_tab = SideTab::Properties,
                }
            }
        }
        Tool::Nodes => {
            if !app.canvas.nodes.is_empty() {
                app.delete_node();
            }
        }
        _ => {}
    }
}

fn welcome(ui: &mut Ui, app: &App, rect: Rect, run: &mut Vec<Action>) {
    let dark = ui.visuals().dark_mode;
    let col = theme::colors(dark);
    let card = Rect::from_center_size(rect.center(), vec2(460.0_f32.min(rect.width() - 20.0), 330.0_f32.min(rect.height() - 20.0)));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card));
    egui::Frame::NONE.fill(col.panel.gamma_multiply(0.97)).corner_radius(14.0).stroke(Stroke::new(1.0, col.border)).inner_margin(22.0).show(&mut child, |ui| {
        ui.set_width(card.width() - 44.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Welcome to BurnCraft").size(22.0).strong());
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Design, trace and burn — open LightBurn projects too.").color(col.weak));
            ui.add_space(16.0);
        });
        let items = [(Action::Open, "Open a project (.lbrn2 / .lbrn)"), (Action::Import, "Import an image to trace or engrave"), (Action::ToolRect, "Start drawing"), (Action::Shortcuts, "Set up keyboard shortcuts")];
        for (a, text) in items {
            // icon and label side by side with room between, so they never overlap
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            let fill = if resp.hovered() { col.hover } else { ui.visuals().widgets.inactive.bg_fill };
            ui.painter().rect_filled(r, 7.0, fill);
            let ir = Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(20.0, 20.0));
            crate::icons::paint(ui.painter(), ir, a.icon(), &crate::ribbon::pal(ui, false, true));
            ui.painter().text(pos2(r.left() + 50.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), col.text);
            if resp.on_hover_text(app.tooltip(a)).clicked() {
                run.push(a);
                if a == Action::ToolRect {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("hide_welcome"), true));
                }
            }
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            if ui.link("…or try a sample project").clicked() {
                run.push(Action::New);
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("load_demo"), true));
            }
            ui.label(egui::RichText::new(format!("Press {} to search all commands", app.settings.keymap.label(Action::CommandSearch))).size(11.0).color(col.weak));
        });
    });
}

/// Smart-guide feedback: remembered points, the reference line, dashed guides with labels, and the snap marker.
fn draw_guides(app: &App, painter: &egui::Painter, tf: &Tf, active: bool) {
    use crate::snap::PointKind;
    let green = Color32::from_rgb(60, 200, 110);
    let sketching = !matches!(app.tool, Tool::Select | Tool::Pan | Tool::Wand | Tool::Lasso | Tool::Trim | Tool::Nodes | Tool::Relate | Tool::Dimension | Tool::MirrorLine) || matches!(app.canvas.drag, Some(Drag::Move { .. }) | Some(Drag::NodeMove { .. }));
    if !sketching {
        return;
    }
    for a in &app.canvas.acquired {
        let p = tf.s(*a);
        painter.line_segment([p - vec2(5.0, 0.0), p + vec2(5.0, 0.0)], Stroke::new(1.2, green));
        painter.line_segment([p - vec2(0.0, 5.0), p + vec2(0.0, 5.0)], Stroke::new(1.2, green));
    }
    if let Some((a, b)) = app.canvas.ref_seg {
        painter.line_segment([tf.s(a), tf.s(b)], Stroke::new(4.0, green.gamma_multiply(0.35)));
    }
    if !active {
        return;
    }
    let inf = &app.canvas.inference;
    let mut labels: Vec<&str> = Vec::new();
    for g in &inf.guides {
        if g.label.is_empty() {
            continue;
        }
        painter.add(EShape::dashed_line(&[tf.s(g.a), tf.s(g.b)], Stroke::new(1.0, green), 6.0, 4.0));
        labels.push(g.label);
    }
    if let Some((p, k)) = inf.mark {
        let c = tf.s(p);
        let st = Stroke::new(1.6, green);
        match k {
            PointKind::Endpoint => {
                painter.rect_stroke(Rect::from_center_size(c, vec2(11.0, 11.0)), 0.0, st, StrokeKind::Middle);
            }
            PointKind::Midpoint => {
                painter.add(EShape::closed_line(vec![c + vec2(0.0, -7.0), c + vec2(6.5, 5.0), c + vec2(-6.5, 5.0)], st));
            }
            PointKind::Center => {
                painter.circle_stroke(c, 6.5, st);
                painter.circle_filled(c, 1.8, green);
            }
            PointKind::Quadrant => {
                painter.add(EShape::closed_line(vec![c + vec2(0.0, -7.0), c + vec2(7.0, 0.0), c + vec2(0.0, 7.0), c + vec2(-7.0, 0.0)], st));
            }
            PointKind::Diagonal => {
                painter.add(EShape::closed_line(vec![c + vec2(0.0, -7.0), c + vec2(7.0, 0.0), c + vec2(0.0, 7.0), c + vec2(-7.0, 0.0)], st));
                painter.line_segment([c - vec2(3.5, -3.5), c + vec2(3.5, -3.5)], st);
            }
            PointKind::Intersection => {
                painter.line_segment([c - vec2(6.0, 6.0), c + vec2(6.0, 6.0)], st);
                painter.line_segment([c - vec2(6.0, -6.0), c + vec2(6.0, -6.0)], st);
                painter.circle_stroke(c, 3.0, st);
            }
            PointKind::OnPath => {
                painter.line_segment([c - vec2(4.0, 4.0), c + vec2(4.0, 4.0)], st);
                painter.line_segment([c - vec2(4.0, -4.0), c + vec2(4.0, -4.0)], st);
            }
            PointKind::Grid => {
                painter.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], st);
                painter.line_segment([c - vec2(0.0, 5.0), c + vec2(0.0, 5.0)], st);
            }
        }
        let tool_label = if k == PointKind::Center && app.tool == Tool::Ellipse { "Concentric" } else { k.label() };
        labels.insert(0, tool_label);
    }
    if !labels.is_empty() {
        labels.dedup();
        painter.text(tf.s(inf.p) + vec2(14.0, 10.0), Align2::LEFT_TOP, labels.join(" · "), FontId::proportional(11.0), green);
    }
}

fn add_path_feats(sc: &mut crate::snap::Scene, pd: &PathData, x: Xform) {
    use crate::snap::PointKind::*;
    for v in pd.verts.iter().take(5000) {
        sc.points.push((x.apply(v.p), Endpoint));
    }
    for pr in pd.prims.iter().take(5000) {
        let (Some(va), Some(vb)) = (pd.verts.get(pr.a), pd.verts.get(pr.b)) else { continue };
        match pr.kind {
            PrimKind::Line => {
                let (a, b) = (x.apply(va.p), x.apply(vb.p));
                sc.points.push((a.lerp(b, 0.5), Midpoint));
                sc.segments.push((a, b));
            }
            PrimKind::Bezier => {
                let m = cubic_point(va.p, va.c0.unwrap_or(va.p), vb.c1.unwrap_or(vb.p), vb.p, 0.5);
                sc.points.push((x.apply(m), Midpoint));
            }
        }
    }
}

/// Dimensions of the selected shapes (or every shape in view) while the dimension tool is active.
fn draw_dims(app: &mut App, painter: &egui::Painter, tf: &Tf, view_box: BBox) {
    let blue = Color32::from_rgb(90, 170, 255);
    let u = app.settings.units;
    let shapes: Vec<usize> = if app.sel.is_empty() {
        (0..app.doc.shapes.len()).filter(|i| app.shape_box(*i).intersects(&view_box)).take(80).collect()
    } else {
        app.sel.clone()
    };
    let mut hits = Vec::new();
    let editing = app.canvas.dim_edit.as_ref().map(|(d, _, _)| d.target);
    for si in shapes {
        let center = app.doc.shapes[si].bbox().center();
        let mut all = crate::dims::dims_for(&app.doc, si);
        if app.sel.contains(&si) {
            all.extend(crate::dims::angle_dims(&app.doc, si));
        }
        for d in all {
            // driving dimensions hold their value; driven ones (in brackets) only report it until you set them
            let num = if d.is_angle() { format!("{}°", fmt_num((d.value * 10.0).round() / 10.0)) } else { fmt_num(u.round(d.value)) };
            let text = if d.driving { format!("{}{}", d.prefix(), num) } else { format!("({}{})", d.prefix(), num) };
            let (label_at, lines): (Pos2, Vec<[Pos2; 2]>) = match d.kind {
                crate::dims::DimKind::Angle { center } => {
                    let c = tf.s(center);
                    let (a1, a2) = ((tf.s(d.a) - c).angle(), (tf.s(d.b) - c).angle());
                    let mut sweep = a2 - a1;
                    while sweep > std::f32::consts::PI {
                        sweep -= std::f32::consts::TAU;
                    }
                    while sweep < -std::f32::consts::PI {
                        sweep += std::f32::consts::TAU;
                    }
                    let r = 28.0;
                    let pts: Vec<Pos2> = (0..=16).map(|i| c + egui::Vec2::angled(a1 + sweep * i as f32 / 16.0) * r).collect();
                    let ls: Vec<[Pos2; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
                    (c + egui::Vec2::angled(a1 + sweep / 2.0) * (r + 18.0), ls)
                }
                crate::dims::DimKind::Linear => {
                    let (a, b) = (tf.s(d.a), tf.s(d.b));
                    let dir = (b - a).normalized();
                    let mut n = vec2(-dir.y, dir.x);
                    let mid = a + (b - a) * 0.5;
                    if (mid - tf.s(center)).dot(n) < 0.0 {
                        n = -n;
                    }
                    let (a2, b2) = (a + n * 14.0, b + n * 14.0);
                    let arrow = |p: Pos2, back: egui::Vec2| [[p, p + back * 6.0 + n * 3.0], [p, p + back * 6.0 - n * 3.0]];
                    let mut ls = vec![[a + n * 3.0, a2 + n * 4.0], [b + n * 3.0, b2 + n * 4.0], [a2, b2]];
                    ls.extend(arrow(a2, dir));
                    ls.extend(arrow(b2, -dir));
                    (a2 + (b2 - a2) * 0.5 + n * 10.0, ls)
                }
                crate::dims::DimKind::Radius { center } => {
                    let (c, b) = (tf.s(center), tf.s(d.b));
                    let dir = (b - c).normalized();
                    let n = vec2(-dir.y, dir.x);
                    (b + dir * 16.0, vec![[c, b], [b, b - dir * 7.0 + n * 3.0], [b, b - dir * 7.0 - n * 3.0], [c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], [c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)]])
                }
                crate::dims::DimKind::Diameter | crate::dims::DimKind::Offset => {
                    let (a, b) = (tf.s(d.a), tf.s(d.b));
                    let dir = (b - a).normalized();
                    let n = vec2(-dir.y, dir.x);
                    (a + (b - a) * 0.5 + n * 12.0, vec![[a, b], [a, a + dir * 7.0 + n * 3.0], [a, a + dir * 7.0 - n * 3.0], [b, b - dir * 7.0 + n * 3.0], [b, b - dir * 7.0 - n * 3.0]])
                }
            };
            for l in lines {
                painter.line_segment(l, Stroke::new(1.0, blue));
            }
            let galley = painter.layout_no_wrap(text, FontId::proportional(11.5), Color32::WHITE);
            let r = Rect::from_center_size(label_at, galley.size() + vec2(8.0, 4.0));
            let hovered = app.canvas.pointer.map(|p| r.expand(3.0).contains(p)).unwrap_or(false);
            if editing == Some(d.target) || hovered {
                painter.rect_filled(r, 4.0, ACCENT);
            } else if d.driving {
                painter.rect_filled(r, 4.0, blue.gamma_multiply(0.85));
            } else {
                painter.rect_filled(r, 4.0, Color32::from_gray(60).gamma_multiply(0.9));
                painter.rect_stroke(r, 4.0, Stroke::new(1.0, blue.gamma_multiply(0.7)), StrokeKind::Inside);
            }
            painter.galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
            hits.push((r, d));
        }
    }
    // the area of a single closed shape, in LightBurn's measure units (mm² or in²)
    if let [si] = app.sel.as_slice() {
        if let Some(s) = app.doc.shapes.get(*si) {
            let rings: Vec<Vec<P2>> = s.world_outline(0.05).into_iter().filter(|l| is_closed(l)).collect();
            if !rings.is_empty() {
                let area: f64 = rings.iter().map(|l| signed_area(l)).sum::<f64>().abs();
                let b = s.bbox();
                let text = format!("Area {} {}²", fmt_num((area * u.area_factor() * 100.0).round() / 100.0), u.suffix());
                painter.text(tf.s(p2(b.center().x, b.min.y)) + vec2(0.0, 40.0), Align2::CENTER_TOP, text, FontId::proportional(11.5), blue);
            }
        }
    }
    app.canvas.dim_hits = hits;
}

/// The little box for typing a new dimension value.
pub fn dim_editor(ctx: &egui::Context, app: &mut App) {
    let Some((dim, mut text, pos)) = app.canvas.dim_edit.clone() else { return };
    let mut apply = false;
    let mut close = false;
    let u = app.settings.units;
    egui::Area::new(egui::Id::unique("dim_edit")).fixed_pos(pos + vec2(0.0, 4.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(dim.prefix());
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
                r.request_focus();
                ui.label(if dim.is_angle() { "°" } else { u.suffix() });
                if dim.driving && crate::dims::driving_relation(&app.doc, dim.target, 0.0).is_some() && ui.button("Make driven").on_hover_text("Stop holding this value; it will just report the size").clicked() {
                    if let Some(rel) = crate::dims::driving_relation(&app.doc, dim.target, 0.0) {
                        app.checkpoint();
                        app.doc.relations.retain(|x| !(x.kind == rel.kind && x.ents == rel.ents));
                        app.touch();
                    }
                    close = true;
                }
                // the field keeps focus every frame, so watch Enter directly (also the keypad Enter)
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) || r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    });
    if apply {
        match text.trim().replace(',', ".").parse::<f64>() {
            Ok(v) => {
                let before = app.doc.clone();
                let held = crate::dims::held_keys(&app.doc, dim.target);
                let value = if dim.is_angle() { v } else { v / u.factor() };
                let rel = crate::dims::driving_relation(&app.doc, dim.target, value);
                if crate::dims::apply(&mut app.doc, dim.target, value) {
                    // a value you type becomes a driving dimension (held from now on), like in a CAD sketch
                    if let Some(rel) = rel {
                        app.doc.relations.retain(|x| !(x.kind == rel.kind && x.ents == rel.ents));
                        app.doc.relations.push(rel);
                    }
                    // keep the sketch relations (horizontal, parallel, tangent, other driving dimensions...)
                    let ids: Vec<u64> = crate::dims::target_shape(dim.target).and_then(|i| app.doc.shapes.get(i)).map(|s| vec![s.id]).unwrap_or_default();
                    let err = crate::relations::solve_connected(&mut app.doc, &ids, &held);
                    if err > 1e-3 {
                        let now = ctx.input(|i| i.time);
                        app.error("The sketch is over-constrained: that value conflicts with other relations or driving dimensions. Make one of them driven or remove a relation.", now);
                    }
                    app.undo.push((before, app.sel.clone()));
                    app.redo.clear();
                    app.touch();
                } else {
                    let now = ctx.input(|i| i.time);
                    app.error("That value can't be applied to this shape (too small or too large).", now);
                }
            }
            Err(_) => {}
        }
        close = true;
    }
    app.canvas.dim_edit = if close { None } else { Some((dim, text, pos)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moved_shapes_reuse_outlines_and_match_a_rebuild() {
        let mut app = App::new(crate::settings::Settings::default());
        app.doc.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 5.0, ry: 3.0 }, Xform::translate(10.0, 10.0), 0));
        app.doc.shapes.push(Shape::new(ShapeKind::Path(PathData::polyline(&[p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 5.0)], true)), Xform::IDENTITY, 0));
        app.touch();
        app.ensure_outlines();
        app.doc.shapes[0].xform = Xform::about(p2(10.0, 10.0), Xform::rotate(0.7)).then_after(Xform::translate(30.0, -4.0).then_after(app.doc.shapes[0].xform));
        app.touch_xform();
        app.ensure_outlines();
        let fast = app.bbox_cache.clone();
        let fresh: Vec<BBox> = app.doc.shapes.iter().map(|s| s.bbox()).collect();
        for (a, b) in fast.iter().zip(&fresh) {
            assert!(a.min.dist(b.min) < 0.05 && a.max.dist(b.max) < 0.05, "{a:?} vs {b:?}");
        }
        assert_eq!(app.hit_test(p2(10.0, 2.0), 4.0), Some(1));
    }
    #[test]
    fn line_then_tangent_arc_joins_up() {
        let pts = vec![p2(0.0, 0.0), p2(10.0, 0.0), p2(20.0, 10.0)];
        let arcs = vec![None, line_end_tangent(&pts[..2], &[None])];
        let pd = line_path(&pts, &arcs, false);
        let f = pd.flatten(0.01);
        assert_eq!(f.len(), 1, "one continuous path");
        let end = *f[0].last().unwrap();
        assert!(end.dist(p2(20.0, 10.0)) < 1e-6);
        // the arc leaves the line smoothly (tangent along +x)
        let first_arc_pt = f[0].iter().find(|p| p.x > 10.0 + 1e-6).unwrap();
        assert!(first_arc_pt.y.abs() < 0.05);
        let closed = line_path(&pts, &[None, None, None], true);
        assert!(is_closed(&closed.flatten(0.01)[0]));
    }
}

/// Drawing big projects quickly: points closer than 0.5 px (|dx| + |dy|) to the last drawn one are skipped,
/// and a shape no bigger than 1 × 1 px is drawn as a single dot.
pub const MIN_SCREEN_STEP: f64 = 0.5;
pub const DOT_SIZE_PX: f64 = 1.0;

/// `burncraft --bench <file>`: open a project and time the work a frame does, to find what makes big files slow.
pub fn bench(path: &std::path::Path) {
    let t = std::time::Instant::now();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let loaded = crate::lbrn::load_str(&text);
    eprintln!("load: {:?}", t.elapsed());
    let Ok(l) = loaded else { return };
    let mut app = App::new(crate::settings::Settings::default());
    app.doc = l.doc;
    app.doc.sync_cuts();
    eprintln!("shapes (top level): {}", app.doc.shapes.len());
    let time = |name: &str, f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        f();
        eprintln!("{name}: {:?}", t.elapsed());
    };
    time("outline cache", &mut || app.ensure_outlines());
    time("cached bounds of all shapes", &mut || {
        let _: Vec<BBox> = (0..app.doc.shapes.len()).map(|i| app.shape_box(i)).collect();
    });
    let all: Vec<usize> = (0..app.doc.shapes.len()).collect();
    let orig: Vec<Xform> = app.doc.shapes.iter().map(|s| s.xform).collect();
    time("drag frame: move every shape, update outlines", &mut || {
        for (i, x) in orig.iter().enumerate() {
            app.doc.shapes[i].xform = Xform::translate(1.0, 0.5).then_after(*x);
        }
        app.touch_xform();
        app.ensure_outlines();
    });
    app.sel = all;
    time("selection bounds (all selected)", &mut || {
        let _ = app.selection_box();
    });
    app.sel.clear();
    time("all shape bboxes (uncached)", &mut || {
        let _: Vec<BBox> = app.doc.shapes.iter().map(|s| s.bbox()).collect();
    });
    time("doc bbox", &mut || {
        let _ = app.doc.bbox();
    });
    let c = app.doc.bbox().center();
    time("hit test", &mut || {
        let _ = app.hit_test(c, 2.0);
    });
    time("snap scene", &mut || {
        let _ = app.infer(c, 2.0, false, None);
    });
    time("job build", &mut || {
        let _ = crate::gcode::build_job(&app.doc, &app.settings.device, None);
    });
    time("doc clone (undo checkpoint)", &mut || {
        let _ = app.doc.clone();
    });
    time("sync cuts", &mut || app.doc.sync_cuts());
    time("off-bed boxes", &mut || {
        let bed = crate::gcode::bed_box(&app.settings.device);
        let _ = app.doc.shapes.iter().filter(|s| !bed.contains_box(&s.bbox())).count();
    });
}
