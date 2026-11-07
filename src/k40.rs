//! K40 lasers with the Lihuiyu "LHYMICRO-GL" M2 Nano controller (and its M, M1, B, B1, B2, A relatives):
//! encode a job in the board's EGV command language and send it over USB.
//!
//! Ported from K40 Whisperer by Scorch (www.scorchworks.com), version 0.68:
//! - `egv.py` (EGV encoding) and `nano_library.py` (USB packets), GNU GPL v2 or later;
//! - `LaserSpeed.py` (speed codes), MIT licence.
//! BurnCraft is GPL-3.0-or-later, which both licences allow.
//!
//! The board steps at 1000 per inch ("mils"). Its home is the back-left corner; a job starts and ends with the
//! head where it was, and BurnCraft keeps the head at home between jobs. Laser power is set with the knob on
//! the machine: the board only switches the laser on and off.

use crate::gcode::{Device, Job};
use crate::geom::*;

pub const VENDOR_ID: u16 = 0x1a86;
pub const PRODUCT_ID: u16 = 0x5512;
/// Board models, as K40 Whisperer names them ("LASER-M2" → "M2"). M2 is by far the most common.
pub const BOARDS: [&str; 7] = ["M2", "M1", "M", "B2", "B1", "B", "A"];
/// The usual K40 work area (mm).
pub const DEFAULT_WIDTH: f64 = 325.0;
pub const DEFAULT_HEIGHT: f64 = 220.0;
/// Travel moves shorter than this (mils, about 10 mm) are made with the laser off at cut speed instead of
/// stopping for a fast move: much quicker for fills, which hop between nearby scan segments.
const SHORT_HOP: i64 = 394;
/// Speed of the head while framing (mm/s).
pub const FRAME_SPEED: f64 = 30.0;

// ---------------------------------------------------------------------------------------------------------
// Speed codes (LaserSpeed.py)

fn gear_for_speed(mm_per_second: f64, uses_raster_step: bool) -> u32 {
    if mm_per_second <= 25.4 {
        return 1;
    }
    if mm_per_second <= 60.0 {
        return 2;
    }
    if !uses_raster_step {
        if mm_per_second < 127.0 {
            3
        } else {
            4
        }
    } else if mm_per_second < 127.0 {
        2
    } else if mm_per_second <= 320.0 {
        3
    } else {
        4
    }
}

/// (b, m, gear) of the board's linear speed equation value = m · period_ms + b.
fn gearing(board: &str, mm_per_second: f64, uses_raster_step: bool) -> (f64, f64, u32) {
    let gear = gear_for_speed(mm_per_second, uses_raster_step);
    let mut b_values = [64752.0, 64752.0, 64640.0, 64512.0];
    let mut m = -2000.0;
    if board.starts_with('M') {
        b_values = [60416.0, 60416.0, 59904.0, 59392.0];
        m = -12120.0;
    }
    if board == "B2" {
        m = -24240.0;
    }
    match board {
        "B2" if mm_per_second < 7.0 => return (b_values[0], m / 12.0, if uses_raster_step { 1 } else { 0 }),
        "M" if mm_per_second < 6.0 => return (b_values[0], m, 0),
        "M1" if mm_per_second < 6.0 || (!uses_raster_step && mm_per_second < 7.0) => return (b_values[0], m, 0),
        "M2" if mm_per_second < 7.0 => return (65528.0, m / 12.0, 0),
        _ => {}
    }
    (b_values[gear as usize - 1], m, gear)
}

/// A 16-bit value as two 3-digit numbers (high byte, low byte); negative values keep the board's 24-bit form.
fn encode_value(value: f64) -> String {
    let v = value.trunc() as i64;
    let b0 = v & 255;
    let b1 = (v >> 8) & 0xFF_FFFF;
    format!("{b1:03}{b0:03}")
}

