//! Galvo lasers with JCZ / BJJCZ "LMC" controller boards (the boards EZCad2 drives): fiber, UV and CO2 galvo
//! markers. BurnCraft builds the board's command lists from a job and sends them over USB.
//!
//! The protocol (12-byte commands, 3072-byte command lists, command numbers, start-up sequence, unit
//! conversions, correction-file layout and default settings) follows MeerK40t's galvo driver
//! (`meerk40t/balormk`, MIT licence, Copyright (c) 2021 meerk40t; see THIRD-PARTY-NOTICES.md).
//!
//! The galvo field is 0..=65535 in each direction across the lens's field (e.g. 110 mm), centred at 32768.
//! BurnCraft's work area is that field, with (0, 0) at its front-left corner.

use crate::gcode::{Device, Job};
use crate::geom::*;
use crate::model::Document;
use serde::{Deserialize, Serialize};

pub const VENDOR_ID: u16 = 0x9588;
pub const PRODUCT_ID: u16 = 0x9899;
const EP_OUT: u8 = 0x02;
const EP_IN: u8 = 0x88;
/// Entries in one command list packet (each 12 bytes: 0xC00 bytes in all).
pub const LIST_ENTRIES: usize = 256;

// list commands (inside a command list)
const LIST_JUMP_TO: u16 = 0x8001;
const LIST_END_OF_LIST: u16 = 0x8002;
const LIST_DELAY_TIME: u16 = 0x8004;
const LIST_MARK_TO: u16 = 0x8005;
const LIST_JUMP_SPEED: u16 = 0x8006;
const LIST_LASER_ON_DELAY: u16 = 0x8007;
const LIST_LASER_OFF_DELAY: u16 = 0x8008;
const LIST_MARK_FREQ: u16 = 0x800A;
const LIST_MARK_POWER_RATIO: u16 = 0x800B;
const LIST_MARK_SPEED: u16 = 0x800C;
const LIST_POLYGON_DELAY: u16 = 0x800F;
const LIST_WRITE_PORT: u16 = 0x8011;
const LIST_MARK_CURRENT: u16 = 0x8012;
const LIST_QSWITCH_PERIOD: u16 = 0x801B;
const LIST_SET_CO2_FPK: u16 = 0x801E;
const LIST_FIBER_YLPM_PULSE_WIDTH: u16 = 0x8026;
const LIST_READY_MARK: u16 = 0x8051;

// single commands
const ENABLE_LASER: u16 = 0x0004;
const EXECUTE_LIST: u16 = 0x0005;
const SET_PWM_PULSE_WIDTH: u16 = 0x0006;
const GET_VERSION: u16 = 0x0007;
const GET_SERIAL_NO: u16 = 0x0009;
const WRITE_COR_LINE: u16 = 0x0010;
const RESET_LIST: u16 = 0x0012;
const WRITE_COR_TABLE: u16 = 0x0015;
const SET_CONTROL_MODE: u16 = 0x0016;
const SET_DELAY_MODE: u16 = 0x0017;
const SET_END_OF_LIST: u16 = 0x0019;
const SET_FIRST_PULSE_KILLER: u16 = 0x001A;
const SET_LASER_MODE: u16 = 0x001B;
const SET_TIMING: u16 = 0x001C;
const SET_STANDBY: u16 = 0x001D;
const SET_PWM_HALF_PERIOD: u16 = 0x001E;
const STOP_EXECUTE: u16 = 0x001F;
const WRITE_PORT: u16 = 0x0021;
const WRITE_ANALOG_PORT_1: u16 = 0x0022;
const SET_FPK_PARAM_2: u16 = 0x002E;
const SET_FLY_RES: u16 = 0x0032;
const FIBER_SET_MO: u16 = 0x0033;
const ENABLE_Z: u16 = 0x003A;
const RESET: u16 = 0x0040;

/// Status bits in the fourth word of the board's answer.
pub const BUSY: u16 = 0x04;
pub const READY: u16 = 0x20;

