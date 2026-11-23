//! The material library: saved laser settings for a material and thickness on a laser, applied to layers.
//! Kept in the app's data folder (the browser's storage on the web), and shared as a .json file.

use crate::model::{CutMode, CutSetting};
use serde::{Deserialize, Serialize};

/// One preset: what was burned, on which laser, and the settings that worked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    /// Unique within the library (kept when shared, so a layer can say which preset it came from).
    pub id: String,
    pub name: String,
    /// Material thickness, mm (0 = not given).
    pub thickness: f64,
    /// The laser (device name) the settings are for.
    pub laser: String,
    /// The layer mode, as saved in projects ("Cut", "Scan", "Image"…).
    pub mode: String,
    pub speed: f64,
    pub min_power: f64,
    pub max_power: f64,
    pub passes: u32,
    pub interval: f64,
    /// Hz (galvo and PWM sources).
    pub frequency: f64,
    /// MOPA fiber pulse width (ns), when it matters.
    pub pulse_width: Option<u16>,
    pub air_assist: bool,
    /// Focus / Z offset, mm.
    pub z_offset: f64,
    pub kerf: f64,
    pub notes: String,
}

impl Default for Material {
    fn default() -> Self {
        let c = CutSetting::new(0);
        Material::from_cut(&c, "", 0.0, "")
    }
}

impl Material {
    /// A preset from a layer's settings.
    pub fn from_cut(c: &CutSetting, name: &str, thickness: f64, laser: &str) -> Material {
        Material {
            id: new_id(),
            name: name.trim().to_string(),
            thickness,
            laser: laser.to_string(),
            mode: c.mode.file_name().to_string(),
            speed: c.speed,
            min_power: c.min_power,
            max_power: c.max_power,
            passes: c.passes,
            interval: c.interval,
            frequency: c.frequency,
            pulse_width: c.pulse_width,
            air_assist: c.air_assist,
            z_offset: c.z_offset,
            kerf: c.kerf,
            notes: String::new(),
        }
    }

    /// Put these settings on a layer (and link it to this preset). The layer keeps its name and index.
    pub fn apply(&self, c: &mut CutSetting) {
        let ok = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        c.mode = CutMode::from_file_name(&self.mode);
        c.speed = ok(self.speed, 0.01, 100_000.0, c.speed);
        c.min_power = ok(self.min_power, 0.0, 100.0, c.min_power);
        c.max_power = ok(self.max_power, 0.0, 100.0, c.max_power);
        c.passes = self.passes.clamp(1, 1000);
        c.interval = ok(self.interval, 0.001, 100.0, c.interval);
        c.frequency = ok(self.frequency, 1.0, 10_000_000.0, c.frequency);
        c.pulse_width = self.pulse_width.map(|p| p.max(1));
        c.air_assist = self.air_assist;
        c.z_offset = ok(self.z_offset, -1000.0, 1000.0, 0.0);
        c.kerf = ok(self.kerf, -5.0, 5.0, 0.0);
        c.material = self.id.clone();
    }

    /// For this laser (or for any laser, when it doesn't say).
    pub fn for_laser(&self, laser: &str) -> bool {
        self.laser.is_empty() || self.laser == laser
    }

    /// "Birch plywood 3 mm · Cut", for lists.
    pub fn title(&self) -> String {
        let name = if self.name.is_empty() { "Unnamed material" } else { &self.name };
        let t = if self.thickness > 0.0 { format!(" {} mm", crate::geom::fmt_num(self.thickness)) } else { String::new() };
        format!("{name}{t} · {}", CutMode::from_file_name(&self.mode).label())
    }

    /// The settings in a line, e.g. "Cut · 10 mm/s · 80% · 2 passes".
    pub fn summary(&self) -> String {
        let f = |v: f64| crate::geom::fmt_num((v * 100.0).round() / 100.0);
        let mut s = format!("{} · {} mm/s · {}%", CutMode::from_file_name(&self.mode).label(), f(self.speed), f(self.max_power));
        if self.passes > 1 {
            s.push_str(&format!(" · {} passes", self.passes));
        }
        if CutMode::from_file_name(&self.mode) != CutMode::Line {
            s.push_str(&format!(" · {} mm interval", f(self.interval)));
        }
        if let Some(p) = self.pulse_width {
            s.push_str(&format!(" · {} kHz · {p} ns", f(self.frequency / 1000.0)));
        }
        s
    }
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = crate::autosave::wall_secs();
    format!("m{:x}{:x}", (t * 1000.0) as u64, N.fetch_add(1, Ordering::Relaxed))
}

/// The whole library.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub format: String,
    pub materials: Vec<Material>,
}

const FORMAT: &str = "BurnCraft materials";
/// The most presets a library (or a file being imported) holds.
const MAX_MATERIALS: usize = 5000;
#[cfg(target_arch = "wasm32")]
const WEB_KEY: &str = "burncraft.materials";

