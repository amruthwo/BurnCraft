//! Autosave and crash recovery.
//!
//! Every running BurnCraft has its own *session* in the app's data folder (the browser's storage on the web):
//! a heartbeat it refreshes while running, and a copy of each project tab that has unsaved changes. The
//! user's own files are never written. A normal Save makes a tab's copy go away; closing a tab or quitting
//! normally removes them too. At the next start, copies left by a session whose heartbeat stopped (a crash,
//! a power cut, a killed browser tab) are offered back: Recover or Discard.

use serde_json::{json, Value};

/// A session counts as gone when its heartbeat is older than this (seconds).
pub const STALE_SECS: f64 = 90.0;
/// How often a running session refreshes its heartbeat (seconds).
pub const HEARTBEAT_SECS: f64 = 30.0;
/// After an edit, wait this long for a pause before saving (seconds).
pub const QUIET_SECS: f64 = 5.0;
/// Copies kept per project: the latest and the ones before it.
#[cfg(not(target_arch = "wasm32"))]
pub const KEEP: usize = 3;

/// Seconds since 1970 (wall clock, for the times shown to the user and heartbeats between runs).
pub fn wall_secs() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
}

// ------------------------------------------------------------------------------------------------- storage

/// Where autosaves live: a folder (desktop) or the browser's local storage. Keys look like
/// `<session>/<file>`.
#[derive(Clone)]
pub struct Store {
    #[cfg(not(target_arch = "wasm32"))]
    root: Option<std::path::PathBuf>,
}

