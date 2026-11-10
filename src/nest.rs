//! Nest: move and rotate the selected shapes so they fit tightly on a sheet (the work area, or any closed
//! shape such as an offcut, whose holes are avoided).
//!
//! How it works: the sheet and every part are laid on a fine grid. A grid cell counts as part of a shape when
//! the shape's outline touches it at all, so cells never under-cover the real outline, and a sheet cell is
//! usable only when it lies wholly inside the sheet. Parts are placed one at a time at the lowest, then
//! leftmost, free position (bottom-left fill), trying every allowed rotation and mirror. Placed parts are grown
//! by the spacing, so no two outlines come closer than that. Several orders are tried within the time limit and
//! the best layout is kept. Written for BurnCraft (GPL-3.0-or-later); no other nesting code was used.

use crate::geom::*;
use crate::model::{Document, Shape};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Rotations {
    /// Keep each part as it is.
    None,
    /// 0°, 90°, 180° and 270°.
    #[default]
    Quarter,
    /// Every `step` degrees.
    Every,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SheetChoice {
    #[default]
    WorkArea,
    /// The last selected closed shape (it stays where it is; its holes are avoided).
    LastSelected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NestSettings {
    pub sheet: SheetChoice,
    /// Gap between parts (mm).
    pub spacing: f64,
    /// Gap from the sheet's edge (mm).
    pub margin: f64,
    pub rotations: Rotations,
    pub step: f64,
    pub mirror: bool,
    /// Small parts may go inside bigger parts' holes.
    pub in_holes: bool,
    /// Copies of the selection to place (1 = just the selection).
    pub copies: u32,
    /// Keep adding copies until the sheet is full.
    pub fill: bool,
    /// A finer grid: tighter, slower.
    pub fine: bool,
    /// Seconds to spend trying other orders.
    pub time_limit: f64,
}

impl Default for NestSettings {
    fn default() -> Self {
        NestSettings { sheet: SheetChoice::WorkArea, spacing: 2.0, margin: 2.0, rotations: Rotations::Quarter, step: 15.0, mirror: false, in_holes: false, copies: 1, fill: false, fine: false, time_limit: 3.0 }
    }
}

impl NestSettings {
    /// The allowed rotations, in degrees.
    pub fn angles(&self) -> Vec<f64> {
        match self.rotations {
            Rotations::None => vec![0.0],
            Rotations::Quarter => vec![0.0, 90.0, 180.0, 270.0],
            Rotations::Every => {
                let step = self.step.clamp(1.0, 180.0);
                let n = (360.0 / step).floor().max(1.0) as usize;
                (0..n).map(|i| i as f64 * step).collect()
            }
        }
    }
}

/// Where one part ends up: `delta` is applied after the shape's own transform. `copy` > 0 means a new copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub shape: usize,
    pub copy: u32,
    pub delta: Xform,
    pub angle: f64,
    pub mirrored: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NestResult {
    pub placed: Vec<Placement>,
    /// Parts (counting copies) that didn't fit.
    pub not_placed: usize,
    /// Area covered by the placed parts and the usable sheet area (mm²).
    pub used_area: f64,
    pub sheet_area: f64,
    pub attempts: usize,
}

impl NestResult {
    pub fn usage(&self) -> f64 {
        if self.sheet_area > 0.0 {
            100.0 * self.used_area / self.sheet_area
        } else {
            0.0
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Grid of cells as bits (1 = blocked)

struct Grid {
    w: usize,
    h: usize,
    words: usize,
    bits: Vec<u64>,
}

impl Grid {
    fn new(w: usize, h: usize, blocked: bool) -> Grid {
        let words = w.div_ceil(64).max(1);
        let mut g = Grid { w, h, words, bits: vec![if blocked { !0 } else { 0 }; words * h] };
        // bits past the right edge are always blocked
        if !blocked && w % 64 != 0 {
            for y in 0..h {
                g.bits[y * words + words - 1] |= !0u64 << (w % 64);
            }
        }
        g
    }
    fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.words + x / 64] >> (x % 64) & 1 == 1
    }
    fn set(&mut self, x: usize, y: usize, v: bool) {
        let i = y * self.words + x / 64;
        if v {
            self.bits[i] |= 1 << (x % 64);
        } else {
            self.bits[i] &= !(1 << (x % 64));
        }
    }
}

/// A shape's cells: `rows[y]` are bit masks, `w × h` cells from `origin` (mm) upward and rightward.
#[derive(Clone)]
struct Footprint {
    w: usize,
    h: usize,
    rows: Vec<Vec<u64>>,
    /// Cells counted for the used area (centre inside a closed outline).
    area_cells: usize,
}

impl Footprint {
    fn from_cells(w: usize, h: usize, on: &dyn Fn(usize, usize) -> bool) -> Footprint {
        let words = w.div_ceil(64).max(1);
        let mut rows = vec![vec![0u64; words]; h];
        for (y, row) in rows.iter_mut().enumerate() {
            for x in 0..w {
                if on(x, y) {
                    row[x / 64] |= 1 << (x % 64);
                }
            }
        }
        Footprint { w, h, rows, area_cells: 0 }
    }

    fn cell(&self, x: usize, y: usize) -> bool {
        self.rows[y][x / 64] >> (x % 64) & 1 == 1
    }

    /// Grown by `s` cells in every direction (a square around each cell).
    fn dilate(&self, s: usize) -> Footprint {
        if s == 0 {
            return self.clone();
        }
        let (w, h) = (self.w + 2 * s, self.h + 2 * s);
        // grow along the rows first: original row y, new column x is on when an original cell lies within s
        let mut horiz = vec![vec![false; w]; self.h];
        for (y, row) in horiz.iter_mut().enumerate() {
            for x in 0..self.w {
                if self.cell(x, y) {
                    for c in row.iter_mut().skip(x).take(2 * s + 1) {
                        *c = true;
                    }
                }
            }
        }
        // then along the columns: new row y covers original rows y − 2s ..= y
        Footprint::from_cells(w, h, &|x, y| {
            let lo = y.saturating_sub(2 * s);
            let hi = y.min(self.h - 1);
            lo <= hi && (lo..=hi).any(|oy| horiz[oy][x])
        })
    }
}

/// Does the cell [x0, x1] × [y0, y1] meet the segment a–b?
fn seg_hits_cell(a: P2, b: P2, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    // Liang–Barsky clipping
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.x - x0), (dx, x1 - a.x), (-dy, a.y - y0), (dy, y1 - a.y)] {
        if p.abs() < 1e-15 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Cells touched by the outline's edges, or whose centre is inside it (`even_odd`: holes stay empty;
/// otherwise every closed ring is filled solid).
fn rasterize(lines: &[Vec<P2>], even_odd: bool, cell: f64, origin: P2, w: usize, h: usize) -> (Vec<bool>, Vec<bool>) {
    let mut edge = vec![false; w * h];
    let eps = cell * 1e-6;
    for l in lines {
        for s in l.windows(2) {
            let (a, b) = (s[0], s[1]);
            let i0 = (((a.x.min(b.x) - origin.x) / cell).floor() as i64 - 1).max(0);
            let i1 = (((a.x.max(b.x) - origin.x) / cell).floor() as i64 + 1).min(w as i64 - 1);
            let j0 = (((a.y.min(b.y) - origin.y) / cell).floor() as i64 - 1).max(0);
            let j1 = (((a.y.max(b.y) - origin.y) / cell).floor() as i64 + 1).min(h as i64 - 1);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let x0 = origin.x + i as f64 * cell - eps;
                    let y0 = origin.y + j as f64 * cell - eps;
                    if seg_hits_cell(a, b, x0, y0, x0 + cell + 2.0 * eps, y0 + cell + 2.0 * eps) {
                        edge[j as usize * w + i as usize] = true;
                    }
                }
            }
        }
    }
    let closed: Vec<Vec<P2>> = lines.iter().filter(|l| is_closed(l)).cloned().collect();
    let mut inside = vec![false; w * h];
    for j in 0..h {
        for i in 0..w {
            let c = p2(origin.x + (i as f64 + 0.5) * cell, origin.y + (j as f64 + 0.5) * cell);
            inside[j * w + i] = if even_odd { point_in_rings(c, &closed) } else { closed.iter().any(|r| point_in_rings(c, std::slice::from_ref(r))) };
        }
    }
    (edge, inside)
}

/// One way to place a part: rotated / mirrored about its middle, rasterised.
struct Variant {
    angle: f64,
    mirrored: bool,
    /// part outline → variant: mirror and rotate about the part's middle
    local: Xform,
    /// bottom-left corner of the variant's cells, in variant space
    origin: P2,
    raw: Footprint,
    grown: Footprint,
}

struct Part {
    shape: usize,
    copy: u32,
    variants: std::rc::Rc<Vec<Variant>>,
    area_cells: usize,
}

fn variants(lines: &[Vec<P2>], set: &NestSettings, cell: f64, grow: usize) -> Vec<Variant> {
    let mut bb = BBox::EMPTY;
    for l in lines {
        for p in l {
            bb.add(*p);
        }
    }
    let c0 = bb.center();
    let mut out = Vec::new();
    for mirrored in if set.mirror { vec![false, true] } else { vec![false] } {
        for angle in set.angles() {
            let local = Xform::rotate(angle.to_radians()).then_after(Xform::scale(if mirrored { -1.0 } else { 1.0 }, 1.0)).then_after(Xform::translate(-c0.x, -c0.y));
            let t: Vec<Vec<P2>> = lines.iter().map(|l| l.iter().map(|p| local.apply(*p)).collect()).collect();
            let mut b = BBox::EMPTY;
            for l in &t {
                for p in l {
                    b.add(*p);
                }
            }
            let origin = b.min;
            let w = ((b.max.x - origin.x) / cell).floor() as usize + 1;
            let h = ((b.max.y - origin.y) / cell).floor() as usize + 1;
            let (edge, inside) = rasterize(&t, set.in_holes, cell, origin, w, h);
            let mut raw = Footprint::from_cells(w, h, &|x, y| edge[y * w + x] || inside[y * w + x]);
            raw.area_cells = inside.iter().zip(&edge).filter(|(i, e)| **i || **e).count();
            let grown = raw.dilate(grow);
            out.push(Variant { angle, mirrored, local, origin, raw, grown });
        }
    }
    out
}

fn fits(g: &Grid, f: &Footprint, gx: usize, gy: usize) -> bool {
    let (wi, s) = (gx / 64, gx % 64);
    for (r, row) in f.rows.iter().enumerate() {
        let base = (gy + r) * g.words;
        for (k, &bits) in row.iter().enumerate() {
            if bits == 0 {
                continue;
            }
            let i = wi + k;
            if i < g.words && g.bits[base + i] & (bits << s) != 0 {
                return false;
            }
            if s > 0 && i + 1 < g.words && g.bits[base + i + 1] & (bits >> (64 - s)) != 0 {
                return false;
            }
            if s > 0 && i + 1 >= g.words && bits >> (64 - s) != 0 {
                return false;
            }
        }
    }
    true
}

fn mark(g: &mut Grid, f: &Footprint, gx: i64, gy: i64) {
    for y in 0..f.h {
        let yy = gy + y as i64;
        if yy < 0 || yy >= g.h as i64 {
            continue;
        }
        for x in 0..f.w {
            let xx = gx + x as i64;
            if xx >= 0 && (xx as usize) < g.w && f.cell(x, y) {
                g.set(xx as usize, yy as usize, true);
            }
        }
    }
}

/// The lowest, then leftmost, place where any variant fits: (variant, gx, gy).
fn best_spot(g: &Grid, vs: &[Variant]) -> Option<(usize, usize, usize)> {
    let mut best: Option<(usize, usize, usize, usize)> = None; // (top, gx, variant, gy)
    for (vi, v) in vs.iter().enumerate() {
        let f = &v.raw;
        if f.w > g.w || f.h > g.h {
            continue;
        }
        'rows: for gy in 0..=g.h - f.h {
            if let Some(b) = best {
                if gy + f.h > b.0 {
                    break 'rows;
                }
            }
            for gx in 0..=g.w - f.w {
                if fits(g, f, gx, gy) {
                    let top = gy + f.h;
                    if best.map(|b| (top, gx) < (b.0, b.1)).unwrap_or(true) {
                        best = Some((top, gx, vi, gy));
                    }
                    break 'rows;
                }
            }
        }
    }
    best.map(|b| (b.2, b.1, b.3))
}