/// Output port bits: 0 = laser enabled (while a job runs), 8 = red pointer.
const PORT_LASER: u16 = 1 << 0;
const PORT_LIGHT: u16 = 1 << 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
    #[default]
    Fiber,
    Co2,
    Uv,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::Fiber, Source::Uv, Source::Co2];
    pub fn label(self) -> &'static str {
        match self {
            Source::Fiber => "Fiber",
            Source::Co2 => "CO2 galvo",
            Source::Uv => "UV",
        }
    }
}

/// The galvo machine's settings (defaults as in MeerK40t).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GalvoSettings {
    pub source: Source,
    /// Field size of the lens (mm); the work area is lens × lens.
    pub lens: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub swap_xy: bool,
    /// Travel ("jump") speed, mm/s.
    pub jump_speed: f64,
    /// Speed of the red pointer while framing, mm/s.
    pub light_speed: f64,
    /// Laser on / off / corner delays, µs.
    pub delay_on: f64,
    pub delay_off: f64,
    pub delay_polygon: f64,
    /// Wait at the end of a job, µs.
    pub delay_end: f64,
    /// Fiber: wait after opening the master oscillator, ms.
    pub delay_openmo: f64,
    /// Frequency used when a layer has none, kHz.
    pub frequency: f64,
    /// First pulse killer for CO2 / UV, percent.
    pub fpk: f64,
    /// MOPA fiber pulse width (ns setting), when the source supports it.
    pub pulse_width: Option<u16>,
    /// Lens correction file (.cor) from the machine's EZCad folder; empty = none.
    pub cor_file: String,
}

impl Default for GalvoSettings {
    fn default() -> Self {
        GalvoSettings {
            source: Source::Fiber,
            lens: 110.0,
            flip_x: false,
            flip_y: true,
            swap_xy: true,
            jump_speed: 2000.0,
            light_speed: 3000.0,
            delay_on: 100.0,
            delay_off: 100.0,
            delay_polygon: 100.0,
            delay_end: 300.0,
            delay_openmo: 8.0,
            frequency: 30.0,
            fpk: 10.0,
            pulse_width: None,
            cor_file: String::new(),
        }
    }
}

impl GalvoSettings {
    /// Galvo units per mm.
    pub fn per_mm(&self) -> f64 {
        65535.0 / self.lens.max(1.0)
    }

    /// Bed position (mm, Y up from the front) → galvo position, or None outside the field.
    pub fn to_galvo(&self, p: P2) -> Option<(u16, u16)> {
        let k = self.per_mm();
        // the picture as seen from above with Y down, then the machine's own flips and swap
        let (mut u, mut v) = (p.x * k, (self.lens - p.y) * k);
        if self.flip_x {
            u = 65535.0 - u;
        }
        if self.flip_y {
            v = 65535.0 - v;
        }
        if self.swap_xy {
            std::mem::swap(&mut u, &mut v);
        }
        let (u, v) = (u.round(), v.round());
        if !(0.0..=65535.0).contains(&u) || !(0.0..=65535.0).contains(&v) {
            return None;
        }
        Some((u as u16, v as u16))
    }

    /// mm/s → the board's speed unit (galvo units per ms).
    pub fn speed(&self, mm_s: f64) -> u16 {
        (mm_s * self.per_mm() / 1000.0).abs().min(65535.0) as u16
    }
}

/// kHz → period for the board: fiber Q-switch period (base 20000), CO2 / UV mark frequency (base 10000).
pub fn period(khz: f64, base: f64) -> u16 {
    ((base / khz.max(0.001)).round() as i64 & 0xFFFF) as u16
}

/// Builds a command list, skipping settings that haven't changed.
pub struct ListBuilder<'a> {
    pub entries: Vec<[u16; 6]>,
    s: &'a GalvoSettings,
    last: (u16, u16),
    mark_speed: Option<u16>,
    jump_speed: Option<u16>,
    power: Option<f64>,
    freq: Option<f64>,
    fpk_set: bool,
    delays_set: bool,
    pulse_set: bool,
    pub port: u16,
}

