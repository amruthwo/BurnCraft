//! Camera overlay: a picture of the bed from a camera above (or beside) the laser, warped onto the canvas so
//! designs can be placed on the real material.
//!
//! Sources: a USB camera (Linux, through V4L2), a network camera's snapshot address (any computer), or a photo
//! file. Alignment: click four points in the picture whose bed positions you know (by default the corners of
//! the work area); the picture is then mapped onto the bed with a perspective transform (homography).

use crate::geom::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    /// "usb:/dev/video0", "url:http://…", or "" for none / a photo.
    pub source: String,
    pub url: String,
    /// The four alignment points in the picture, as fractions of its width and height (0..1, y down).
    pub image_pts: [[f64; 2]; 4],
    /// Where those points are on the bed (mm). None = the corners of the work area.
    pub bed_pts: Option<[[f64; 2]; 4]>,
    pub aligned: bool,
    pub opacity: f32,
    pub show: bool,
    /// Seconds between pictures when live (network cameras).
    pub interval: f64,
}

impl Default for CameraSettings {
    fn default() -> Self {
        CameraSettings { source: String::new(), url: String::new(), image_pts: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], bed_pts: None, aligned: false, opacity: 0.85, show: true, interval: 1.0 }
    }
}

impl CameraSettings {
    /// The bed positions of the four alignment points: front-left, front-right, back-right, back-left by default.
    pub fn bed(&self, w: f64, h: f64) -> [P2; 4] {
        match self.bed_pts {
            Some(b) => b.map(|q| p2(q[0], q[1])),
            None => [p2(0.0, 0.0), p2(w, 0.0), p2(w, h), p2(0.0, h)],
        }
    }
}

/// A picture as RGBA bytes.
#[derive(Clone)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    /// Counts up with every new picture.
    pub seq: u64,
}

pub type Latest = Arc<Mutex<Option<Result<Frame, String>>>>;

/// Decode a JPEG / PNG / … picture.
pub fn decode(bytes: &[u8], seq: u64) -> Result<Frame, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a picture BurnCraft can read ({e})"))?.to_rgba8();
    Ok(Frame { w: img.width() as usize, h: img.height() as usize, rgba: img.into_raw(), seq })
}

// ---------------------------------------------------------------------------------------------------------
// Perspective transform

/// 3×3 matrix mapping (x, y, 1) → (u, v, w); point = (u/w, v/w).
#[derive(Clone, Copy, Debug)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// The transform taking each `from[i]` to `to[i]` (four points, no three on a line).
    pub fn between(from: &[P2; 4], to: &[P2; 4]) -> Option<Homography> {
        // h = [a b c d e f g h], with i = 1:  u = (a x + b y + c) / (g x + h y + 1), v = (d x + e y + f) / (…)
        let mut m = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y, u, v) = (from[i].x, from[i].y, to[i].x, to[i].y);
            m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        // Gaussian elimination with partial pivoting
        for c in 0..8 {
            let p = (c..8).max_by(|&a, &b| m[a][c].abs().partial_cmp(&m[b][c].abs()).unwrap())?;
            if m[p][c].abs() < 1e-12 {
                return None;
            }
            m.swap(c, p);
            for r in 0..8 {
                if r != c {
                    let f = m[r][c] / m[c][c];
                    for k in c..9 {
                        m[r][k] -= f * m[c][k];
                    }
                }
            }
        }
        let h: Vec<f64> = (0..8).map(|i| m[i][8] / m[i][i]).collect();
        Some(Homography([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0]))
    }

    pub fn apply(&self, p: P2) -> P2 {
        let h = &self.0;
        let w = h[6] * p.x + h[7] * p.y + h[8];
        p2((h[0] * p.x + h[1] * p.y + h[2]) / w, (h[3] * p.x + h[4] * p.y + h[5]) / w)
    }
}

/// Bed point → picture position (fractions of width / height) for the current alignment.
pub fn bed_to_image(s: &CameraSettings, bed_w: f64, bed_h: f64) -> Option<Homography> {
    let img = s.image_pts.map(|q| p2(q[0], q[1]));
    Homography::between(&s.bed(bed_w, bed_h), &img)
}

// ---------------------------------------------------------------------------------------------------------
// Capture