/// The speed code for `mm_per_second` (vector cuts when `raster_step` is 0, otherwise raster rows `raster_step`
/// mils apart).
pub fn speed_code(mm_per_second: f64, raster_step: u32, board: &str) -> String {
    let mut mm_per_second = mm_per_second;
    if mm_per_second > 240.0 && raster_step == 0 {
        mm_per_second = 19.05; // out of range: the board's default speed
    }
    let (b, m, mut gear) = gearing(board, mm_per_second, raster_step != 0);
    let period_ms = if mm_per_second > 0.0 { 25.4 / mm_per_second } else { 0.0 };
    let mut speed_value = if mm_per_second > 0.0 { m * period_ms + b } else { b };
    if speed_value - speed_value.round_ties_even() > 0.005 {
        speed_value = speed_value.ceil();
    }
    let speed_value = speed_value.round_ties_even();
    let encoded = encode_value(speed_value);
    if raster_step != 0 {
        if gear == 0 {
            gear = 1;
        }
        return format!("V{encoded}{gear}G{raster_step:03}");
    }
    if matches!(board, "A" | "B" | "M") {
        return if gear == 0 { format!("CV{encoded}1C") } else { format!("CV{encoded}{gear}") };
    }
    let d_ratio = 0.261199033289;
    let step_value = ((mm_per_second.floor() as i64) + 1).min(128);
    let d_value = d_ratio * -m * period_ms / step_value as f64;
    let diag = encode_value(d_value);
    if gear == 0 {
        format!("CV{encoded}1{step_value:03}{diag}C")
    } else {
        format!("CV{encoded}{gear}{step_value:03}{diag}")
    }
}

// ---------------------------------------------------------------------------------------------------------
// EGV encoding (egv.py)

const RIGHT: u8 = b'B';
const LEFT: u8 = b'T';
const UP: u8 = b'L';
const DOWN: u8 = b'R';
const ANGLE: u8 = b'M';
const ON: u8 = b'D';
const OFF: u8 = b'U';

#[derive(Default)]
pub struct Egv {
    pub out: Vec<u8>,
    dir: u8,
    dist: i64,
    on: bool,
    ax: u8,
    ay: u8,
}

/// A distance in mils as EGV letters: 'z' = 255, 'a'..'y' = 1..25, '|a'..'|z' = 26..51, else three digits.
pub fn distance_code(mils: i64) -> Vec<u8> {
    let mut code = Vec::new();
    let mut rest = mils;
    while rest >= 255 {
        code.push(b'z');
        rest -= 255;
    }
    if rest == 0 {
    } else if rest < 26 {
        code.push(96 + rest as u8);
    } else if rest < 52 {
        code.push(b'|');
        code.push(96 + (rest - 25) as u8);
    } else {
        code.extend(format!("{rest:03}").bytes());
    }
    code
}

impl Egv {
    fn write(&mut self, b: u8) {
        self.out.push(b);
    }
    fn write_all(&mut self, s: &str) {
        self.out.extend(s.bytes());
    }

    fn move_(&mut self, direction: u8, distance: i64, laser_on: bool, angle_dirs: Option<(u8, u8)>) {
        let (ax, ay) = angle_dirs.unwrap_or((self.ax, self.ay));
        if direction == self.dir && laser_on == self.on && ax == self.ax && ay == self.ay {
            self.dist += distance;
            return;
        }
        self.flush(None);
        if laser_on != self.on {
            self.write(if laser_on { ON } else { OFF });
            self.on = laser_on;
        }
        if direction == ANGLE {
            if ax != self.ax {
                self.write(ax);
                self.ax = ax;
            }
            if ay != self.ay {
                self.write(ay);
                self.ay = ay;
            }
        }
        self.dir = direction;
        self.dist = distance;
        if direction == RIGHT || direction == LEFT {
            self.ax = direction;
        }
        if direction == UP || direction == DOWN {
            self.ay = direction;
        }
    }

    pub fn flush(&mut self, laser_on: Option<bool>) {
        if self.dist > 0 {
            let d = self.dir;
            self.write(d);
            let code = distance_code(self.dist);
            self.out.extend(code);
        }
        if let Some(on) = laser_on {
            if on != self.on {
                self.write(if on { ON } else { OFF });
                self.on = on;
            }
        }
        self.dist = 0;
    }