impl Store {
    /// The app's own data folder (never next to the user's files).
    pub fn app() -> Store {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Store { root: directories::ProjectDirs::from("org", "BurnCraft", "BurnCraft").map(|p| p.data_dir().join("autosave")) }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Store {}
        }
    }

    /// A store in a given folder (tests).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn at(root: std::path::PathBuf) -> Store {
        Store { root: Some(root) }
    }

    /// No storage at all (screenshots and tests that mustn't write anything).
    pub fn none() -> Store {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Store { root: None }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Store {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn put(&self, key: &str, value: &str) -> Result<(), String> {
        let root = self.root.as_ref().ok_or("no data folder")?;
        let p = root.join(key);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        // write a temporary file, then rename it into place, so a crash mid-write never leaves half a file
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, value).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.root.as_ref()?.join(key)).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn del(&self, key: &str) {
        if let Some(r) = &self.root {
            let _ = std::fs::remove_file(r.join(key));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn rename(&self, from: &str, to: &str) {
        if let Some(r) = &self.root {
            let _ = std::fs::rename(r.join(from), r.join(to));
        }
    }
    /// Every key, as `<session>/<file>`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn keys(&self) -> Vec<String> {
        let Some(r) = &self.root else { return Vec::new() };
        let mut out = Vec::new();
        for s in std::fs::read_dir(r).into_iter().flatten().flatten() {
            if !s.path().is_dir() {
                continue;
            }
            let sname = s.file_name().to_string_lossy().to_string();
            for f in std::fs::read_dir(s.path()).into_iter().flatten().flatten() {
                out.push(format!("{sname}/{}", f.file_name().to_string_lossy()));
            }
        }
        out
    }
    /// Remove a session's folder once it's empty.
    #[cfg(not(target_arch = "wasm32"))]
    fn tidy(&self, session: &str) {
        if let Some(r) = &self.root {
            let _ = std::fs::remove_dir(r.join(session));
        }
    }

    // the browser: local storage, keys prefixed
    #[cfg(target_arch = "wasm32")]
    const PREFIX: &'static str = "burncraft.autosave/";
    #[cfg(target_arch = "wasm32")]
    fn ls() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn put(&self, key: &str, value: &str) -> Result<(), String> {
        Self::ls().ok_or("no browser storage")?.set_item(&format!("{}{key}", Self::PREFIX), value).map_err(|_| "the browser's storage is full".to_string())
    }
    #[cfg(target_arch = "wasm32")]
    pub fn get(&self, key: &str) -> Option<String> {
        Self::ls()?.get_item(&format!("{}{key}", Self::PREFIX)).ok().flatten()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn del(&self, key: &str) {
        if let Some(s) = Self::ls() {
            let _ = s.remove_item(&format!("{}{key}", Self::PREFIX));
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn keys(&self) -> Vec<String> {
        let Some(s) = Self::ls() else { return Vec::new() };
        let n = s.length().unwrap_or(0);
        (0..n).filter_map(|i| s.key(i).ok().flatten()).filter_map(|k| k.strip_prefix(Self::PREFIX).map(|k| k.to_string())).collect()
    }
    #[cfg(target_arch = "wasm32")]
    fn tidy(&self, _session: &str) {}
}

// ------------------------------------------------------------------------------------------------- sessions

/// A project left behind by a session that didn't close normally.
#[derive(Clone, Debug)]
pub struct Found {
    pub session: String,
    pub tab: String,
    /// The project's name (tab title).
    pub name: String,
    /// The file it was opened from, if any.
    pub path: Option<String>,
    /// When the copy was made (seconds since 1970).
    pub time: f64,
}

/// What a project tab's latest copy is, as known to this session.
#[derive(Clone, Default)]
struct TabState {
    /// It changed since its last copy was written.
    pending: bool,
    /// When it last changed / was last written (the app's clock, seconds).
    last_change: f64,
    last_write: f64,
    /// A copy of it is stored.
    stored: bool,
}

/// This running session's autosave state.
pub struct Autosave {
    pub store: Store,
    pub session: String,
    tabs: std::collections::HashMap<u64, TabState>,
    last_heartbeat: f64,
    /// Projects found from earlier sessions, waiting for Recover / Discard.
    pub found: Vec<Found>,
    /// A copy couldn't be written (said once).
    pub failed: Option<String>,
}

impl Autosave {
    pub fn new(store: Store) -> Autosave {
        // a session name nobody else has: the time plus a random part
        let t = wall_secs();
        let r = (t.fract() * 1e9) as u64 ^ (std::ptr::addr_of!(t) as u64).rotate_left(17);
        Autosave { store, session: format!("s{:x}{:04x}", t as u64, r & 0xffff), tabs: Default::default(), last_heartbeat: f64::NEG_INFINITY, found: Vec::new(), failed: None }
    }

    /// Look for projects left by sessions that stopped without closing (their heartbeat is old). Sessions
    /// with nothing left in them are tidied away.
    pub fn scan(&mut self) {
        let now = wall_secs();
        let keys = self.store.keys();
        let mut sessions: Vec<String> = keys.iter().filter_map(|k| k.split_once('/').map(|(s, _)| s.to_string())).collect();
        sessions.sort();
        sessions.dedup();
        self.found.clear();
        for s in sessions {
            if s == self.session {
                continue;
            }
            let beat = self.store.get(&format!("{s}/session.json")).and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["heartbeat"].as_f64());
            if beat.is_some_and(|b| now - b < STALE_SECS) {
                continue; // still running (another BurnCraft window)
            }
            let mut any = false;
            for k in keys.iter().filter(|k| k.starts_with(&format!("{s}/")) && k.ends_with(".json") && !k.ends_with("/session.json")) {
                let tab = k[s.len() + 1..k.len() - 5].to_string();
                let Some(meta) = self.store.get(k).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
                if self.store.get(&format!("{s}/{tab}.lbrn2")).is_none() {
                    continue;
                }
                any = true;
                self.found.push(Found {
                    session: s.clone(),
                    tab,
                    name: meta["name"].as_str().unwrap_or("Untitled").to_string(),
                    path: meta["path"].as_str().map(|p| p.to_string()),
                    time: meta["time"].as_f64().unwrap_or(0.0),
                });
            }
            if !any {
                self.drop_session(&s);
            }
        }
        self.found.sort_by(|a, b| b.time.total_cmp(&a.time));
    }

    /// The project text of a found copy (the latest one that reads back, older copies if it's damaged).
    pub fn recover(&self, f: &Found) -> Option<String> {
        let mut names = vec![format!("{}/{}.lbrn2", f.session, f.tab)];
        #[cfg(not(target_arch = "wasm32"))]
        names.extend((1..KEEP).map(|k| format!("{}/{}.{k}.lbrn2", f.session, f.tab)));
        names.into_iter().filter_map(|n| self.store.get(&n)).find(|t| crate::lbrn::load_str(t).is_ok())
    }

    /// Forget a found copy (after recovering or discarding it).
    pub fn forget(&mut self, f: &Found) {
        self.delete_files(&f.session, &f.tab);
        self.found.retain(|x| !(x.session == f.session && x.tab == f.tab));
        if !self.found.iter().any(|x| x.session == f.session) {
            self.drop_session(&f.session);
        }
    }

    fn delete_files(&self, session: &str, tab: &str) {
        self.store.del(&format!("{session}/{tab}.lbrn2"));
        self.store.del(&format!("{session}/{tab}.json"));
        #[cfg(not(target_arch = "wasm32"))]
        for k in 1..KEEP {
            self.store.del(&format!("{session}/{tab}.{k}.lbrn2"));
        }
    }

    fn drop_session(&self, s: &str) {
        let prefix = format!("{s}/");
        for k in self.store.keys().into_iter().filter(|k| k.starts_with(&prefix)) {
            self.store.del(&k);
        }
        self.store.tidy(s);
    }

    /// The project in tab `id` changed (`dirty`: it has unsaved changes).
    pub fn changed(&mut self, id: u64, now: f64) {
        let t = self.tabs.entry(id).or_default();
        t.pending = true;
        t.last_change = now;
    }

    /// Is a copy of tab `id` due? Only tabs with unsaved changes are copied: a few seconds after the last
    /// edit, or every `interval` seconds while editing goes on.
    pub fn due(&self, id: u64, dirty: bool, now: f64, interval: f64) -> bool {
        let Some(t) = self.tabs.get(&id) else { return false };
        dirty && t.pending && (now - t.last_change >= QUIET_SECS || now - t.last_write >= interval)
    }

    /// Store a copy of tab `id`: its project text, name and file. Older copies move down one place.
    pub fn write(&mut self, id: u64, text: &str, name: &str, path: Option<&str>, now: f64) {
        let s = self.session.clone();
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs.get(&id).is_some_and(|t| t.stored) {
            for k in (1..KEEP).rev() {
                let from = if k == 1 { format!("{s}/{id}.lbrn2") } else { format!("{s}/{id}.{}.lbrn2", k - 1) };
                self.store.rename(&from, &format!("{s}/{id}.{k}.lbrn2"));
            }
        }
        let meta = json!({ "name": name, "path": path, "time": wall_secs() }).to_string();
        let res = self.store.put(&format!("{s}/{id}.lbrn2"), text).and_then(|_| self.store.put(&format!("{s}/{id}.json"), &meta));
        let t = self.tabs.entry(id).or_default();
        t.pending = false;
        t.last_write = now;
        match res {
            Ok(()) => t.stored = true,
            Err(e) => {
                if self.failed.is_none() {
                    self.failed = Some(e);
                }
            }
        }
        self.heartbeat(now);
    }

    /// Tab `id` has no unsaved changes (saved, or closed): its copies go.
    pub fn clear(&mut self, id: u64) {
        let had = self.tabs.get(&id).is_some_and(|t| t.stored);
        if let Some(t) = self.tabs.get_mut(&id) {
            t.pending = false;
            t.stored = false;
        }
        if had {
            let s = self.session.clone();
            self.delete_files(&s, &id.to_string());
        }
    }

    /// Does tab `id` have a stored copy?
    pub fn stored(&self, id: u64) -> bool {
        self.tabs.get(&id).is_some_and(|t| t.stored)
    }

    /// Refresh this session's heartbeat now and then, while it has copies stored.
    pub fn heartbeat(&mut self, now: f64) {
        if now - self.last_heartbeat < HEARTBEAT_SECS && self.last_heartbeat.is_finite() {
            return;
        }
        if !self.tabs.values().any(|t| t.stored) {
            return;
        }
        self.last_heartbeat = now;
        let _ = self.store.put(&format!("{}/session.json", self.session), &json!({ "heartbeat": wall_secs() }).to_string());
    }

    /// A normal close: this session's copies go.
    pub fn end(&mut self) {
        let s = self.session.clone();
        self.drop_session(&s);
        self.tabs.clear();
    }
}

/// "3 minutes ago", for the recovery list.
pub fn ago(time: f64) -> String {
    let d = (wall_secs() - time).max(0.0);
    match d {
        d if d < 90.0 => "just now".into(),
        d if d < 3600.0 * 1.5 => format!("{:.0} minutes ago", d / 60.0),
        d if d < 86400.0 * 1.5 => format!("{:.0} hours ago", d / 3600.0),
        d => format!("{:.0} days ago", d / 86400.0),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("burncraft-autosave-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn project(text: &str) -> String {
        let mut doc = crate::model::Document::default();
        doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Text { text: text.into(), font: String::new(), height: 10.0, style: Default::default(), backup: None }, crate::geom::Xform::translate(10.0, 10.0), 0));
        doc.sync_cuts();
        crate::lbrn::save_string(&doc, &[])
    }

    #[test]
    fn a_crashed_session_is_recovered_and_a_saved_one_leaves_nothing() {
        let root = tmp("crash");
        // session A edits two projects, then "crashes" (never calls end) and its heartbeat goes stale
        let mut a = Autosave::new(Store::at(root.clone()));
        a.changed(1, 0.0);
        assert!(!a.due(1, true, 1.0, 120.0), "waits for a pause");
        assert!(a.due(1, true, QUIET_SECS + 0.1, 120.0));
        assert!(!a.due(1, false, QUIET_SECS + 0.1, 120.0), "nothing unsaved, nothing to copy");
        a.write(1, &project("first"), "Sign", Some("/home/me/sign.lbrn2"), 6.0);
        a.changed(1, 7.0);
        a.write(1, &project("second"), "Sign", Some("/home/me/sign.lbrn2"), 13.0);
        a.changed(2, 8.0);
        a.write(2, &project("other"), "Untitled 2", None, 14.0);
        // make A's heartbeat old
        a.store.put(&format!("{}/session.json", a.session), &json!({ "heartbeat": wall_secs() - STALE_SECS - 10.0 }).to_string()).unwrap();

        // the next start finds both, newest copy first
        let mut b = Autosave::new(Store::at(root.clone()));
        b.scan();
        assert_eq!(b.found.len(), 2);
        let sign = b.found.iter().find(|f| f.name == "Sign").unwrap().clone();
        assert_eq!(sign.path.as_deref(), Some("/home/me/sign.lbrn2"));
        let text = b.recover(&sign).unwrap();
        assert!(text.contains("second"), "the latest copy");
        // a damaged latest copy falls back to the one before
        b.store.put(&format!("{}/{}.lbrn2", sign.session, sign.tab), "damaged").unwrap();
        assert!(b.recover(&sign).unwrap().contains("first"));
        b.forget(&sign);
        let other = b.found[0].clone();
        b.forget(&other);
        assert!(b.found.is_empty());
        b.scan();
        assert!(b.found.is_empty(), "recovered and discarded copies are gone");
        assert!(!root.join(&a.session).exists(), "the old session folder is tidied away");

        // a session still running (fresh heartbeat) isn't offered
        let mut c = Autosave::new(Store::at(root.clone()));
        c.changed(5, 0.0);
        c.write(5, &project("live"), "Live", None, 6.0);
        let mut d = Autosave::new(Store::at(root.clone()));
        d.scan();
        assert!(d.found.is_empty(), "another BurnCraft that's still open");

        // saving (no unsaved changes left) clears the copy; a normal close removes everything
        c.clear(5);
        assert!(c.store.get(&format!("{}/5.lbrn2", c.session)).is_none());
        c.changed(6, 0.0);
        c.write(6, &project("x"), "X", None, 6.0);
        c.end();
        assert!(!root.join(&c.session).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_users_own_file_is_never_written() {
        let root = tmp("own");
        let own = std::env::temp_dir().join(format!("burncraft-own-{}.lbrn2", std::process::id()));
        std::fs::write(&own, "the user's file").unwrap();
        let mut a = Autosave::new(Store::at(root.clone()));
        a.changed(1, 0.0);
        a.write(1, &project("edited"), "Own", Some(&own.to_string_lossy()), 6.0);
        a.write(1, &project("edited again"), "Own", Some(&own.to_string_lossy()), 12.0);
        a.clear(1);
        a.end();
        assert_eq!(std::fs::read_to_string(&own).unwrap(), "the user's file");
        // and every copy lives inside the autosave folder
        assert!(Store::at(root.clone()).keys().is_empty());
        let _ = std::fs::remove_file(&own);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn older_copies_are_kept_and_no_temporary_files_are_left() {
        let root = tmp("keep");
        let mut a = Autosave::new(Store::at(root.clone()));
        for (i, t) in ["one", "two", "three", "four"].iter().enumerate() {
            a.changed(1, i as f64 * 10.0);
            a.write(1, &project(t), "P", None, i as f64 * 10.0 + 6.0);
        }
        let keys = a.store.keys();
        assert_eq!(keys.iter().filter(|k| k.ends_with(".lbrn2")).count(), KEEP);
        assert!(!keys.iter().any(|k| k.ends_with(".tmp")));
        assert!(a.store.get(&format!("{}/1.lbrn2", a.session)).unwrap().contains("four"));
        assert!(a.store.get(&format!("{}/1.2.lbrn2", a.session)).unwrap().contains("two"));
        a.end();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_store_writes_nothing() {
        let mut a = Autosave::new(Store::none());
        a.changed(1, 0.0);
        a.write(1, "x", "P", None, 6.0);
        assert!(a.failed.is_some());
        assert!(a.store.keys().is_empty());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod app_tests {
    use super::*;

    #[test]
    fn the_app_copies_unsaved_tabs_and_drops_them_when_saved_or_closed() {
        let root = std::env::temp_dir().join(format!("burncraft-autosave-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = crate::app::App::new(crate::settings::Settings::default());
        app.autosave = Autosave::new(Store::at(root.clone()));
        let files = |app: &crate::app::App| app.autosave.store.keys().into_iter().filter(|k| k.ends_with(".lbrn2")).count();
        // an edit: nothing until editing pauses, then one copy
        app.doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Rect { w: 10.0, h: 10.0, cr: 0.0 }, crate::geom::Xform::translate(20.0, 20.0), 0));
        app.touch();
        app.autosave_tick(100.0);
        assert_eq!(files(&app), 0);
        app.autosave_tick(100.0 + QUIET_SECS + 0.1);
        assert_eq!(files(&app), 1);
        // a second tab with its own unsaved work
        app.new_tab(None);
        app.doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Ellipse { rx: 5.0, ry: 5.0 }, crate::geom::Xform::translate(20.0, 20.0), 0));
        app.touch();
        app.autosave_tick(200.0);
        app.autosave_tick(200.0 + QUIET_SECS + 0.1);
        assert_eq!(files(&app), 2);
        // saved (no unsaved changes): its copy goes
        app.dirty = false;
        app.autosave_tick(300.0);
        assert_eq!(files(&app), 1);
        // the other tab closed (after its question): its copy goes too
        app.close_tab(0);
        assert_eq!(files(&app), 0);
        app.autosave.end();
        let _ = std::fs::remove_dir_all(&root);
    }
}
