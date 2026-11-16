//! Fonts installed on the computer (Windows, macOS, Linux), found with `fontdb`.
//!
//! Text shapes keep their font as a Qt-style description like LightBurn writes it:
//! `Family,-1,100,5,Weight,Italic,0,0,0,0` (weight 50 normal / 75 bold, italic 0/1).

use ab_glyph::{FontArc, FontVec};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn db() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut d = fontdb::Database::new();
        d.load_system_fonts();
        d
    })
}

/// Sorted, de-duplicated family names of installed fonts.
pub fn families() -> &'static [String] {
    static F: OnceLock<Vec<String>> = OnceLock::new();
    F.get_or_init(|| {
        let mut v: Vec<String> = db().faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup();
        v
    })
}

/// New text starts in Arial, LightBurn's default; falls back to the built-in font.
pub const DEFAULT_FONT: &str = "Arial,-1,100,5,50,0,0,0,0,0";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontSpec {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

impl FontSpec {
    pub fn parse(s: &str) -> FontSpec {
        let parts: Vec<&str> = s.split(',').collect();
        let family = parts.first().map(|f| f.trim().to_string()).unwrap_or_default();
        let weight: i32 = parts.get(4).and_then(|w| w.trim().parse().ok()).unwrap_or(50);
        // Qt 5 uses 0-99 (75 = bold); Qt 6 uses 100-900 (700 = bold)
        let bold = if weight > 100 { weight >= 600 } else { weight >= 63 };
        let italic = parts.get(5).map(|i| i.trim() == "1").unwrap_or(false);
        FontSpec { family, bold, italic }
    }
    pub fn to_qt(&self) -> String {
        format!("{},-1,100,5,{},{},0,0,0,0", self.family, if self.bold { 75 } else { 50 }, if self.italic { 1 } else { 0 })
    }
    pub fn label(&self) -> String {
        if self.family.is_empty() {
            "Default (built-in)".into()
        } else {
            self.family.clone()
        }
    }
}

/// A loaded font, and whether bold / italic have to be simulated because the family has no such face (or
/// it's a variable font, which keeps every weight in one file).
#[derive(Clone)]
pub struct Loaded {
    pub font: FontArc,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

/// Load a font for the spec (`None` means "use the built-in font"), saying whether bold / italic need
/// simulating.
pub fn load_face(spec: &FontSpec) -> Option<Loaded> {
    if spec.family.is_empty() {
        return None;
    }
    static CACHE: OnceLock<Mutex<HashMap<FontSpec, Option<Loaded>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(f) = cache.lock().unwrap().get(spec) {
        return f.clone();
    }
    let q = fontdb::Query {
        families: &[fontdb::Family::Name(&spec.family)],
        weight: if spec.bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
        style: if spec.italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
        ..Default::default()
    };
    let loaded = db().query(&q).and_then(|id| {
        let face = db().face(id)?;
        let (weight, style) = (face.weight.0, face.style);
        let font = db().with_face_data(id, |data, index| FontVec::try_from_vec_and_index(data.to_vec(), index).ok()).flatten().map(FontArc::new)?;
        Some(Loaded { font, fake_bold: spec.bold && weight < 600, fake_italic: spec.italic && style == fontdb::Style::Normal })
    });
    cache.lock().unwrap().insert(spec.clone(), loaded.clone());
    loaded
}

/// Bold / italic that will be simulated for this spec (the font has no real face for them).
pub fn simulated(spec: &FontSpec) -> (bool, bool) {
    // the built-in font has a single face: both are simulated
    load_face(spec).map(|l| (l.fake_bold, l.fake_italic)).unwrap_or((spec.bold, spec.italic))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spec_roundtrip() {
        let s = FontSpec::parse("Arial,-1,100,5,75,1,0,0,0,0");
        assert_eq!(s, FontSpec { family: "Arial".into(), bold: true, italic: true });
        assert_eq!(FontSpec::parse(&s.to_qt()), s);
        assert!(FontSpec::parse("Segoe UI,-1,100,5,700,0,0,0,0,0").bold);
    }
}