    /// Y first, then X, as separate straight moves.
    pub fn dir_dist(&mut self, dx: i64, dy: i64, laser_on: bool) {
        if dy > 0 {
            self.move_(UP, dy, laser_on, None);
        } else if dy < 0 {
            self.move_(DOWN, -dy, laser_on, None);
        }
        if dx > 0 {
            self.move_(RIGHT, dx, laser_on, None);
        } else if dx < 0 {
            self.move_(LEFT, -dx, laser_on, None);
        }
    }

    /// A straight line made of orthogonal and 45° steps that stay within half a mil of the true line.
    pub fn cut_line(&mut self, dx: i64, dy: i64, spindle: bool) {
        let xcode = if dx < 0 { LEFT } else { RIGHT };
        let ycode = if dy < 0 { DOWN } else { UP };
        if dx == 0 {
            self.move_(ycode, dy.abs(), spindle, None);
        } else if dy == 0 {
            self.move_(xcode, dx.abs(), spindle, None);
        } else if dx == dy {
            self.move_(ANGLE, dx.abs(), spindle, Some((xcode, ycode)));
        } else {
            let (adx, ady) = (dx.abs() as f64 / 1000.0, dy.abs() as f64 / 1000.0);
            let (slope, n, code) = if adx > ady { (ady / adx, dx.abs(), xcode) } else { (adx / ady, dy.abs(), ycode) };
            let mut lh = 0.0;
            let (mut d1, mut d2) = (0i64, 0i64);
            for i in 1..=n {
                let h = (i as f64 * slope).round_ties_even();
                if h == lh {
                    d1 += 1;
                    if d2 > 0 {
                        self.move_(ANGLE, d2, spindle, Some((xcode, ycode)));
                        d2 = 0;
                    }
                } else {
                    d2 += 1;
                    if d1 > 0 {
                        self.move_(code, d1, spindle, None);
                        d1 = 0;
                    }
                }
                lh = h;
            }
            if d1 > 0 {
                self.move_(code, d1, spindle, None);
            }
            if d2 > 0 {
                self.move_(ANGLE, d2, spindle, Some((xcode, ycode)));
            }
        }
    }

    /// A fast move: the head stops, travels at full speed, and restarts.
    pub fn rapid_move_fast(&mut self, dx: i64, dy: i64) {
        let mut pad = 3;
        if pad == -dx {
            pad += 3;
        }
        self.dir_dist(-pad, 0, false);
        self.dir_dist(0, pad, false);
        self.flush(Some(false));
        self.write(if dx + pad < 0 { RIGHT } else { LEFT });
        self.write(b'N');
        self.dir_dist(dx + pad, dy - pad, false);
        self.flush(Some(false));
        self.write_all("SE");
    }
}

/// One block of the job at one speed: `segments` are (end point in mils, laser on) from the head's position
/// `start`; the head returns to `start` at the end. Mirrors K40 Whisperer's vector output.
pub fn block(start: (i64, i64), first: (i64, i64), segments: &[((i64, i64), bool)], speed: f64, board: &str) -> Vec<u8> {
    let mut e = Egv::default();
    e.write_all(&speed_code(speed, 0, board));
    let (mut lastx, mut lasty) = first;
    e.dir_dist(lastx - start.0, lasty - start.1, false);
    e.flush(Some(false));
    e.write(b'N');
    e.write(if lasty - start.1 <= 0 { DOWN } else { UP });
    e.write(if lastx - start.0 >= 0 { RIGHT } else { LEFT });
    e.write_all("S1E");
    let min_rapid = 5;
    for &((x, y), cut) in segments {
        let (dx, dy) = (x - lastx, y - lasty);
        if dx != 0 || dy != 0 {
            if cut {
                e.cut_line(dx, dy, true);
            } else if (dx.abs() < min_rapid && dy.abs() < min_rapid) || (dx.abs() < SHORT_HOP && dy.abs() < SHORT_HOP) {
                e.dir_dist(dx, dy, false);
            } else {
                e.rapid_move_fast(dx, dy);
            }
        }
        lastx = x;
        lasty = y;
    }
    let (dx, dy) = (start.0 - lastx, start.1 - lasty);
    if dx.abs() < min_rapid && dy.abs() < min_rapid {
        e.dir_dist(dx, dy, false);
    } else {
        e.rapid_move_fast(dx, dy);
    }
    e.flush(Some(false));
    e.write_all("FNSE");
    e.out
}

