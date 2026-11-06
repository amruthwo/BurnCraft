//! G-code generation for GRBL-style lasers, plus a toolpath for the preview window.

use crate::geom::*;
use crate::imaging;
use crate::model::*;

/// The kind of controller in the laser. BurnCraft drives the same G-code family LightBurn supports
/// (GRBL, GRBL-M3, GRBL-LPC, Smoothieware, Marlin, RepRap, LinuxCNC and custom G-code).
/// DSP and galvo controllers (Ruida, Trocen, TopWisdom, EZCad, JCZ) speak their own binary protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    Grbl,
    GrblM3,
    GrblLpc,
    FluidNc,
    Smoothie,
    Marlin,
    RepRap,
    LinuxCnc,
    Custom,
    /// DSP controllers (LightBurn's "dsp controller type").
    Ruida,
    Trocen,
    TopWisdom,
    /// Galvo controllers (LightBurn's "galvo controller type").
    EzCad2,
    EzCad3,
    Bsl,
}

/// LightBurn's device families (strings "G-code", "dsp controller type", "galvo controller type").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    GCode,
    Dsp,
    Galvo,
}

/// How far BurnCraft can drive a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Serial/USB streaming and saved G-code files.
    Full,
    /// Saved job files and sending over the network (Ruida).
    FileAndNetwork,
    /// Listed so you know where it stands; its protocol isn't implemented yet.
    NotYet,
}

impl Controller {
    pub const ALL: [Controller; 15] = [
        Controller::Grbl,
        Controller::GrblM3,
        Controller::GrblLpc,
        Controller::FluidNc,
        Controller::Smoothie,
        Controller::Marlin,
        Controller::RepRap,
        Controller::LinuxCnc,
        Controller::Custom,
        Controller::Ruida,
        Controller::Trocen,
        Controller::TopWisdom,
        Controller::EzCad2,
        Controller::EzCad3,
        Controller::Bsl,
    ];
    pub fn family(self) -> Family {
        match self {
            Controller::Ruida | Controller::Trocen | Controller::TopWisdom => Family::Dsp,
            Controller::EzCad2 | Controller::EzCad3 | Controller::Bsl => Family::Galvo,
            _ => Family::GCode,
        }
    }
    pub fn support(self) -> Support {
        match self {
            Controller::Ruida => Support::FileAndNetwork,
            Controller::Trocen | Controller::TopWisdom | Controller::EzCad2 | Controller::EzCad3 | Controller::Bsl => Support::NotYet,
            _ => Support::Full,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Controller::Grbl => "GRBL",
            Controller::GrblM3 => "GRBL-M3 (constant power)",
            Controller::GrblLpc => "GRBL-LPC",
            Controller::FluidNc => "FluidNC / ESP32 GRBL",
            Controller::Smoothie => "Smoothieware",
            Controller::Marlin => "Marlin",
            Controller::RepRap => "RepRapFirmware (Duet)",
            Controller::LinuxCnc => "LinuxCNC",
            Controller::Custom => "Custom G-code",
            Controller::Ruida => "Ruida (RDC644x, 6445...)",
            Controller::Trocen => "Trocen (AWC)",
            Controller::TopWisdom => "TopWisdom",
            Controller::EzCad2 => "EZCad2 / JCZ (fiber, UV, CO2 galvo)",
            Controller::EzCad3 => "EZCad3",
            Controller::Bsl => "BSL galvo",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Controller::Grbl => "grbl",
            Controller::GrblM3 => "grbl-m3",
            Controller::GrblLpc => "grbl-lpc",
            Controller::FluidNc => "fluidnc",
            Controller::Smoothie => "smoothie",
            Controller::Marlin => "marlin",
            Controller::RepRap => "reprap",
            Controller::LinuxCnc => "linuxcnc",
            Controller::Custom => "custom",
            Controller::Ruida => "ruida",
            Controller::Trocen => "trocen",
            Controller::TopWisdom => "topwisdom",
            Controller::EzCad2 => "ezcad2",
            Controller::EzCad3 => "ezcad3",
            Controller::Bsl => "bsl",
        }
    }
    pub fn from_id(s: &str) -> Controller {
        Controller::ALL.into_iter().find(|c| c.id() == s).unwrap_or(Controller::Grbl)
    }
    /// From a LightBurn device "Type" string.
    pub fn from_lightburn(kind: &str) -> Controller {
        let k = kind.to_ascii_lowercase();
        if k.contains("ruida") {
            Controller::Ruida
        } else if k.contains("trocen") {
            Controller::Trocen
        } else if k.contains("topwisdom") {
            Controller::TopWisdom
        } else if k.contains("ezcad3") {
            Controller::EzCad3
        } else if k.contains("jcz") || k.contains("ezcad") {
            Controller::EzCad2
        } else if k.contains("bsl") {
            Controller::Bsl
        } else if k.contains("m3") {
            Controller::GrblM3
        } else if k.contains("lpc") {
            Controller::GrblLpc
        } else if k.contains("smoothie") {
            Controller::Smoothie
        } else if k.contains("marlin") {
            Controller::Marlin
        } else if k.contains("reprap") || k.contains("rrf") || k.contains("duet") {
            Controller::RepRap
        } else if k.contains("linuxcnc") {
            Controller::LinuxCnc
        } else if k.contains("fluidnc") {
            Controller::FluidNc
        } else {
            Controller::Grbl
        }
    }
    /// Highest S value: GRBL family 1000 (GRBL's `$30` default); Marlin 255; Smoothieware 0–1.
    pub fn default_s_max(self) -> f64 {
        match self {
            Controller::Marlin | Controller::RepRap => 255.0,
            Controller::Smoothie => 1.0,
            _ => 1000.0,
        }
    }
    /// Serial speed: 115200 (GRBL's default); Marlin 250000 (Marlin's usual `BAUDRATE`).
    pub fn default_baud(self) -> u32 {
        match self {
            Controller::Marlin => 250000,
            _ => 115200,
        }
    }
    /// Can it lower power in corners (M4)?
    pub fn has_dynamic(self) -> bool {
        matches!(self, Controller::Grbl | Controller::GrblLpc | Controller::FluidNc | Controller::Custom | Controller::Marlin)
    }
    /// Lines at the start of a job, after units/absolute mode.
    fn header(self) -> &'static str {
        match self {
            // RepRapFirmware: laser mode
            Controller::RepRap => "M452\n",
            _ => "",
        }
    }
}

