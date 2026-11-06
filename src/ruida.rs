//! Ruida DSP controllers (RDC644x, 6445, 6442...): build the binary job these controllers accept, save it as an
//! .rd file (for a USB stick or RDWorks), or send it over the network.
//!
//! Protocol notes (Ruida's protocol is community-documented; see https://edutechwiki.unige.ch/en/Ruida):
//! - numbers: long values are 5 bytes of 7 bits, most significant first; short values are 2 bytes of 7 bits;
//! - commands: 0x88 move to (x, y), 0xA8 cut to (x, y), 0xC9 0x02 speed, 0xC6 0x01 / 0xC6 0x02 laser-1 min / max
//!   power (power % × 163.83 → 0..16383), 0xCA 0x01 work mode / 0xCA 0x02 layer number, footer 0xE7 0x00 … 0xD7;
//! - scrambling ("swizzle"): each byte b → ((b & 0x7E) | b >> 7 | b << 7) ^ key, + 1, with key 0x88 by default;
//!   a network packet starts with the 16-bit sum of its scrambled bytes;
//! - network: the controller listens on UDP 50200 and answers on 40200.

use crate::gcode::{Device, Job};

pub const SWIZZLE_KEY: u8 = 136; // 0x88
pub const PORT_TO_LASER: u16 = 50200;
pub const PORT_FROM_LASER: u16 = 40200;
/// Power % → Ruida's 14-bit power value (0..16383).
pub const POWER_SCALE: f64 = 163.83;
/// Largest network packet payload.
const PACKET: usize = 1470;

fn long(out: &mut Vec<u8>, v: i64) {
    let v = v as u32;
    out.extend([((v >> 28) & 0x0f) as u8, ((v >> 21) & 0x7f) as u8, ((v >> 14) & 0x7f) as u8, ((v >> 7) & 0x7f) as u8, (v & 0x7f) as u8]);
}

fn short(out: &mut Vec<u8>, v: u16) {
    out.extend([((v >> 7) & 0x7f) as u8, (v & 0x7f) as u8]);
}

fn swizzle_byte(b: u8) -> u8 {
    let s = (b & 0x7e) | (b >> 7) | (b << 7);
    (s ^ SWIZZLE_KEY).wrapping_add(1)
}

fn unswizzle_byte(b: u8) -> u8 {
    let s = b.wrapping_sub(1) ^ SWIZZLE_KEY;
    (s & 0x7e) | (s >> 7) | (s << 7)
}

pub fn swizzle(data: &[u8]) -> Vec<u8> {
    data.iter().map(|b| swizzle_byte(*b)).collect()
}

/// The job as Ruida commands (not yet scrambled). Ruida machines count Y down from the back of the bed,
/// so Y is flipped against the bed height; coordinates and speeds are in µm and µm/s.
pub fn job_bytes(job: &Job, dev: &Device) -> Vec<u8> {
    let um = |v: f64| (v * 1000.0).round() as i64;
    let xy = |out: &mut Vec<u8>, x: f64, y: f64| {
        long(out, um(x));
        long(out, um(dev.height - y));
    };
    let mut out = Vec::new();
    out.extend([0xCA, 0x01, 0x00]);
    out.extend([0xCA, 0x02, 0x00]);
    let (mut speed, mut power) = (-1.0f64, -1.0f64);
    let mut at: Option<crate::geom::P2> = None;
    for m in &job.moves {
        if m.power <= 0.0 {
            out.push(0x88);
            xy(&mut out, m.to.x, m.to.y);
        } else {
            if at.map(|p| p.dist(m.from) > 1e-6).unwrap_or(true) {
                out.push(0x88);
                xy(&mut out, m.from.x, m.from.y);
            }
            if (m.speed - speed).abs() > 1e-9 {
                out.extend([0xC9, 0x02]);
                long(&mut out, um(m.speed));
                speed = m.speed;
            }
            let p = m.power as f64 * 100.0;
            if (p - power).abs() > 1e-9 {
                let v = (p * POWER_SCALE).round().clamp(0.0, 16383.0) as u16;
                out.extend([0xC6, 0x01]);
                short(&mut out, v);
                out.extend([0xC6, 0x02]);
                short(&mut out, v);
                power = p;
            }
            out.push(0xA8);
            xy(&mut out, m.to.x, m.to.y);
        }
        at = Some(m.to);
    }
    out.extend([0xE7, 0x00]);
    out.push(0xD7);
    out
}