/// mm (BurnCraft bed, Y up from the front) → mils.
pub fn mils(p: P2) -> (i64, i64) {
    ((p.x / 25.4 * 1000.0).round() as i64, (p.y / 25.4 * 1000.0).round() as i64)
}

/// Where the head waits between jobs: home, the back-left corner.
pub fn home_mils(dev: &Device) -> (i64, i64) {
    mils(p2(0.0, dev.height))
}

/// Join blocks the way K40 Whisperer does: "I", then each block, with every block's final "F" (finish) turned
/// into "@" (carry on) when another block follows.
pub fn join(blocks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut data = vec![b'I'];
    for b in blocks {
        if data.len() > 4 {
            let n = data.len();
            data[n - 4] = b'@';
        }
        data.extend(b);
    }
    data
}

/// The whole job as EGV data: one block per run of moves at the same speed.
pub fn job_egv(job: &Job, dev: &Device) -> Vec<u8> {
    let start = home_mils(dev);
    let mut blocks = Vec::new();
    let mut i = 0;
    let moves = &job.moves;
    while i < moves.len() {
        // a block starts at the first cut of a run at one speed
        while i < moves.len() && moves[i].power <= 0.0 {
            i += 1;
        }
        if i >= moves.len() {
            break;
        }
        let speed = moves[i].speed;
        let first = mils(moves[i].from);
        let mut segs = Vec::new();
        while i < moves.len() && (moves[i].power <= 0.0 || (moves[i].speed - speed).abs() < 1e-9) {
            segs.push((mils(moves[i].to), moves[i].power > 0.0));
            i += 1;
        }
        // trailing travel isn't needed: the block returns home itself
        while matches!(segs.last(), Some((_, false))) {
            segs.pop();
        }
        blocks.push(block(start, first, &segs, speed, &dev.board));
    }
    join(blocks)
}

/// Trace the job's bounding box with the laser off.
pub fn frame_egv(b: BBox, dev: &Device) -> Vec<u8> {
    let start = home_mils(dev);
    let c = [p2(b.min.x, b.max.y), p2(b.max.x, b.max.y), p2(b.max.x, b.min.y), p2(b.min.x, b.min.y), p2(b.min.x, b.max.y)];
    let segs: Vec<((i64, i64), bool)> = c[1..].iter().map(|p| (mils(*p), false)).collect();
    // the frame's sides are long moves, so make them as laser-off cuts (steady speed, no stops)
    let mut e = Egv::default();
    e.write_all(&speed_code(FRAME_SPEED, 0, &dev.board));
    let first = mils(c[0]);
    e.dir_dist(first.0 - start.0, first.1 - start.1, false);
    e.flush(Some(false));
    e.write(b'N');
    e.write(if first.1 - start.1 <= 0 { DOWN } else { UP });
    e.write(if first.0 - start.0 >= 0 { RIGHT } else { LEFT });
    e.write_all("S1E");
    let mut last = first;
    for (p, _) in segs {
        e.cut_line(p.0 - last.0, p.1 - last.1, false);
        last = p;
    }
    e.rapid_move_fast(start.0 - last.0, start.1 - last.1);
    e.flush(Some(false));
    e.write_all("FNSE");
    join(vec![e.out])
}

/// An .egv file (the format K40 Whisperer and LaserDRW save).
pub fn egv_file(data: &[u8]) -> Vec<u8> {
    let mut out = b"Document type : LHYMICRO-GL file\nCreator-Software: BurnCraft\n\n%0%0%0%0%".to_vec();
    out.extend_from_slice(data);
    out
}

// ---------------------------------------------------------------------------------------------------------
// USB packets (nano_library.py)