/// Cut optimisation options and their defaults (the same out-of-the-box choices as LightBurn's Optimization
/// Settings): order by layer first, then by priority; grouping off; inner shapes before outer; reduce travel on;
/// allow reversing on; optimal entry point off; remove overlaps off (overlap distance 0.1 mm); hide backlash off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutOptimize {
    /// Cut shapes inside other shapes first, so parts don't move before their holes are cut.
    pub inner_first: bool,
    /// Go to the nearest next shape instead of following the shape list.
    pub reduce_travel: bool,
    /// Open lines may be cut from either end ("Choose Best Direction").
    pub best_direction: bool,
    /// Closed shapes start at the corner nearest the head ("Choose Best Starting Point").
    pub best_start: bool,
}

impl Default for CutOptimize {
    fn default() -> Self {
        CutOptimize { inner_first: true, reduce_travel: true, best_direction: true, best_start: false }
    }
}

#[derive(Clone, Debug)]
pub struct Device {
    pub controller: Controller,
    /// Network address of a Ruida (or other network) controller.
    pub ip: String,
    pub opt: CutOptimize,
    pub name: String,
    pub width: f64,
    pub height: f64,
    /// Highest S value the controller accepts (GRBL $30). Default 1000.
    pub s_max: f64,
    pub rapid_speed: f64,
    pub baud: u32,
    pub port: String,
    pub dynamic_power: bool,
}

impl Default for Device {
    fn default() -> Self {
        Device { controller: Controller::Grbl, ip: String::new(), opt: CutOptimize::default(), name: "GRBL laser".into(), width: LBDEV_DEFAULT_WIDTH, height: LBDEV_DEFAULT_HEIGHT, s_max: 1000.0, rapid_speed: 100.0, baud: 115200, port: String::new(), dynamic_power: true }
    }
}

/// LightBurn device profile (.lbdev): a JSON object whose "DeviceList" array holds one object per laser with
/// "Name", "Type", "Width", "Height" (300 × 200 mm when missing), "MirrorX", "MirrorY", "HomeOnStartup",
/// "Settings", "CutList"... Returns the devices BurnCraft can drive (G-code controllers), plus a note for each one it can't.
pub const LBDEV_DEFAULT_WIDTH: f64 = 300.0;
pub const LBDEV_DEFAULT_HEIGHT: f64 = 200.0;