/// An .rd file: the scrambled job.
pub fn rd_file(job: &Job, dev: &Device) -> Vec<u8> {
    swizzle(&job_bytes(job, dev))
}

/// Network packets: 16-bit sum of the scrambled payload, then the payload.
pub fn packets(job: &Job, dev: &Device) -> Vec<Vec<u8>> {
    rd_file(job, dev)
        .chunks(PACKET)
        .map(|c| {
            let sum: u16 = c.iter().fold(0u16, |s, b| s.wrapping_add(*b as u16));
            let mut p = vec![(sum >> 8) as u8, sum as u8];
            p.extend_from_slice(c);
            p
        })
        .collect()
}

/// Send a job to the controller at `ip` in the background; the result appears in the returned slot.
#[cfg(not(target_arch = "wasm32"))]
pub fn send(job: &Job, dev: &Device, ip: &str) -> std::sync::Arc<std::sync::Mutex<Option<Result<String, String>>>> {
    use std::net::UdpSocket;
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let out = slot.clone();
    let pkts = packets(job, dev);
    let ip = ip.trim().to_string();
    std::thread::spawn(move || {
        let run = || -> Result<String, String> {
            let sock = UdpSocket::bind(("0.0.0.0", PORT_FROM_LASER)).map_err(|e| format!("can't open UDP port {PORT_FROM_LASER}: {e}"))?;
            sock.set_read_timeout(Some(std::time::Duration::from_secs(4))).ok();
            let n = pkts.len();
            for (i, p) in pkts.iter().enumerate() {
                let mut ok = false;
                for _try in 0..3 {
                    sock.send_to(p, (ip.as_str(), PORT_TO_LASER)).map_err(|e| format!("couldn't reach {ip}: {e}"))?;
                    let mut buf = [0u8; 64];
                    match sock.recv_from(&mut buf) {
                        // 0xC6 = accepted; 0x46 = checksum error, send again
                        Ok((k, _)) if k > 0 && unswizzle_byte(buf[0]) == 0xC6 => {
                            ok = true;
                            break;
                        }
                        _ => continue,
                    }
                }
                if !ok {
                    return Err(format!("the laser at {ip} didn't accept packet {} of {n}", i + 1));
                }
            }
            Ok(format!("Sent {n} packet(s) to the Ruida controller at {ip}"))
        };
        *out.lock().unwrap() = Some(run());
    });
    slot
}

#[cfg(target_arch = "wasm32")]
pub fn send(_job: &Job, _dev: &Device, _ip: &str) -> std::sync::Arc<std::sync::Mutex<Option<Result<String, String>>>> {
    std::sync::Arc::new(std::sync::Mutex::new(Some(Err("Sending to a network laser isn't possible from the browser.".into()))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scrambling_round_trips_and_numbers_are_7_bit() {
        for b in 0..=255u8 {
            assert_eq!(unswizzle_byte(swizzle_byte(b)), b);
        }
        let mut v = Vec::new();
        long(&mut v, 100_000);
        assert_eq!(v, vec![0, 0, 6, 13, 32]);
        assert!(v.iter().all(|b| *b < 0x80));
    }
    #[test]
    fn a_square_becomes_moves_and_cuts() {
        let mut d = crate::model::Document::default();
        d.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, crate::geom::Xform::translate(50.0, 50.0), 0));
        d.sync_cuts();
        let dev = Device::default();
        let job = crate::gcode::build_job(&d, &dev, None);
        let raw = job_bytes(&job, &dev);
        assert_eq!(raw.iter().filter(|b| **b == 0xA8).count() >= 4, true);
        assert_eq!(*raw.last().unwrap(), 0xD7);
        let p = packets(&job, &dev);
        let sum: u16 = p[0][2..].iter().fold(0u16, |s, b| s.wrapping_add(*b as u16));
        assert_eq!(((p[0][0] as u16) << 8) | p[0][1] as u16, sum);
    }
}
