//! Configurable keyboard shortcuts: chords, presets, conflict detection.

use crate::actions::Action;
use egui::{Key, Modifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: KeyId,
}

/// egui::Key wrapped so it can be ordered and hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(pub u32);

const KEY_NAMES: &[(&str, Key)] = &[
    ("A", Key::A), ("B", Key::B), ("C", Key::C), ("D", Key::D), ("E", Key::E), ("F", Key::F), ("G", Key::G), ("H", Key::H), ("I", Key::I), ("J", Key::J),
    ("K", Key::K), ("L", Key::L), ("M", Key::M), ("N", Key::N), ("O", Key::O), ("P", Key::P), ("Q", Key::Q), ("R", Key::R), ("S", Key::S), ("T", Key::T),
    ("U", Key::U), ("V", Key::V), ("W", Key::W), ("X", Key::X), ("Y", Key::Y), ("Z", Key::Z),
    ("0", Key::Num0), ("1", Key::Num1), ("2", Key::Num2), ("3", Key::Num3), ("4", Key::Num4), ("5", Key::Num5), ("6", Key::Num6), ("7", Key::Num7), ("8", Key::Num8), ("9", Key::Num9),
    ("F1", Key::F1), ("F2", Key::F2), ("F3", Key::F3), ("F4", Key::F4), ("F5", Key::F5), ("F6", Key::F6), ("F7", Key::F7), ("F8", Key::F8), ("F9", Key::F9), ("F10", Key::F10), ("F11", Key::F11), ("F12", Key::F12),
    ("Left", Key::ArrowLeft), ("Right", Key::ArrowRight), ("Up", Key::ArrowUp), ("Down", Key::ArrowDown),
    ("PgUp", Key::PageUp), ("PgDown", Key::PageDown), ("Home", Key::Home), ("End", Key::End), ("Insert", Key::Insert),
    ("Delete", Key::Delete), ("Backspace", Key::Backspace), ("Escape", Key::Escape), ("Tab", Key::Tab), ("Space", Key::Space), ("Enter", Key::Enter),
    ("=", Key::Equals), ("-", Key::Minus), ("+", Key::Plus), (".", Key::Period), (",", Key::Comma), ("`", Key::Backtick),
    ("[", Key::OpenBracket), ("]", Key::CloseBracket), ("/", Key::Slash), ("\\", Key::Backslash), (";", Key::Semicolon), ("'", Key::Quote),
];

fn key_index(k: Key) -> Option<u32> {
    KEY_NAMES.iter().position(|(_, kk)| *kk == k).map(|i| i as u32)
}

impl KeyId {
    pub fn from_key(k: Key) -> Option<KeyId> {
        key_index(k).map(KeyId)
    }
    #[allow(dead_code)]
    pub fn key(self) -> Key {
        KEY_NAMES[self.0 as usize].1
    }
    pub fn name(self) -> &'static str {
        KEY_NAMES[self.0 as usize].0
    }
}

impl Chord {
    pub fn parse(s: &str) -> Option<Chord> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        // Split on '+' but allow a trailing "+" key (e.g. "Alt++").
        let mut parts: Vec<&str> = Vec::new();
        let mut rest = s;
        loop {
            match rest.find('+') {
                Some(0) => {
                    parts.push("+");
                    rest = &rest[1..];
                    if rest.starts_with('+') {
                        rest = &rest[1..];
                    }
                }
                Some(i) => {
                    parts.push(&rest[..i]);
                    rest = &rest[i + 1..];
                }
                None => {
                    if !rest.is_empty() {
                        parts.push(rest);
                    }
                    break;
                }
            }
            if rest.is_empty() {
                break;
            }
        }
        let key_name = parts.pop()?;
        for m in parts {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" | "meta" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                _ => return None,
            }
        }
        let key_name = match key_name {
            "Del" => "Delete",
            "Esc" => "Escape",
            "PageUp" => "PgUp",
            "PageDown" => "PgDown",
            k => k,
        };
        let idx = KEY_NAMES.iter().position(|(n, _)| n.eq_ignore_ascii_case(key_name))?;
        Some(Chord { ctrl, alt, shift, key: KeyId(idx as u32) })
    }

    pub fn from_event(key: Key, m: &Modifiers) -> Option<Chord> {
        Some(Chord { ctrl: m.command || m.ctrl, alt: m.alt, shift: m.shift, key: KeyId::from_key(key)? })
    }

    pub fn display(&self) -> String {
        let mac = cfg!(target_os = "macos");
        let mut s = String::new();
        if self.ctrl {
            s.push_str(if mac { "⌘" } else { "Ctrl+" });
        }
        if self.alt {
            s.push_str(if mac { "⌥" } else { "Alt+" });
        }
        if self.shift {
            s.push_str(if mac { "⇧" } else { "Shift+" });
        }
        s.push_str(self.key.name());
        s
    }

    /// Canonical text form saved to the settings file.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.name());
        s
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    LightBurn,
    Cad,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Preset::LightBurn => "LightBurn style",
            Preset::Cad => "CAD style (Fusion 360 / Onshape)",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Preset::LightBurn => "lightburn",
            Preset::Cad => "cad",
        }
    }
    pub fn from_id(s: &str) -> Preset {
        if s == "cad" {
            Preset::Cad
        } else {
            Preset::LightBurn
        }
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    pub preset: Preset,
    /// Per-action overrides on top of the preset (empty vec = no shortcut).
    pub overrides: BTreeMap<Action, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap { preset: Preset::Cad, overrides: BTreeMap::new() }
    }
}