/// Status codes in the second byte of the board's answer.
pub const OK: u8 = 206;
pub const BUFFER_FULL: u8 = 238;
pub const CRC_ERROR: u8 = 207;
pub const TASK_COMPLETE: u8 = 236;
pub const TASK_COMPLETE_M3: u8 = 204;
pub const UNKNOWN_2: u8 = 239;

/// Dallas/Maxim one-wire CRC-8 (reflected polynomial 0x8C), as the board checks each packet.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        let mut inbyte = b;
        for _ in 0..8 {
            let mix = (crc ^ inbyte) & 1;
            crc >>= 1;
            if mix != 0 {
                crc ^= 0x8C;
            }
            inbyte >>= 1;
        }
    }
    crc
}

/// Split data into the board's 34-byte packets: 166, 0, 30 data bytes (padded with 'F'), 166, CRC of the 30.
pub fn packets(data: &[u8]) -> Vec<[u8; 34]> {
    let mut out = Vec::new();
    for chunk in data.chunks(30) {
        let mut p = [b'F'; 34];
        p[0] = 166;
        p[1] = 0;
        p[2..2 + chunk.len()].copy_from_slice(chunk);
        p[32] = 166;
        p[33] = crc8(&p[2..32]);
        out.push(p);
    }
    // a full last packet is followed by an empty one, as K40 Whisperer does
    if data.len() % 30 == 0 {
        let mut p = [b'F'; 34];
        p[0] = 166;
        p[1] = 0;
        p[32] = 166;
        p[33] = crc8(&p[2..32]);
        out.push(p);
    }
    out
}

/// A one-off command packet ("IPP" home, "IS2P" unlock the rail, "I" + "FF.." stop).
pub fn command_packet(cmd: &str) -> [u8; 34] {
    let mut p = [b'F'; 34];
    p[0] = 166;
    p[1] = 0;
    p[2..2 + cmd.len()].copy_from_slice(cmd.as_bytes());
    p[32] = 166;
    p[33] = crc8(&p[2..32]);
    p
}

#[cfg(not(target_arch = "wasm32"))]
pub mod usb {
    use super::*;
    use nusb::transfer::{Bulk, ControlOut, ControlType, In, Out, Recipient};
    use nusb::MaybeFuture;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_millis(200);
    const TRIES: u32 = 10;

    pub struct Board {
        out: nusb::Endpoint<Bulk, Out>,
        inp: nusb::Endpoint<Bulk, In>,
    }

    impl Board {
        /// Find and open the first M2 Nano (USB 1a86:5512).
        pub fn open() -> Result<Board, String> {
            let info = nusb::list_devices()
                .wait()
                .map_err(|e| e.to_string())?
                .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
                .ok_or("No K40 (M2 Nano, USB 1a86:5512) found. Check the USB cable and that the laser is switched on.")?;
            let dev = info.open().wait().map_err(|e| format!("can't open the K40's USB device: {e}{}", permission_hint()))?;
            // Linux binds the board to its ch341 serial driver: take it over
            #[cfg(target_os = "linux")]
            let iface = dev.detach_and_claim_interface(0).wait().map_err(|e| format!("can't claim the K40's USB interface: {e}{}", permission_hint()))?;
            #[cfg(not(target_os = "linux"))]
            let iface = dev.claim_interface(0).wait().map_err(|e| format!("can't claim the K40's USB interface: {e}{}", permission_hint()))?;
            iface
                .control_out(ControlOut { control_type: ControlType::Vendor, recipient: Recipient::Device, request: 177, value: 0x0102, index: 0, data: &[] }, Duration::from_millis(2000))
                .wait()
                .map_err(|e| format!("the K40 didn't accept the start-up request: {e}"))?;
            let out = iface.endpoint::<Bulk, Out>(0x02).map_err(|e| e.to_string())?;
            let inp = iface.endpoint::<Bulk, In>(0x82).map_err(|e| e.to_string())?;
            let mut b = Board { out, inp };
            b.status();
            Ok(b)
        }

        fn write(&mut self, p: &[u8]) -> Result<(), String> {
            let c = self.out.transfer_blocking(p.to_vec().into(), TIMEOUT);
            c.status.map_err(|e| e.to_string())
        }

