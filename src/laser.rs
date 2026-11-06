//! Talking to a GRBL laser over a serial port, on a background thread.

use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

pub enum Cmd {
    Connect { port: String, baud: u32 },
    Disconnect,
    /// Queue lines; each waits for GRBL's "ok" before the next is sent.
    Send(Vec<String>),
    /// Real-time byte: '!' feed hold, '~' resume, '?' status, 0x18 reset.
    Realtime(u8),
    ClearQueue,
}

#[derive(Clone, Debug)]
pub enum Event {
    Connected(String),
    Disconnected(String),
    Line(String),
    Status { state: String, x: f64, y: f64 },
    Progress { sent: usize, total: usize },
}

pub struct Laser {
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
    pub connected: bool,
    pub state: String,
    pub pos: (f64, f64),
    pub console: VecDeque<String>,
    pub progress: (usize, usize),
}

impl Default for Laser {
    fn default() -> Self {
        Self::new()
    }
}

impl Laser {
    pub fn new() -> Laser {
        let (tx, crx) = channel::<Cmd>();
        let (etx, rx) = channel::<Event>();
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::Builder::new().name("laser-io".into()).spawn(move || worker(crx, etx)).ok();
        #[cfg(target_arch = "wasm32")]
        drop((crx, etx));
        Laser { tx, rx, connected: false, state: "Not connected".into(), pos: (0.0, 0.0), console: VecDeque::new(), progress: (0, 0) }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn log(&mut self, s: impl Into<String>) {
        self.console.push_back(s.into());
        while self.console.len() > 400 {
            self.console.pop_front();
        }
    }

    /// Drain events from the worker; returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(e) = self.rx.try_recv() {
            changed = true;
            match e {
                Event::Connected(p) => {
                    self.connected = true;
                    self.state = "Connected".into();
                    self.log(format!("Connected to {p}"));
                }
                Event::Disconnected(why) => {
                    self.connected = false;
                    self.state = "Not connected".into();
                    self.log(why);
                }
                Event::Line(l) => self.log(l),
                Event::Status { state, x, y } => {
                    self.state = state;
                    self.pos = (x, y);
                }
                Event::Progress { sent, total } => self.progress = (sent, total),
            }
        }
        changed
    }

    pub fn ports() -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        return serialport::available_ports().map(|v| v.into_iter().map(|p| p.port_name).collect()).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        Vec::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker(rx: Receiver<Cmd>, tx: Sender<Event>) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut waiting_ok = false;
    let mut buf = String::new();
    let mut total = 0usize;
    let mut sent = 0usize;
    let mut last_status = Instant::now();
    loop {
        // commands
        loop {
            let msg = if port.is_some() { rx.try_recv() } else { rx.recv_timeout(Duration::from_millis(200)).map_err(|_| TryRecvError::Empty) };
            match msg {
                Ok(Cmd::Connect { port: name, baud }) => match serialport::new(&name, baud).timeout(Duration::from_millis(20)).open() {
                    Ok(mut p) => {
                        let _ = p.write_all(b"\r\n\r\n");
                        port = Some(p);
                        let _ = tx.send(Event::Connected(name));
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Disconnected(format!("Could not open {name}: {e}")));
                    }
                },
                Ok(Cmd::Disconnect) => {
                    port = None;
                    queue.clear();
                    waiting_ok = false;
                    let _ = tx.send(Event::Disconnected("Disconnected".into()));
                }
                Ok(Cmd::Send(lines)) => {
                    for l in lines {
                        let l = l.split(';').next().unwrap_or("").trim().to_string();
                        if !l.is_empty() {
                            queue.push_back(l);
                        }
                    }
                    total = sent + queue.len();
                }
                Ok(Cmd::Realtime(b)) => {
                    if let Some(p) = port.as_mut() {
                        let _ = p.write_all(&[b]);
                    }
                    if b == 0x18 {
                        queue.clear();
                        waiting_ok = false;
                    }
                }
                Ok(Cmd::ClearQueue) => {
                    queue.clear();
                    waiting_ok = false;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let Some(p) = port.as_mut() else { continue };
        // send next line
        if !waiting_ok {
            if let Some(l) = queue.pop_front() {
                if p.write_all(format!("{l}\n").as_bytes()).is_err() {
                    port = None;
                    let _ = tx.send(Event::Disconnected("Connection lost".into()));
                    continue;
                }
                waiting_ok = true;
                sent += 1;
                let _ = tx.send(Event::Progress { sent, total });
                if queue.is_empty() {
                    total = 0;
                    sent = 0;
                }
            }
        }
        if last_status.elapsed() > Duration::from_millis(250) {
            let _ = p.write_all(b"?");
            last_status = Instant::now();
        }
        // read replies
        let mut chunk = [0u8; 512];
        match p.read(&mut chunk) {
            Ok(n) if n > 0 => {
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                while let Some(i) = buf.find('\n') {
                    let line = buf[..i].trim().to_string();
                    buf.drain(..=i);
                    if line.is_empty() {
                        continue;
                    }
                    if line == "ok" || line.starts_with("error") {
                        waiting_ok = false;
                        if line.starts_with("error") {
                            let _ = tx.send(Event::Line(line));
                        }
                    } else if line.starts_with('<') {
                        if let Some(ev) = parse_status(&line) {
                            let _ = tx.send(ev);
                        }
                    } else {
                        let _ = tx.send(Event::Line(line));
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => {
                port = None;
                let _ = tx.send(Event::Disconnected("Connection lost".into()));
            }
        }
    }
}

/// `<Idle|MPos:1.000,2.000,0.000|FS:0,0>` → status event.
pub fn parse_status(line: &str) -> Option<Event> {
    let inner = line.trim_start_matches('<').trim_end_matches('>');
    let mut parts = inner.split('|');
    let state = parts.next()?.to_string();
    let mut x = 0.0;
    let mut y = 0.0;
    for p in parts {
        if let Some(v) = p.strip_prefix("MPos:").or_else(|| p.strip_prefix("WPos:")) {
            let n: Vec<f64> = v.split(',').filter_map(|s| s.parse().ok()).collect();
            if n.len() >= 2 {
                x = n[0];
                y = n[1];
            }
        }
    }
    Some(Event::Status { state, x, y })
}

#[cfg(test)]
mod tests {
    #[test]
    fn status() {
        match super::parse_status("<Run|MPos:10.5,20.25,0.000|FS:500,0>") {
            Some(super::Event::Status { state, x, y }) => {
                assert_eq!(state, "Run");
                assert_eq!((x, y), (10.5, 20.25));
            }
            _ => panic!(),
        }
    }
}