pub fn devices_from_lbdev(text: &str) -> Result<(Vec<Device>, Vec<String>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not a device profile ({e})"))?;
    let list = v.get("DeviceList").and_then(|l| l.as_array()).ok_or("no \"DeviceList\" in this file")?;
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    for d in list {
        let name = d.get("Name").and_then(|x| x.as_str()).unwrap_or("Laser").to_string();
        let kind = d.get("Type").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let num = |k: &str, def: f64| d.get(k).and_then(|x| x.as_f64()).filter(|x| *x > 0.0).unwrap_or(def);
        // BurnCraft speaks G-code (GRBL and friends); DSP / galvo controllers use their own protocols
        if Controller::from_lightburn(&kind).support() == Support::NotYet {
            notes.push(format!("{name} ({kind}) uses a controller BurnCraft can't drive yet; its size was still read."));
        }
        let controller = Controller::from_lightburn(&kind);
        let mut dev = Device { name, width: num("Width", LBDEV_DEFAULT_WIDTH), height: num("Height", LBDEV_DEFAULT_HEIGHT), controller, s_max: controller.default_s_max(), baud: controller.default_baud(), dynamic_power: controller.has_dynamic(), ..Device::default() };
        if let Some(s) = d.get("Settings") {
            let pick = |keys: &[&str]| keys.iter().find_map(|k| s.get(*k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|t| t.parse().ok()))));
            if let Some(smax) = pick(&["MaxSValue", "SMax", "MaxS"]) {
                dev.s_max = smax;
            }
            if let Some(baud) = pick(&["BaudRate", "Baud"]) {
                dev.baud = baud as u32;
            }
        }
        out.push(dev);
    }
    if out.is_empty() {
        return Err("the device list in this file is empty".into());
    }
    Ok((out, notes))
}

#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub from: P2,
    pub to: P2,
    /// 0..1 of the device's power; 0 = travel.
    pub power: f32,
    pub layer: usize,
    pub speed: f64,
}

#[derive(Default, Clone)]
pub struct Job {
    pub moves: Vec<Move>,
    pub gcode: String,
    pub cut_length: f64,
    pub travel_length: f64,
    pub seconds: f64,
    pub cut_seconds: f64,
    pub travel_seconds: f64,
    /// Shapes left out because they are outside (or hang over the edge of) the work area.
    pub skipped_outside: usize,
}

struct Emitter<'a> {
    dev: &'a Device,
    out: String,
    pos: P2,
    moves: Vec<Move>,
    cut_len: f64,
    travel_len: f64,
    secs: f64,
    cut_secs: f64,
    travel_secs: f64,
    last_f: f64,
    last_s: f64,
}

impl<'a> Emitter<'a> {
    fn travel(&mut self, to: P2) {
        if self.pos.dist(to) < MIN_MOVE {
            return;
        }
        self.out.push_str(&format!("G0 X{} Y{}\n", f3(to.x), f3(to.y)));
        let d = self.pos.dist(to);
        self.travel_len += d;
        self.secs += d / self.dev.rapid_speed.max(1.0);
        self.travel_secs += d / self.dev.rapid_speed.max(1.0);
        self.moves.push(Move { from: self.pos, to, power: 0.0, layer: 0, speed: self.dev.rapid_speed });
        self.pos = to;
    }
    fn cut(&mut self, to: P2, power_pct: f64, speed: f64, layer: usize) {
        if self.pos.dist(to) < MIN_MOVE {
            return;
        }
        let s = (power_pct / 100.0 * self.dev.s_max).clamp(0.0, self.dev.s_max);
        let f = speed * FEED_PER_MINUTE;
        let mut line = format!("G1 X{} Y{}", f3(to.x), f3(to.y));
        if (s - self.last_s).abs() > 1e-6 {
            // 0–1 controllers (Smoothieware) need more decimals than 0–255 / 0–1000 ones
            let k = if self.dev.s_max <= 1.0 { 1000.0 } else { 10.0 };
            line.push_str(&format!(" S{}", fmt_num((s * k).round() / k)));
            self.last_s = s;
        }
        if (f - self.last_f).abs() > 1e-6 {
            line.push_str(&format!(" F{}", fmt_num((f * 100.0).round() / 100.0)));
            self.last_f = f;
        }
        line.push('\n');
        self.out.push_str(&line);
        let d = self.pos.dist(to);
        self.cut_len += d;
        self.secs += d / speed.max(0.1);
        self.cut_secs += d / speed.max(0.1);
        self.moves.push(Move { from: self.pos, to, power: (power_pct / 100.0) as f32, layer, speed });
        self.pos = to;
    }
}