impl<'a> ListBuilder<'a> {
    pub fn new(s: &'a GalvoSettings, port: u16) -> Self {
        ListBuilder { entries: Vec::new(), s, last: (0x8000, 0x8000), mark_speed: None, jump_speed: None, power: None, freq: None, fpk_set: false, delays_set: false, pulse_set: false, port }
    }

    fn write(&mut self, c: u16, v: [u16; 5]) {
        self.entries.push([c, v[0], v[1], v[2], v[3], v[4]]);
    }

    fn delay_word(us: f64) -> [u16; 2] {
        [us.abs().min(65535.0) as u16, if us > 0.0 { 0 } else { 0x8000 }]
    }

    /// Start of a list: ready mark, (fiber) wait for the oscillator, port bits, travel speed.
    pub fn start(&mut self, jump: f64) {
        self.write(LIST_READY_MARK, [0; 5]);
        if self.s.source == Source::Fiber && self.port & PORT_LASER != 0 && self.s.delay_openmo != 0.0 {
            self.delay_time((self.s.delay_openmo * 100.0) as u16);
        }
        let p = self.port;
        self.write(LIST_WRITE_PORT, [p, 0, 0, 0, 0]);
        self.set_jump_speed(jump);
    }

    pub fn delay_time(&mut self, units_10us: u16) {
        self.write(LIST_DELAY_TIME, [units_10us, 0, 0, 0, 0]);
    }

    pub fn set_jump_speed(&mut self, mm_s: f64) {
        let v = self.s.speed(mm_s);
        if self.jump_speed != Some(v) {
            self.jump_speed = Some(v);
            self.write(LIST_JUMP_SPEED, [v, 0, 0, 0, 0]);
        }
    }

    pub fn set_mark_speed(&mut self, mm_s: f64) {
        let v = self.s.speed(mm_s);
        if self.mark_speed != Some(v) {
            self.mark_speed = Some(v);
            self.write(LIST_MARK_SPEED, [v, 0, 0, 0, 0]);
        }
    }

    /// Frequency (kHz) then power (percent), in the way each source takes them.
    pub fn set_power(&mut self, percent: f64, khz: f64) {
        if self.freq != Some(khz) {
            self.freq = Some(khz);
            match self.s.source {
                Source::Fiber => self.write(LIST_QSWITCH_PERIOD, [period(khz, 20000.0), 0, 0, 0, 0]),
                Source::Co2 | Source::Uv => self.write(LIST_MARK_FREQ, [period(khz, 10000.0), 0, 0, 0, 0]),
            }
            self.power = None; // CO2 / UV power depends on frequency
            self.fpk_set = false;
        }
        if matches!(self.s.source, Source::Co2 | Source::Uv) && !self.fpk_set {
            let fpk = (2000.0 / khz.max(0.001)).round() as u16;
            self.write(LIST_SET_CO2_FPK, [fpk, fpk, 0, 0, 0]);
            self.fpk_set = true;
        }
        if self.power != Some(percent) {
            self.power = Some(percent);
            match self.s.source {
                Source::Fiber => self.write(LIST_MARK_CURRENT, [(percent * 4095.0 / 100.0).round().clamp(0.0, 4095.0) as u16, 0, 0, 0, 0]),
                Source::Co2 | Source::Uv => self.write(LIST_MARK_POWER_RATIO, [(200.0 * percent / khz.max(0.001)).round().clamp(0.0, 65535.0) as u16, 0, 0, 0, 0]),
            }
        }
        if self.s.source == Source::Fiber && !self.pulse_set {
            if let Some(pw) = self.s.pulse_width {
                self.write(LIST_FIBER_YLPM_PULSE_WIDTH, [pw, 0, 0, 0, 0]);
            }
            self.pulse_set = true;
        }
    }