/// A camera feeding pictures on a background thread until stopped.
pub struct Capture {
    pub latest: Latest,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Capture {
    /// Start reading pictures from `source` ("usb:/dev/video0" or "url:http://…").
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(source: &str, interval: f64) -> Capture {
        use std::sync::atomic::{AtomicBool, Ordering};
        let latest: Latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (l, st) = (latest.clone(), stop.clone());
        let source = source.to_string();
        std::thread::spawn(move || {
            let put = |r: Result<Frame, String>| *l.lock().unwrap() = Some(r);
            if let Some(url) = source.strip_prefix("url:") {
                let mut seq = 0;
                while !st.load(Ordering::Relaxed) {
                    seq += 1;
                    put(fetch_url(url).and_then(|b| decode(&b, seq)));
                    std::thread::sleep(std::time::Duration::from_secs_f64(interval.clamp(0.2, 60.0)));
                }
            } else if let Some(dev) = source.strip_prefix("usb:") {
                #[cfg(target_os = "linux")]
                {
                    match v4l2::Camera::open(dev) {
                        Ok(mut cam) => {
                            let mut seq = 0;
                            while !st.load(Ordering::Relaxed) {
                                match cam.frame() {
                                    Ok(f) => {
                                        seq += 1;
                                        let r = match f {
                                            v4l2::Raw::Jpeg(b) => decode(&b, seq),
                                            v4l2::Raw::Yuyv(w, h, b) => Ok(Frame { w, h, rgba: yuyv_to_rgba(&b, w, h), seq }),
                                        };
                                        put(r);
                                    }
                                    Err(e) => {
                                        put(Err(e));
                                        break;
                                    }
                                }
                                std::thread::sleep(std::time::Duration::from_millis(150));
                            }
                        }
                        Err(e) => put(Err(e)),
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = dev;
                    put(Err("USB cameras are read directly on Linux for now; on this computer use a network camera address or a photo.".into()));
                }
            }
        });
        Capture { latest, stop }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| format!("couldn't reach the camera: {e}"))?;
    let mut body = resp.into_body();
    body.with_config().limit(40 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

/// USB cameras that can be opened (Linux: /dev/video*).
pub fn usb_cameras() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        let mut v: Vec<String> = std::fs::read_dir("/dev")
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().to_string()).filter(|p| p.starts_with("/dev/video")).collect())
            .unwrap_or_default();
        v.sort_by_key(|p| p.trim_start_matches("/dev/video").parse::<u32>().unwrap_or(0));
        v
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Packed YUYV (4:2:2) → RGBA.
pub fn yuyv_to_rgba(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![255u8; w * h * 4];
    let conv = |y: f32, u: f32, v: f32| -> [u8; 3] {
        let (c, d, e) = (y - 16.0, u - 128.0, v - 128.0);
        let r = 1.164 * c + 1.596 * e;
        let g = 1.164 * c - 0.392 * d - 0.813 * e;
        let b = 1.164 * c + 2.017 * d;
        [r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8]
    };
    for (i, q) in src.chunks_exact(4).enumerate().take(w * h / 2) {
        let (y0, u, y1, v) = (q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32);
        let a = conv(y0, u, v);
        let b = conv(y1, u, v);
        out[i * 8..i * 8 + 3].copy_from_slice(&a);
        out[i * 8 + 4..i * 8 + 7].copy_from_slice(&b);
    }
    out
}

/// Video4Linux2 capture with memory-mapped buffers: MJPEG if the camera offers it, else YUYV.
#[cfg(target_os = "linux")]
mod v4l2 {
    use std::os::fd::AsRawFd;

    // ioctl numbers (generic Linux encoding, 64-bit struct sizes: format 208, buffer 88, requestbuffers 20)
    const VIDIOC_S_FMT: libc::c_ulong = 0xc0d0_5605;
    const VIDIOC_REQBUFS: libc::c_ulong = 0xc014_5608;
    const VIDIOC_QUERYBUF: libc::c_ulong = 0xc058_5609;
    const VIDIOC_QBUF: libc::c_ulong = 0xc058_560f;
    const VIDIOC_DQBUF: libc::c_ulong = 0xc058_5611;
    const VIDIOC_STREAMON: libc::c_ulong = 0x4004_5612;
    const VIDIOC_STREAMOFF: libc::c_ulong = 0x4004_5613;
    const CAPTURE: u32 = 1;
    const MMAP: u32 = 1;
    const MJPG: u32 = u32::from_le_bytes(*b"MJPG");
    const YUYV: u32 = u32::from_le_bytes(*b"YUYV");

    #[repr(C)]
    struct Format {
        kind: u32,
        _pad: u32,
        width: u32,
        height: u32,
        pixelformat: u32,
        field: u32,
        bytesperline: u32,
        sizeimage: u32,
        rest: [u32; 44],
    }

    #[repr(C)]
    struct ReqBufs {
        count: u32,
        kind: u32,
        memory: u32,
        capabilities: u32,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Buffer {
        index: u32,
        kind: u32,
        bytesused: u32,
        flags: u32,
        field: u32,
        _pad: u32,
        timestamp: [i64; 2],
        timecode: [u32; 4],
        sequence: u32,
        memory: u32,
        offset: u64,
        length: u32,
        reserved2: u32,
        request_fd: u32,
        _pad2: u32,
    }

    pub enum Raw {
        Jpeg(Vec<u8>),
        Yuyv(usize, usize, Vec<u8>),
    }

    pub struct Camera {
        file: std::fs::File,
        maps: Vec<(*mut libc::c_void, usize)>,
        format: u32,
        w: usize,
        h: usize,
    }

    // the mapped buffers belong to this camera only
    unsafe impl Send for Camera {}

    fn ioctl<T>(fd: i32, req: libc::c_ulong, arg: &mut T) -> Result<(), String> {
        loop {
            let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
            if r == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EINTR) {
                return Err(e.to_string());
            }
        }
    }

    impl Camera {
        pub fn open(path: &str) -> Result<Camera, String> {
            assert_eq!(std::mem::size_of::<Format>(), 208);
            assert_eq!(std::mem::size_of::<Buffer>(), 88);
            assert_eq!(std::mem::size_of::<ReqBufs>(), 20);
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("can't open {path}: {e}"))?;
            let fd = file.as_raw_fd();
            let mut got = None;
            for pf in [MJPG, YUYV] {
                let mut f: Format = unsafe { std::mem::zeroed() };
                f.kind = CAPTURE;
                f.width = 1280;
                f.height = 720;
                f.pixelformat = pf;
                if ioctl(fd, VIDIOC_S_FMT, &mut f).is_ok() && f.pixelformat == pf {
                    got = Some((pf, f.width as usize, f.height as usize));
                    break;
                }
            }
            let (format, w, h) = got.ok_or(format!("{path} doesn't offer MJPEG or YUYV pictures"))?;
            let mut rb = ReqBufs { count: 4, kind: CAPTURE, memory: MMAP, capabilities: 0, flags: 0 };
            ioctl(fd, VIDIOC_REQBUFS, &mut rb).map_err(|e| format!("{path}: {e}"))?;
            let mut maps = Vec::new();
            for i in 0..rb.count {
                let mut b: Buffer = unsafe { std::mem::zeroed() };
                b.index = i;
                b.kind = CAPTURE;
                b.memory = MMAP;
                ioctl(fd, VIDIOC_QUERYBUF, &mut b)?;
                let p = unsafe { libc::mmap(std::ptr::null_mut(), b.length as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, b.offset as libc::off_t) };
                if p == libc::MAP_FAILED {
                    return Err("couldn't map the camera's buffers".into());
                }
                maps.push((p, b.length as usize));
                ioctl(fd, VIDIOC_QBUF, &mut b)?;
            }
            let mut t: i32 = CAPTURE as i32;
            ioctl(fd, VIDIOC_STREAMON, &mut t)?;
            Ok(Camera { file, maps, format, w, h })
        }

        /// The next picture (waits up to 2 s).
        pub fn frame(&mut self) -> Result<Raw, String> {
            let fd = self.file.as_raw_fd();
            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut pfd, 1, 2000) } <= 0 {
                return Err("the camera stopped sending pictures".into());
            }
            let mut b: Buffer = unsafe { std::mem::zeroed() };
            b.kind = CAPTURE;
            b.memory = MMAP;
            ioctl(fd, VIDIOC_DQBUF, &mut b)?;
            let (p, len) = self.maps[b.index as usize];
            let n = (b.bytesused as usize).min(len);
            let data = unsafe { std::slice::from_raw_parts(p as *const u8, n) }.to_vec();
            ioctl(fd, VIDIOC_QBUF, &mut b)?;
            Ok(if self.format == MJPG { Raw::Jpeg(data) } else { Raw::Yuyv(self.w, self.h, data) })
        }
    }

    impl Drop for Camera {
        fn drop(&mut self) {
            let mut t: i32 = CAPTURE as i32;
            let _ = ioctl(self.file.as_raw_fd(), VIDIOC_STREAMOFF, &mut t);
            for (p, len) in &self.maps {
                unsafe {
                    libc::munmap(*p, *len);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homography_maps_corners_and_keeps_lines_straight() {
        let bed = [p2(0.0, 0.0), p2(300.0, 0.0), p2(300.0, 200.0), p2(0.0, 200.0)];
        // a camera looking at the bed at an angle: the back edge looks narrower
        let img = [p2(0.1, 0.9), p2(0.9, 0.95), p2(0.75, 0.1), p2(0.25, 0.12)];
        let h = Homography::between(&bed, &img).unwrap();
        for i in 0..4 {
            let q = h.apply(bed[i]);
            assert!((q.x - img[i].x).abs() < 1e-9 && (q.y - img[i].y).abs() < 1e-9);
        }
        // the middle of the front edge lands on the straight line between its corners
        let m = h.apply(p2(150.0, 0.0));
        let t = (m.x - img[0].x) / (img[1].x - img[0].x);
        assert!((img[0].y + t * (img[1].y - img[0].y) - m.y).abs() < 1e-9);
        // three points on a line can't align
        assert!(Homography::between(&[p2(0.0, 0.0), p2(1.0, 0.0), p2(2.0, 0.0), p2(0.0, 1.0)], &img).is_none());
    }

    #[test]
    fn yuyv_grey_and_settings_default_bed() {
        let px = yuyv_to_rgba(&[128, 128, 128, 128], 2, 1);
        assert!(px[0].abs_diff(130) <= 2 && px[4] == px[0]);
        let s = CameraSettings::default();
        assert_eq!(s.bed(300.0, 200.0)[2], p2(300.0, 200.0));
        let h = bed_to_image(&s, 300.0, 200.0).unwrap();
        let c = h.apply(p2(150.0, 100.0));
        assert!((c.x - 0.5).abs() < 1e-9 && (c.y - 0.5).abs() < 1e-9, "default alignment: the bed fills the picture");
    }
}