fn f3(v: f64) -> String {
    fmt_num((v * 1000.0).round() / 1000.0)
}

/// Build the job. `only_selected`, when given, limits output to those top-level shapes.
/// Feed rate: speed in mm/s × 60 = mm/min, written with 2 decimals (in inch mode it is further ÷ 25.4).
pub const FEED_PER_MINUTE: f64 = 60.0;
/// Moves shorter than this (mm) are left out.
pub const MIN_MOVE: f64 = 0.0025;

/// Job header pieces (standard G-code modal commands).
pub const HEADER_MODES: &str = "G00 G17 G40 ";
pub const HEADER_WORK: &str = "G54";
pub const ABSOLUTE: &str = "G90";

/// The laser-on command for a layer: "M4" (GRBL dynamic power) when the laser runs in dynamic mode and the layer
/// doesn't force constant power, otherwise "M3" (constant power). Off is "M5". See GRBL's laser mode documentation.
pub fn laser_on(dev: &Device, cs: &CutSetting) -> &'static str {
    if dev.dynamic_power && dev.controller.has_dynamic() && !cs.force_constant_power {
        "M4"
    } else {
        "M3"
    }
}
pub const LASER_OFF: &str = "M5";

pub fn build_job(doc: &Document, dev: &Device, only: Option<&[usize]>) -> Job {
    let mut em = Emitter { dev, out: String::new(), pos: P2::default(), moves: Vec::new(), cut_len: 0.0, travel_len: 0.0, secs: 0.0, cut_secs: 0.0, travel_secs: 0.0, last_f: -1.0, last_s: -1.0 };
    em.out.push_str(&format!("; BurnCraft {} - GRBL G-code\n; Device: {} ({} x {} mm)\n", env!("CARGO_PKG_VERSION"), dev.name, dev.width, dev.height));
    // Header: "G00 G17 G40 " (rapid, XY plane, no cutter compensation) + G21 (G20 in inches) + G54,
    // then G90 for absolute coordinates
    em.out.push_str(&format!("{HEADER_MODES}G21 {HEADER_WORK}\n{ABSOLUTE}\n"));
    em.out.push_str(&format!("; Controller: {}\n", dev.controller.label()));
    em.out.push_str(dev.controller.header());

    let burns = |layer: usize| doc.cut(layer).map(|c| c.output).unwrap_or(layer < COLOR_LAYERS);
    let mut leaves: Vec<(Shape, Xform)> = Vec::new();
    // Only burn what fits on the bed: anything outside or over the edge could crash the head.
    let bed = bed_box(dev);
    let mut skipped_outside = 0;
    for (i, s) in doc.shapes.iter().enumerate() {
        if only.map(|o| o.contains(&i)).unwrap_or(true) {
            let mut mine = Vec::new();
            s.leaves(Xform::IDENTITY, &mut mine);
            let before = mine.iter().filter(|(l, _)| burns(l.layer)).count();
            mine.retain(|(l, x)| {
                let mut probe = l.clone();
                probe.xform = *x;
                bed.contains_box(&probe.bbox())
            });
            // count whole shapes (a group of 3000 pieces is one shape), and only ones that would burn
            if mine.iter().filter(|(l, _)| burns(l.layer)).count() < before {
                skipped_outside += 1;
            }
            leaves.extend(mine);
        }
    }
    if skipped_outside > 0 {
        em.out.push_str(&format!("; {skipped_outside} shape(s) outside the work area were left out\n"));
    }
    let mut cuts: Vec<CutSetting> = doc.cuts.iter().filter(|c| c.output).cloned().collect();
    cuts.sort_by_key(|c| (c.priority, c.index));
    for cs in &cuts {
        let on: Vec<&(Shape, Xform)> = leaves.iter().filter(|(s, _)| s.layer == cs.index).collect();
        if on.is_empty() {
            continue;
        }
        em.out.push_str(&format!("; Layer {} ({}) - {}, {} mm/s, {}%\n", cs.name, layer_name(cs.index), cs.mode.label(), fmt_num(cs.speed), fmt_num(cs.max_power)));
        em.out.push_str(laser_on(dev, cs));
        em.out.push('\n');
        if cs.air_assist {
            em.out.push_str("M8\n");
        }
        for _pass in 0..cs.passes.max(1) {
            match cs.mode {
                CutMode::Line => emit_lines(&mut em, &on, cs),
                CutMode::Fill | CutMode::OffsetFill => emit_fill(&mut em, &on, cs),
                CutMode::FillLine => {
                    emit_fill(&mut em, &on, cs);
                    emit_lines(&mut em, &on, cs);
                }
                CutMode::Image => emit_image(&mut em, &on, cs),
            }
        }
        if cs.air_assist {
            em.out.push_str("M9\n");
        }
    }
    em.out.push_str(&format!("G1 S0\n{LASER_OFF}\nG0 X0 Y0\nM2\n"));
    Job { moves: em.moves, gcode: em.out, cut_length: em.cut_len, travel_length: em.travel_len, seconds: em.secs, cut_seconds: em.cut_secs, travel_seconds: em.travel_secs, skipped_outside }
}