    pub fn set_delays(&mut self) {
        if !self.delays_set {
            self.delays_set = true;
            let (on, off, poly) = (Self::delay_word(self.s.delay_on), Self::delay_word(self.s.delay_off), Self::delay_word(self.s.delay_polygon));
            self.write(LIST_LASER_ON_DELAY, [on[0], on[1], 0, 0, 0]);
            self.write(LIST_LASER_OFF_DELAY, [off[0], off[1], 0, 0, 0]);
            self.write(LIST_POLYGON_DELAY, [poly[0], poly[1], 0, 0, 0]);
        }
    }

    fn dist(&self, to: (u16, u16)) -> u16 {
        let (dx, dy) = (to.0 as f64 - self.last.0 as f64, to.1 as f64 - self.last.1 as f64);
        (dx.hypot(dy) as i64).min(0xFFFF) as u16
    }

    pub fn jump(&mut self, to: (u16, u16)) {
        if to == self.last {
            return;
        }
        let d = self.dist(to);
        self.write(LIST_JUMP_TO, [to.0, to.1, 0, d, 0]);
        self.last = to;
    }

    pub fn mark(&mut self, to: (u16, u16)) {
        if to == self.last {
            return;
        }
        let d = self.dist(to);
        self.write(LIST_MARK_TO, [to.0, to.1, 0, d, 0]);
        self.last = to;
    }

    pub fn end(&mut self) {
        self.write(LIST_END_OF_LIST, [0; 5]);
    }
}

/// The job as one command list. Returns the entries and how many marks fell outside the lens field.
pub fn job_list(job: &Job, doc: &Document, dev: &Device) -> (Vec<[u16; 6]>, usize) {
    let s = &dev.galvo;
    let mut b = ListBuilder::new(s, PORT_LASER);
    b.start(s.jump_speed);
    let mut outside = 0;
    for m in &job.moves {
        if m.power <= 0.0 {
            continue;
        }
        let (Some(a), Some(z)) = (s.to_galvo(m.from), s.to_galvo(m.to)) else {
            outside += 1;
            continue;
        };
        let khz = doc.cut(m.layer).map(|c| c.frequency / 1000.0).filter(|f| *f > 0.0).unwrap_or(s.frequency);
        b.set_jump_speed(s.jump_speed);
        b.set_power((m.power as f64 * 100.0).clamp(0.0, 100.0), khz);
        b.set_mark_speed(m.speed);
        b.set_delays();
        b.jump(a);
        b.mark(z);
    }
    b.delay_time((s.delay_end / 10.0) as u16);
    b.end();
    (b.entries, outside)
}

/// Trace a box with the red pointer, `loops` times, laser off.
pub fn frame_list(bb: BBox, s: &GalvoSettings, loops: usize) -> Vec<[u16; 6]> {
    let mut b = ListBuilder::new(s, PORT_LIGHT);
    b.start(s.light_speed);
    let c = [p2(bb.min.x, bb.min.y), p2(bb.max.x, bb.min.y), p2(bb.max.x, bb.max.y), p2(bb.min.x, bb.max.y)];
    let pts: Vec<(u16, u16)> = c.iter().filter_map(|p| s.to_galvo(*p)).collect();
    if pts.len() == 4 {
        for _ in 0..loops {
            for p in &pts {
                b.jump(*p);
            }
        }
        b.jump(pts[0]);
    }
    b.end();
    b.entries
}