impl Library {
    pub fn get(&self, id: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.id == id)
    }

    /// The library as a file to share.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&Library { format: FORMAT.into(), materials: self.materials.clone() }).unwrap_or_default()
    }

    /// Read a shared library file.
    pub fn from_json(text: &str) -> Result<Library, String> {
        let mut l: Library = serde_json::from_str(text).map_err(|_| "This isn't a BurnCraft materials file.".to_string())?;
        if l.format != FORMAT {
            return Err("This isn't a BurnCraft materials file.".into());
        }
        l.materials.truncate(MAX_MATERIALS);
        for m in &mut l.materials {
            m.name = m.name.chars().take(200).collect();
            m.notes = m.notes.chars().take(5000).collect();
            if m.id.is_empty() {
                m.id = new_id();
            }
        }
        Ok(l)
    }

    /// Add the presets from another library: one with the same id is replaced (a newer copy of the same
    /// preset), the rest are added. Returns (added, replaced).
    pub fn merge(&mut self, other: Library) -> (usize, usize) {
        let (mut added, mut replaced) = (0, 0);
        for m in other.materials {
            let full = self.materials.len() >= MAX_MATERIALS;
            match self.materials.iter_mut().find(|x| x.id == m.id) {
                Some(x) => {
                    *x = m;
                    replaced += 1;
                }
                None if !full => {
                    self.materials.push(m);
                    added += 1;
                }
                None => {}
            }
        }
        (added, replaced)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn path() -> Option<std::path::PathBuf> {
        directories::ProjectDirs::from("org", "BurnCraft", "BurnCraft").map(|p| p.data_dir().join("materials.json"))
    }

    /// The saved library (empty if there's none yet). A library that can't be read is kept aside, renamed
    /// (desktop) or under another key (browser), so saving never writes over it.
    pub fn load() -> Library {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(p) = Self::path() else { return Library::default() };
            let Ok(text) = std::fs::read_to_string(&p) else { return Library::default() };
            match Library::from_json(&text) {
                Ok(l) => l,
                Err(_) => {
                    let _ = std::fs::rename(&p, p.with_extension(format!("unreadable-{}.json", crate::autosave::wall_secs() as u64)));
                    Library::default()
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else { return Library::default() };
            let Some(text) = s.get_item(WEB_KEY).ok().flatten() else { return Library::default() };
            match Library::from_json(&text) {
                Ok(l) => l,
                Err(_) => {
                    let _ = s.set_item(&format!("{WEB_KEY}.unreadable"), &text);
                    Library::default()
                }
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let p = Self::path().ok_or("no data folder")?;
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            // write a new file, then swap it in, so a crash never leaves half a library
            let tmp = p.with_extension("json.new");
            std::fs::write(&tmp, self.to_json()).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let s = web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or("the browser doesn't allow saving (private window?)")?;
            s.set_item(WEB_KEY, &self.to_json()).map_err(|_| "the browser's storage is full or blocked".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_apply_share_and_merge() {
        let mut c = CutSetting::new(2);
        (c.mode, c.speed, c.max_power, c.passes, c.kerf) = (CutMode::Line, 8.0, 85.0, 2, 0.08);
        let mut m = Material::from_cut(&c, " Birch plywood ", 3.0, "My diode");
        m.notes = "clean edges".into();
        assert_eq!(m.title(), "Birch plywood 3 mm · Line");
        assert_eq!(m.mode, "Cut", "stored by the name projects use");
        let mut layer = CutSetting::new(5);
        m.apply(&mut layer);
        assert_eq!((layer.index, layer.mode, layer.speed, layer.max_power, layer.passes, layer.kerf), (5, CutMode::Line, 8.0, 85.0, 2, 0.08));
        assert_eq!(layer.material, m.id);
        // shared as a file and read back
        let lib = Library { materials: vec![m.clone()], ..Default::default() };
        let back = Library::from_json(&lib.to_json()).unwrap();
        assert_eq!(back.materials, vec![m.clone()]);
        assert!(Library::from_json("{\"materials\": []}").is_err(), "not ours");
        // merging: the same preset is replaced, a new one added
        let mut mine = lib.clone();
        let mut newer = m.clone();
        newer.speed = 9.0;
        let other = Material::from_cut(&c, "Acrylic", 3.0, "My diode");
        assert_eq!(mine.merge(Library { materials: vec![newer, other], ..Default::default() }), (1, 1));
        assert_eq!(mine.materials.len(), 2);
        assert_eq!(mine.get(&m.id).unwrap().speed, 9.0);
        // nonsense values are kept within limits
        let bad = Material { speed: f64::NAN, max_power: 500.0, passes: 0, ..m };
        let mut layer = CutSetting::new(0);
        bad.apply(&mut layer);
        assert!(layer.speed.is_finite() && layer.max_power == 100.0 && layer.passes == 1);
    }
}