fn vector_lines(on: &[&(Shape, Xform)]) -> Vec<Vec<P2>> {
    let mut lines = Vec::new();
    for (s, x) in on {
        if s.is_bitmap() {
            continue;
        }
        let tol = 0.02 / x.mean_scale();
        for l in s.local_outline(tol) {
            lines.push(l.into_iter().map(|p| x.apply(p)).collect::<Vec<P2>>());
        }
    }
    lines
}

/// For each closed line, the closed lines inside it (by bounding box and a point test).
fn containment(lines: &[Vec<P2>]) -> Vec<Vec<usize>> {
    let boxes: Vec<BBox> = lines.iter().map(|l| BBox::from_points(l)).collect();
    let mut inside = vec![Vec::new(); lines.len()];
    if lines.len() > 4000 {
        return inside;
    }
    for (i, a) in lines.iter().enumerate() {
        if !is_closed(a) {
            continue;
        }
        for (j, b) in lines.iter().enumerate() {
            if i != j && boxes[i].contains_box(&boxes[j]) && boxes[i] != boxes[j] && point_in_rings(b[0], std::slice::from_ref(a)) {
                inside[i].push(j);
            }
        }
    }
    inside
}

fn emit_lines(em: &mut Emitter, on: &[&(Shape, Xform)], cs: &CutSetting) {
    let lines = vector_lines(on);
    let o = em.dev.opt;
    // a shape waits until everything inside it has been cut ("Cut Inner Shapes First")
    let inside = if o.inner_first { containment(&lines) } else { vec![Vec::new(); lines.len()] };
    let mut done = vec![false; lines.len()];
    let mut slots: Vec<Option<Vec<P2>>> = lines.into_iter().map(Some).collect();
    for _ in 0..slots.len() {
        let pos = em.pos;
        let ready = |i: usize| slots[i].is_some() && inside[i].iter().all(|j| done[*j]);
        let (mut best, mut rev, mut start, mut bd) = (usize::MAX, false, 0usize, f64::INFINITY);
        for (i, l) in slots.iter().enumerate() {
            let Some(l) = l else { continue };
            if !ready(i) {
                continue;
            }
            if !o.reduce_travel {
                // shape-list order: the first one that's ready
                (best, rev, start) = (i, false, 0);
                break;
            }
            let closed = is_closed(l);
            if closed && o.best_start {
                for (k, p) in l[..l.len() - 1].iter().enumerate() {
                    let d = p.dist(pos);
                    if d < bd {
                        (best, rev, start, bd) = (i, false, k, d);
                    }
                }
                continue;
            }
            let d0 = l[0].dist(pos);
            if d0 < bd {
                (best, rev, start, bd) = (i, false, 0, d0);
            }
            if !closed && o.best_direction {
                let d1 = l.last().unwrap().dist(pos);
                if d1 < bd {
                    (best, rev, start, bd) = (i, true, 0, d1);
                }
            }
        }
        if best == usize::MAX {
            // nothing is ready (shouldn't happen): take any remaining line
            match slots.iter().position(|s| s.is_some()) {
                Some(i) => best = i,
                None => break,
            }
        }
        let mut l = slots[best].take().unwrap();
        done[best] = true;
        if start > 0 {
            // a closed loop starting at another corner
            l.pop();
            l.rotate_left(start);
            l.push(l[0]);
        }
        if rev {
            l.reverse();
        }
        em.travel(l[0]);
        for p in &l[1..] {
            em.cut(*p, cs.max_power, cs.speed, cs.index);
        }
    }
}

