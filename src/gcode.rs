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
    /// K40 lasers with the Lihuiyu M2 Nano board (USB, see k40.rs).
    K40Nano,
}

/// LightBurn's device families (strings "G-code", "dsp controller type", "galvo controller type"), plus the
/// K40's Lihuiyu boards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    GCode,
    Dsp,
    Galvo,
    Lihuiyu,
}

/// How far BurnCraft can drive a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Serial/USB streaming and saved G-code files.
    Full,
    /// Saved job files and sending over the network (Ruida).
    FileAndNetwork,
    /// Its own USB protocol and saved .egv files (K40 M2 Nano).
    Usb,
    /// Listed so you know where it stands; its protocol isn't implemented yet.
    NotYet,
}

impl Controller {
    pub const ALL: [Controller; 16] = [
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
        Controller::K40Nano,
    ];
    pub fn family(self) -> Family {
        match self {
            Controller::Ruida | Controller::Trocen | Controller::TopWisdom => Family::Dsp,
            Controller::EzCad2 | Controller::EzCad3 | Controller::Bsl => Family::Galvo,
            Controller::K40Nano => Family::Lihuiyu,
            _ => Family::GCode,
        }
    }
    /// Driven over its own USB protocol (K40 M2 Nano, galvo) rather than G-code or the network.
    pub fn is_usb_laser(self) -> bool {
        matches!(self, Controller::K40Nano | Controller::EzCad2)
    }
    pub fn support(self) -> Support {
        match self {
            Controller::Ruida => Support::FileAndNetwork,
            Controller::K40Nano | Controller::EzCad2 => Support::Usb,
            Controller::Trocen | Controller::TopWisdom | Controller::EzCad3 | Controller::Bsl => Support::NotYet,
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
            Controller::K40Nano => "K40 (Lihuiyu M2 Nano)",
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
            Controller::K40Nano => "k40-nano",
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
        } else if k.contains("lihuiyu") || k.contains("nano") || k.contains("k40") {
            Controller::K40Nano
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
    /// K40 controller board model ("M2", "M1", "M", "B2", "B1", "B", "A"); it sets the speed codes.
    pub board: String,
    /// Galvo (JCZ / LMC) machine settings.
    pub galvo: crate::galvo::GalvoSettings,
    /// Rotary attachment (roller or chuck), when one is fitted.
    pub rotary: crate::rotary::RotarySettings,
}

impl Default for Device {
    fn default() -> Self {
        Device { controller: Controller::Grbl, ip: String::new(), opt: CutOptimize::default(), name: "GRBL laser".into(), width: LBDEV_DEFAULT_WIDTH, height: LBDEV_DEFAULT_HEIGHT, s_max: 1000.0, rapid_speed: 100.0, baud: 115200, port: String::new(), dynamic_power: true, board: "M2".into(), galvo: Default::default(), rotary: Default::default() }
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
        // a galvo's work area is its lens field: square, from the file's width when it has one
        if controller == Controller::EzCad2 {
            dev.galvo.lens = d.get("Width").and_then(|x| x.as_f64()).filter(|x| *x > 0.0).unwrap_or(dev.galvo.lens);
            dev.width = dev.galvo.lens;
            dev.height = dev.galvo.lens;
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
    /// Where Frame brings the head back to afterwards (the start point of a Current Position job).
    pub return_to: Option<P2>,
}

/// Where a job is placed on the laser ("Start from").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StartFrom {
    /// The design's own position: the bed's 0,0 is the design's 0,0.
    #[default]
    Absolute,
    /// The job's anchor point goes to a saved point on the bed.
    UserOrigin,
    /// The job's anchor point goes where the head is when the job starts.
    CurrentPosition,
}

impl StartFrom {
    pub const ALL: [StartFrom; 3] = [StartFrom::Absolute, StartFrom::UserOrigin, StartFrom::CurrentPosition];
    pub fn label(self) -> &'static str {
        match self {
            StartFrom::Absolute => "Absolute coordinates",
            StartFrom::UserOrigin => "User origin",
            StartFrom::CurrentPosition => "Current position",
        }
    }
}

/// The Laser panel's job placement choices, kept in the settings.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct JobStart {
    pub from: StartFrom,
    /// Which point of the job goes to the start point: [0 left, 1 centre, 2 right; 0 bottom, 1 middle, 2 top].
    pub anchor: [u8; 2],
    /// The user origin, in mm on the bed.
    pub user_origin: (f64, f64),
}

impl Default for JobStart {
    fn default() -> Self {
        JobStart { from: StartFrom::Absolute, anchor: [0, 0], user_origin: (0.0, 0.0) }
    }
}

/// Where and how one job is put on the laser (see `StartFrom`).
#[derive(Clone, Debug)]
pub struct Placement {
    /// Move the job so its anchor point lands here (laser coordinates). None: leave the design where it is.
    pub target: Option<P2>,
    pub anchor: [u8; 2],
    /// The work area in laser coordinates, or None when it isn't known (an unhomed laser with Current
    /// position): then nothing is left out for being off the bed, and the caller checks the job's size.
    pub bed: Option<BBox>,
    /// Where the head is when the job starts, and where it goes at the end.
    pub start: P2,
    pub end: P2,
    /// Lines before the job and after it (a saved Current position job makes its start point 0,0 with G92).
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub return_to: Option<P2>,
    /// Write every move relative to the last (G91): a saved file that starts wherever the head is, without
    /// changing the laser's coordinate settings.
    pub relative: bool,
}

impl Placement {
    pub fn absolute(dev: &Device) -> Placement {
        Placement { target: None, anchor: [0, 0], bed: Some(bed_box(dev)), start: P2::default(), end: P2::default(), before: Vec::new(), after: Vec::new(), return_to: None, relative: false }
    }
}

/// Rewrite a job's moves relative to the previous position (G91), starting from 0,0 (where the head is when
/// the file starts). The deltas come from the written (rounded) positions, so they add up exactly. The file
/// switches back to absolute (G90) at the end.
pub fn to_relative(gcode: &str) -> String {
    let mut out = String::with_capacity(gcode.len() + 64);
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
    for line in gcode.lines() {
        let t = line.trim();
        if t == ABSOLUTE {
            out.push_str("G91 ; every move is from where the head is: the job starts at the head's position\n");
            continue;
        }
        if t == "M2" {
            out.push_str(&format!("{ABSOLUTE}\nM2\n"));
            continue;
        }
        if t.starts_with("G0 ") || t.starts_with("G1 ") {
            let words: Vec<String> = t
                .split(' ')
                .map(|w| {
                    let (k, v) = w.split_at(1);
                    match (k, v.parse::<f64>()) {
                        ("X", Ok(nx)) => {
                            let d = r3(nx - x);
                            x = nx;
                            format!("X{}", fmt_num(d))
                        }
                        ("Y", Ok(ny)) => {
                            let d = r3(ny - y);
                            y = ny;
                            format!("Y{}", fmt_num(d))
                        }
                        _ => w.to_string(),
                    }
                })
                .collect();
            out.push_str(&words.join(" "));
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The point of a box that an anchor names ([0 left, 1 centre, 2 right; 0 bottom, 1 middle, 2 top]).
pub fn anchor_point(b: &BBox, anchor: [u8; 2]) -> P2 {
    let k = |lo: f64, hi: f64, a: u8| lo + (hi - lo) * a.min(2) as f64 / 2.0;
    p2(k(b.min.x, b.max.x, anchor[0]), k(b.min.y, b.max.y, anchor[1]))
}

struct Emitter<'a> {
    dev: &'a Device,
    /// The work area in job coordinates, when known (overscan stays inside it).
    bed: Option<BBox>,
    /// Tabs placed by hand, in job coordinates, with the layers of the shape they were placed on.
    tab_points: Vec<(P2, Vec<usize>)>,
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
    /// A laser-off move at burning speed (overscan, and gaps within a scan line), so the head doesn't stop
    /// and start between burns.
    fn coast(&mut self, to: P2, speed: f64) {
        if self.pos.dist(to) < MIN_MOVE {
            return;
        }
        let f = speed * FEED_PER_MINUTE;
        let mut line = format!("G1 X{} Y{} S0", f3(to.x), f3(to.y));
        self.last_s = 0.0;
        if (f - self.last_f).abs() > 1e-6 {
            line.push_str(&format!(" F{}", fmt_num((f * 100.0).round() / 100.0)));
            self.last_f = f;
        }
        line.push('\n');
        self.out.push_str(&line);
        let d = self.pos.dist(to);
        self.travel_len += d;
        self.secs += d / speed.max(0.1);
        self.travel_secs += d / speed.max(0.1);
        self.moves.push(Move { from: self.pos, to, power: 0.0, layer: 0, speed });
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
    build_job_placed(doc, dev, only, &Placement::absolute(dev))
}

/// Build the job, placed on the laser as `place` says (Start from).
pub fn build_job_placed(doc: &Document, dev: &Device, only: Option<&[usize]>, place: &Placement) -> Job {
    let mut em = Emitter { dev, bed: place.bed, tab_points: Vec::new(), out: String::new(), pos: place.start, moves: Vec::new(), cut_len: 0.0, travel_len: 0.0, secs: 0.0, cut_secs: 0.0, travel_secs: 0.0, last_f: -1.0, last_s: -1.0 };
    em.out.push_str(&format!("; BurnCraft {} - GRBL G-code\n; Device: {} ({} x {} mm)\n", env!("CARGO_PKG_VERSION"), dev.name, dev.width, dev.height));
    // Header: "G00 G17 G40 " (rapid, XY plane, no cutter compensation) + G21 (G20 in inches) + G54,
    // then G90 for absolute coordinates
    em.out.push_str(&format!("{HEADER_MODES}G21 {HEADER_WORK}\n{ABSOLUTE}\n"));
    em.out.push_str(&format!("; Controller: {}\n", dev.controller.label()));
    em.out.push_str(dev.controller.header());
    for l in &place.before {
        em.out.push_str(l);
        em.out.push('\n');
    }

    let burns = |layer: usize| doc.cut(layer).map(|c| c.output).unwrap_or(layer < COLOR_LAYERS);
    let bbox_of = |l: &Shape, x: &Xform| {
        let mut probe = l.clone();
        probe.xform = *x;
        probe.bbox()
    };
    let chosen: Vec<&Shape> = doc.shapes.iter().enumerate().filter(|(i, _)| only.map(|o| o.contains(i)).unwrap_or(true)).map(|(_, s)| s).collect();
    let mut per_shape: Vec<Vec<(Shape, Xform)>> = chosen
        .iter()
        .map(|s| {
            let mut mine = Vec::new();
            s.leaves(Xform::IDENTITY, &mut mine);
            mine
        })
        .collect();
    // Start from: move the whole job so its anchor point lands on the start point
    let mut shift = P2::default();
    if let Some(t) = place.target {
        let b = per_shape.iter().flatten().filter(|(l, _)| burns(l.layer)).fold(BBox::EMPTY, |b, (l, x)| b.union(&bbox_of(l, x)));
        if !b.is_empty() {
            shift = t - anchor_point(&b, place.anchor);
        }
        em.out.push_str(&format!("; Start from: the job's anchor point at X{} Y{}\n", f3(t.x), f3(t.y)));
    }
    let moved = Xform::translate(shift.x, shift.y);
    for (s, mine) in chosen.iter().zip(per_shape.iter_mut()) {
        for (_, x) in mine.iter_mut() {
            *x = x.then(moved);
        }
        if !s.tabs.is_empty() {
            let mut layers: Vec<usize> = mine.iter().map(|(l, _)| l.layer).collect();
            layers.sort_unstable();
            layers.dedup();
            em.tab_points.extend(s.tabs.iter().map(|p| (moved.apply(s.xform.apply(*p)), layers.clone())));
        }
    }
    let mut leaves: Vec<(Shape, Xform)> = Vec::new();
    // Only burn what fits on the bed: anything outside or over the edge could crash the head.
    let mut skipped_outside = 0;
    for mut mine in per_shape {
        if let Some(bed) = place.bed {
            let before = mine.iter().filter(|(l, _)| burns(l.layer)).count();
            mine.retain(|(l, x)| bed.contains_box(&bbox_of(l, x)));
            // count whole shapes (a group of 3000 pieces is one shape), and only ones that would burn
            if mine.iter().filter(|(l, _)| burns(l.layer)).count() < before {
                skipped_outside += 1;
            }
        }
        leaves.extend(mine);
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
    em.out.push_str(&format!("G1 S0\n{LASER_OFF}\nG0 X{} Y{}\n", f3(place.end.x), f3(place.end.y)));
    for l in &place.after {
        em.out.push_str(l);
        em.out.push('\n');
    }
    em.out.push_str("M2\n");
    if place.relative {
        em.out = to_relative(&em.out);
    }
    Job { moves: em.moves, gcode: em.out, cut_length: em.cut_len, travel_length: em.travel_len, seconds: em.secs, cut_seconds: em.cut_secs, travel_seconds: em.travel_secs, skipped_outside, return_to: place.return_to }
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

/// One line to burn: its pieces in order (more than one when tabs split a closed outline), and whether it is
/// a whole closed outline (so it can start at any of its points).
struct Item {
    pieces: Vec<Vec<P2>>,
    whole_loop: bool,
}

/// Split the closed lines that get tabs: tabs placed by hand on the outlines they sit on, otherwise the
/// layer's automatic tabs.
fn tab_items(lines: Vec<Vec<P2>>, cs: &CutSetting, manual: &[P2]) -> Vec<Item> {
    use crate::toolpath as tp;
    let n = lines.len();
    let closed: Vec<bool> = lines.iter().map(|l| is_closed(l) && l.len() > 3).collect();
    // a tab placed by hand goes on the nearest closed outline within reach (kerf moves outlines a little)
    let reach = 1.0 + cs.kerf.abs();
    let mut by_hand: Vec<Vec<f64>> = vec![Vec::new(); n];
    if !manual.is_empty() {
        let boxes: Vec<BBox> = lines.iter().map(|l| BBox::from_points(l)).collect();
        for p in manual {
            let mut best: Option<(usize, f64, f64)> = None;
            for i in (0..n).filter(|&i| closed[i]) {
                let b = boxes[i];
                if p.x < b.min.x - reach || p.x > b.max.x + reach || p.y < b.min.y - reach || p.y > b.max.y + reach {
                    continue;
                }
                let (s, d) = tp::project(&lines[i], *p);
                if d <= reach && best.map_or(true, |(_, _, bd)| d < bd) {
                    best = Some((i, s, d));
                }
            }
            if let Some((i, s, _)) = best {
                by_hand[i].push(s);
            }
        }
    }
    let t = &cs.tabs;
    let depth = if t.on && t.skip_inner { tp::nesting_depth(&lines) } else { vec![0; n] };
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if !closed[i] {
                return Item { pieces: vec![l], whole_loop: false };
            }
            // (hand-placed tabs that would leave less cut than tab are ignored, so a small outline is never
            // lost altogether)
            let hand = std::mem::take(&mut by_hand[i]);
            let at = if !hand.is_empty() {
                if tp::length(&l) >= 2.0 * t.width * hand.len() as f64 {
                    hand
                } else {
                    Vec::new()
                }
            } else if t.on && !(t.skip_inner && depth[i] % 2 == 1) {
                tp::auto_tab_positions(tp::length(&l), t)
            } else {
                Vec::new()
            };
            if at.is_empty() {
                Item { pieces: vec![l], whole_loop: true }
            } else {
                Item { pieces: tp::split_at_tabs(&l, &at, t.width), whole_loop: false }
            }
        })
        .collect()
}

fn emit_lines(em: &mut Emitter, on: &[&(Shape, Xform)], cs: &CutSetting) {
    let lines = crate::toolpath::apply_kerf(vector_lines(on), cs.kerf);
    let o = em.dev.opt;
    // a shape waits until everything inside it has been cut ("Cut Inner Shapes First")
    let inside = if o.inner_first { containment(&lines) } else { vec![Vec::new(); lines.len()] };
    let mine: Vec<P2> = em.tab_points.iter().filter(|(_, ls)| ls.contains(&cs.index)).map(|(p, _)| *p).collect();
    let items = tab_items(lines, cs, &mine);
    let mut done = vec![false; items.len()];
    let mut slots: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    for _ in 0..slots.len() {
        let pos = em.pos;
        let ready = |i: usize| slots[i].is_some() && inside[i].iter().all(|j| done[*j]);
        // (item, burn it backwards, start at this point of a whole loop / this piece of a tabbed one)
        let (mut best, mut rev, mut start, mut bd) = (usize::MAX, false, 0usize, f64::INFINITY);
        for (i, it) in slots.iter().enumerate() {
            let Some(it) = it else { continue };
            if !ready(i) {
                continue;
            }
            if !o.reduce_travel || it.pieces.is_empty() {
                // shape-list order: the first one that's ready
                (best, rev, start) = (i, false, 0);
                break;
            }
            let l = &it.pieces[0];
            if it.whole_loop && o.best_start {
                for (k, p) in l[..l.len() - 1].iter().enumerate() {
                    let d = p.dist(pos);
                    if d < bd {
                        (best, rev, start, bd) = (i, false, k, d);
                    }
                }
                continue;
            }
            if it.pieces.len() > 1 {
                // a loop split by tabs: it can begin with any of its pieces
                let last = if o.best_start { it.pieces.len() } else { 1 };
                for (k, pc) in it.pieces[..last].iter().enumerate() {
                    let d = pc[0].dist(pos);
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
            if !it.whole_loop && o.best_direction && !is_closed(l) {
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
        let mut it = slots[best].take().unwrap();
        done[best] = true;
        if it.whole_loop && start > 0 {
            // a closed loop starting at another corner
            let l = &mut it.pieces[0];
            l.pop();
            l.rotate_left(start);
            l.push(l[0]);
        } else if start > 0 {
            it.pieces.rotate_left(start);
        }
        for mut l in it.pieces {
            if rev {
                l.reverse();
            }
            em.travel(l[0]);
            for p in &l[1..] {
                em.cut(*p, cs.max_power, cs.speed, cs.index);
            }
        }
    }
}

/// The overscan for a layer's scan lines. Only G-code lasers: galvo mirrors don't need a run-up, and Ruida and
/// K40 jobs are written as their own moves, where a laser-off move at burning speed isn't kept.
fn overscan(dev: &Device, cs: &CutSetting) -> f64 {
    if dev.controller.family() != Family::GCode {
        0.0
    } else {
        crate::toolpath::overscan_distance(cs.speed, cs.overscan_percent)
    }
}

/// One scan line of a fill or picture: its burns `(from, to, power %)`, all in the same direction. With
/// overscan the head runs on at burning speed (laser off) before the first burn, across short gaps, and after
/// the last, staying on the bed.
fn emit_row(em: &mut Emitter, segs: &[(P2, P2, f64)], cs: &CutSetting, over: f64) {
    let Some(first) = segs.first() else { return };
    let dir = (segs[segs.len() - 1].1 - first.0).normalized();
    if over <= 1e-6 || dir.len() < 0.5 {
        for (a, b, p) in segs {
            em.travel(*a);
            em.cut(*b, *p, cs.speed, cs.index);
        }
        return;
    }
    let lead_in = |em: &mut Emitter, a: P2| {
        let r = crate::toolpath::room(a, dir * -1.0, over, em.bed);
        em.travel(a - dir * r);
        em.coast(a, cs.speed);
    };
    let lead_out = |em: &mut Emitter, b: P2| {
        let r = crate::toolpath::room(b, dir, over, em.bed);
        em.coast(b + dir * r, cs.speed);
    };
    lead_in(em, first.0);
    for (i, (a, b, p)) in segs.iter().enumerate() {
        if i > 0 {
            let prev = segs[i - 1].1;
            if prev.dist(*a) <= 2.0 * over {
                em.coast(*a, cs.speed);
            } else {
                lead_out(em, prev);
                lead_in(em, *a);
            }
        }
        em.cut(*b, *p, cs.speed, cs.index);
    }
    lead_out(em, segs[segs.len() - 1].1);
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

/// G-code that traces the outline of what a job burns (the bounds of its moves with the laser on), laser
/// off. None if nothing would burn.
pub fn frame_gcode(job: &Job, rapid_speed: f64) -> Option<Vec<String>> {
    let mut b = BBox::EMPTY;
    for m in job.moves.iter().filter(|m| m.power > 0.0) {
        b.add(m.from);
        b.add(m.to);
    }
    if b.is_empty() {
        return None;
    }
    let f = rapid_speed * 60.0;
    let pts = [b.min, p2(b.max.x, b.min.y), b.max, p2(b.min.x, b.max.y), b.min];
    let mut lines = vec!["G90".to_string(), "G21".to_string()];
    for p in pts {
        lines.push(format!("G1 X{:.3} Y{:.3} F{:.0} S0", p.x, p.y, f));
    }
    // a Current position job starts where the head was: go back there
    if let Some(p) = job.return_to {
        lines.push(format!("G1 X{:.3} Y{:.3} F{:.0} S0", p.x, p.y, f));
    }
    Some(lines)
}

/// Safety check before any G-code is sent to a laser: would a move go outside the work area
/// (`0..=w` × `0..=h`)? Absolute moves (G90, the default) are checked as they are. Relative moves (G91, or a
/// `$J=G91` jog) are checked from `pos`, the head's last known position, and only refused when they take the
/// head further outside (so it can always be jogged back in). `check_y` is false when a rotary turns
/// instead of Y. Inches (G20) are understood.
#[cfg(test)]
pub fn check_moves(lines: &[String], w: f64, h: f64, check_y: bool, pos: Option<(f64, f64)>) -> Result<(), String> {
    check_moves_from(lines, w, h, check_y, pos, (0.0, 0.0))
}

/// The same, when the work origin (0, 0) has been moved to `origin` on the bed (G92 "Set origin here"): the
/// moves are checked against the real bed. `pos` is the head's position on the bed.
pub fn check_moves_from(lines: &[String], w: f64, h: f64, check_y: bool, pos: Option<(f64, f64)>, origin: (f64, f64)) -> Result<(), String> {
    const EPS: f64 = 0.001;
    let outside = |v: f64, max: f64| v < -EPS || v > max + EPS;
    // how far outside 0..=max a value is
    let beyond = |v: f64, max: f64| if v < 0.0 { -v } else if v > max { v - max } else { 0.0 };
    let (mut abs, mut inch) = (true, false);
    let mut cur = pos;
    for raw in lines {
        let line = raw.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
        let (body, jog) = match line.strip_prefix("$J=") {
            Some(b) => (b.to_string(), true),
            None if line.starts_with('$') => continue,
            None => (line, false),
        };
        // a jog's modes apply to that line only
        let (mut l_abs, mut l_inch) = if jog { (true, false) } else { (abs, inch) };
        let (mut x, mut y) = (None, None);
        // words: a letter followed by a number, with or without spaces between them
        let b = body.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i] as char;
            if !c.is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let start = i + 1;
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.' || b[j] == b'-' || b[j] == b'+' || b[j] == b' ') {
                j += 1;
            }
            let num: Option<f64> = body[start..j].replace(' ', "").parse().ok();
            i = j;
            let Some(n) = num else { continue };
            match c {
                'G' if n == 90.0 => l_abs = true,
                'G' if n == 91.0 => l_abs = false,
                'G' if n == 20.0 => l_inch = true,
                'G' if n == 21.0 => l_inch = false,
                'X' => x = Some(n),
                'Y' => y = Some(n),
                _ => {}
            }
        }
        if !jog {
            abs = l_abs;
            inch = l_inch;
        }
        if x.is_none() && y.is_none() {
            continue;
        }
        let k = if l_inch { 25.4 } else { 1.0 };
        let (x, y) = (x.map(|v| v * k), y.map(|v| v * k));
        let refuse = || Err("This would move outside the work area, so it wasn't sent. Check the work area size in Settings and that nothing is off the bed.".to_string());
        if l_abs {
            let (x, y) = (x.map(|v| v + origin.0), y.map(|v| v + origin.1));
            if x.is_some_and(|v| outside(v, w)) || (check_y && y.is_some_and(|v| outside(v, h))) {
                return refuse();
            }
            let (px, py) = cur.unwrap_or((f64::NAN, f64::NAN));
            cur = Some((x.unwrap_or(px), y.unwrap_or(py)));
        } else if let Some((px, py)) = cur {
            let (nx, ny) = (px + x.unwrap_or(0.0), py + y.unwrap_or(0.0));
            let worse_x = outside(nx, w) && beyond(nx, w) > beyond(px, w) + EPS;
            let worse_y = check_y && outside(ny, h) && beyond(ny, h) > beyond(py, h) + EPS;
            if worse_x || worse_y {
                return refuse();
            }
            cur = Some((nx, ny));
        }
    }
    Ok(())
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
    let over = overscan(em.dev, cs);
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
                let segs: Vec<(P2, P2, f64)> = if flip { row.into_iter().rev().map(|(a, b)| (b, a, cs.max_power)).collect() } else { row.into_iter().map(|(a, b)| (a, b, cs.max_power)).collect() };
                emit_row(em, &segs, cs, over);
            }
        }
    }
}

/// The power for one image pixel (0 = black, 255 = white), or None for laser off. Dithered modes switch the
/// laser fully on (the layer's max power) or off per pixel; grayscale varies the power with the darkness,
/// from the min power (white) to the max power (black).
pub fn image_power(g: u8, mode: Dither, cs: &CutSetting) -> Option<f64> {
    if mode == Dither::Grayscale {
        Some(cs.min_power + (cs.max_power - cs.min_power) * (1.0 - g as f64 / 255.0)).filter(|p| *p > 0.01)
    } else {
        (g < 128 && cs.max_power > 0.01).then_some(cs.max_power)
    }
}

fn emit_image(em: &mut Emitter, on: &[&(Shape, Xform)], cs: &CutSetting) {
    let over = overscan(em.dev, cs);
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
            let mut segs: Vec<(P2, P2, f64)> = Vec::new();
            for i in order {
                let (xa, xb) = if ltr { (xs[i], xs[i + 1]) } else { (xs[i + 1], xs[i]) };
                let mid = p2((xa + xb) / 2.0, y);
                let pw = sample(mid).and_then(|g| image_power(g, adjust.dither, cs));
                match (run, pw) {
                    (Some((st, rp)), Some(p)) if (rp - p).abs() < 0.5 => {
                        run = Some((st, rp));
                    }
                    (prev, cur) => {
                        if let Some((st, rp)) = prev {
                            segs.push((st, p2(xa, y), rp));
                        }
                        run = cur.map(|p| (p2(xa, y), p));
                    }
                }
                if i == if ltr { n - 1 } else { 0 } {
                    if let Some((st, rp)) = run.take() {
                        segs.push((st, p2(xb, y), rp));
                    }
                }
            }
            emit_row(em, &segs, cs, over);
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
    #[test]
    fn frame_stays_on_the_bed_when_a_shape_is_off_it() {
        let dev = Device::default();
        let mut doc = Document::default();
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(20.0, 20.0), 0));
        // far off the bed, and hanging over the right edge
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(-200.0, 50.0), 0));
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 30.0, h: 10.0, cr: 0.0 }, Xform::translate(dev.width - 5.0, 50.0), 0));
        doc.sync_cuts();
        let job = build_job(&doc, &dev, None);
        let lines = frame_gcode(&job, dev.rapid_speed).unwrap();
        assert!(check_moves(&lines, dev.width, dev.height, true, None).is_ok());
        assert!(lines.contains(&"G1 X15.000 Y15.000 F6000 S0".to_string()) || lines.iter().any(|l| l.starts_with("G1 X15.000 Y15.000")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("G1 X25.000 Y25.000")), "{lines:?}");
        assert!(check_moves(&job.gcode.lines().map(String::from).collect::<Vec<_>>(), dev.width, dev.height, true, None).is_ok());
        // nothing on the bed: nothing to frame
        let mut off = Document::default();
        off.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(-200.0, 50.0), 0));
        off.sync_cuts();
        assert!(frame_gcode(&build_job(&off, &dev, None), dev.rapid_speed).is_none());
    }
    #[test]
    fn image_layers_burn_the_chosen_dithering() {
        // a 40 × 1 pixel strip, black on the left to white on the right, 20 × 1 mm
        let (pw, ph) = (40u32, 1u32);
        let mut rgba = Vec::new();
        for x in 0..pw {
            let g = (x * 255 / (pw - 1)) as u8;
            rgba.extend_from_slice(&[g, g, g, 255]);
        }
        let data = BitmapData { encoded: std::sync::Arc::new(Vec::new()), width: pw, height: ph, rgba: std::sync::Arc::new(rgba), source_file: String::new() };
        let dev = Device::default();
        for mode in Dither::ALL {
            let mut doc = Document::default();
            let adjust = ImageAdjust { dither: mode, ..Default::default() };
            doc.shapes.push(Shape::new(ShapeKind::Bitmap { w: 20.0, h: 1.0, data: data.clone(), adjust }, Xform::translate(50.0, 50.0), 4));
            doc.sync_cuts();
            let c = doc.cut_mut(4);
            c.mode = CutMode::Image;
            c.min_power = 10.0;
            c.max_power = 60.0;
            c.interval = 0.25;
            c.bidir = false;
            let job = build_job(&doc, &dev, None);
            let burns: Vec<&Move> = job.moves.iter().filter(|m| m.power > 0.0).collect();
            assert!(!burns.is_empty(), "{mode:?}: nothing burns");
            let burned_len: f64 = burns.iter().map(|m| m.from.dist(m.to)).sum::<f64>() / 4.0; // 4 rows in 1 mm
            if mode == Dither::Grayscale {
                // the power follows the darkness: highest on the black (left) end, never below the min
                let powers: Vec<f32> = burns.iter().map(|m| m.power).collect();
                assert!(powers.iter().all(|p| (0.0999..=0.6001).contains(p)), "{powers:?}");
                assert!(powers.iter().copied().fold(0.0, f32::max) > 0.55);
                let left = burns.iter().min_by(|a, b| a.from.x.total_cmp(&b.from.x)).unwrap();
                assert!(left.power > 0.55, "darkest end at {}", left.power);
                assert!(burned_len > 19.0, "grayscale burns the whole strip ({burned_len})");
            } else {
                // dithered: fully on at the max power, or off
                assert!(burns.iter().all(|m| (m.power - 0.6).abs() < 1e-6), "{mode:?}: {:?}", burns.iter().map(|m| m.power).collect::<Vec<_>>());
                assert!(burned_len > 6.0 && burned_len < 14.0, "{mode:?}: about half the strip burns, not {burned_len}");
                assert!(job.gcode.contains("S600"), "{mode:?}");
            }
        }
    }
    #[test]
    fn a_moved_origin_is_checked_against_the_real_bed() {
        let s = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        // origin moved to (300, 50) on a 400 × 400 bed: X 0..100 still fits, X 150 would be at 450
        let o = (300.0, 50.0);
        assert!(check_moves_from(&s(&["G90", "G1 X100 Y100"]), 400.0, 400.0, true, None, o).is_ok());
        assert!(check_moves_from(&s(&["G90", "G1 X150 Y100"]), 400.0, 400.0, true, None, o).is_err());
        // negative work coordinates are fine while they stay on the bed
        assert!(check_moves_from(&s(&["G90", "G1 X-250 Y-40"]), 400.0, 400.0, true, None, o).is_ok());
        assert!(check_moves_from(&s(&["G90", "G1 X-310 Y0"]), 400.0, 400.0, true, None, o).is_err());
        // jogs are checked from the head's place on the bed
        assert!(check_moves_from(&s(&["$J=G91 X20"]), 400.0, 400.0, true, Some((390.0, 60.0)), o).is_err());
    }
    #[test]
    fn moves_outside_the_work_area_are_refused() {
        let s = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        assert!(check_moves(&s(&["G90", "G1 X100 Y100 S0"]), 400.0, 400.0, true, None).is_ok());
        assert!(check_moves(&s(&["G90", "G1 X-1 Y100 S0"]), 400.0, 400.0, true, None).is_err());
        assert!(check_moves(&s(&["G0X401Y0"]), 400.0, 400.0, true, None).is_err());
        assert!(check_moves(&s(&["G1 X10 Y450"]), 400.0, 400.0, true, None).is_err());
        // a rotary turning instead of Y: Y isn't limited
        assert!(check_moves(&s(&["G1 X10 Y450"]), 400.0, 400.0, false, None).is_ok());
        // inches: 16 in = 406.4 mm
        assert!(check_moves(&s(&["G20", "G1 X16 Y1"]), 400.0, 400.0, true, None).is_err());
        assert!(check_moves(&s(&["G20", "G1 X15 Y1"]), 400.0, 400.0, true, None).is_ok());
        // relative moves and jogs from a known position
        assert!(check_moves(&s(&["$J=G91 G21 X10 Y0 F3000"]), 400.0, 400.0, true, Some((395.0, 10.0))).is_err());
        assert!(check_moves(&s(&["$J=G91 G21 X-10 Y0 F3000"]), 400.0, 400.0, true, Some((395.0, 10.0))).is_ok());
        assert!(check_moves(&s(&["G91", "G1 X0 Y-20"]), 400.0, 400.0, true, Some((5.0, 10.0))).is_err());
        // already outside: moving back in is allowed, further out isn't
        assert!(check_moves(&s(&["$J=G91 X5"]), 400.0, 400.0, true, Some((-3.0, 10.0))).is_ok());
        assert!(check_moves(&s(&["$J=G91 X-5"]), 400.0, 400.0, true, Some((-3.0, 10.0))).is_err());
        // a jog's G91 doesn't carry over to the next line
        assert!(check_moves(&s(&["$J=G91 X5", "G1 X450"]), 400.0, 400.0, true, Some((0.0, 0.0))).is_err());
    }
}