/// What to nest, gathered from the document (so the search can run on another thread).
pub struct Job {
    /// (shape index, closed or open outline lines in world space)
    parts: Vec<(usize, Vec<Vec<P2>>)>,
    sheet: Vec<Vec<P2>>,
    settings: NestSettings,
}

/// Gather the parts (the selection, minus the sheet shape) and the sheet outline.
pub fn prepare(doc: &Document, sel: &[usize], bed_w: f64, bed_h: f64, settings: &NestSettings) -> Result<Job, String> {
    let mut sel: Vec<usize> = sel.iter().copied().filter(|i| *i < doc.shapes.len()).collect();
    let sheet = match settings.sheet {
        SheetChoice::WorkArea => vec![vec![p2(0.0, 0.0), p2(bed_w, 0.0), p2(bed_w, bed_h), p2(0.0, bed_h), p2(0.0, 0.0)]],
        SheetChoice::LastSelected => {
            let last = sel.pop().ok_or("Select the parts, then the sheet shape last.")?;
            let s: &Shape = &doc.shapes[last];
            let rings: Vec<Vec<P2>> = s.world_outline(0.1).into_iter().filter(|l| is_closed(l)).collect();
            if rings.is_empty() {
                return Err("The last selected shape isn't closed, so it can't be the sheet.".into());
            }
            rings
        }
    };
    let parts: Vec<(usize, Vec<Vec<P2>>)> = sel.iter().map(|&i| (i, doc.shapes[i].world_outline(0.1))).filter(|(_, l)| l.iter().any(|x| x.len() >= 2)).collect();
    if parts.is_empty() {
        return Err("Select the shapes to nest first.".into());
    }
    Ok(Job { parts, sheet, settings: settings.clone() })
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// Run the nest. `stop` ends it early (the best layout so far is kept); `progress` gets 0..1000.
pub fn run(job: &Job, stop: &AtomicBool, progress: &dyn Fn(u32)) -> NestResult {
    let set = &job.settings;
    let mut sb = BBox::EMPTY;
    for l in &job.sheet {
        for p in l {
            sb.add(*p);
        }
    }
    let target_cells = if set.fine { 800_000.0 } else { 200_000.0 };
    let cell = ((sb.width() * sb.height()) / target_cells).sqrt().max(0.1);
    let (gw, gh) = ((sb.width() / cell).floor() as usize, (sb.height() / cell).floor() as usize);
    if gw == 0 || gh == 0 {
        return NestResult::default();
    }
    // usable sheet cells: wholly inside the sheet, then pulled in by the margin
    let (edge, inside) = rasterize(&job.sheet, true, cell, sb.min, gw, gh);
    let mut sheet = Grid::new(gw, gh, false);
    let mut usable = 0usize;
    for y in 0..gh {
        for x in 0..gw {
            let ok = inside[y * gw + x] && !edge[y * gw + x];
            sheet.set(x, y, !ok);
            usable += inside[y * gw + x] as usize;
        }
    }
    let m = (set.margin.max(0.0) / cell).ceil() as usize;
    if m > 0 {
        let blocked: Vec<(usize, usize)> = (0..gh).flat_map(|y| (0..gw).map(move |x| (x, y))).filter(|&(x, y)| sheet.get(x, y)).collect();
        let fp = Footprint::from_cells(1, 1, &|_, _| true).dilate(m);
        for (x, y) in blocked {
            mark(&mut sheet, &fp, x as i64 - m as i64, y as i64 - m as i64);
        }
    }
    let grow = (set.spacing.max(0.0) / cell).ceil() as usize;
    let mut parts: Vec<Part> = Vec::new();
    let mut by_shape = Vec::new();
    for (shape, lines) in &job.parts {
        let vs = std::rc::Rc::new(variants(lines, set, cell, grow));
        let area = vs.first().map(|v| v.raw.area_cells).unwrap_or(0);
        by_shape.push((*shape, vs.clone(), area));
        for copy in 0..set.copies.max(1) {
            parts.push(Part { shape: *shape, copy, variants: vs.clone(), area_cells: area });
        }
    }
    let fill_limit = 2000;
    let start = now_secs();
    let mut best: Option<(usize, usize, usize, Vec<Placement>)> = None; // (placed, area cells, -top, list)
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut attempts = 0;
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(parts[i].area_cells));
    loop {
        attempts += 1;
        let mut g = Grid { w: sheet.w, h: sheet.h, words: sheet.words, bits: sheet.bits.clone() };
        let mut placed = Vec::new();
        let mut area = 0usize;
        let mut top = 0usize;
        let mut try_part = |p: &Part, copy: u32, g: &mut Grid, placed: &mut Vec<Placement>| -> bool {
            match best_spot(g, &p.variants) {
                Some((vi, gx, gy)) => {
                    let v = &p.variants[vi];
                    mark(g, &v.grown, gx as i64 - grow as i64, gy as i64 - grow as i64);
                    let t = p2(sb.min.x + gx as f64 * cell - v.origin.x, sb.min.y + gy as f64 * cell - v.origin.y);
                    placed.push(Placement { shape: p.shape, copy, delta: Xform::translate(t.x, t.y).then_after(v.local), angle: v.angle, mirrored: v.mirrored });
                    area += v.raw.area_cells;
                    top = top.max(gy + v.raw.h);
                    true
                }
                None => false,
            }
        };
        for &i in &order {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let p = &parts[i];
            try_part(p, p.copy, &mut g, &mut placed);
        }
        if set.fill {
            // keep adding copies of the selection while any fits
            let mut copy = set.copies.max(1);
            'fill: while placed.len() < fill_limit && !stop.load(Ordering::Relaxed) {
                let mut any = false;
                for (shape, vs, a) in &by_shape {
                    let p = Part { shape: *shape, copy, variants: vs.clone(), area_cells: *a };
                    if try_part(&p, copy, &mut g, &mut placed) {
                        any = true;
                    }
                    if placed.len() >= fill_limit {
                        break 'fill;
                    }
                }
                copy += 1;
                if !any {
                    break;
                }
            }
        }
        let score = (placed.len(), area, usize::MAX - top);
        if best.as_ref().map(|b| score > (b.0, b.1, usize::MAX - b.2)).unwrap_or(true) {
            best = Some((placed.len(), area, top, placed));
        }
        let elapsed = now_secs() - start;
        progress(((elapsed / set.time_limit.max(0.1)) * 1000.0).min(1000.0) as u32);
        // other orders can place more parts, or the same ones more compactly
        if stop.load(Ordering::Relaxed) || elapsed >= set.time_limit || parts.len() <= 1 {
            break;
        }
        rng.shuffle(&mut order);
        // keep big parts early most of the time
        if attempts % 2 == 0 {
            order.sort_by_key(|&i| std::cmp::Reverse(parts[i].area_cells / 4 + (rng.next() % 3) as usize));
        }
    }
    let (n, area, _, placed) = best.unwrap_or((0, 0, 0, Vec::new()));
    let total = if set.fill { n } else { parts.len() };
    NestResult { not_placed: total.saturating_sub(n), used_area: area as f64 * cell * cell, sheet_area: usable as f64 * cell * cell, placed, attempts }
}