/// Scanline fill of all closed outlines (even-odd), at `interval` spacing and `angle` degrees.
pub fn fill_segments(rings: &[Vec<P2>], interval: f64, angle_deg: f64) -> Vec<Vec<(P2, P2)>> {
    let rot = Xform::rotate(-angle_deg.to_radians());
    let back = Xform::rotate(angle_deg.to_radians());
    let rr: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|p| rot.apply(*p)).collect()).collect();
    let bb = rr.iter().fold(BBox::EMPTY, |b, r| b.union(&BBox::from_points(r)));
    if bb.is_empty() || interval <= 0.0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut y = bb.min.y + interval * 0.5;
    while y < bb.max.y {
        let mut xs: Vec<f64> = Vec::new();
        for r in &rr {
            for w in r.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let row: Vec<(P2, P2)> = xs.chunks(2).filter(|c| c.len() == 2).map(|c| (back.apply(p2(c[0], y)), back.apply(p2(c[1], y)))).collect();
        if !row.is_empty() {
            rows.push(row);
        }
        y += interval;
        if rows.len() > 200_000 {
            break;
        }
    }
    rows
}

pub fn bed_box(dev: &Device) -> BBox {
    BBox { min: p2(-1e-6, -1e-6), max: p2(dev.width + 1e-6, dev.height + 1e-6) }
}

/// Split closed outlines into islands: an outline with the holes directly inside it.
/// Nesting alternates (even-odd): a shape inside a hole is its own island.
pub fn islands(rings: &[Vec<P2>]) -> Vec<Vec<Vec<P2>>> {
    crate::ops::to_multipolygon(rings).0.iter().map(|poly| crate::ops::from_multipolygon(&geo::MultiPolygon(vec![poly.clone()]))).collect()
}

fn emit_fill(em: &mut Emitter, on: &[&(Shape, Xform)], cs: &CutSetting) {
    let rings: Vec<Vec<P2>> = vector_lines(on).into_iter().filter(|l| is_closed(l)).collect();
    let groups: Vec<Vec<Vec<P2>>> = if cs.fill_individually { islands(&rings) } else { vec![rings] };
    let mut angles = vec![cs.angle];
    if cs.cross_hatch {
        angles.push(cs.angle + 90.0);
    }
    for group in &groups {
        for ang in &angles {
            let rows = fill_segments(group, cs.interval.max(0.01), *ang);
            for (ri, row) in rows.into_iter().enumerate() {
                let flip = cs.bidir && ri % 2 == 1;
                let segs: Vec<(P2, P2)> = if flip { row.into_iter().rev().map(|(a, b)| (b, a)).collect() } else { row };
                for (a, b) in segs {
                    em.travel(a);
                    em.cut(b, cs.max_power, cs.speed, cs.index);
                }
            }
        }
    }
}