        /// Ask how the board is doing ("hello" = 160); the answer's second byte is the status.
        pub fn status(&mut self) -> Option<u8> {
            let mut sent = false;
            for _ in 0..TRIES {
                if self.write(&[160]).is_ok() {
                    sent = true;
                    break;
                }
            }
            if !sent {
                return None;
            }
            for _ in 0..TRIES {
                let buf = self.inp.allocate(168);
                let c = self.inp.transfer_blocking(buf, TIMEOUT);
                if c.status.is_ok() && c.actual_len >= 2 {
                    return Some(c.buffer[1]);
                }
            }
            None
        }

        /// Send one packet, waiting while the board's buffer is full and resending on CRC errors.
        fn send_checked(&mut self, p: &[u8; 34], stop: &AtomicBool) -> Result<(), String> {
            let mut timeouts = 0;
            let mut crc_errors = 0;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return Err("stopped".into());
                }
                while self.status() == Some(BUFFER_FULL) {
                    if stop.load(Ordering::Relaxed) {
                        return Err("stopped".into());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                if self.write(p).is_err() {
                    timeouts += 1;
                    if timeouts >= TRIES * 2 {
                        return Err(format!("the laser isn't responding ({timeouts} USB timeouts)"));
                    }
                    continue;
                }
                match self.status() {
                    Some(CRC_ERROR) => {
                        crc_errors += 1;
                        if crc_errors >= TRIES {
                            return Err(format!("too many data errors ({crc_errors}); check the USB cable"));
                        }
                    }
                    _ => return Ok(()),
                }
            }
        }

        pub fn send(&mut self, data: &[u8], progress: &AtomicU32, stop: &AtomicBool) -> Result<(), String> {
            let ps = packets(data);
            for (i, p) in ps.iter().enumerate() {
                if let Err(e) = self.send_checked(p, stop) {
                    let _ = self.write(&command_packet("I"));
                    return Err(e);
                }
                progress.store(((i + 1) * 1000 / ps.len().max(1)) as u32, Ordering::Relaxed);
            }
            Ok(())
        }

        /// Wait until the board reports the job finished.
        pub fn wait_finished(&mut self, stop: &AtomicBool) {
            loop {
                if stop.load(Ordering::Relaxed) {
                    let _ = self.write(&command_packet("I"));
                    return;
                }
                match self.status() {
                    Some(TASK_COMPLETE) | Some(TASK_COMPLETE_M3) | None => return,
                    _ => std::thread::sleep(Duration::from_millis(250)),
                }
            }
        }

        pub fn command(&mut self, cmd: &str) -> Result<(), String> {
            self.write(&command_packet(cmd))
        }
    }