/// Split list entries into 3072-byte packets, padded with end-of-list entries.
pub fn list_packets(entries: &[[u16; 6]]) -> Vec<Vec<u8>> {
    entries
        .chunks(LIST_ENTRIES)
        .map(|chunk| {
            let mut p = Vec::with_capacity(LIST_ENTRIES * 12);
            for e in chunk {
                for w in e {
                    p.extend_from_slice(&w.to_le_bytes());
                }
            }
            while p.len() < LIST_ENTRIES * 12 {
                p.extend_from_slice(&[0x02, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            }
            p
        })
        .collect()
}

/// Read an EZCad lens correction file (.cor) into the board's 65 × 65 table of (dx, dy) words, and the field
/// size (mm) it was made for.
pub fn read_cor(bytes: &[u8]) -> Result<(Vec<(u16, u16)>, f64), String> {
    if bytes.len() < 0x16 + 16 {
        return Err("too short to be a correction file".into());
    }
    let label: Vec<u16> = bytes[..0x16].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let label = String::from_utf16_lossy(&label);
    let word = |v: i64| -> u16 { (if v >= 0 { v } else { -v + 0x8000 }) as u16 };
    let mut table = Vec::with_capacity(65 * 65);
    if label.trim_end_matches('\0') == "LMC1COR_1.0" {
        let scale_at = 0x16 + 2 + 43 * 8;
        let data_at = 0x16 + 0x1FA;
        if bytes.len() < data_at + 65 * 65 * 16 {
            return Err("the correction file is cut short".into());
        }
        let scale = f64::from_le_bytes(bytes[scale_at..scale_at + 8].try_into().unwrap());
        for i in 0..65 * 65 {
            let o = data_at + i * 16;
            let dx = f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap()).round() as i64;
            let dy = f64::from_le_bytes(bytes[o + 8..o + 16].try_into().unwrap()).round() as i64;
            table.push((word(dx), word(dy)));
        }
        Ok((table, 65536.0 / scale))
    } else {
        let scale_at = 0x16 + 6;
        let data_at = 0x16 + 0xE;
        if bytes.len() < data_at + 65 * 65 * 8 {
            return Err("the correction file is cut short".into());
        }
        let scale = f64::from_le_bytes(bytes[scale_at..scale_at + 8].try_into().unwrap());
        for i in 0..65 * 65 {
            let o = data_at + i * 8;
            let dx = i32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as i64;
            let dy = i32::from_le_bytes(bytes[o + 4..o + 8].try_into().unwrap()) as i64;
            table.push((word(dx), word(dy)));
        }
        Ok((table, 65536.0 / scale))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod usb {
    use super::*;
    use crate::k40::usb::Running;
    use nusb::transfer::{Bulk, In, Out};
    use nusb::MaybeFuture;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_millis(500);

    pub struct Board {
        out: nusb::Endpoint<Bulk, Out>,
        inp: nusb::Endpoint<Bulk, In>,
    }

    impl Board {
        pub fn open() -> Result<Board, String> {
            let info = nusb::list_devices()
                .wait()
                .map_err(|e| e.to_string())?
                .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
                .ok_or("No galvo controller (JCZ/LMC board, USB 9588:9899) found. Check the USB cable and that the laser is on.")?;
            let hint = if cfg!(target_os = "windows") { " (on Windows, install the WinUSB driver for it with Zadig, see Help → Devices)" } else if cfg!(target_os = "linux") { " (on Linux, allow your user to use it, see Help → Devices)" } else { "" };
            let dev = info.open().wait().map_err(|e| format!("can't open the galvo board: {e}{hint}"))?;
            #[cfg(target_os = "linux")]
            let iface = dev.detach_and_claim_interface(0).wait().map_err(|e| format!("can't claim the galvo board: {e}{hint}"))?;
            #[cfg(not(target_os = "linux"))]
            let iface = dev.claim_interface(0).wait().map_err(|e| format!("can't claim the galvo board: {e}{hint}"))?;
            let out = iface.endpoint::<Bulk, Out>(EP_OUT).map_err(|e| e.to_string())?;
            let inp = iface.endpoint::<Bulk, In>(EP_IN).map_err(|e| e.to_string())?;
            Ok(Board { out, inp })
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.out.transfer_blocking(bytes.to_vec().into(), TIMEOUT).status.map_err(|e| format!("USB write failed: {e}"))
        }

        /// A single command; with `read`, the board's four-word answer.
        pub fn cmd(&mut self, c: u16, v: [u16; 5], read: bool) -> Result<[u16; 4], String> {
            let mut p = Vec::with_capacity(12);
            for w in [c, v[0], v[1], v[2], v[3], v[4]] {
                p.extend_from_slice(&w.to_le_bytes());
            }
            self.write(&p)?;
            if !read {
                return Ok([0; 4]);
            }
            let buf = self.inp.allocate(8);
            let r = self.inp.transfer_blocking(buf, TIMEOUT);
            r.status.map_err(|e| format!("USB read failed: {e}"))?;
            let b = &r.buffer[..r.actual_len.min(8)];
            let w = |i: usize| b.get(i * 2..i * 2 + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).unwrap_or(0);
            Ok([w(0), w(1), w(2), w(3)])
        }

        fn c(&mut self, c: u16, v: &[u16]) -> Result<[u16; 4], String> {
            let mut a = [0u16; 5];
            a[..v.len()].copy_from_slice(v);
            self.cmd(c, a, true)
        }

        pub fn status(&mut self) -> Result<u16, String> {
            Ok(self.c(GET_VERSION, &[])?[3])
        }

        fn wait_for(&mut self, want_ready: bool, stop: &AtomicBool, limit: Duration) -> Result<(), String> {
            let t = Instant::now();
            loop {
                let s = self.status()?;
                let ok = if want_ready { s & READY != 0 } else { s & BUSY == 0 };
                if ok {
                    return Ok(());
                }
                if stop.load(Ordering::Relaxed) {
                    return Err("stopped".into());
                }
                if t.elapsed() > limit {
                    return Err("the galvo board didn't respond in time".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// The start-up sequence (MeerK40t's defaults), with the lens correction table if there is one.
        pub fn init(&mut self, s: &GalvoSettings) -> Result<(), String> {
            self.c(GET_SERIAL_NO, &[])?;
            self.c(GET_VERSION, &[])?;
            self.c(RESET, &[])?;
            let cor = if s.cor_file.trim().is_empty() { None } else { Some(std::fs::read(s.cor_file.trim()).map_err(|e| format!("can't read the correction file: {e}")).and_then(|b| read_cor(&b))?) };
            match cor {
                Some((table, _)) => {
                    self.c(WRITE_COR_TABLE, &[1])?;
                    for (i, (dx, dy)) in table.iter().enumerate() {
                        self.cmd(WRITE_COR_LINE, [*dx, *dy, if i == 0 { 0 } else { 1 }, 0, 0], false)?;
                    }
                }
                None => {
                    self.c(WRITE_COR_TABLE, &[0])?;
                }
            }
            self.c(ENABLE_LASER, &[])?;
            self.c(SET_CONTROL_MODE, &[0])?;
            self.c(SET_LASER_MODE, &[1])?;
            self.c(SET_DELAY_MODE, &[1])?;
            self.c(SET_TIMING, &[1])?;
            self.c(SET_STANDBY, &[2000, 20])?;
            self.c(SET_FIRST_PULSE_KILLER, &[200])?;
            self.c(SET_PWM_HALF_PERIOD, &[125])?;
            self.c(SET_PWM_PULSE_WIDTH, &[125])?;
            self.c(FIBER_SET_MO, &[0])?;
            self.c(SET_FPK_PARAM_2, &[0xFFB, 1, 409, 409])?;
            self.c(SET_FLY_RES, &[0, 99, 1000, 25])?;
            self.c(ENABLE_Z, &[])?;
            self.c(WRITE_ANALOG_PORT_1, &[0x7FF])?;
            self.c(ENABLE_Z, &[])?;
            std::thread::sleep(Duration::from_millis(50));
            Ok(())
        }

        /// Run a command list: reset, send each packet when the board is ready, start it, wait until done.
        pub fn run_list(&mut self, entries: &[[u16; 6]], s: &GalvoSettings, port: u16, progress: &AtomicU32, stop: &AtomicBool) -> Result<(), String> {
            let fiber = s.source == Source::Fiber && port & PORT_LASER != 0;
            self.c(RESET_LIST, &[])?;
            self.c(WRITE_PORT, &[port])?;
            if fiber {
                self.c(FIBER_SET_MO, &[1])?;
            }
            let packets = list_packets(entries);
            let mut executing = false;
            let result = (|| {
                for (i, p) in packets.iter().enumerate() {
                    self.wait_for(true, stop, Duration::from_secs(600))?;
                    self.write(p)?;
                    self.c(SET_END_OF_LIST, &[0])?;
                    if i + 1 > 2 && !executing {
                        self.c(EXECUTE_LIST, &[])?;
                        executing = true;
                    }
                    progress.store(((i + 1) * 1000 / packets.len().max(1)) as u32, Ordering::Relaxed);
                }
                if !executing {
                    self.c(EXECUTE_LIST, &[])?;
                }
                self.wait_for(false, stop, Duration::from_secs(24 * 3600))
            })();
            if result.is_err() {
                self.abort();
            }
            if fiber {
                let _ = self.c(FIBER_SET_MO, &[0]);
            }
            let _ = self.c(WRITE_PORT, &[0]);
            result
        }

        /// Stop whatever is running and leave the laser off.
        pub fn abort(&mut self) {
            let _ = self.c(STOP_EXECUTE, &[]);
            let _ = self.c(FIBER_SET_MO, &[0]);
            let _ = self.c(RESET_LIST, &[]);
            if let Some(p) = list_packets(&[[LIST_END_OF_LIST, 0, 0, 0, 0, 0]]).pop() {
                let _ = self.write(&p);
                let _ = self.c(EXECUTE_LIST, &[]);
            }
            let _ = self.c(FIBER_SET_MO, &[0]);
            let _ = self.c(WRITE_PORT, &[0]);
        }
    }

    fn running() -> Running {
        Running { progress: Arc::new(AtomicU32::new(0)), stop: Arc::new(AtomicBool::new(false)), result: Arc::new(Mutex::new(None)) }
    }

    /// Send a job (port = laser enabled) or a red-pointer frame (port = light), on a background thread.
    pub fn run(entries: Vec<[u16; 6]>, s: GalvoSettings, light: bool, done: String) -> Running {
        let r = running();
        let rr = r.clone();
        std::thread::spawn(move || {
            let port = if light { PORT_LIGHT } else { PORT_LASER };
            let out = Board::open().and_then(|mut b| {
                b.init(&s)?;
                b.run_list(&entries, &s, port, &rr.progress, &rr.stop)?;
                Ok(done)
            });
            let out = match out {
                Err(e) if e == "stopped" => Err("Stopped".to_string()),
                o => o,
            };
            *rr.result.lock().unwrap() = Some(out);
        });
        r
    }

    /// Stop the board (when no job of ours is running).
    pub fn stop() -> Running {
        let r = running();
        let rr = r.clone();
        std::thread::spawn(move || {
            let out = Board::open().map(|mut b| {
                b.abort();
                "Galvo stopped".to_string()
            });
            *rr.result.lock().unwrap() = Some(out);
        });
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_follow_meerk40ts_default_orientation() {
        let s = GalvoSettings::default();
        // the middle of the field is the galvo's centre
        assert_eq!(s.to_galvo(p2(55.0, 55.0)), Some((32768, 32768)));
        // default flip-Y + swap: galvo X follows bed Y, galvo Y follows bed X
        assert_eq!(s.to_galvo(p2(0.0, 0.0)), Some((0, 0)));
        assert_eq!(s.to_galvo(p2(110.0, 0.0)), Some((0, 65535)));
        assert_eq!(s.to_galvo(p2(0.0, 110.0)), Some((65535, 0)));
        assert_eq!(s.to_galvo(p2(120.0, 10.0)), None, "outside the lens field");
        let plain = GalvoSettings { flip_y: false, swap_xy: false, ..GalvoSettings::default() };
        assert_eq!(plain.to_galvo(p2(110.0, 110.0)), Some((65535, 0)));
        // 100 mm/s on a 110 mm lens: 100 · 65535/110 / 1000 = 59
        assert_eq!(s.speed(100.0), 59);
        assert_eq!(period(30.0, 20000.0), 667);
        assert_eq!(period(30.0, 10000.0), 333);
    }

    #[test]
    fn job_list_marks_with_settings_once() {
        let dev = Device { controller: crate::gcode::Controller::EzCad2, width: 110.0, height: 110.0, ..Device::default() };
        let mut doc = crate::model::Document::default();
        doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Rect { w: 20.0, h: 10.0, cr: 0.0 }, Xform::translate(55.0, 55.0), 0));
        doc.sync_cuts();
        doc.cut_mut(0).frequency = 50_000.0;
        let job = crate::gcode::build_job(&doc, &dev, None);
        let (entries, outside) = job_list(&job, &doc, &dev);
        assert_eq!(outside, 0);
        assert_eq!(entries[0][0], LIST_READY_MARK);
        assert_eq!(entries.last().unwrap()[0], LIST_END_OF_LIST);
        let count = |c: u16| entries.iter().filter(|e| e[0] == c).count();
        assert!(count(LIST_MARK_TO) >= 4, "the rectangle's sides are marked");
        assert_eq!(count(LIST_QSWITCH_PERIOD), 1, "frequency set once");
        assert_eq!(entries.iter().find(|e| e[0] == LIST_QSWITCH_PERIOD).unwrap()[1], 400, "50 kHz fiber → period 400");
        assert_eq!(count(LIST_LASER_ON_DELAY), 1);
        // the marks stay around the field's centre
        for e in entries.iter().filter(|e| e[0] == LIST_MARK_TO) {
            assert!((e[1] as i32 - 32768).abs() < 7000 && (e[2] as i32 - 32768).abs() < 7000);
        }
        let packets = list_packets(&entries);
        assert!(packets.iter().all(|p| p.len() == 0xC00));
        // red-pointer frame: light port, jumps only
        let f = frame_list(BBox { min: p2(40.0, 50.0), max: p2(70.0, 60.0) }, &dev.galvo, 3);
        assert!(f.iter().any(|e| e[0] == LIST_WRITE_PORT && e[1] == PORT_LIGHT));
        assert!(f.iter().all(|e| e[0] != LIST_MARK_TO));
        assert_eq!(f.iter().filter(|e| e[0] == LIST_JUMP_TO).count(), 13);
    }

    #[test]
    fn reads_both_correction_file_kinds() {
        // integer kind: 0x16 label bytes, 6 unknown, the scale, then 65×65 (dx, dy) i32 pairs
        let mut b = vec![0u8; 0x16 + 6];
        b.extend(600.0f64.to_le_bytes());
        for i in 0..65 * 65 {
            b.extend((i as i32 % 7 - 3).to_le_bytes());
            b.extend((-(i as i32 % 5)).to_le_bytes());
        }
        let (t, lens) = read_cor(&b).unwrap();
        assert_eq!(t.len(), 65 * 65);
        assert!((lens - 65536.0 / 600.0).abs() < 1e-9);
        assert_eq!(t[0], (0x8000 + 3, 0));
        assert_eq!(t[1], (0x8000 + 2, 0x8000 + 1));
        // float kind, labelled LMC1COR_1.0
        let mut f: Vec<u8> = "LMC1COR_1.0".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        f.resize(0x16 + 0x1FA, 0);
        f[0x16 + 2 + 43 * 8..0x16 + 2 + 44 * 8].copy_from_slice(&595.78f64.to_le_bytes());
        for _ in 0..65 * 65 {
            f.extend(1.6f64.to_le_bytes());
            f.extend((-2.4f64).to_le_bytes());
        }
        let (t, lens) = read_cor(&f).unwrap();
        assert_eq!(t[100], (2, 0x8000 + 2));
        assert!((lens - 110.0).abs() < 0.01);
        assert!(read_cor(&[0u8; 10]).is_err());
    }
}