fn emit_image(em: &mut Emitter, on: &[&(Shape, Xform)], cs: &CutSetting) {
    for (s, x) in on {
        let ShapeKind::Bitmap { w, h, data, adjust } = &s.kind else { continue };
        let gray = imaging::adjusted_gray(data, adjust);
        let dith = imaging::dither(&gray, data.width as usize, data.height as usize, adjust.dither);
        let Some(inv) = x.inverse() else { continue };
        let corners = [p2(-w / 2.0, -h / 2.0), p2(w / 2.0, -h / 2.0), p2(w / 2.0, h / 2.0), p2(-w / 2.0, h / 2.0)];
        let bb = BBox::from_points(&corners.map(|c| x.apply(c)));
        let step = cs.interval.max(0.02);
        let sample = |p: P2| -> Option<u8> {
            let l = inv.apply(p);
            let u = (l.x + w / 2.0) / w;
            let v = (h / 2.0 - l.y) / h;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                return None;
            }
            let px = (u * data.width as f64) as usize;
            let py = (v * data.height as f64) as usize;
            Some(dith[py * data.width as usize + px])
        };
        let mut row = 0usize;
        let mut y = bb.max.y - step * 0.5;
        while y > bb.min.y {
            let ltr = !cs.bidir || row % 2 == 0;
            let n = ((bb.width() / step).ceil() as usize).max(1);
            let xs: Vec<f64> = (0..=n).map(|i| bb.min.x + i as f64 * step).collect();
            let order: Vec<usize> = if ltr { (0..n).collect() } else { (0..n).rev().collect() };
            // runs of equal power
            let mut run: Option<(P2, f64)> = None;
            for i in order {
                let (xa, xb) = if ltr { (xs[i], xs[i + 1]) } else { (xs[i + 1], xs[i]) };
                let mid = p2((xa + xb) / 2.0, y);
                let pw = sample(mid).map(|g| cs.min_power + (cs.max_power - cs.min_power) * (1.0 - g as f64 / 255.0)).filter(|p| *p > 0.01);
                match (run, pw) {
                    (Some((st, rp)), Some(p)) if (rp - p).abs() < 0.5 => {
                        run = Some((st, rp));
                    }
                    (prev, cur) => {
                        if let Some((st, rp)) = prev {
                            em.travel(st);
                            em.cut(p2(xa, y), rp, cs.speed, cs.index);
                        }
                        run = cur.map(|p| (p2(xa, y), p));
                    }
                }
                if i == if ltr { n - 1 } else { 0 } {
                    if let Some((st, rp)) = run.take() {
                        em.travel(st);
                        em.cut(p2(xb, y), rp, cs.speed, cs.index);
                    }
                }
            }
            row += 1;
            y -= step;
            if row > 100_000 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn square_fill_rows() {
        let sq = vec![p2(0.0, 0.0), p2(10.0, 0.0), p2(10.0, 10.0), p2(0.0, 10.0), p2(0.0, 0.0)];
        let rows = fill_segments(&[sq], 1.0, 0.0);
        assert_eq!(rows.len(), 10);
        assert!((rows[0][0].1.x - rows[0][0].0.x - 10.0).abs() < 1e-9);
    }
    #[test]
    fn line_job() {
        let mut doc = Document::default();
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 0));
        doc.sync_cuts();
        let job = build_job(&doc, &Device::default(), None);
        assert!(job.gcode.contains("G1"));
        assert!((job.cut_length - 40.0).abs() < 1e-6);
        assert!(job.gcode.contains("S200"));
        assert!(job.gcode.contains("F6000"));
    }
}

#[cfg(test)]
mod lbdev_tests {
    use super::*;
    #[test]
    fn reads_a_lightburn_device_profile() {
        let text = r#"{"DeviceList":[{"Name":"My Laser","Type":"GRBL","Width":410,"Height":400,"Settings":{"MaxSValue":"1000"}},{"Name":"Galvo","Type":"EzCad2"}]}"#;
        let (devs, notes) = devices_from_lbdev(text).unwrap();
        assert_eq!(devs.len(), 2);
        assert_eq!((devs[0].name.as_str(), devs[0].width, devs[0].height), ("My Laser", 410.0, 400.0));
        assert_eq!((devs[1].width, devs[1].height), (LBDEV_DEFAULT_WIDTH, LBDEV_DEFAULT_HEIGHT));
        assert_eq!(notes.len(), 1);
        assert!(devices_from_lbdev("{}").is_err());
    }
}

#[cfg(test)]
mod laser_on_tests {
    use super::*;
    #[test]
    fn m4_unless_the_layer_forces_constant_power() {
        let dev = Device::default();
        let mut cs = CutSetting::new(0);
        assert_eq!(laser_on(&dev, &cs), "M4");
        cs.force_constant_power = true;
        assert_eq!(laser_on(&dev, &cs), "M3");
        let m3 = Device { controller: Controller::GrblM3, ..Device::default() };
        cs.force_constant_power = false;
        assert_eq!(laser_on(&m3, &cs), "M3");
        let mut d = Document::default();
        d.shapes.push(Shape::new(ShapeKind::Rect { w: 5.0, h: 5.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 0));
        d.sync_cuts();
        let job = build_job(&d, &dev, None);
        assert!(job.gcode.starts_with(';') && job.gcode.contains("G00 G17 G40 G21 G54\nG90\n") && job.gcode.contains("\nM4\n") && job.gcode.contains("\nM5\n"));
    }
}