    fn permission_hint() -> &'static str {
        if cfg!(target_os = "linux") {
            " (on Linux, allow your user to use it: add a udev rule for 1a86:5512, see Help → Devices)"
        } else if cfg!(target_os = "windows") {
            " (on Windows, install the WinUSB driver for the K40 with Zadig, see Help → Devices)"
        } else {
            ""
        }
    }

    /// A job running in the background: progress in tenths of a percent, a stop flag, and the outcome.
    #[derive(Clone)]
    pub struct Running {
        pub progress: Arc<AtomicU32>,
        pub stop: Arc<AtomicBool>,
        pub result: Arc<Mutex<Option<Result<String, String>>>>,
    }

    /// Send EGV data on a background thread; `done` is the message shown when it finishes.
    pub fn run(data: Vec<u8>, done: String, wait: bool) -> Running {
        let r = Running { progress: Arc::new(AtomicU32::new(0)), stop: Arc::new(AtomicBool::new(false)), result: Arc::new(Mutex::new(None)) };
        let rr = r.clone();
        std::thread::spawn(move || {
            let out = Board::open().and_then(|mut b| {
                b.send(&data, &rr.progress, &rr.stop)?;
                if wait {
                    b.wait_finished(&rr.stop);
                }
                if rr.stop.load(Ordering::Relaxed) {
                    return Err("Stopped".into());
                }
                Ok(done)
            });
            *rr.result.lock().unwrap() = Some(out);
        });
        r
    }

    /// One-off commands on a background thread: "IPP" home, "IS2P" unlock the rail, "I" stop.
    pub fn command(cmd: &'static str, done: String) -> Running {
        let r = Running { progress: Arc::new(AtomicU32::new(0)), stop: Arc::new(AtomicBool::new(false)), result: Arc::new(Mutex::new(None)) };
        let rr = r.clone();
        std::thread::spawn(move || {
            let out = Board::open().and_then(|mut b| b.command(cmd)).map(|_| done);
            *rr.result.lock().unwrap() = Some(out);
        });
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_codes_follow_the_documented_equation() {
        // on M boards a 2 ms step delay (12.7 mm/s) is 60416 − 12120·2 = 36176: high byte 141, low byte 080, gear 1
        assert_eq!(speed_code(12.7, 0, "M"), "CV1410801");
        // the M2 adds its diagonal correction: step floor(12.7)+1 = 13, 0.261199·12120·2/13 = 487 → 001 231
        assert_eq!(speed_code(12.7, 0, "M2"), "CV1410801013001231");
        // below 7 mm/s the M2 uses its slow gear (b 65528, m −1010) and the C suffix
        assert_eq!(speed_code(5.0, 0, "M2"), "CV2352381006000223C");
        // A/B boards: 64752 − 2000·period (1 ms at 25.4 mm/s → 62752 = 245 032)
        assert_eq!(speed_code(25.4, 0, "B"), "CV2450321");
        // raster rows use the G form
        let r = speed_code(50.0, 2, "M2");
        assert!(r.starts_with('V') && r.ends_with("G002"));
    }

    #[test]
    fn distances_and_crc() {
        assert_eq!(distance_code(1), b"a");
        assert_eq!(distance_code(25), b"y");
        assert_eq!(distance_code(26), b"|a");
        assert_eq!(distance_code(51), b"|z");
        assert_eq!(distance_code(52), b"052");
        assert_eq!(distance_code(255), b"z");
        assert_eq!(distance_code(600), b"zz090");
        // K40 Whisperer's fixed packets carry these CRCs
        assert_eq!(command_packet("IPP")[33], 228);
        assert_eq!(command_packet("IS2P")[33], 15);
        assert_eq!(command_packet("I")[33], 130);
        assert_eq!(command_packet("")[33], 80);
    }

    /// Replay EGV movement: the head path as (x, y, laser on) after each step, in mils.
    fn replay(data: &[u8], start: (i64, i64)) -> Vec<(i64, i64, bool)> {
        let (mut x, mut y, mut on) = (start.0, start.1, false);
        let (mut ax, mut ay, mut dir) = (RIGHT, UP, 0u8);
        let mut path = Vec::new();
        let mut i = 0;
        let mut go = |d: u8, n: i64, ax: u8, ay: u8, x: &mut i64, y: &mut i64| match d {
            RIGHT => *x += n,
            LEFT => *x -= n,
            UP => *y += n,
            DOWN => *y -= n,
            ANGLE => {
                *x += if ax == RIGHT { n } else { -n };
                *y += if ay == UP { n } else { -n };
            }
            _ => {}
        };
        while i < data.len() {
            let c = data[i];
            i += 1;
            let n = match c {
                b'C' | b'V' => {
                    while i < data.len() && (data[i].is_ascii_digit() || data[i] == b'V') {
                        i += 1;
                    }
                    if i < data.len() && data[i] == b'C' {
                        i += 1;
                    }
                    continue;
                }
                ON => {
                    on = true;
                    continue;
                }
                OFF => {
                    on = false;
                    continue;
                }
                RIGHT | LEFT => {
                    dir = c;
                    ax = c;
                    continue;
                }
                UP | DOWN => {
                    dir = c;
                    ay = c;
                    continue;
                }
                ANGLE => {
                    dir = ANGLE;
                    continue;
                }
                b'a'..=b'y' => (c - 96) as i64,
                b'z' => 255,
                b'|' => {
                    i += 1;
                    (data[i - 1] - 96) as i64 + 25
                }
                b'0'..=b'9' => {
                    i += 2;
                    std::str::from_utf8(&data[i - 3..i]).unwrap().parse().unwrap()
                }
                b'S' => {
                    if data.get(i) == Some(&b'1') {
                        i += 1; // "S1E": start, not a distance
                    }
                    continue;
                }
                _ => continue, // I, N, E, F, @: modes, no movement
            };
            go(dir, n, ax, ay, &mut x, &mut y);
            path.push((x, y, on));
        }
        path
    }

    #[test]
    fn vector_block_draws_the_shapes_and_returns_home() {
        let start = (0, 8661);
        let first = (1000, 7000);
        let segs = [((2000, 7000), true), ((2000, 6000), true), ((1000, 6000), true), ((1000, 7000), true), ((3000, 5000), false), ((3700, 5300), true), ((3100, 4200), true)];
        let data = block(start, first, &segs, 20.0, "M2");
        let path = replay(&data, start);
        let last = *path.last().unwrap();
        assert_eq!((last.0, last.1), start, "back where it started");
        assert!(!last.2, "laser off at the end");
        for (p, _) in segs.iter() {
            assert!(path.iter().any(|q| (q.0, q.1) == *p), "never reached {p:?}");
        }
        // no burning during the jump between the square and the second shape
        assert!(path.iter().filter(|q| q.2).all(|q| !(q.0 > 2000 && q.0 < 3000 && q.1 > 5000 && q.1 < 6000)));
        // the sloped cut stays within a mil of the true line from (3000, 5000) to (3700, 5300)
        let from = path.iter().position(|q| (q.0, q.1) == (3000, 5000)).unwrap();
        let to = path.iter().position(|q| (q.0, q.1) == (3700, 5300)).unwrap();
        for q in &path[from..=to] {
            let t = (q.0 - 3000) as f64 / 700.0;
            assert!((q.1 as f64 - (5000.0 + 300.0 * t)).abs() <= 1.0, "off the line at {q:?}");
        }
        assert!(data.starts_with(speed_code(20.0, 0, "M2").as_bytes()));
        assert!(data.ends_with(b"FNSE"));
    }

    #[test]
    fn whole_project_returns_home() {
        let dev = Device { controller: crate::gcode::Controller::K40Nano, width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT, ..Device::default() };
        let doc = crate::app::demo_document(&dev);
        let job = crate::gcode::build_job(&doc, &dev, None);
        let data = job_egv(&job, &dev);
        assert!(data.starts_with(b"ICV") && data.ends_with(b"FNSE"));
        // replay each block: every block starts and ends at home
        let home = home_mils(&dev);
        let path = replay(&data, home);
        let last = *path.last().unwrap();
        assert_eq!((last.0, last.1), home);
        // nothing goes off the bed
        let (w, h) = mils(p2(DEFAULT_WIDTH, DEFAULT_HEIGHT));
        assert!(path.iter().all(|q| q.0 >= -10 && q.0 <= w + 10 && q.1 >= -10 && q.1 <= h + 10));
        let frame = frame_egv(BBox { min: p2(10.0, 10.0), max: p2(50.0, 40.0) }, &dev);
        let fp = replay(&frame, home);
        assert!(fp.iter().all(|q| !q.2), "framing never fires the laser");
        assert_eq!((fp.last().unwrap().0, fp.last().unwrap().1), home);
    }

    #[test]
    fn packets_and_joining() {
        let ps = packets(&[b'I'; 31]);
        assert_eq!(ps.len(), 2);
        assert_eq!((ps[1][2], ps[1][3]), (b'I', b'F'));
        assert_eq!(ps[0][33], crc8(&ps[0][2..32]));
        assert_eq!(packets(&[b'I'; 30]).len(), 2, "a full packet is followed by an empty one");
        let j = join(vec![b"CV1FNSE".to_vec(), b"CV2FNSE".to_vec()]);
        assert_eq!(String::from_utf8_lossy(&j), "ICV1@NSECV2FNSE");
    }
}