#[cfg(test)]
mod lbdev_tests {
    use super::*;
    #[test]
    fn reads_a_lightburn_device_profile() {
        let text = r#"{"DeviceList":[{"Name":"My Laser","Type":"GRBL","Width":410,"Height":400,"Settings":{"MaxSValue":"1000"}},{"Name":"Galvo","Type":"EzCad2"},{"Name":"Old DSP","Type":"Trocen"}]}"#;
        let (devs, notes) = devices_from_lbdev(text).unwrap();
        assert_eq!(devs.len(), 3);
        assert_eq!(devs[1].controller, Controller::EzCad2);
        assert_eq!((devs[0].name.as_str(), devs[0].width, devs[0].height), ("My Laser", 410.0, 400.0));
        assert_eq!((devs[1].width, devs[1].height), (110.0, 110.0), "galvo: the lens field");
        assert_eq!((devs[2].width, devs[2].height), (LBDEV_DEFAULT_WIDTH, LBDEV_DEFAULT_HEIGHT));
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

    fn one_rect(x: f64, y: f64, w: f64, h: f64) -> Document {
        let mut doc = Document::default();
        doc.shapes.push(Shape::new(ShapeKind::Rect { w, h, cr: 0.0 }, Xform::translate(x, y), 0));
        doc.sync_cuts();
        doc
    }

    fn burn_box(job: &Job) -> BBox {
        let mut b = BBox::EMPTY;
        for m in job.moves.iter().filter(|m| m.power > 0.0) {
            b.add(m.from);
            b.add(m.to);
        }
        b
    }

    #[test]
    fn kerf_moves_cuts_out_so_parts_keep_their_size() {
        let mut doc = one_rect(50.0, 50.0, 20.0, 20.0);
        doc.cut_mut(0).kerf = 0.1;
        let b = burn_box(&build_job(&doc, &Device::default(), None));
        assert!((b.width() - 20.2).abs() < 1e-3 && (b.height() - 20.2).abs() < 1e-3, "{b:?}");
        // two overlapping parts are both cut in full, not merged into one outline
        let mut two = one_rect(50.0, 50.0, 20.0, 20.0);
        two.shapes.push(Shape::new(ShapeKind::Rect { w: 20.0, h: 20.0, cr: 0.0 }, Xform::translate(60.0, 55.0), 0));
        two.cut_mut(0).kerf = 0.1;
        let job = build_job(&two, &Device::default(), None);
        assert!(job.cut_length > 2.0 * 80.0, "{}", job.cut_length);
        // fills aren't moved
        doc.cut_mut(0).mode = CutMode::Fill;
        doc.cut_mut(0).overscan_percent = 0.0;
        assert!(burn_box(&build_job(&doc, &Device::default(), None)).width() <= 20.0 + 1e-9);
    }

    #[test]
    fn tabs_leave_uncut_gaps_and_hand_placed_ones_win() {
        let mut doc = one_rect(50.0, 50.0, 10.0, 10.0);
        doc.cut_mut(0).tabs = Tabs { on: true, by_count: true, count: 4, width: 1.0, ..Default::default() };
        let job = build_job(&doc, &Device::default(), None);
        assert!((job.cut_length - 36.0).abs() < 1e-6, "{}", job.cut_length);
        // tabs placed by hand replace the automatic ones on that outline (one, in the middle of the top edge)
        doc.shapes[0].tabs = vec![p2(0.0, 5.0)];
        let job = build_job(&doc, &Device::default(), None);
        assert!((job.cut_length - 39.0).abs() < 1e-6, "{}", job.cut_length);
        let gap_mid = p2(50.0, 55.0);
        assert!(job.moves.iter().filter(|m| m.power > 0.0).all(|m| m.from.dist(gap_mid) >= 0.5 - 1e-9 && m.to.dist(gap_mid) >= 0.5 - 1e-9));
        // and they work with tabs switched off on the layer
        doc.cut_mut(0).tabs.on = false;
        assert!((build_job(&doc, &Device::default(), None).cut_length - 39.0).abs() < 1e-6);
        // a hand-placed tab doesn't cut into a shape on another layer next to it
        let mut doc = one_rect(50.0, 50.0, 10.0, 10.0);
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(50.0, 65.5), 1));
        doc.sync_cuts();
        doc.shapes[0].tabs = vec![p2(0.0, 5.0)];
        let job = build_job(&doc, &Device::default(), None);
        assert!((job.cut_length - 79.5).abs() < 1e-6, "{}", job.cut_length);
        // tabs too wide for a small outline are ignored rather than leaving nothing to cut
        let mut doc = one_rect(50.0, 50.0, 1.0, 1.0);
        doc.cut_mut(0).tabs.width = 3.0;
        doc.shapes[0].tabs = vec![p2(0.0, 0.5)];
        assert!((build_job(&doc, &Device::default(), None).cut_length - 4.0).abs() < 1e-6);
        // holes get no automatic tabs (skip holes is on by default)
        let mut doc = one_rect(50.0, 50.0, 20.0, 20.0);
        doc.shapes.push(Shape::new(ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, Xform::translate(50.0, 50.0), 0));
        doc.cut_mut(0).tabs = Tabs { on: true, count: 2, width: 1.0, ..Default::default() };
        assert!((build_job(&doc, &Device::default(), None).cut_length - (80.0 - 2.0 + 40.0)).abs() < 1e-6);
    }

    #[test]
    fn overscan_runs_on_with_the_laser_off_and_stays_on_the_bed() {
        // a 10 mm fill at 100 mm/s with 2.5% overscan: 2.5 mm before and after each line (rapids at 300)
        let dev = Device { rapid_speed: 300.0, ..Device::default() };
        let mut doc = one_rect(50.0, 50.0, 10.0, 10.0);
        let c = doc.cut_mut(0);
        (c.mode, c.interval, c.speed, c.overscan_percent) = (CutMode::Fill, 1.0, 100.0, 2.5);
        let job = build_job(&doc, &dev, None);
        let coasts: Vec<&Move> = job.moves.iter().filter(|m| m.power == 0.0 && (m.speed - 100.0).abs() < 1e-9).collect();
        assert_eq!(coasts.len(), 20, "a run-in and a run-out on each of 10 lines");
        assert!(coasts.iter().all(|m| (m.from.dist(m.to) - 2.5).abs() < 1e-9));
        assert!(job.gcode.contains(" S0 F6000"), "laser off at burning speed");
        // against the bed's left edge there's no room before the line: it starts on the edge
        let mut doc = one_rect(5.0, 50.0, 10.0, 10.0);
        let c = doc.cut_mut(0);
        (c.mode, c.interval, c.speed, c.overscan_percent, c.bidir) = (CutMode::Fill, 1.0, 100.0, 2.5, false);
        let job = build_job(&doc, &dev, None);
        assert!(job.moves.iter().all(|m| m.to.x >= -1e-5 && m.from.x >= -1e-5));
        // (the run-out on the right still has room)
        assert!(job.moves.iter().any(|m| m.power == 0.0 && (m.speed - 100.0).abs() < 1e-9 && (m.to.x - 12.5).abs() < 1e-9));
        // no overscan: as before
        doc.cut_mut(0).overscan_percent = 0.0;
        assert!(build_job(&doc, &dev, None).moves.iter().all(|m| m.power > 0.0 || (m.speed - 100.0).abs() > 1e-9));
    }

    #[test]
    fn start_from_moves_the_job_to_its_start_point() {
        let dev = Device::default();
        let doc = one_rect(50.0, 50.0, 10.0, 10.0);
        // the job's centre on (100, 80)
        let place = Placement { target: Some(p2(100.0, 80.0)), anchor: [1, 1], end: p2(100.0, 80.0), return_to: Some(p2(100.0, 80.0)), ..Placement::absolute(&dev) };
        let job = build_job_placed(&doc, &dev, None, &place);
        let b = burn_box(&job);
        assert!(b.center().dist(p2(100.0, 80.0)) < 1e-9, "{b:?}");
        assert!(job.gcode.trim_end().ends_with("G0 X100 Y80\nM2"), "ends back at the start point");
        let frame = frame_gcode(&job, 100.0).unwrap();
        assert!(frame.last().unwrap().starts_with("G1 X100.000 Y80.000"), "Frame goes back to where the head started");
        // the bottom-left corner on a user origin
        let place = Placement { target: Some(p2(10.0, 10.0)), anchor: [0, 0], ..Placement::absolute(&dev) };
        assert!(burn_box(&build_job_placed(&doc, &dev, None, &place)).min.dist(p2(10.0, 10.0)) < 1e-9);
        // moved off the bed: left out, as always
        let place = Placement { target: Some(p2(-50.0, 10.0)), anchor: [0, 0], ..Placement::absolute(&dev) };
        assert_eq!(build_job_placed(&doc, &dev, None, &place).skipped_outside, 1);
        // a bed that isn't known (unhomed, current position): nothing is left out, the caller checks the size
        let place = Placement { target: Some(p2(0.0, 0.0)), anchor: [1, 1], bed: None, relative: true, ..Placement::absolute(&dev) };
        let job = build_job_placed(&doc, &dev, None, &place);
        assert_eq!(job.skipped_outside, 0);
        assert!(burn_box(&job).center().dist(p2(0.0, 0.0)) < 1e-9);
        // a saved Current position file: relative moves (no G92), back to absolute at the end
        assert!(!job.gcode.contains("G92") && job.gcode.contains("\nG91 ") && !job.gcode.contains("\nG90\nG91"));
        assert!(job.gcode.trim_end().ends_with("G90\nM2"));
        // the moves add up: first to the square's corner (-5, -5), and back to the start at the end
        let (mut x, mut y) = (0.0, 0.0);
        let mut first = None;
        for l in job.gcode.lines().filter(|l| l.starts_with("G0 ") || l.starts_with("G1 X")) {
            for w in l.split(' ') {
                if let Some(v) = w.strip_prefix('X') {
                    x += v.parse::<f64>().unwrap();
                }
                if let Some(v) = w.strip_prefix('Y') {
                    y += v.parse::<f64>().unwrap();
                }
            }
            first.get_or_insert((x, y));
        }
        let f = first.unwrap();
        assert!((f.0 + 5.0).abs() < 1e-9 && (f.1 + 5.0).abs() < 1e-9, "{f:?}");
        assert!(x.abs() < 1e-9 && y.abs() < 1e-9, "ends where it started: {x} {y}");
        // absolute: unchanged
        assert!(burn_box(&build_job(&doc, &dev, None)).center().dist(p2(50.0, 50.0)) < 1e-9);
    }
}
