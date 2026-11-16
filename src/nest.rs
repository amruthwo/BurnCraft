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
    /// Time ran out (or Stop was pressed) before even one layout was finished: this one is part-done.
    pub partial: bool,
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

#[derive(Clone)]
struct Grid {
    w: usize,
    h: usize,
    words: usize,
    bits: Vec<u64>,
    /// For each cell: how many free cells start there, going right (0 = blocked). Lets the search jump over
    /// blocked stretches instead of testing every position.
    run: Vec<u32>,
    /// The longest free stretch in each row.
    row_max: Vec<u32>,
}

impl Grid {
    fn new(w: usize, h: usize, blocked: bool) -> Grid {
        let words = w.div_ceil(64).max(1);
        let mut g = Grid { w, h, words, bits: vec![if blocked { !0 } else { 0 }; words * h], run: vec![0; w * h], row_max: vec![0; h] };
        // bits past the right edge are always blocked
        if !blocked && w % 64 != 0 {
            for y in 0..h {
                g.bits[y * words + words - 1] |= !0u64 << (w % 64);
            }
        }
        g.refresh(0, h);
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
    /// Recount the free stretches of rows `y0..y1`.
    fn refresh(&mut self, y0: usize, y1: usize) {
        for y in y0..y1.min(self.h) {
            let mut n = 0u32;
            let mut best = 0u32;
            for x in (0..self.w).rev() {
                n = if self.get(x, y) { 0 } else { n + 1 };
                self.run[y * self.w + x] = n;
                best = best.max(n);
            }
            self.row_max[y] = best;
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
    /// The row with the longest stretch of cells: (row, first cell, length). A position can only fit where
    /// the grid has a free stretch at least that long under it.
    pivot: (usize, usize, usize),
}

impl Footprint {
    fn from_cells(w: usize, h: usize, on: &dyn Fn(usize, usize) -> bool) -> Footprint {
        let words = w.div_ceil(64).max(1);
        let mut rows = vec![vec![0u64; words]; h];
        let mut pivot = (0, 0, 0);
        for (y, row) in rows.iter_mut().enumerate() {
            let mut run_start = 0;
            let mut run = 0;
            for x in 0..w {
                if on(x, y) {
                    row[x / 64] |= 1 << (x % 64);
                    if run == 0 {
                        run_start = x;
                    }
                    run += 1;
                    if run > pivot.2 {
                        pivot = (y, run_start, run);
                    }
                } else {
                    run = 0;
                }
            }
        }
        Footprint { w, h, rows, area_cells: 0, pivot }
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

/// Cells within `pad` of the outline's edges, and cells whose centre is inside it (`even_odd`: holes stay
/// empty; otherwise every closed ring is filled solid). The inside test goes row by row (where each row's
/// centre line crosses the edges), so detailed outlines stay quick.
fn rasterize(lines: &[Vec<P2>], even_odd: bool, cell: f64, origin: P2, w: usize, h: usize, pad: f64) -> (Vec<bool>, Vec<bool>) {
    let mut edge = vec![false; w * h];
    let eps = cell * 1e-6 + pad;
    for l in lines {
        for s in l.windows(2) {
            let (a, b) = (s[0], s[1]);
            let i0 = (((a.x.min(b.x) - pad - origin.x) / cell).floor() as i64 - 1).max(0);
            let i1 = (((a.x.max(b.x) + pad - origin.x) / cell).floor() as i64 + 1).min(w as i64 - 1);
            let j0 = (((a.y.min(b.y) - pad - origin.y) / cell).floor() as i64 - 1).max(0);
            let j1 = (((a.y.max(b.y) + pad - origin.y) / cell).floor() as i64 + 1).min(h as i64 - 1);
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
    let closed: Vec<&Vec<P2>> = lines.iter().filter(|l| is_closed(l)).collect();
    let groups: Vec<Vec<&Vec<P2>>> = if even_odd { vec![closed] } else { closed.into_iter().map(|r| vec![r]).collect() };
    let mut inside = vec![false; w * h];
    let mut xs: Vec<f64> = Vec::new();
    for grp in &groups {
        for j in 0..h {
            let cy = origin.y + (j as f64 + 0.5) * cell;
            xs.clear();
            for r in grp {
                for s in r.windows(2) {
                    let (a, b) = (s[0], s[1]);
                    if (a.y > cy) != (b.y > cy) {
                        xs.push(a.x + (cy - a.y) * (b.x - a.x) / (b.y - a.y));
                    }
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                // cells whose centre lies between the two crossings
                let i0 = ((pair[0] - origin.x) / cell - 0.5).ceil().max(0.0) as usize;
                let i1 = ((pair[1] - origin.x) / cell - 0.5).floor();
                if i1 < 0.0 {
                    continue;
                }
                for i in i0..=(i1 as usize).min(w.saturating_sub(1)) {
                    inside[j * w + i] = true;
                }
            }
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

/// More ways to turn each part than this makes nesting slow (the window warns).
pub const MANY_VARIANTS: usize = 24;

impl NestSettings {
    /// How many rotations and mirrorings each part is tried in.
    pub fn variant_count(&self) -> usize {
        self.angles().len() * if self.mirror { 2 } else { 1 }
    }
}

/// Every allowed rotation / mirroring of one part. None if stopped part-way.
fn variants(lines: &[Vec<P2>], set: &NestSettings, cell: f64, grow: usize, halted: &dyn Fn() -> bool) -> Option<Vec<Variant>> {
    // very detailed outlines (traced pictures) are simplified first; the cells are grown by the same amount,
    // so they still cover the real outline
    let tol = cell * 0.4;
    let lines: Vec<Vec<P2>> = lines
        .iter()
        .map(|l| {
            let closed = is_closed(l);
            let mut s = simplify(l, tol);
            if closed && s.first() != s.last() {
                s.push(s[0]);
            }
            s
        })
        .collect();
    let mut bb = BBox::EMPTY;
    for l in &lines {
        for p in l {
            bb.add(*p);
        }
    }
    let c0 = bb.center();
    let mut out = Vec::new();
    for mirrored in if set.mirror { vec![false, true] } else { vec![false] } {
        for angle in set.angles() {
            if halted() {
                return None;
            }
            let local = Xform::rotate(angle.to_radians()).then_after(Xform::scale(if mirrored { -1.0 } else { 1.0 }, 1.0)).then_after(Xform::translate(-c0.x, -c0.y));
            let t: Vec<Vec<P2>> = lines.iter().map(|l| l.iter().map(|p| local.apply(*p)).collect()).collect();
            let mut b = BBox::EMPTY;
            for l in &t {
                for p in l {
                    b.add(*p);
                }
            }
            let origin = p2(b.min.x - tol, b.min.y - tol);
            let w = ((b.max.x + tol - origin.x) / cell).floor() as usize + 1;
            let h = ((b.max.y + tol - origin.y) / cell).floor() as usize + 1;
            let (edge, inside) = rasterize(&t, set.in_holes, cell, origin, w, h, tol);
            let mut raw = Footprint::from_cells(w, h, &|x, y| edge[y * w + x] || inside[y * w + x]);
            raw.area_cells = inside.iter().zip(&edge).filter(|(i, e)| **i || **e).count();
            let grown = raw.dilate(grow);
            out.push(Variant { angle, mirrored, local, origin, raw, grown });
        }
    }
    Some(out)
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

/// Block the footprint's cells (without recounting the free stretches: call `refresh` after).
fn mark_cells(g: &mut Grid, f: &Footprint, gx: i64, gy: i64) {
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

fn mark(g: &mut Grid, f: &Footprint, gx: i64, gy: i64) {
    mark_cells(g, f, gx, gy);
    let y0 = gy.max(0) as usize;
    g.refresh(y0, (gy + f.h as i64).max(0) as usize);
}

struct Halted;

/// The lowest, then leftmost, place where any variant fits: (variant, gx, gy). Checks `halted` every row.
fn best_spot(g: &Grid, vs: &[Variant], halted: &dyn Fn() -> bool) -> Result<Option<(usize, usize, usize)>, Halted> {
    let mut best: Option<(usize, usize, usize, usize)> = None; // (top, gx, variant, gy)
    for (vi, v) in vs.iter().enumerate() {
        let f = &v.raw;
        if f.w > g.w || f.h > g.h {
            continue;
        }
        let (pr, pa, plen) = f.pivot;
        let xmax = pa + (g.w - f.w);
        'rows: for gy in 0..=g.h - f.h {
            if let Some(b) = best {
                if gy + f.h > b.0 {
                    break 'rows;
                }
            }
            if halted() {
                return Err(Halted);
            }
            let row = gy + pr;
            if (g.row_max[row] as usize) < plen {
                continue;
            }
            // walk the pivot row's free stretches; only those long enough can hold the part
            let mut x = pa;
            while x <= xmax {
                let r = g.run[row * g.w + x] as usize;
                if r >= plen {
                    let gx = x - pa;
                    if fits(g, f, gx, gy) {
                        let top = gy + f.h;
                        if best.map(|b| (top, gx) < (b.0, b.1)).unwrap_or(true) {
                            best = Some((top, gx, vi, gy));
                        }
                        break 'rows;
                    }
                    x += 1;
                } else {
                    x += r + 1;
                }
            }
        }
    }
    Ok(best.map(|b| (b.2, b.1, b.3)))
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

const FILL_LIMIT: usize = 2000;

/// A layout being built: the grid so far and what's on it.
#[derive(Clone)]
struct Attempt {
    g: Grid,
    placed: Vec<Placement>,
    area: usize,
    top: usize,
    /// next part in `order`
    next: usize,
    /// filling: which copy number, which part next, and whether this round placed any
    fill_copy: u32,
    fill_k: usize,
    fill_any: bool,
}

/// The nest, worked out a step at a time so it can be stopped at any moment, and so the browser can run it a
/// slice per frame without freezing the page.
pub struct Nester {
    set: NestSettings,
    job_parts: Vec<(usize, Vec<Vec<P2>>)>,
    cell: f64,
    origin: P2,
    grow: usize,
    usable: usize,
    sheet: Option<Grid>,
    by_shape: Vec<(usize, std::rc::Rc<Vec<Variant>>, usize)>,
    parts: Vec<Part>,
    order: Vec<usize>,
    rng: Rng,
    attempts: usize,
    cur: Option<Attempt>,
    /// (placed, area cells, top, list)
    best: Option<(usize, usize, usize, Vec<Placement>)>,
    start: f64,
    done: bool,
    partial: bool,
}

/// How far a nest has got.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    /// Still turning the parts into cells (0..1000), or None once placing has begun.
    pub preparing: Option<u32>,
    /// Parts placed in the layout being tried, out of the parts to place (0..1000).
    pub placed: u32,
    /// Layouts finished so far.
    pub attempts: u32,
}

impl Nester {
    pub fn new(job: &Job) -> Nester {
        let set = job.settings.clone();
        let mut sb = BBox::EMPTY;
        for l in &job.sheet {
            for p in l {
                sb.add(*p);
            }
        }
        let target_cells = if set.fine { 800_000.0 } else { 200_000.0 };
        let cell = ((sb.width() * sb.height()) / target_cells).sqrt().max(0.1);
        let (gw, gh) = ((sb.width() / cell).floor() as usize, (sb.height() / cell).floor() as usize);
        let mut n = Nester {
            grow: (set.spacing.max(0.0) / cell).ceil() as usize,
            set,
            job_parts: job.parts.clone(),
            cell,
            origin: sb.min,
            usable: 0,
            sheet: None,
            by_shape: Vec::new(),
            parts: Vec::new(),
            order: Vec::new(),
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            attempts: 0,
            cur: None,
            best: None,
            start: now_secs(),
            done: false,
            partial: false,
        };
        if gw == 0 || gh == 0 || sb.is_empty() {
            n.done = true;
            return n;
        }
        // usable sheet cells: wholly inside the sheet, then pulled in by the margin
        let (edge, inside) = rasterize(&job.sheet, true, cell, sb.min, gw, gh, 0.0);
        let mut sheet = Grid::new(gw, gh, false);
        for y in 0..gh {
            for x in 0..gw {
                let ok = inside[y * gw + x] && !edge[y * gw + x];
                sheet.set(x, y, !ok);
                n.usable += inside[y * gw + x] as usize;
            }
        }
        let m = (n.set.margin.max(0.0) / cell).ceil() as usize;
        if m > 0 {
            let blocked: Vec<(usize, usize)> = (0..gh).flat_map(|y| (0..gw).map(move |x| (x, y))).filter(|&(x, y)| sheet.get(x, y)).collect();
            let fp = Footprint::from_cells(1, 1, &|_, _| true).dilate(m);
            for (x, y) in blocked {
                mark_cells(&mut sheet, &fp, x as i64 - m as i64, y as i64 - m as i64);
            }
        }
        sheet.refresh(0, gh);
        n.sheet = Some(sheet);
        n
    }

    pub fn progress(&self) -> Progress {
        let total = self.job_parts.len().max(1);
        if self.by_shape.len() < self.job_parts.len() && !self.done {
            return Progress { preparing: Some((self.by_shape.len() * 1000 / total) as u32), placed: 0, attempts: 0 };
        }
        let placed = self.cur.as_ref().map(|a| a.placed.len()).unwrap_or(0);
        let want = if self.set.fill { placed.max(1) } else { self.parts.len().max(1) };
        Progress { preparing: None, placed: (placed.min(want) * 1000 / want) as u32, attempts: self.attempts as u32 }
    }

    fn new_attempt(&self) -> Attempt {
        Attempt { g: self.sheet.clone().unwrap(), placed: Vec::new(), area: 0, top: 0, next: 0, fill_copy: self.set.copies.max(1), fill_k: 0, fill_any: false }
    }

    /// Place one part on the attempt's grid (or not, when it doesn't fit).
    fn place(&self, a: &mut Attempt, shape: usize, copy: u32, vs: &[Variant], halted: &dyn Fn() -> bool) -> Result<bool, Halted> {
        let Some((vi, gx, gy)) = best_spot(&a.g, vs, halted)? else { return Ok(false) };
        let v = &vs[vi];
        mark(&mut a.g, &v.grown, gx as i64 - self.grow as i64, gy as i64 - self.grow as i64);
        let t = p2(self.origin.x + gx as f64 * self.cell - v.origin.x, self.origin.y + gy as f64 * self.cell - v.origin.y);
        a.placed.push(Placement { shape, copy, delta: Xform::translate(t.x, t.y).then_after(v.local), angle: v.angle, mirrored: v.mirrored });
        a.area += v.raw.area_cells;
        a.top = a.top.max(gy + v.raw.h);
        Ok(true)
    }

    fn keep(&mut self, a: Attempt) {
        let score = (a.placed.len(), a.area, usize::MAX - a.top);
        if self.best.as_ref().map(|b| score > (b.0, b.1, usize::MAX - b.2)).unwrap_or(true) {
            self.best = Some((a.placed.len(), a.area, a.top, a.placed));
        }
    }

    /// Stopped or out of time: keep the best finished layout, or else the one being built.
    fn finish(&mut self) {
        if let Some(a) = self.cur.take() {
            if self.best.is_none() {
                self.partial = true;
                self.keep(a);
            }
        }
        self.done = true;
    }

    /// Work until `slice_end` (seconds, as `now_secs`), Stop or the time limit. True when finished.
    pub fn step(&mut self, stop: &AtomicBool, slice_end: f64) -> bool {
        let deadline = self.start + self.set.time_limit.max(0.1);
        let halted = || stop.load(Ordering::Relaxed) || now_secs() >= deadline;
        while !self.done {
            if halted() {
                self.finish();
                break;
            }
            // 1. turn each part into cells, one part per step
            if self.by_shape.len() < self.job_parts.len() {
                let (shape, lines) = &self.job_parts[self.by_shape.len()];
                let Some(vs) = variants(lines, &self.set, self.cell, self.grow, &halted) else {
                    self.finish();
                    break;
                };
                let vs = std::rc::Rc::new(vs);
                let area = vs.first().map(|v| v.raw.area_cells).unwrap_or(0);
                self.by_shape.push((*shape, vs.clone(), area));
                for copy in 0..self.set.copies.max(1) {
                    self.parts.push(Part { shape: *shape, copy, variants: vs.clone(), area_cells: area });
                }
                if self.by_shape.len() == self.job_parts.len() {
                    self.order = (0..self.parts.len()).collect();
                    self.order.sort_by_key(|&i| std::cmp::Reverse(self.parts[i].area_cells));
                    self.cur = Some(self.new_attempt());
                }
            } else {
                // 2. place the next part of the current layout
                let mut a = self.cur.take().unwrap_or_else(|| self.new_attempt());
                let res = if a.next < self.order.len() {
                    let p = &self.parts[self.order[a.next]];
                    a.next += 1;
                    self.place(&mut a, p.shape, p.copy, &p.variants, &halted).map(|_| false)
                } else if self.set.fill && a.placed.len() < FILL_LIMIT && !self.by_shape.is_empty() {
                    let (shape, vs, _) = &self.by_shape[a.fill_k];
                    let (shape, vs) = (*shape, vs.clone());
                    let copy = a.fill_copy;
                    let r = self.place(&mut a, shape, copy, &vs, &halted);
                    if let Ok(true) = r {
                        a.fill_any = true;
                    }
                    a.fill_k += 1;
                    let mut finished = false;
                    if a.fill_k == self.by_shape.len() {
                        a.fill_k = 0;
                        a.fill_copy += 1;
                        finished = !std::mem::take(&mut a.fill_any);
                    }
                    r.map(|_| finished)
                } else {
                    Ok(true)
                };
                match res {
                    Err(Halted) => {
                        self.cur = Some(a);
                        self.finish();
                        break;
                    }
                    Ok(false) => self.cur = Some(a),
                    Ok(true) => {
                        // a layout is finished: keep it if it's the best, then try another order
                        self.attempts += 1;
                        self.keep(a);
                        if self.parts.len() <= 1 && !self.set.fill {
                            self.done = true;
                            break;
                        }
                        self.rng.shuffle(&mut self.order);
                        // keep big parts early most of the time
                        if self.attempts % 2 == 0 {
                            let parts = &self.parts;
                            let rng = &mut self.rng;
                            self.order.sort_by_cached_key(|&i| std::cmp::Reverse(parts[i].area_cells / 4 + (rng.next() % 3) as usize));
                        }
                        self.cur = Some(self.new_attempt());
                    }
                }
            }
            if now_secs() >= slice_end {
                break;
            }
        }
        self.done
    }

    pub fn result(&self) -> NestResult {
        let (n, area, placed) = self.best.as_ref().map(|b| (b.0, b.1, b.3.clone())).unwrap_or((0, 0, Vec::new()));
        let total = if self.set.fill { n } else { self.job_parts.len() * self.set.copies.max(1) as usize };
        NestResult {
            not_placed: total.saturating_sub(n),
            used_area: area as f64 * self.cell * self.cell,
            sheet_area: self.usable as f64 * self.cell * self.cell,
            placed,
            attempts: self.attempts,
            partial: self.partial,
        }
    }
}

/// Run the nest to the end. `stop` ends it early (the best layout so far is kept); `progress` is told how
/// far it has got now and then.
pub fn run(job: &Job, stop: &AtomicBool, progress: &dyn Fn(Progress)) -> NestResult {
    let mut n = Nester::new(job);
    while !n.step(stop, now_secs() + 0.05) {
        progress(n.progress());
    }
    n.result()
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

/// Run on a background thread (desktop); the browser works a slice of each frame (`tick`).
pub mod running {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    pub struct Running {
        pub progress: Arc<Mutex<Progress>>,
        pub stop: Arc<AtomicBool>,
        pub result: Arc<Mutex<Option<NestResult>>>,
        /// The document revision it was started for.
        pub rev: u64,
        #[cfg(target_arch = "wasm32")]
        nester: std::rc::Rc<std::cell::RefCell<Nester>>,
    }

    impl Running {
        pub fn progress(&self) -> Progress {
            self.progress.lock().map(|p| *p).unwrap_or_default()
        }
        /// Is Stop pressed but the nest not finished yet?
        pub fn stopping(&self) -> bool {
            self.stop.load(Ordering::Relaxed)
        }
        /// The browser: work for a few milliseconds each frame, so the page stays responsive. (Desktop: nothing
        /// to do, the work runs on its own thread.)
        pub fn tick(&self) {
            #[cfg(target_arch = "wasm32")]
            {
                let mut n = self.nester.borrow_mut();
                if n.step(&self.stop, now_secs() + 0.03) {
                    *self.result.lock().unwrap() = Some(n.result());
                } else {
                    *self.progress.lock().unwrap() = n.progress();
                }
            }
        }
    }

    pub fn start(job: Job, rev: u64) -> Running {
        let progress = Arc::new(Mutex::new(Progress::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (p, s, r) = (progress.clone(), stop.clone(), result.clone());
            std::thread::spawn(move || {
                let res = run(&job, &s, &|v| *p.lock().unwrap() = v);
                *r.lock().unwrap() = Some(res);
            });
            Running { progress, stop, result, rev }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let nester = std::rc::Rc::new(std::cell::RefCell::new(Nester::new(&job)));
            Running { progress, stop, result, rev, nester }
        }
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

    /// Many detailed parts, every 15° with mirroring, Fine, fill the sheet: far more work than fits in the
    /// time, so it must stop on time (or on Stop) and still give a valid layout.
    fn slow_job() -> (Document, NestSettings, Job) {
        let mut doc = Document::default();
        for i in 0..40 {
            let kind = if i % 2 == 0 { ShapeKind::Ellipse { rx: 9.0 + (i % 5) as f64, ry: 4.0 + (i % 3) as f64 } } else { ShapeKind::Polygon { n: 7 + i % 5, rx: 8.0, ry: 6.0 } };
            doc.shapes.push(Shape::new(kind, Xform::translate(600.0 + i as f64, 600.0), 0));
        }
        let set = NestSettings { spacing: 1.0, margin: 1.0, rotations: Rotations::Every, step: 15.0, mirror: true, fine: true, fill: true, time_limit: 1.0, ..Default::default() };
        let sel: Vec<usize> = (0..doc.shapes.len()).collect();
        let job = prepare(&doc, &sel, 400.0, 400.0, &set).unwrap();
        (doc, set, job)
    }

    fn valid(doc: &Document, set: &NestSettings, job: &Job, r: &NestResult) {
        assert!(!r.placed.is_empty(), "something was placed");
        // check the first few parts exactly (all of them would take too long here)
        let mut d2 = doc.clone();
        let moved = apply(&mut d2, &NestResult { placed: r.placed.iter().take(12).cloned().collect(), ..Default::default() });
        check(&d2, set, &job.sheet, &moved);
        for p in &r.placed {
            assert!((p.angle / 15.0).fract().abs() < 1e-9);
        }
    }

    #[test]
    fn a_slow_nest_stops_at_the_time_limit() {
        let (doc, set, job) = slow_job();
        let t = std::time::Instant::now();
        let r = run(&job, &AtomicBool::new(false), &|_| {});
        let took = t.elapsed().as_secs_f64();
        assert!(r.placed.len() >= 10, "only {} placed in {took:.2} s", r.placed.len());
        assert!(took < set.time_limit + 0.5, "took {took:.2} s with a {} s limit", set.time_limit);
        valid(&doc, &set, &job, &r);
    }

    #[test]
    fn a_slow_nest_stops_when_stop_is_pressed() {
        let (doc, mut set, _) = slow_job();
        set.time_limit = 600.0;
        let sel: Vec<usize> = (0..doc.shapes.len()).collect();
        let job = prepare(&doc, &sel, 400.0, 400.0, &set).unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let presser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800));
            s2.store(true, Ordering::Relaxed);
            std::time::Instant::now()
        });
        let r = run(&job, &stop, &|_| {});
        let ended = std::time::Instant::now();
        let pressed = presser.join().unwrap();
        let after = ended.saturating_duration_since(pressed).as_secs_f64();
        assert!(after < 0.5, "kept going {after:.2} s after Stop");
        valid(&doc, &set, &job, &r);
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