impl Keymap {
    pub fn preset_chords(preset: Preset, a: Action) -> Vec<Chord> {
        let src = match preset {
            Preset::LightBurn => a.default_lightburn(),
            Preset::Cad => a.default_cad(),
        };
        src.iter().filter_map(|s| Chord::parse(s)).collect()
    }

    pub fn chords(&self, a: Action) -> Vec<Chord> {
        self.overrides.get(&a).cloned().unwrap_or_else(|| Self::preset_chords(self.preset, a))
    }

    pub fn label(&self, a: Action) -> String {
        self.chords(a).first().map(|c| c.display()).unwrap_or_default()
    }

    pub fn set(&mut self, a: Action, chords: Vec<Chord>) {
        if chords == Self::preset_chords(self.preset, a) {
            self.overrides.remove(&a);
        } else {
            self.overrides.insert(a, chords);
        }
    }

    pub fn reset(&mut self, a: Action) {
        self.overrides.remove(&a);
    }

    pub fn lookup(&self, c: &Chord) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| self.chords(*a).contains(c))
    }

    /// Actions sharing a chord with `a`.
    pub fn conflicts(&self, a: Action) -> Vec<(Chord, Action)> {
        let mine = self.chords(a);
        let mut out = Vec::new();
        for other in Action::ALL.iter().copied().filter(|o| *o != a) {
            for c in self.chords(other) {
                if mine.contains(&c) {
                    out.push((c, other));
                }
            }
        }
        out
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (a, cs) in &self.overrides {
            m.insert(a.id().to_string(), serde_json::Value::Array(cs.iter().map(|c| serde_json::Value::String(c.to_text())).collect()));
        }
        serde_json::json!({ "preset": self.preset.id(), "overrides": m })
    }

    pub fn from_json(v: &serde_json::Value) -> Keymap {
        let mut k = Keymap { preset: Preset::from_id(v.get("preset").and_then(|p| p.as_str()).unwrap_or("cad")), overrides: BTreeMap::new() };
        if let Some(o) = v.get("overrides").and_then(|o| o.as_object()) {
            for (id, arr) in o {
                if let (Some(a), Some(arr)) = (Action::from_id(id), arr.as_array()) {
                    k.overrides.insert(a, arr.iter().filter_map(|s| s.as_str().and_then(Chord::parse)).collect());
                }
            }
        }
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_forms() {
        assert_eq!(Chord::parse("Ctrl+Shift+Z").unwrap().to_text(), "Ctrl+Shift+Z");
        assert_eq!(Chord::parse("Alt++").unwrap().key.name(), "+");
        assert_eq!(Chord::parse("Alt+-").unwrap().key.name(), "-");
        assert_eq!(Chord::parse("Ctrl+=").unwrap().key.name(), "=");
        assert!(Chord::parse("Ctrl+`").is_some());
        assert!(Chord::parse("Ctrl+Cancel").is_none());
    }
    #[test]
    fn every_default_parses() {
        for a in Action::ALL {
            for s in a.default_lightburn().iter().chain(a.default_cad()) {
                if *s != "Ctrl+Cancel" {
                    assert!(Chord::parse(s).is_some(), "{s} for {:?}", a);
                }
            }
        }
    }
    #[test]
    fn no_default_conflicts() {
        for p in [Preset::LightBurn, Preset::Cad] {
            let k = Keymap { preset: p, overrides: Default::default() };
            for a in Action::ALL {
                assert!(k.conflicts(*a).is_empty(), "{:?} conflicts in {:?}: {:?}", a, p, k.conflicts(*a));
            }
        }
    }
}