fn now_secs() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
}

/// Apply a result to the document: moves the shapes and adds the copies. (The caller saves an undo step first.)
pub fn apply(doc: &mut Document, r: &NestResult) -> Vec<usize> {
    let mut new_sel = Vec::new();
    let originals: Vec<Shape> = doc.shapes.clone();
    for p in &r.placed {
        if p.shape >= originals.len() {
            continue;
        }
        if p.copy == 0 {
            let s = &mut doc.shapes[p.shape];
            s.xform = p.delta.then_after(originals[p.shape].xform);
            new_sel.push(p.shape);
        } else {
            let mut s = originals[p.shape].clone();
            s.renew_ids();
            s.xform = p.delta.then_after(originals[p.shape].xform);
            doc.shapes.push(s);
            new_sel.push(doc.shapes.len() - 1);
        }
    }
    new_sel
}

/// Run on a background thread (desktop); the browser runs it straight away.
pub mod running {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    pub struct Running {
        pub progress: Arc<AtomicU32>,
        pub stop: Arc<AtomicBool>,
        pub result: Arc<Mutex<Option<NestResult>>>,
        /// The document revision it was started for.
        pub rev: u64,
    }

    pub fn start(job: Job, rev: u64) -> Running {
        let r = Running { progress: Arc::new(AtomicU32::new(0)), stop: Arc::new(AtomicBool::new(false)), result: Arc::new(Mutex::new(None)), rev };
        let rr = r.clone();
        let work = move || {
            let p = rr.progress.clone();
            let res = run(&job, &rr.stop, &move |v| p.store(v, Ordering::Relaxed));
            *rr.result.lock().unwrap() = Some(res);
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(work);
        #[cfg(target_arch = "wasm32")]
        work();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    fn rect(doc: &mut Document, w: f64, h: f64, x: f64, y: f64) {
        doc.shapes.push(Shape::new(ShapeKind::Rect { w, h, cr: 0.0 }, Xform::translate(x, y), 0));
    }

    fn seg_dist(a: P2, b: P2, c: P2, d: P2) -> f64 {
        if segments_cross(a, b, c, d) {
            return 0.0;
        }
        let pt = |p: P2, a: P2, b: P2| {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let l = dx * dx + dy * dy;
            let t = if l > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
            p.dist(p2(a.x + t * dx, a.y + t * dy))
        };
        pt(a, c, d).min(pt(b, c, d)).min(pt(c, a, b)).min(pt(d, a, b))
    }

    fn segments_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
        let o = |p: P2, q: P2, r: P2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    }

    /// Smallest gap between two sets of outlines (0 if they cross or one is inside the other).
    fn gap(a: &[Vec<P2>], b: &[Vec<P2>]) -> f64 {
        let closed_a: Vec<Vec<P2>> = a.iter().filter(|l| is_closed(l)).cloned().collect();
        let closed_b: Vec<Vec<P2>> = b.iter().filter(|l| is_closed(l)).cloned().collect();
        if b.iter().flatten().any(|p| point_in_rings(*p, &closed_a)) || a.iter().flatten().any(|p| point_in_rings(*p, &closed_b)) {
            return 0.0;
        }
        let mut m = f64::MAX;
        for la in a {
            for sa in la.windows(2) {
                for lb in b {
                    for sb in lb.windows(2) {
                        m = m.min(seg_dist(sa[0], sa[1], sb[0], sb[1]));
                    }
                }
            }
        }
        m
    }

    fn check(doc: &Document, set: &NestSettings, sheet: &[Vec<P2>], moved: &[usize]) {
        let outs: Vec<Vec<Vec<P2>>> = moved.iter().map(|&i| doc.shapes[i].world_outline(0.1)).collect();
        for i in 0..outs.len() {
            for j in i + 1..outs.len() {
                let g = gap(&outs[i], &outs[j]);
                assert!(g >= set.spacing - 1e-6, "parts {i} and {j} are {g:.3} mm apart (spacing {})", set.spacing);
            }
            // inside the sheet, at least the margin from its edge
            for p in outs[i].iter().flatten() {
                assert!(point_in_rings(*p, sheet), "a part sticks out of the sheet at {p:?}");
            }
            let edge: Vec<Vec<P2>> = sheet.to_vec();
            let m = outs[i].iter().flat_map(|l| l.windows(2)).flat_map(|s| edge.iter().flat_map(|e| e.windows(2)).map(move |t| seg_dist(s[0], s[1], t[0], t[1]))).fold(f64::MAX, f64::min);
            assert!(m >= set.margin - 1e-6, "part {i} is {m:.3} mm from the sheet edge");
        }
    }

    #[test]
    fn rectangles_fill_the_work_area_without_overlap() {
        let mut doc = Document::default();
        for i in 0..12 {
            rect(&mut doc, 40.0, 20.0, 300.0 + i as f64 * 3.0, 300.0);
        }
        let set = NestSettings { spacing: 1.0, margin: 2.0, time_limit: 0.5, ..Default::default() };
        let sel: Vec<usize> = (0..12).collect();
        let job = prepare(&doc, &sel, 200.0, 100.0, &set).unwrap();
        let r = run(&job, &AtomicBool::new(false), &|_| {});
        assert_eq!(r.not_placed, 0, "all 12 fit on a 200 × 100 sheet");
        assert!(r.usage() >= 45.0, "usage {:.1}%", r.usage());
        let before: Vec<Xform> = doc.shapes.iter().map(|s| s.xform).collect();
        let mut d2 = doc.clone();
        let moved = apply(&mut d2, &r);
        check(&d2, &set, &job.sheet, &moved);
        // rotations are only the allowed ones
        for p in &r.placed {
            assert!([0.0, 90.0, 180.0, 270.0].contains(&p.angle));
        }
        // undo = the untouched original (apply works on a copy here; the app saves an undo step first)
        assert_eq!(doc.shapes.iter().map(|s| s.xform).collect::<Vec<_>>(), before);
    }

    #[test]
    fn odd_shapes_rotations_holes_and_a_custom_sheet() {
        let mut doc = Document::default();
        // the sheet: a 150 × 120 offcut with a 40 × 40 hole in the middle
        let outer = vec![p2(0.0, 0.0), p2(150.0, 0.0), p2(150.0, 120.0), p2(0.0, 120.0), p2(0.0, 0.0)];
        let hole = vec![p2(55.0, 40.0), p2(95.0, 40.0), p2(95.0, 80.0), p2(55.0, 80.0), p2(55.0, 40.0)];
        doc.shapes.push(Shape::new(ShapeKind::Path(crate::model::PathData::from_polylines(&[outer.clone(), hole.clone()])), Xform::translate(400.0, 0.0), 0));
        // parts: triangles and an L shape
        for i in 0..6 {
            let tri = vec![p2(0.0, 0.0), p2(30.0, 0.0), p2(0.0, 25.0), p2(0.0, 0.0)];
            doc.shapes.push(Shape::new(ShapeKind::Path(crate::model::PathData::polyline(&tri[..3], true)), Xform::translate(10.0 * i as f64, 200.0), 0));
        }
        let l = vec![p2(0.0, 0.0), p2(50.0, 0.0), p2(50.0, 10.0), p2(10.0, 10.0), p2(10.0, 40.0), p2(0.0, 40.0)];
        doc.shapes.push(Shape::new(ShapeKind::Path(crate::model::PathData::polyline(&l, true)), Xform::translate(0.0, 300.0), 0));
        let mut sel: Vec<usize> = (1..doc.shapes.len()).collect();
        sel.push(0); // the sheet last
        let set = NestSettings { sheet: SheetChoice::LastSelected, spacing: 2.0, margin: 1.5, rotations: Rotations::Every, step: 45.0, time_limit: 0.5, ..Default::default() };
        let job = prepare(&doc, &sel, 0.0, 0.0, &set).unwrap();
        let r = run(&job, &AtomicBool::new(false), &|_| {});
        assert_eq!(r.not_placed, 0);
        let mut d2 = doc.clone();
        let moved = apply(&mut d2, &r);
        check(&d2, &set, &job.sheet, &moved);
        for p in &r.placed {
            assert!((p.angle / 45.0).fract().abs() < 1e-9, "angle {} isn't a 45° step", p.angle);
        }
        // the sheet itself didn't move
        assert_eq!(d2.shapes[0].xform, doc.shapes[0].xform);
    }

    #[test]
    fn copies_fill_and_nothing_fits() {
        let mut doc = Document::default();
        rect(&mut doc, 30.0, 30.0, 500.0, 500.0);
        let set = NestSettings { spacing: 0.0, margin: 0.0, rotations: Rotations::None, fill: true, time_limit: 0.0, ..Default::default() };
        let job = prepare(&doc, &[0], 100.0, 100.0, &set).unwrap();
        let r = run(&job, &AtomicBool::new(false), &|_| {});
        // a 30 mm square: 3 × 3 fit in 100 × 100 (the grid can cost a little, so at least 2 × 3)
        assert!(r.placed.len() >= 6 && r.placed.len() <= 9, "{} copies", r.placed.len());
        let mut d2 = doc.clone();
        let moved = apply(&mut d2, &r);
        assert_eq!(d2.shapes.len(), r.placed.len());
        check(&d2, &set, &job.sheet, &moved);
        // too big for the sheet: nothing placed, nothing moved
        let big = prepare(&doc, &[0], 20.0, 20.0, &NestSettings { time_limit: 0.0, ..Default::default() }).unwrap();
        let r = run(&big, &AtomicBool::new(false), &|_| {});
        assert!(r.placed.is_empty() && r.not_placed == 1);
        assert!(prepare(&doc, &[], 100.0, 100.0, &set).is_err());
    }
}
