//! Application state and command dispatch.

use crate::actions::Action;
use crate::gcode::{self, Job};
use crate::geom::*;
use crate::imaging::TraceParams;
use crate::laser::{Cmd, Laser};
use crate::model::*;
use crate::ops;
use crate::settings::{Settings, Theme};
use crate::shortcuts::Chord;
use crate::xml::Node;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArcMode {
    ThreePoint,
    Center,
    Tangent,
}

impl ArcMode {
    pub const ALL: [ArcMode; 3] = [ArcMode::ThreePoint, ArcMode::Center, ArcMode::Tangent];
    pub fn label(self) -> &'static str {
        match self {
            ArcMode::ThreePoint => "3-point",
            ArcMode::Center => "Centre",
            ArcMode::Tangent => "Tangent",
        }
    }
}

/// Options shown in the bar under the ribbon for the current tool.
#[derive(Clone, Debug)]
pub struct ToolOpts {
    pub rect_radius: f64,
    pub poly_sides: u32,
    pub poly_radius: f64,
    pub star_points: u32,
    pub star_ratio: f64,
    pub star_radius: f64,
    /// LightBurn "Bulge": 0 = straight star, up to 1 = round petals.
    pub star_bulge: f64,
    pub arc_mode: ArcMode,
    pub arc_ccw: bool,
    pub fillet: crate::sketch::Fillet,
    pub wand_tol: u8,
    pub wand_contiguous: bool,
    /// Edge clean-up for wand / lasso selections (Smooth, Grow, Remove specks, Curves).
    pub wand_refine: crate::wand::Refine,
    pub sel_mode: crate::wand::SelMode,
    pub lasso_magnetic: bool,
}

impl Default for ToolOpts {
    fn default() -> Self {
        ToolOpts {
            rect_radius: 0.0,
            poly_sides: 6,
            poly_radius: 0.0,
            star_points: 5,
            star_ratio: 0.4,
            star_bulge: 0.0,
            star_radius: 0.0,
            arc_mode: ArcMode::ThreePoint,
            arc_ccw: true,
            fillet: Default::default(),
            wand_tol: 32,
            wand_contiguous: true,
            wand_refine: Default::default(),
            sel_mode: Default::default(),
            lasso_magnetic: true,
        }
    }
}

/// A project that isn't the active tab.
pub struct DocTab {
    pub doc: Document,
    pub kept: Vec<Node>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub sel: Vec<usize>,
    pub undo: Vec<(Document, Vec<usize>)>,
    pub redo: Vec<(Document, Vec<usize>)>,
    pub view: View,
    pub warnings: Vec<String>,
}

/// Paste offset when the mouse isn't over the canvas: 5 mm.
pub const PASTE_OFFSET: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pan,
    Line,
    Rect,
    Ellipse,
    Polygon,
    Text,
    Nodes,
    Measure,
    Pen,
    Arc,
    Star,
    Trim,
    Fillet,
    Wand,
    Lasso,
    Dimension,
    Relate,
    MirrorLine,
    Tabs,
    Warp,
}

impl Tool {
    pub fn action(self) -> Action {
        match self {
            Tool::Select => Action::ToolSelect,
            Tool::Pan => Action::ToolPan,
            Tool::Line => Action::ToolLine,
            Tool::Rect => Action::ToolRect,
            Tool::Ellipse => Action::ToolEllipse,
            Tool::Polygon => Action::ToolPolygon,
            Tool::Text => Action::ToolText,
            Tool::Nodes => Action::ToolNodes,
            Tool::Measure => Action::ToolMeasure,
            Tool::Pen => Action::ToolPen,
            Tool::Arc => Action::ToolArc,
            Tool::Star => Action::ToolStar,
            Tool::Trim => Action::ToolTrim,
            Tool::Fillet => Action::ToolFillet,
            Tool::Wand => Action::ToolWand,
            Tool::Lasso => Action::ToolLasso,
            Tool::Dimension => Action::ToolDimension,
            Tool::Relate => Action::ToolRelate,
            Tool::MirrorLine => Action::ToolMirrorLine,
            Tool::Tabs => Action::ToolTabs,
            Tool::Warp => Action::ToolWarp,
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click to select · drag left-to-right: shapes fully inside · right-to-left: anything touched · Shift/Ctrl adds",
            Tool::Pan => "Drag to move the view · scroll to zoom",
            Tool::Line => "Click to add points · rest on the last point to switch the next segment to an arc · Shift locks horizontal / 45° / vertical · double-click or Enter finishes · click the first point or press C to close",
            Tool::Relate => "Click nodes, lines or circles (one or two), then pick a relation above · click a relation tag to remove it",
            Tool::MirrorLine => "Click a straight edge or construction line to mirror the selection about it",
            Tool::Warp => "Choose a warp above, drag the handles to shape it, then Apply · Esc or another tool cancels",
            Tool::Tabs => "Click on an outline to add a tab · click a tab to remove it · set the tab width in the layer's settings",
            Tool::Rect => "Drag to draw · Shift = square · Ctrl = from centre",
            Tool::Ellipse => "Drag to draw · Shift = circle · Ctrl = from centre",
            Tool::Polygon => "Drag to draw · Ctrl = from centre · set sides in Properties",
            Tool::Text => "Click where the text should go, then type in Properties",
            Tool::Nodes => "Drag points to reshape · double-click a point to remove it",
            Tool::Measure => "Drag to measure distance and angle",
            Tool::Pen => "Click for a sharp point · click-and-drag for a smooth curve · Shift snaps angles · Enter or double-click finishes · click the first point or press C to close",
            Tool::Arc => "3-point: click start, end, then a point on the arc · Centre: click centre, start, end · Tangent: continue from a line end",
            Tool::Star => "Drag to draw · Ctrl = from centre · set points and depth in the options bar",
            Tool::Trim => "Click the piece of a line to cut away, up to where it crosses other lines",
            Tool::Fillet => "Click a corner of a shape to round or bevel it",
            Tool::Wand => "Click an image to select similar colours · Shift adds · Alt subtracts",
            Tool::Lasso => "Drag around part of an image · Shift adds · Alt subtracts",
            Tool::Dimension => "Shows lengths, radii and sizes · click a value to type a new one · click a shape to dimension just that",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonTab {
    Home,
    Sketch,
    Modify,
    Arrange,
    Image,
    Laser,
    View,
}

impl RibbonTab {
    pub const ALL: [RibbonTab; 7] = [RibbonTab::Home, RibbonTab::Sketch, RibbonTab::Modify, RibbonTab::Arrange, RibbonTab::Image, RibbonTab::Laser, RibbonTab::View];
    pub fn label(self) -> &'static str {
        match self {
            RibbonTab::Home => "Home",
            RibbonTab::Sketch => "Sketch",
            RibbonTab::Modify => "Modify",
            RibbonTab::Arrange => "Arrange",
            RibbonTab::Image => "Image",
            RibbonTab::Laser => "Laser",
            RibbonTab::View => "View",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideTab {
    Layers,
    Properties,
    Laser,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: P2,
    /// Screen points per millimetre.
    pub zoom: f64,
    pub fitted: bool,
}

pub struct TraceState {
    pub target: Option<u64>,
    pub params: TraceParams,
    pub preview: Vec<Vec<P2>>,
    pub preview_for: Option<(u64, TraceParams)>,
    pub delete_image: bool,
    pub layer: usize,
    /// Thumbnail of what will be traced (ink pixels) and the outlines in its pixel space.
    pub thumb: Option<egui::TextureHandle>,
    pub preview_px: Vec<Vec<P2>>,
    pub thumb_size: (usize, usize),
    pub show_original: bool,
}

pub enum McpState {
    Idle,
    Running {
        #[allow(dead_code)]
        target: u64,
        rx: Receiver<Result<Vec<Vec<P2>>, String>>,
        started: f64,
    },
    Done {
        #[allow(dead_code)]
        target: u64,
        lines: Vec<Vec<P2>>,
    },
    Failed(String),
}

pub struct Toast {
    pub text: String,
    pub until: f64,
    pub error: bool,
}

pub struct Screenshot {
    pub path: PathBuf,
    pub frame: u32,
    pub requested: bool,
    pub result: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
}

pub struct App {
    pub doc: Document,
    pub kept: Vec<Node>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub sel: Vec<usize>,
    pub undo: Vec<(Document, Vec<usize>)>,
    pub redo: Vec<(Document, Vec<usize>)>,
    pub rev: u64,
    pub settings: Settings,
    pub view: View,
    pub tool: Tool,
    pub current_layer: usize,
    pub ribbon_tab: RibbonTab,
    pub side_tab: SideTab,
    pub clipboard: Vec<Shape>,
    pub toasts: Vec<Toast>,
    pub laser: Laser,
    pub job: Option<Job>,
    pub preview_on: bool,
    pub preview_t: f32,
    pub textures: HashMap<u64, (u64, egui::TextureHandle)>,
    pub outline_cache: (u64, Vec<Vec<Vec<P2>>>),
    /// Per shape: the id and transform its cached outline was made with, and its bounds (world).
    pub outline_meta: Vec<(u64, Xform)>,
    pub bbox_cache: Vec<BBox>,
    /// Bumped by every change except moving / scaling / rotating whole shapes (those reuse cached outlines).
    pub content_rev: u64,
    pub cached_content_rev: u64,
    /// The revision the layer list was last brought up to date at.
    pub cuts_synced_rev: u64,
    pub fill_cache: (u64, f64, Vec<(usize, Vec<(P2, P2)>)>),
    pub trace: TraceState,
    pub mcp: McpState,
    pub screenshot: Option<Screenshot>,
    pub show_left: bool,
    pub show_right: bool,
    pub capture_for: Option<Action>,
    pub shortcut_filter: String,
    // dialog flags
    pub win_shortcuts: bool,
    pub win_settings: bool,
    pub win_trace: bool,
    pub win_mcp: bool,
    pub win_notes: bool,
    pub win_array: bool,
    pub win_offset: bool,
    pub win_search: bool,
    #[allow(dead_code)]
    pub win_about: bool,
    pub search_text: String,
    pub search_index: usize,
    pub array: (usize, usize, f64, f64),
    #[allow(dead_code)]
    pub offset_dist: f64,
    #[allow(dead_code)]
    pub offset_replace: bool,
    pub canvas: crate::canvas::CanvasState,
    pub lock_aspect: bool,
    pub jog_step: f64,
    pub console_input: String,
    pub pending_quit: bool,
    pub ports: Vec<String>,
    pub warnings: Vec<String>,
    pub opts: ToolOpts,
    pub image_sel: Option<crate::wand::Selection>,
    pub preview_playing: bool,
    pub preview_speed: f32,
    /// When the running job was sent (app clock seconds) and its estimated length.
    pub job_started: Option<f64>,
    pub job_estimate: f64,
    pub job_finished_after: Option<f64>,
    pub invert_bed: bool,
    /// All project tabs; the active one's slot is `None` (its data lives in the fields above).
    pub tabs: Vec<Option<DocTab>>,
    pub active_tab: usize,
    pub win_circular: bool,
    /// Pattern on Path / Align to Path: (align instead of copying, copies, rotate to follow, anchor).
    pub win_curve: bool,
    /// Grid array spacing measured centre to centre (else padding between edges), as in LightBurn's dialog.
    pub array_c2c: bool,
    /// A job being sent to a network laser (Ruida): its result when done.
    pub net_job: Option<std::sync::Arc<std::sync::Mutex<Option<Result<String, String>>>>>,
    /// The array being edited (index into doc.arrays), while its dialog is open.
    pub array_edit: Option<usize>,
    /// The welcome card was dismissed with "Start with an empty design" (until the next new project).
    pub welcome_off: bool,
    /// Welcome Tour step being shown.
    pub tour: Option<usize>,
    /// Where each part of the window was drawn last frame (for the tour's highlight).
    pub ui_rects: HashMap<&'static str, egui::Rect>,
    pub win_help: bool,
    pub help_topic: usize,
    pub help_search: String,
    /// Tab being renamed: (index, text being typed).
    pub tab_rename: Option<(usize, String)>,
    pub curve_dlg: (bool, usize, bool, crate::sketch::CurveAnchor),
    pub path_copy: crate::sketch::PathCopyOpts,
    /// Layer to go back to when construction drawing is switched off.
    pub construction_prev: Option<usize>,
    pub circular: (usize, f64, bool, P2),
    pub win_barcode: bool,
    pub barcode: crate::barcode::BarcodeState,
    pub hover_action: Option<Action>,
    pub offset_preview: Vec<Vec<P2>>,
    pub offset_opts: crate::ops::OffsetOpts,
    pub win_vartext: bool,
    pub win_camera: bool,
    /// A job or command being sent to a K40 over USB.
    #[cfg(usb_lasers)]
    pub usb_job: Option<crate::k40::usb::Running>,
    /// The camera reading pictures in the background.
    #[cfg(not(target_arch = "wasm32"))]
    pub camera: Option<crate::camera::Capture>,
    /// The latest camera picture (or the loaded photo) and its texture.
    pub camera_frame: Option<crate::camera::Frame>,
    pub camera_tex: Option<(u64, egui::TextureHandle)>,
    pub camera_error: Option<String>,
    /// Which alignment point the next click in the camera window sets (0..4).
    pub camera_pick: Option<usize>,
    /// The update checker's progress (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    pub update: crate::update::net::Shared,
    /// A newer version was found by the check at start-up and already announced.
    pub update_announced: bool,
    pub win_rotary: bool,
    /// Test & Calibrate, and the kerf gap measured in its kerf gauge (mm).
    pub win_test: bool,
    pub test_params: crate::calibrate::TestParams,
    pub kerf_gap: f64,
    /// Put tests on a Colony tile, and the tile size for materials' tiles.
    pub test_on_tile: bool,
    /// The warp being set up with the Warp tool.
    pub warp: crate::warp::Warp,
    /// Which point of the selection rotating turns around ([0 left, 1 centre, 2 right; 0 bottom … 2 top]).
    pub rotate_about: [u8; 2],
    /// The Bolden / Stencil window and its amounts (mm).
    pub win_shape_tools: bool,
    pub bolden_amount: f64,
    pub bridge_width: f64,
    /// The date put on Colony tiles (today's, and it can be changed).
    pub tile_date: String,
    pub tile_size: crate::colony::TileSize,
    /// The material library, its window (and whether it lists only this laser's), and a material being saved.
    pub materials: crate::materials::Library,
    pub win_materials: bool,
    pub materials_this_laser: bool,
    /// The library was changed and not saved yet (saved once a drag or typing ends).
    pub materials_dirty: bool,
    pub save_material: Option<crate::matwin::SaveMaterial>,
    pub win_nest: bool,
    /// A nest being worked out.
    pub nest_job: Option<crate::nest::running::Running>,
    /// The nest result being previewed, and the document revision it was made for.
    pub nest_result: Option<(crate::nest::NestResult, u64)>,
    /// Renaming a shape in the Shapes list: its id and the text being typed.
    pub rename: Option<(u64, String)>,
    /// A second click on a selected row starts renaming after a pause, like a file manager: row and when.
    pub rename_click: Option<(usize, f64)>,
    /// Autosave and crash recovery (see autosave.rs).
    pub autosave: crate::autosave::Autosave,
    /// A lasting id for each project tab (same order as `tabs`), naming its autosave copy.
    pub tab_ids: Vec<u64>,
    next_tab_id: u64,
    /// The document revision autosave last looked at.
    autosave_rev: u64,
    /// The Recover window (projects from a session that didn't close normally).
    pub win_recover: bool,
    /// The Export window and its choices.
    pub win_export: bool,
    pub export_opts: crate::export::Options,
    /// A PDF with several pages, waiting for the page to import: (file name, bytes, pages, chosen page).
    pub pdf_pending: Option<(String, Vec<u8>, usize, usize)>,
    /// Fingerprint of the picture Copy as Picture put on the clipboard (pasting it back gives the shapes).
    clip_picture: Option<u64>,
    autosave_warned: bool,
}

/// What a chosen file is for (the browser hands files over later, so the purpose travels with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Open = 1,
    Import = 2,
    VarText = 3,
    CameraPhoto = 4,
    Lbdev = 5,
    Shortcuts = 6,
    Materials = 7,
}

impl Pick {
    /// (used when the browser hands a picked file over)
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn from_u8(v: u8) -> Option<Pick> {
        [Pick::Open, Pick::Import, Pick::VarText, Pick::CameraPhoto, Pick::Lbdev, Pick::Shortcuts, Pick::Materials].into_iter().find(|p| *p as u8 == v)
    }
}

impl App {
    pub fn new(settings: Settings) -> App {
        let d = settings.device.clone();
        App {
            doc: Document::default(),
            kept: Vec::new(),
            path: None,
            dirty: false,
            sel: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            rev: 1,
            settings,
            view: View { center: p2(d.width / 2.0, d.height / 2.0), zoom: 1.5, fitted: false },
            tool: Tool::Select,
            current_layer: 0,
            ribbon_tab: RibbonTab::Home,
            side_tab: SideTab::Layers,
            clipboard: Vec::new(),
            toasts: Vec::new(),
            laser: Laser::new(),
            job: None,
            preview_on: false,
            preview_t: 1.0,
            textures: HashMap::new(),
            outline_cache: (0, Vec::new()),
            outline_meta: Vec::new(),
            bbox_cache: Vec::new(),
            content_rev: 0,
            cached_content_rev: u64::MAX,
            cuts_synced_rev: u64::MAX,
            fill_cache: (0, 0.0, Vec::new()),
            trace: TraceState { target: None, params: TraceParams::default(), preview: Vec::new(), preview_for: None, delete_image: false, layer: 0, thumb: None, preview_px: Vec::new(), thumb_size: (0, 0), show_original: false },
            mcp: McpState::Idle,
            screenshot: None,
            show_left: true,
            show_right: true,
            capture_for: None,
            shortcut_filter: String::new(),
            win_shortcuts: false,
            win_settings: false,
            win_trace: false,
            win_mcp: false,
            win_notes: false,
            win_array: false,
            win_offset: false,
            win_search: false,
            win_about: false,
            search_text: String::new(),
            search_index: 0,
            array: (3, 2, 5.0, 5.0),
            offset_dist: 2.0,
            offset_replace: false,
            canvas: Default::default(),
            lock_aspect: true,
            jog_step: 10.0,
            console_input: String::new(),
            pending_quit: false,
            ports: Vec::new(),
            warnings: Vec::new(),
            opts: ToolOpts::default(),
            image_sel: None,
            preview_playing: false,
            preview_speed: 50.0,
            job_started: None,
            job_estimate: 0.0,
            job_finished_after: None,
            invert_bed: false,
            tabs: vec![None],
            active_tab: 0,
            win_circular: false,
            win_curve: false,
            array_c2c: false,
            net_job: None,
            array_edit: None,
            welcome_off: false,
            tour: None,
            ui_rects: HashMap::new(),
            win_help: false,
            help_topic: 0,
            help_search: String::new(),
            tab_rename: None,
            curve_dlg: (false, 5, true, crate::sketch::CurveAnchor::Centre),
            path_copy: crate::sketch::PathCopyOpts::default(),
            construction_prev: None,
            circular: (6, 360.0, true, P2::default()),
            win_barcode: false,
            barcode: Default::default(),
            hover_action: None,
            offset_preview: Vec::new(),
            offset_opts: Default::default(),
            win_vartext: false,
            win_camera: false,
            #[cfg(usb_lasers)]
            usb_job: None,
            #[cfg(not(target_arch = "wasm32"))]
            camera: None,
            camera_frame: None,
            camera_tex: None,
            camera_error: None,
            camera_pick: None,
            #[cfg(not(target_arch = "wasm32"))]
            update: std::sync::Arc::new(std::sync::Mutex::new(crate::update::State::Idle)),
            update_announced: false,
            win_rotary: false,
            win_test: false,
            test_params: Default::default(),
            kerf_gap: 0.0,
            test_on_tile: false,
            warp: Default::default(),
            rotate_about: [1, 1],
            win_shape_tools: false,
            bolden_amount: 0.2,
            bridge_width: 1.0,
            tile_date: crate::colony::today(),
            tile_size: Default::default(),
            materials: crate::materials::Library::load(),
            win_materials: false,
            materials_this_laser: true,
            materials_dirty: false,
            save_material: None,
            win_nest: false,
            rename: None,
            rename_click: None,
            autosave: crate::autosave::Autosave::new(crate::autosave::Store::none()),
            tab_ids: vec![1],
            next_tab_id: 2,
            autosave_rev: 0,
            win_recover: false,
            win_export: false,
            export_opts: Default::default(),
            pdf_pending: None,
            clip_picture: None,
            autosave_warned: false,
            nest_job: None,
            nest_result: None,
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, now: f64) {
        self.toasts.push(Toast { text: text.into(), until: now + 4.0, error: false });
    }
    pub fn error(&mut self, text: impl Into<String>, now: f64) {
        self.toasts.push(Toast { text: text.into(), until: now + 7.0, error: true });
    }

    /// Call before changing the document.
    pub fn checkpoint(&mut self) {
        self.undo.push((self.doc.clone(), self.sel.clone()));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.touch();
    }
    /// Mark the document as changed (for caches and the title).
    /// Autosave, once a frame: copy projects with unsaved changes after a pause in editing (or every few
    /// minutes while editing goes on), drop the copies of projects that are saved, and keep the heartbeat.
    pub fn autosave_tick(&mut self, now: f64) {
        if !self.settings.autosave || self.screenshot.is_some() || self.tab_ids.len() != self.tabs.len() {
            return;
        }
        let active = self.tab_ids[self.active_tab];
        if self.rev != self.autosave_rev {
            self.autosave_rev = self.rev;
            if self.dirty {
                self.autosave.changed(active, now);
            }
        }
        let interval = self.settings.autosave_minutes.clamp(0.5, 60.0) * 60.0;
        for i in 0..self.tabs.len() {
            let id = self.tab_ids[i];
            let dirty = if i == self.active_tab { self.dirty } else { self.tabs[i].as_ref().is_some_and(|t| t.dirty) };
            if !dirty {
                if self.autosave.stored(id) {
                    self.autosave.clear(id);
                }
                continue;
            }
            if !self.autosave.due(id, dirty, now, interval) {
                continue;
            }
            let name = self.tab_title(i).0;
            let (text, path) = if i == self.active_tab {
                (crate::lbrn::save_string(&self.doc, &self.kept), self.path.clone())
            } else {
                let Some(t) = self.tabs[i].as_ref() else { continue };
                (crate::lbrn::save_string(&t.doc, &t.kept), t.path.clone())
            };
            let path = path.map(|p| p.to_string_lossy().to_string());
            self.autosave.write(id, &text, &name, path.as_deref(), now);
        }
        self.autosave.heartbeat(now);
        if let Some(e) = self.autosave.failed.take() {
            if !self.autosave_warned {
                self.autosave_warned = true;
                self.error(format!("Autosave couldn't keep a copy of your project ({e}). Save your work as usual."), now);
            }
        }
    }

    /// Does any open project tab have unsaved changes?
    pub fn any_dirty(&self) -> bool {
        self.dirty || self.tabs.iter().flatten().any(|t| t.dirty)
    }

    /// Bring back a project found by autosave (as a new tab, unsaved, so you choose where to save it).
    pub fn recover_project(&mut self, f: &crate::autosave::Found, now: f64) {
        let Some(text) = self.autosave.recover(f) else {
            self.error(format!("\u{201c}{}\u{201d} couldn't be recovered: its copy is damaged.", f.name), now);
            return;
        };
        match crate::lbrn::load_str(&text) {
            Ok(l) => {
                if !self.doc.shapes.is_empty() || self.dirty || self.path.is_some() {
                    self.new_tab(None);
                }
                self.new_doc();
                self.doc = l.doc;
                self.kept = l.kept;
                self.doc.sync_cuts();
                self.doc.title = Some(format!("{} (recovered)", f.name));
                // never pointed at the original file: Save asks where, so nothing is overwritten by surprise
                self.path = None;
                self.dirty = true;
                self.rev += 1;
                self.zoom_to_fit_all();
                self.autosave.forget(f);
                let from = f.path.as_deref().map(|p| format!(" (it was {p})")).unwrap_or_default();
                self.toast(format!("Recovered \u{201c}{}\u{201d}{from}. Save it to keep it.", f.name), now);
            }
            Err(e) => self.error(format!("\u{201c}{}\u{201d} couldn't be recovered: {e}", f.name), now),
        }
    }

    pub fn touch(&mut self) {
        self.rev += 1;
        self.content_rev += 1;
        self.dirty = true;
        self.job = None;
    }

    /// A change that only moved, scaled or rotated whole shapes: cached outlines are transformed, not rebuilt.
    pub fn touch_xform(&mut self) {
        self.rev += 1;
        self.dirty = true;
        self.job = None;
    }

    /// Bounds of shape `i` (world), from the cache when it is current.
    pub fn shape_box(&self, i: usize) -> BBox {
        if self.outline_cache.0 == self.rev && i < self.bbox_cache.len() && self.bbox_cache.len() == self.doc.shapes.len() {
            self.bbox_cache[i]
        } else {
            self.doc.shapes.get(i).map(|s| s.bbox()).unwrap_or(BBox::EMPTY)
        }
    }

    /// Bounds of the selection, from the cache when it is current.
    pub fn selection_box(&self) -> BBox {
        self.sel.iter().fold(BBox::EMPTY, |b, i| b.union(&self.shape_box(*i)))
    }

    pub fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        format!("{}{} — BurnCraft", if self.dirty { "• " } else { "" }, name)
    }

    pub fn sel_valid(&mut self) {
        let n = self.doc.shapes.len();
        self.sel.retain(|i| *i < n);
        self.sel.dedup();
    }

    pub fn selected_bitmap(&self) -> Option<usize> {
        self.sel.iter().copied().find(|i| self.doc.shapes.get(*i).map(|s| s.is_bitmap()).unwrap_or(false))
    }

    pub fn index_of_id(&self, id: u64) -> Option<usize> {
        self.doc.shapes.iter().position(|s| s.id == id)
    }

    pub fn enabled(&self, a: Action) -> bool {
        let has_sel = !self.sel.is_empty();
        match a {
            Action::Copy | Action::CopyPicture | Action::Cut | Action::Duplicate | Action::Delete | Action::FlipH | Action::FlipV | Action::RotateCW | Action::RotateCCW | Action::PageCenter | Action::PushFront | Action::PushBack | Action::ZoomSelection | Action::GridArray | Action::ConvertToPath | Action::Offset | Action::BreakApart => has_sel,
            Action::Group | Action::Weld | Action::BoolUnion | Action::BoolSubtract | Action::BoolIntersect | Action::AlignLeft | Action::AlignRight | Action::AlignTop | Action::AlignBottom | Action::AlignHCenter | Action::AlignVCenter => self.sel.len() >= 2,
            Action::DistributeH | Action::DistributeV => self.sel.len() >= 3,
            Action::Ungroup => self.sel.iter().any(|i| matches!(self.doc.shapes.get(*i).map(|s| &s.kind), Some(ShapeKind::Group(_)))),
            // a picture copied in another app can always be pasted
            Action::Paste => true,
            Action::PasteInPlace => !self.clipboard.is_empty(),
            Action::Undo => !self.undo.is_empty(),
            Action::Redo => !self.redo.is_empty(),
            Action::TraceImage | Action::TraceMcp | Action::ImageInvert | Action::ImageReset => self.selected_bitmap().is_some(),
            Action::TextOnCircle => self.sel.iter().any(|i| self.doc.shapes.get(*i).is_some_and(|s| matches!(s.kind, ShapeKind::Text { .. }))),
            Action::TextOnPath => self.sel.len() == 2 && self.sel.iter().any(|i| self.doc.shapes.get(*i).is_some_and(|s| matches!(s.kind, ShapeKind::Text { .. }))),
            Action::ImageMask | Action::ImageMaskOut => self.sel.iter().filter(|i| self.doc.shapes.get(**i).is_some_and(|s| s.is_bitmap())).count() == 1 && self.sel.len() >= 2,
            // the K40 is opened over USB for each job, so there's nothing to connect first (no pause on its board)
            Action::LaserHome | Action::LaserStop if self.settings.device.controller == gcode::Controller::K40Nano => cfg!(usb_lasers),
            Action::LaserPause if self.settings.device.controller == gcode::Controller::K40Nano => false,
            // galvo: no homing, pause or job file; Stop always reaches the board
            Action::LaserHome | Action::LaserPause | Action::ExportGcode if self.settings.device.controller == gcode::Controller::EzCad2 => false,
            Action::LaserStop if self.settings.device.controller == gcode::Controller::EzCad2 => cfg!(usb_lasers),
            Action::LaserHome | Action::LaserPause | Action::LaserStop => self.laser.connected,
            Action::LaserStart | Action::LaserFrame => !self.doc.shapes.is_empty(),
            Action::Preview | Action::ExportGcode | Action::PreviewPlay => !self.doc.shapes.is_empty(),
            Action::NodeSmooth | Action::NodeCorner | Action::NodeLine | Action::NodeCurve | Action::NodeInsert | Action::NodeBreak => self.tool == Tool::Nodes && !self.canvas.nodes.is_empty(),
            Action::SelectionToPath | Action::ClearSelection => self.image_sel.is_some(),
            Action::CircularArray => has_sel,
            Action::PathPattern | Action::AlignToPath => self.sel.len() >= 2,
            Action::JoinPaths => has_sel,
            Action::EditArray => self.sel.iter().any(|i| self.doc.shapes.get(*i).map(|s| crate::relations::array_of(&self.doc, s.id).is_some()).unwrap_or(false)),
            Action::CloseTab => self.tabs.len() > 1,
            _ => true,
        }
    }

    pub fn is_active(&self, a: Action) -> bool {
        match a {
            Action::ToggleFilled => self.settings.filled_view,
            Action::ToggleGrid => self.settings.show_grid,
            Action::Preview => self.preview_on,
            Action::LaserConnect => self.laser.connected,
            Action::PreviewPlay => self.preview_playing,
            Action::InvertBed => self.invert_bed,
            _ => self.tool.action() == a,
        }
    }

    pub fn tooltip(&self, a: Action) -> String {
        let k = self.settings.keymap.label(a);
        if k.is_empty() {
            format!("{}\n{}", a.label(), a.tip())
        } else {
            format!("{}  ({})\n{}", a.label(), k, a.tip())
        }
    }

    pub fn chord_action(&self, c: &Chord) -> Option<Action> {
        self.settings.keymap.lookup(c)
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.canvas.cancel();
        self.canvas.rel_picks.clear();
    }

    pub fn save_materials(&mut self, now: f64) {
        self.materials_dirty = false;
        if let Err(e) = self.materials.save() {
            self.error(format!("Could not save the materials: {e}"), now);
        }
    }

    /// The next Colony serial number ("Ant #n"), counted per user and remembered.
    pub fn next_serial(&mut self, now: f64) -> u32 {
        self.settings.colony_count = self.settings.colony_count.saturating_add(1);
        self.save_settings(now);
        self.settings.colony_count
    }

    /// Open "Save as material" for these settings (a layer's, or a test square's).
    pub fn ask_save_material(&mut self, cut: CutSetting, pulse_width: Option<u16>, link_layer: Option<usize>) {
        self.save_material = Some(crate::matwin::SaveMaterial { cut, pulse_width, name: String::new(), thickness: 0.0, notes: String::new(), link_layer });
    }

    pub fn save_settings(&mut self, now: f64) {
        if let Err(e) = self.settings.save() {
            self.error(format!("Could not save settings: {e}"), now);
        }
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        crate::theme::apply(ctx, self.settings.theme == Theme::Dark, self.settings.ui_scale);
    }

    // ------------------------------------------------------------ file handling

    pub fn new_doc(&mut self) {
        self.doc = Document::default();
        self.welcome_off = false;
        self.kept.clear();
        self.path = None;
        self.dirty = false;
        self.sel.clear();
        self.undo.clear();
        self.redo.clear();
        self.textures.clear();
        self.preview_on = false;
        self.rev += 1;
        self.content_rev += 1;
        self.job = None;
        self.view.fitted = false;
        self.image_sel = None;
        self.warnings.clear();
        self.canvas.nodes.clear();
    }

    // ------------------------------------------------------------ project tabs

    fn stash(&mut self) -> DocTab {
        DocTab {
            doc: std::mem::take(&mut self.doc),
            kept: std::mem::take(&mut self.kept),
            path: self.path.take(),
            dirty: self.dirty,
            sel: std::mem::take(&mut self.sel),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            view: self.view,
            warnings: std::mem::take(&mut self.warnings),
        }
    }

    fn restore(&mut self, t: DocTab) {
        self.doc = t.doc;
        self.kept = t.kept;
        self.path = t.path;
        self.dirty = t.dirty;
        self.sel = t.sel;
        self.undo = t.undo;
        self.redo = t.redo;
        self.view = t.view;
        self.warnings = t.warnings;
        self.textures.clear();
        self.image_sel = None;
        self.preview_on = false;
        self.job = None;
        self.canvas.cancel();
        self.canvas.nodes.clear();
        self.rev += 1;
        self.content_rev += 1;
    }

    pub fn switch_tab(&mut self, i: usize) {
        if i == self.active_tab || i >= self.tabs.len() {
            return;
        }
        let cur = self.stash();
        self.tabs[self.active_tab] = Some(cur);
        if let Some(t) = self.tabs[i].take() {
            self.restore(t);
        }
        self.active_tab = i;
    }

    /// Open a new tab (empty, or with `doc`) and switch to it.
    pub fn new_tab(&mut self, doc: Option<Document>) {
        let cur = self.stash();
        self.tabs[self.active_tab] = Some(cur);
        self.tabs.push(None);
        self.tab_ids.push(self.next_tab_id);
        self.next_tab_id += 1;
        self.active_tab = self.tabs.len() - 1;
        self.new_doc();
        if let Some(d) = doc {
            self.doc = d;
            self.dirty = true;
            self.zoom_to_fit_all();
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        if self.tabs.len() <= 1 || i >= self.tabs.len() {
            return;
        }
        // like LightBurn's Remove Page, ask first — here only when there's unsaved work
        let dirty = if i == self.active_tab { self.dirty } else { self.tabs[i].as_ref().map(|t| t.dirty).unwrap_or(false) };
        if dirty && self.screenshot.is_none() && !cfg!(test) {
            let name = self.tab_title(i).0;
            let ok = matches!(
                crate::platform::MessageDialog::new()
                    .set_title("Close Tab")
                    .set_description(&format!("\"{name}\" has unsaved changes. Close it anyway?"))
                    .set_buttons(crate::platform::MessageButtons::YesNo)
                    .show(),
                crate::platform::MessageDialogResult::Yes
            );
            if !ok {
                return;
            }
        }
        if i == self.active_tab {
            let next = if i + 1 < self.tabs.len() { i + 1 } else { i - 1 };
            self.switch_tab(next);
        }
        self.tabs.remove(i);
        // a closed project's autosave copy goes with it
        if i < self.tab_ids.len() {
            let id = self.tab_ids.remove(i);
            self.autosave.clear(id);
        }
        if self.active_tab > i {
            self.active_tab -= 1;
        }
    }

    pub fn tab_title(&self, i: usize) -> (String, bool) {
        let (path, dirty) = if i == self.active_tab {
            (self.path.as_ref(), self.dirty)
        } else {
            match &self.tabs[i] {
                Some(t) => (t.path.as_ref(), t.dirty),
                None => (None, false),
            }
        };
        let title = if i == self.active_tab { self.doc.title.clone() } else { self.tabs[i].as_ref().and_then(|t| t.doc.title.clone()) };
        let name = title.or_else(|| path.and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| format!("Untitled {}", i + 1));
        (name, dirty)
    }

    /// Rename project tab `i` (an empty name goes back to the file name).
    /// Like LightBurn's Rename Page: the name is trimmed, blank means the default
    /// name, and an unchanged name changes nothing.
    pub fn rename_tab(&mut self, i: usize, name: &str) {
        let name = name.trim();
        let title = (!name.is_empty()).then(|| name.to_string());
        let current = if i == self.active_tab { self.doc.title.clone() } else { self.tabs.get(i).and_then(|t| t.as_ref()).and_then(|t| t.doc.title.clone()) };
        if title == current || title.as_deref() == Some(self.tab_title(i).0.as_str()) && current.is_none() {
            return;
        }
        if i == self.active_tab {
            self.doc.title = title;
            self.dirty = true;
        } else if let Some(Some(t)) = self.tabs.get_mut(i) {
            t.doc.title = title;
            t.dirty = true;
        }
    }

    /// "Project_Name_-_Device_Name_-_Print_Time.gcode" for the Save G-code dialog.
    pub fn gcode_file_name(&self, seconds: f64) -> String {
        let clean = |s: &str| -> String {
            let t: String = s.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
            let mut out = String::new();
            for c in t.chars() {
                if !(c == '_' && out.ends_with('_')) {
                    out.push(c);
                }
            }
            out.trim_matches('_').to_string()
        };
        let (project, _) = self.tab_title(self.active_tab);
        let total = seconds.max(0.0).round() as i64;
        let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
        let time = if h > 0 { format!("{h}h{m:02}m{s:02}s") } else { format!("{m}m{s:02}s") };
        format!("{}_-_{}_-_{}.gcode", clean(&project), clean(&self.settings.device.name), time)
    }

    /// Set up the laser from a LightBurn device profile (.lbdev); the first laser in the file is used.
    pub fn import_lbdev(&mut self, text: &str, now: f64) {
        match crate::gcode::devices_from_lbdev(text) {
            Ok((devs, notes)) => {
                let d = devs[0].clone();
                let keep_port = std::mem::take(&mut self.settings.device.port);
                self.settings.device = crate::gcode::Device { port: keep_port, ..d };
                self.save_settings(now);
                self.touch();
                let more = if devs.len() > 1 { format!(" ({} more in the file)", devs.len() - 1) } else { String::new() };
                self.toast(format!("Laser set up: {} · {} × {} {}{more}", self.settings.device.name, fmt_num(self.settings.units.round(self.settings.device.width)), fmt_num(self.settings.units.round(self.settings.device.height)), self.settings.units.suffix()), now);
                for n in notes {
                    self.warnings.push(n);
                }
            }
            Err(e) => self.error(format!("Could not read this device profile: {e}"), now),
        }
    }

    /// Open a file from disk (command line, recent files, dropped files).
    pub fn open_path(&mut self, p: PathBuf, now: f64) {
        match std::fs::read(&p) {
            Ok(bytes) => self.open_bytes(&p.to_string_lossy(), bytes, Some(p.clone()), now),
            Err(e) => self.error(format!("Could not open the file: {e}"), now),
        }
    }

    /// Show a file picker (desktop) or the browser's picker; the file then goes to `picked`.
    pub fn pick_file(&mut self, purpose: Pick, label: &str, exts: &[&str], now: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(p) = crate::platform::FileDialog::new().add_filter(label, exts).add_filter("All files", &["*"]).pick_file() {
                match std::fs::read(&p) {
                    Ok(bytes) => self.picked(purpose, &p.to_string_lossy(), bytes, Some(p.clone()), now),
                    Err(e) => self.error(format!("Could not read the file: {e}"), now),
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (label, now);
            crate::platform::request_file(purpose as u8, exts);
        }
    }

    /// A file arrived (picked, dropped or loaded in the browser).
    pub fn picked(&mut self, purpose: Pick, name: &str, bytes: Vec<u8>, path: Option<PathBuf>, now: f64) {
        match purpose {
            Pick::Open => self.open_bytes(name, bytes, path, now),
            Pick::Import => self.import_bytes(name, bytes, now),
            Pick::Lbdev => self.import_lbdev(&String::from_utf8_lossy(&bytes), now),
            Pick::Materials => match crate::materials::Library::from_json(&String::from_utf8_lossy(&bytes)) {
                Ok(l) => {
                    let (added, replaced) = self.materials.merge(l);
                    self.save_materials(now);
                    self.toast(format!("Materials imported: {added} new, {replaced} updated"), now);
                }
                Err(e) => self.error(e, now),
            },
            Pick::Shortcuts => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(v) => {
                    self.settings.keymap = crate::shortcuts::Keymap::from_json(&v);
                    self.save_settings(now);
                    self.toast("Shortcuts imported", now);
                }
                Err(e) => self.error(format!("That isn't a shortcuts file: {e}"), now),
            },
            Pick::VarText => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let short = std::path::Path::new(name).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
                self.doc.vartext.load(&short, &text);
                self.dirty = true;
                self.touch();
                let v = &self.doc.vartext;
                self.toast(format!("{} row(s) loaded from {short}{}", v.rows.len(), if v.headers.is_empty() { String::new() } else { format!(" · columns: {}", v.headers.join(", ")) }), now);
            }
            Pick::CameraPhoto => match crate::camera::decode(&bytes, 1) {
                Ok(f) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.camera = None;
                    }
                    self.camera_frame = Some(f);
                    self.camera_tex = None;
                    self.camera_error = None;
                    self.settings.camera.source.clear();
                }
                Err(e) => self.error(format!("Could not use that photo: {e}"), now),
            },
        }
    }

    /// Save bytes: a save dialog on desktop (returns the path), a download in the browser (returns None).
    /// Err(None) = cancelled.
    pub fn save_file(&mut self, label: &str, exts: &[&str], name: &str, bytes: &[u8]) -> Result<Option<PathBuf>, Option<String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(p) = crate::platform::FileDialog::new().add_filter(label, exts).set_file_name(name).save_file() else { return Err(None) };
            std::fs::write(&p, bytes).map(|_| Some(p)).map_err(|e| Some(e.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (label, exts);
            let mime = if name.ends_with(".lbrn2") { "application/xml" } else if name.ends_with(".burn") { crate::burn::MIME } else { "application/octet-stream" };
            crate::platform::download(name, bytes, mime).map(|_| None).map_err(Some)
        }
    }

    /// After a job starts: the next variable-text row and serial number, when that's switched on and the
    /// project has variable text.
    /// Is a rotary being used for jobs? (Galvo rotaries work differently and aren't covered yet.)
    pub fn rotary_on(&self) -> bool {
        self.settings.device.rotary.enabled && self.settings.device.controller != gcode::Controller::EzCad2
    }

    /// The design as the laser will run it: with a rotary, distances around the object become rotary travel
    /// (and the G-code uses A instead of Y for an A-axis rotary). The preview keeps showing the flat design.
    pub fn machine_job(&self) -> gcode::Job {
        self.machine_job_at(&gcode::Placement::absolute(&self.settings.device))
    }

    /// The same, placed on the laser as `place` says (Start from).
    pub fn machine_job_at(&self, place: &gcode::Placement) -> gcode::Job {
        let dev = &self.settings.device;
        if !self.rotary_on() {
            return gcode::build_job_placed(&self.doc, dev, None, place);
        }
        let d = crate::rotary::machine_doc(&self.doc, &dev.rotary, dev.height);
        let mut job = gcode::build_job_placed(&d, dev, None, place);
        if dev.rotary.axis == crate::rotary::RotaryAxis::A && dev.controller.family() == gcode::Family::GCode {
            job.gcode = crate::rotary::y_to_a(&job.gcode);
        }
        job
    }

    /// Put the selected text along the other selected shape's first line (copied into the text, so the text
    /// stays editable and the curve can be deleted), or around a circle.
    pub fn text_on_path(&mut self, circle: bool) -> Result<(), String> {
        let ti = self.sel.iter().copied().find(|i| self.doc.shapes.get(*i).is_some_and(|s| matches!(s.kind, ShapeKind::Text { .. }))).ok_or("Select a text first.")?;
        let tx = self.doc.shapes[ti].xform;
        // the text's own width (before its scaling), since the circle is in the text's own coordinates
        let width = self.doc.shapes[ti].bbox().width() / p2(tx.a, tx.b).len().max(1e-9);
        let mut tp = TextPath::default();
        if circle {
            // big enough that the text goes about a third of the way round
            tp.radius = (width / (std::f64::consts::TAU / 3.0)).max(5.0);
        } else {
            let pi = self.sel.iter().copied().find(|i| *i != ti).ok_or("Select the text and a line or curve to put it on.")?;
            let line = self.doc.shapes[pi].world_outline(0.02).into_iter().max_by(|a, b| {
                let len = |l: &Vec<P2>| l.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
                len(a).total_cmp(&len(b))
            });
            let inv = tx.inverse().ok_or("This text is squashed flat.")?;
            let line = line.filter(|l| l.len() >= 2).ok_or("That shape has no line to follow.")?;
            tp.curve = line.into_iter().map(|p| inv.apply(p)).collect();
        }
        self.checkpoint();
        if let ShapeKind::Text { style, backup, .. } = &mut self.doc.shapes[ti].kind {
            style.path = Some(tp);
            *backup = None;
        }
        self.sel = vec![ti];
        self.touch();
        Ok(())
    }

    /// Mask the selected picture with the other selected closed shapes: keep the inside (or the outside).
    /// The pixels left out become see-through (they never burn); the shapes stay. Returns how many changed.
    pub fn mask_picture(&mut self, keep_inside: bool) -> Result<usize, String> {
        let pics: Vec<usize> = self.sel.iter().copied().filter(|i| self.doc.shapes.get(*i).is_some_and(|s| s.is_bitmap())).collect();
        let [pi] = pics[..] else { return Err("Select one picture and the closed shape to mask it with.".into()) };
        let masks: Vec<Vec<P2>> = self.sel.iter().filter(|i| **i != pi).flat_map(|i| self.doc.shapes[*i].world_outline(0.02)).filter(|l| is_closed(l)).collect();
        if masks.is_empty() {
            return Err("Select a closed shape too (a rectangle, ellipse, closed path or text) to mask the picture with.".into());
        }
        let s = &self.doc.shapes[pi];
        let ShapeKind::Bitmap { w, h, data, .. } = &s.kind else { unreachable!() };
        let inv = s.xform.inverse().ok_or("This picture can't be masked (it's squashed flat).")?;
        let (pw, ph) = (data.width as f64, data.height as f64);
        // bed → the picture's own pixels (y down)
        let rings: Vec<Vec<P2>> = masks.iter().map(|r| r.iter().map(|p| inv.apply(*p)).map(|l| p2((l.x + w / 2.0) / w * pw, (h / 2.0 - l.y) / h * ph)).collect()).collect();
        let mut rgba = (*data.rgba).clone();
        let n = crate::imaging::mask_pixels(&mut rgba, data.width as usize, data.height as usize, &rings, keep_inside);
        if n == 0 {
            return Err(if keep_inside { "Nothing changed: the picture is all inside the shape, or the parts outside it are already masked." } else { "Nothing changed: the shape doesn't cover the picture, or that part is already masked." }.into());
        }
        let png = crate::platform::png_bytes(data.width, data.height, rgba.clone()).ok_or("The masked picture couldn't be stored.")?;
        let new = BitmapData { width: data.width, height: data.height, rgba: std::sync::Arc::new(rgba), encoded: std::sync::Arc::new(png), source_file: data.source_file.clone() };
        self.checkpoint();
        if let ShapeKind::Bitmap { data, .. } = &mut self.doc.shapes[pi].kind {
            *data = new;
        }
        self.sel = vec![pi];
        self.touch();
        Ok(n)
    }

    /// Where the next job goes on the laser (the Laser panel's Start from). `for_file`: a saved G-code file,
    /// which runs later from wherever the head is then. Only G-code lasers can start anywhere but the
    /// design's own position.
    pub fn job_placement(&self, for_file: bool) -> Result<gcode::Placement, String> {
        let dev = &self.settings.device;
        let js = self.settings.job_start;
        let mut p = gcode::Placement::absolute(dev);
        if dev.controller.family() != gcode::Family::GCode {
            return Ok(p);
        }
        p.anchor = js.anchor;
        // the laser's 0,0 on the bed: moved by "Set origin here" (G92); unknown if that was done unhomed
        let origin = if for_file { Some((0.0, 0.0)) } else { self.laser.origin };
        let o = origin.unwrap_or((0.0, 0.0));
        let bed_here = |o: (f64, f64)| {
            let b = gcode::bed_box(dev);
            BBox { min: p2(b.min.x - o.0, b.min.y - o.1), max: p2(b.max.x - o.0, b.max.y - o.1) }
        };
        // after "Set origin here" on an unhomed laser BurnCraft doesn't know how the laser's coordinates sit on the
        // bed, so it can't put a job on a point of the bed (or the head's position) safely
        let unknown_origin = "The origin was moved (Set origin here) before the laser was homed, so BurnCraft can't tell where this start point is. Press Clear origin (or home and set it again), or use Absolute coordinates.";
        match js.from {
            gcode::StartFrom::Absolute => {
                // the shapes left out are the ones off the real bed, after a moved origin too
                if let Some(o) = origin {
                    p.bed = Some(bed_here(o));
                }
            }
            gcode::StartFrom::UserOrigin => {
                if origin.is_none() {
                    return Err(unknown_origin.into());
                }
                let t = p2(js.user_origin.0 - o.0, js.user_origin.1 - o.1);
                p.target = Some(t);
                p.end = t;
                p.bed = origin.map(bed_here);
            }
            gcode::StartFrom::CurrentPosition if for_file => {
                // the file moves relative to where the head is when it starts (G91), so it changes none of
                // the laser's coordinate settings
                p.target = Some(P2::default());
                p.bed = None;
                p.relative = true;
            }
            gcode::StartFrom::CurrentPosition => {
                if !self.laser.connected {
                    return Err("Start from: Current position needs to know where the head is. Connect to the laser first.".into());
                }
                if origin.is_none() {
                    return Err(unknown_origin.into());
                }
                // the position is only right once the head has stopped
                if !self.laser.state.starts_with("Idle") {
                    return Err(format!("Start from: Current position needs the head to be still, and the laser says \u{201c}{}\u{201d}. Wait until it's Idle, then try again.", self.laser.state));
                }
                let pos = self.laser.pos;
                let t = if self.laser.machine_pos { p2(pos.0 - o.0, pos.1 - o.1) } else { p2(pos.0, pos.1) };
                p.target = Some(t);
                p.start = t;
                p.end = t;
                p.return_to = Some(t);
                p.bed = (self.laser.homed && origin.is_some()).then(|| bed_here(o));
            }
        }
        Ok(p)
    }

    /// Send a placed job's (or its frame's) G-code. When the bed position isn't known (Current position on a
    /// laser that wasn't homed) the edges can't be checked: BurnCraft checks the job is no bigger than the
    /// work area, and says so.
    fn send_placed(&mut self, lines: Vec<String>, job: &gcode::Job, place: &gcode::Placement, now: f64) -> bool {
        if place.bed.is_some() {
            return self.send_gcode(lines, now);
        }
        let mut b = BBox::EMPTY;
        for m in &job.moves {
            b.add(m.from);
            b.add(m.to);
        }
        let d = &self.settings.device;
        let check_y = !(self.rotary_on() && d.rotary.axis == crate::rotary::RotaryAxis::Y);
        if !b.is_empty() && (b.width() > d.width + 0.001 || (check_y && b.height() > d.height + 0.001)) {
            self.error("This job is bigger than the work area, so it wasn't sent.", now);
            return false;
        }
        self.toast("The laser wasn't homed, so BurnCraft can't check that this stays on the bed (only that it's no bigger than the bed). Frame it first.", now);
        self.laser.send(Cmd::Send(lines));
        true
    }

    /// Send G-code lines to the connected laser, unless a move would leave the work area (then say so and
    /// send nothing). Every G-code move BurnCraft sends (Frame, Start, jog) goes through here.
    pub fn send_gcode(&mut self, lines: Vec<String>, now: f64) -> bool {
        let d = &self.settings.device;
        let check_y = !(self.rotary_on() && d.rotary.axis == crate::rotary::RotaryAxis::Y);
        // after "Set origin here" the job's 0,0 isn't the bed's: check against the real bed
        let origin = self.laser.origin;
        if origin.is_none() {
            self.error("The origin was moved (Set origin here) before the laser was homed, so BurnCraft can't check that this stays on the bed. Make sure the job fits from where the head is, or press Clear origin.", now);
        }
        let o = origin.unwrap_or((0.0, 0.0));
        let pos = (self.laser.homed && origin.is_some()).then(|| if self.laser.machine_pos { self.laser.pos } else { (self.laser.pos.0 + o.0, self.laser.pos.1 + o.1) });
        let d = &self.settings.device;
        match gcode::check_moves_from(&lines, d.width, d.height, check_y, pos, o) {
            Ok(()) => {
                self.laser.send(Cmd::Send(lines));
                true
            }
            Err(e) => {
                self.error(e, now);
                false
            }
        }
    }

    /// "Set origin here" (G92 X0 Y0): jobs then start from where the head is. BurnCraft remembers where that is on
    /// the bed (when the laser has been homed), so the work-area check still uses the real bed.
    pub fn set_origin_here(&mut self, now: f64) {
        self.laser.send(Cmd::Send(vec!["G92 X0 Y0".into()]));
        if self.laser.homed {
            let o = self.laser.origin.unwrap_or((0.0, 0.0));
            let here = if self.laser.machine_pos { self.laser.pos } else { (self.laser.pos.0 + o.0, self.laser.pos.1 + o.1) };
            self.laser.origin = Some(here);
            self.toast("Origin set here: jobs start from this point, and BurnCraft still keeps them on the bed.", now);
        } else {
            self.laser.origin = None;
            self.error("Origin set, but the laser hasn't been homed, so BurnCraft can't tell where this is on the bed and can't fully check that jobs stay on it. Home first if your laser has home switches.", now);
        }
    }

    /// Before Frame or Start: if the laser hasn't been homed since connecting, positions may be wrong. Once
    /// per connection (many diode lasers have no home switches, so it's a warning, not a stop).
    fn warn_if_not_homed(&mut self, now: f64) {
        if self.laser.state.starts_with("Alarm") {
            self.error("The laser reports an alarm (often: not homed yet). Press Home, or unlock it ($X) in the console, before framing or burning.", now);
        } else if !self.laser.homed && !self.laser.home_warned {
            self.laser.home_warned = true;
            self.toast("The laser hasn't been homed since connecting, so its position may be wrong. Press Home first if it has home switches.", now);
        }
    }

    pub fn advance_variable_text(&mut self, now: f64) {
        let has = self.doc.shapes.iter().any(|s| matches!(&s.kind, ShapeKind::Text { style, .. } if style.variable));
        if !has || !self.doc.vartext.advance {
            return;
        }
        self.doc.vartext.advance_row();
        crate::vartext::set_current(&self.doc.vartext);
        // the job already built keeps the old text; the canvas shows the next one
        let job = self.job.take();
        self.touch();
        self.job = job;
        let v = &self.doc.vartext;
        let row = if v.rows.is_empty() { String::new() } else { format!("row {} of {}, ", v.row + 1, v.rows.len()) };
        self.toast(format!("Variable text: next is {row}serial {}", v.serial_text()), now);
    }

    /// Open a project (or hand pictures, device profiles and EZCad files to import).
    pub fn open_bytes(&mut self, name: &str, bytes: Vec<u8>, path: Option<PathBuf>, now: f64) {
        let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "lbdev" {
            self.import_lbdev(&String::from_utf8_lossy(&bytes), now);
            return;
        }
        if ext == "csv" || ext == "txt" {
            self.picked(Pick::VarText, name, bytes, path, now);
            return;
        }
        if ext != "lbrn2" && ext != "lbrn" && ext != "burn" && ext != "xml" {
            self.import_bytes(name, bytes, now);
            return;
        }
        // BurnCraft's own .burn (a zip), or a LightBurn project (XML)
        let loaded = if crate::burn::looks_like(&bytes) { crate::burn::load(&bytes).map(|l| crate::lbrn::Loaded { doc: l.doc, warnings: l.warnings, kept: l.kept }) } else { crate::lbrn::load_str(&String::from_utf8_lossy(&bytes)) };
        match loaded {
            Ok(l) => {
                if !self.doc.shapes.is_empty() || self.dirty || self.path.is_some() {
                    self.new_tab(None);
                }
                self.new_doc();
                self.doc = l.doc;
                self.kept = l.kept;
                self.doc.sync_cuts();
                if let Some(p) = &path {
                    self.settings.add_recent(&p.to_string_lossy());
                    self.save_settings(now);
                }
                if path.is_none() {
                    self.doc.title = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string());
                }
                let n = self.doc.shapes.len();
                self.path = path;
                self.dirty = false;
                self.warnings = l.warnings;
                self.zoom_to_fit_all();
                let w = self.warnings.len();
                self.toast(format!("Opened project with {n} shape{}{}", if n == 1 { "" } else { "s" }, if w > 0 { format!(" ({w} note(s) in the Layers panel)") } else { String::new() }), now);
            }
            Err(e) => self.error(format!("Could not open this project: {e}"), now),
        }
    }

    /// Add a file's contents to this project: another project, an EZCad2 file, or a picture.
    pub fn import_bytes(&mut self, name: &str, bytes: Vec<u8>, now: f64) {
        let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "svg" {
            self.add_svg(&bytes, None, "Imported", now);
            return;
        }
        if ext == "dxf" {
            match crate::dxf::import(&bytes) {
                Ok(imp) => self.add_imported(imp, None, "Imported DXF", now),
                Err(e) => self.error(e, now),
            }
            return;
        }
        if ext == "pdf" {
            match crate::pdf::page_count(&bytes) {
                Ok(n) if n > 1 => {
                    // several pages: ask which one
                    self.pdf_pending = Some((name.to_string(), bytes, n, 1));
                }
                Ok(_) => self.import_pdf_page(&bytes, 1, now),
                Err(e) => self.error(e, now),
            }
            return;
        }
        if ext == "dwg" {
            self.error("DWG drawings can't be opened. In your CAD program, save the drawing as DXF (File → Save As → DXF), then import that.", now);
            return;
        }
        if ext == "ezd" {
            match crate::ezd::import(&bytes, self.settings.device.width, self.settings.device.height) {
                Ok(imp) => {
                    self.checkpoint();
                    let start = self.doc.shapes.len();
                    for (layer, pen) in &imp.pens {
                        let c = self.doc.cut_mut(*layer);
                        if !pen.name.is_empty() {
                            c.name = pen.name.clone();
                        }
                        if pen.speed > 0.0 {
                            c.speed = pen.speed;
                        }
                        if pen.power > 0.0 {
                            c.max_power = pen.power.min(100.0);
                        }
                        c.passes = pen.passes.max(1);
                        if pen.frequency > 0.0 {
                            c.frequency = pen.frequency * 1000.0;
                        }
                    }
                    for (layer, spacing, angle) in &imp.fills {
                        let c = self.doc.cut_mut(*layer);
                        c.mode = CutMode::Fill;
                        if *spacing > 0.0 {
                            c.interval = *spacing;
                        }
                        c.angle = *angle;
                    }
                    let n = imp.shapes.len();
                    self.doc.shapes.extend(imp.shapes);
                    self.doc.sync_cuts();
                    self.sel = (start..self.doc.shapes.len()).collect();
                    self.warnings.extend(imp.notes);
                    self.touch();
                    self.toast(format!("Imported {n} shape(s) from EZCad2, with its pens as layers"), now);
                }
                Err(e) => self.error(format!("Could not import this EZCad2 file: {e}"), now),
            }
            return;
        }
        if ext == "lbrn2" || ext == "lbrn" || ext == "burn" {
            // merge another project (LightBurn's, or BurnCraft's own .burn)
            let loaded = if crate::burn::looks_like(&bytes) { crate::burn::load(&bytes).map(|l| crate::lbrn::Loaded { doc: l.doc, warnings: l.warnings, kept: l.kept }) } else { crate::lbrn::load_str(&String::from_utf8_lossy(&bytes)) };
            match loaded {
                Ok(l) => {
                    self.checkpoint();
                    let start = self.doc.shapes.len();
                    for c in l.doc.cuts {
                        if self.doc.cut(c.index).is_none() {
                            self.doc.cuts.push(c);
                        }
                    }
                    self.doc.shapes.extend(l.doc.shapes);
                    self.doc.sync_cuts();
                    self.sel = (start..self.doc.shapes.len()).collect();
                }
                Err(e) => self.error(format!("Could not import: {e}"), now),
            }
            return;
        }
        self.add_image_bytes(bytes, name, None, now);
    }

    /// Add a picture (file contents or pasted PNG) as an image shape, at `at` or in the middle of the bed.
    pub fn add_image_bytes(&mut self, bytes: Vec<u8>, name: &str, at: Option<P2>, now: f64) {
        {
            match crate::lbrn::decode_bitmap(bytes, name) {
                Some(bd) => {
                    self.checkpoint();
                    // 254 DPI by default (0.1 mm per pixel), shrunk to fit the bed
                    let mut w = bd.width as f64 * 0.1;
                    let mut h = bd.height as f64 * 0.1;
                    let maxw = self.settings.device.width * 0.8;
                    let maxh = self.settings.device.height * 0.8;
                    let k = (maxw / w).min(maxh / h).min(1.0);
                    w *= k;
                    h *= k;
                    let c = at.unwrap_or(p2(self.settings.device.width / 2.0, self.settings.device.height / 2.0));
                    let layer = self.image_layer();
                    let s = Shape::new(ShapeKind::Bitmap { w, h, data: bd, adjust: ImageAdjust::default() }, Xform::translate(c.x, c.y), layer);
                    self.doc.shapes.push(s);
                    self.doc.cut_mut(layer).mode = CutMode::Image;
                    self.doc.sync_cuts();
                    self.sel = vec![self.doc.shapes.len() - 1];
                    self.side_tab = SideTab::Properties;
                    self.toast("Image added. Use Image → Trace Image to turn it into outlines.", now);
                }
                None => self.error("That file isn't an image BurnCraft can read (PNG, JPG, BMP, GIF, WebP, TIFF).", now),
            }
        }
    }

    fn image_layer(&self) -> usize {
        // first layer that's free or already an image layer, starting from C01 (blue) like LightBurn
        for i in 1..COLOR_LAYERS {
            match self.doc.cut(i) {
                None => return i,
                Some(c) if c.mode == CutMode::Image => return i,
                _ => {}
            }
        }
        self.current_layer
    }

    /// Does this project save as a LightBurn file (it was opened from one) rather than BurnCraft's own .burn?
    pub fn saves_as_lbrn(&self) -> bool {
        match self.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_ascii_lowercase()) {
            Some(e) if e == "burn" => false,
            Some(e) if e == "lbrn2" || e == "lbrn" => true,
            _ => self.doc.from_lbrn,
        }
    }

    /// The project's file contents in a format: BurnCraft's own .burn, or a LightBurn .lbrn2.
    pub fn project_bytes(&mut self, burn: bool) -> Result<Vec<u8>, String> {
        self.doc.sync_cuts();
        if burn {
            crate::burn::save(&self.doc, &self.kept)
        } else {
            Ok(crate::lbrn::save_string(&self.doc, &self.kept).into_bytes())
        }
    }

    /// After saving as .lbrn2: say what couldn't go in it, or (once) that .burn keeps everything.
    fn after_lbrn_save(&mut self, now: f64) {
        let n = crate::burn::newer_fields(&self.doc);
        if n > 0 {
            self.error(format!("{n} detail(s) from a newer BurnCraft can't go in a .lbrn2 file and weren't saved there. Save As BurnCraft project (.burn) to keep them."), now);
        } else if !self.settings.burn_hint_shown {
            self.settings.burn_hint_shown = true;
            self.save_settings(now);
            self.toast("Saved as a LightBurn project. Tip: Save As BurnCraft project (.burn) keeps everything, with pictures as real files.", now);
        }
    }

    /// Save to `p`; the extension says the format (.burn, or .lbrn2; none: BurnCraft's .burn).
    pub fn save_to(&mut self, p: PathBuf, now: f64) {
        let mut p = p;
        if p.extension().is_none() {
            p.set_extension(if self.saves_as_lbrn() { "lbrn2" } else { "burn" });
        }
        let burn = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("burn"));
        if !burn && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lbrn")) {
            // BurnCraft writes the newer LightBurn format
            p.set_extension("lbrn2");
        }
        let bytes = match self.project_bytes(burn) {
            Ok(b) => b,
            Err(e) => {
                self.error(format!("Could not save: {e}"), now);
                return;
            }
        };
        match std::fs::write(&p, bytes) {
            Ok(_) => {
                self.settings.add_recent(&p.to_string_lossy());
                self.save_settings(now);
                self.path = Some(p);
                self.dirty = false;
                self.doc.from_lbrn = !burn;
                self.toast("Saved", now);
                if !burn {
                    self.after_lbrn_save(now);
                }
            }
            Err(e) => self.error(format!("Could not save: {e}"), now),
        }
    }

    /// Save As: BurnCraft's own .burn, or a LightBurn .lbrn2 (offered first for projects that came from one).
    pub fn save_as(&mut self, now: f64) {
        let lbrn_first = self.saves_as_lbrn();
        let stem = self.doc.title.clone().or_else(|| self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "project".into());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (burn_f, lbrn_f) = (("BurnCraft project (keeps everything)", ["burn"]), ("LightBurn project", ["lbrn2"]));
            let (first, second) = if lbrn_first { (lbrn_f, burn_f) } else { (burn_f, lbrn_f) };
            let name = format!("{stem}.{}", first.1[0]);
            let Some(p) = crate::platform::FileDialog::new().add_filter(first.0, &first.1).add_filter(second.0, &second.1).set_file_name(&name).save_file() else { return };
            self.save_to(p, now);
        }
        #[cfg(target_arch = "wasm32")]
        {
            // the browser downloads it, in the project's own format
            let burn = !lbrn_first;
            let name = format!("{stem}.{}", if burn { "burn" } else { "lbrn2" });
            match self.project_bytes(burn) {
                Ok(bytes) => {
                    let mime = if burn { crate::burn::MIME } else { "application/xml" };
                    match crate::platform::download(&name, &bytes, mime) {
                        Ok(_) => {
                            self.dirty = false;
                            self.toast(format!("Downloaded {name}"), now);
                            if !burn {
                                self.after_lbrn_save(now);
                            }
                        }
                        Err(e) => self.error(format!("Could not save: {e}"), now),
                    }
                }
                Err(e) => self.error(format!("Could not save: {e}"), now),
            }
        }
    }

    pub fn zoom_to_fit_all(&mut self) {
        let mut b = self.doc.bbox();
        let bed = BBox { min: p2(0.0, 0.0), max: p2(self.settings.device.width, self.settings.device.height) };
        b = b.union(&bed);
        self.canvas.pending_fit = Some(b);
    }

    // ------------------------------------------------------------ commands

    pub fn run(&mut self, a: Action, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if !self.enabled(a) {
            return;
        }
        let sel = self.sel.clone();
        match a {
            Action::New => {
                if self.dirty && !confirm_discard() {
                    return;
                }
                self.new_doc();
            }
            Action::Open => self.pick_file(Pick::Open, "Projects (.burn, .lbrn2), drawings, device profiles, EZCad2", &["burn", "lbrn2", "lbrn", "svg", "dxf", "pdf", "lbdev", "ezd"], now),
            Action::Import => self.pick_file(Pick::Import, "SVG, DXF, PDF, images, projects, EZCad2", &["svg", "dxf", "pdf", "png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff", "burn", "lbrn2", "lbrn", "ezd"], now),
            // a .burn or .lbrn2 file it came from is saved again in its format; otherwise Save As asks
            Action::Save => match self.path.clone() {
                Some(p) if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("burn") || e.eq_ignore_ascii_case("lbrn2")) => self.save_to(p, now),
                _ => self.run(Action::SaveAs, ctx),
            },
            Action::SaveAs => self.save_as(now),
            Action::ExportGcode => {
                // a G-code file is placed as Start from says (Current position: from wherever the head is
                // when the file runs)
                let job = match self.job_placement(true) {
                    Ok(p) => self.machine_job_at(&p),
                    Err(_) => self.machine_job(),
                };
                let name = self.gcode_file_name(job.seconds);
                let ruida = self.settings.device.controller == gcode::Controller::Ruida;
                let k40 = self.settings.device.controller == gcode::Controller::K40Nano;
                // Ruida: the scrambled .rd job file, for a USB stick or RDWorks; K40: an .egv file
                let (label, exts, name, bytes): (&str, &[&str], String, Vec<u8>) = if k40 {
                    ("K40 job (EGV)", &["egv"], name.replace(".gcode", ".egv"), crate::k40::egv_file(&crate::k40::job_egv(&job, &self.settings.device)))
                } else if ruida {
                    ("Ruida job", &["rd"], name.replace(".gcode", ".rd"), crate::ruida::rd_file(&job, &self.settings.device))
                } else {
                    ("G-code", &["gcode", "gc", "nc"], name, job.gcode.clone().into_bytes())
                };
                match self.save_file(label, exts, &name, &bytes) {
                    Ok(_) => self.toast(format!("G-code saved — about {}", fmt_time(job.seconds)), now),
                    Err(Some(e)) => self.error(format!("Could not save G-code: {e}"), now),
                    Err(None) => {}
                }
                if self.job.as_ref().map(|j| j.skipped_outside).unwrap_or(0) == 0 && job.skipped_outside > 0 {
                    self.error(format!("{} shape(s) outside the bed were left out of the G-code.", job.skipped_outside), now);
                }
                self.job = Some(job);
            }
            Action::Exit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::Undo => {
                if let Some((d, s)) = self.undo.pop() {
                    self.redo.push((std::mem::replace(&mut self.doc, d), std::mem::replace(&mut self.sel, s)));
                    self.touch();
                }
            }
            Action::Redo => {
                if let Some((d, s)) = self.redo.pop() {
                    self.undo.push((std::mem::replace(&mut self.doc, d), std::mem::replace(&mut self.sel, s)));
                    self.touch();
                }
            }
            Action::Copy => {
                self.clipboard = sel.iter().map(|i| self.doc.shapes[*i].clone()).collect();
                // as SVG, so other programs can paste it too (BurnCraft knows its own and pastes the exact shapes)
                crate::platform::clipboard_set_text(&self.clipboard_svg());
                self.toast(format!("Copied {}", sel.len()), now);
            }
            Action::Cut => {
                self.clipboard = sel.iter().map(|i| self.doc.shapes[*i].clone()).collect();
                crate::platform::clipboard_set_text(&self.clipboard_svg());
                self.checkpoint();
                ops::delete(&mut self.doc, &sel);
                self.sel.clear();
            }
            // our own shapes were copied last (the system clipboard still holds our marker) → paste those
            Action::Paste if !crate::platform::clipboard_text().is_some_and(|t| crate::svg::is_ours(&t)) => {
                // something newer than our own shapes is on the clipboard: paste, in this order, files copied in
                // the file manager, SVG, a picture (our own Copy as Picture gives back the shapes), then plain
                // text as a Text shape
                let files = crate::platform::clipboard_files();
                let text = crate::platform::clipboard_text();
                let uris = text.as_deref().map(file_uris).unwrap_or_default();
                if !files.is_empty() || !uris.is_empty() {
                    self.import_paths(if files.is_empty() { uris } else { files }, now);
                } else if let Some(t) = text.as_deref().filter(|t| is_svg_text(t)) {
                    self.paste_svg(t, now);
                } else if let Some((w, h, raw)) = crate::platform::clipboard_image_raw() {
                    if self.clip_picture == Some(crate::raster::fingerprint(w, h, &raw)) && !self.clipboard.is_empty() {
                        self.paste_shapes(a);
                    } else if let Some(png) = crate::platform::png_bytes(w as u32, h as u32, raw) {
                        let at = self.canvas.pointer_world;
                        self.add_image_bytes(png, "pasted.png", at, now);
                    }
                } else if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
                    self.paste_text(&t);
                } else if !self.clipboard.is_empty() {
                    self.paste_shapes(a);
                } else {
                    self.toast("Nothing to paste: copy shapes, a picture, SVG or text in another app.", now);
                }
            }
            Action::CopyPicture => self.copy_picture(now),
            Action::Export => self.win_export = true,
            Action::Paste | Action::PasteInPlace | Action::Duplicate => self.paste_shapes(a),
            Action::Delete => {
                if self.tool == Tool::Nodes && !self.canvas.nodes.is_empty() {
                    self.delete_node();
                    return;
                }
                self.checkpoint();
                ops::delete(&mut self.doc, &sel);
                self.sel.clear();
                self.doc.sync_cuts();
            }
            Action::SelectAll => {
                self.sel = (0..self.doc.shapes.len()).collect();
            }
            Action::ToolSelect => {
                if self.tool == Tool::Select && !self.canvas.busy() {
                    self.sel.clear();
                }
                self.set_tool(Tool::Select);
                self.win_search = false;
            }
            Action::ToolPan => self.set_tool(Tool::Pan),
            Action::ToolLine => self.set_tool(Tool::Line),
            Action::ToolRect => self.set_tool(Tool::Rect),
            Action::ToolEllipse => self.set_tool(Tool::Ellipse),
            Action::ToolPolygon => self.set_tool(Tool::Polygon),
            Action::ToolText => self.set_tool(Tool::Text),
            Action::ToolNodes => self.set_tool(Tool::Nodes),
            Action::ToolMeasure => self.set_tool(Tool::Measure),
            Action::Group => {
                self.checkpoint();
                if let Some(g) = ops::group(&mut self.doc, &sel) {
                    self.sel = vec![g];
                }
            }
            Action::Ungroup => {
                self.checkpoint();
                self.sel = ops::ungroup(&mut self.doc, &sel);
            }
            Action::FlipH | Action::FlipV => {
                self.checkpoint();
                ops::flip(&mut self.doc, &sel, a == Action::FlipH);
            }
            Action::RotateCW | Action::RotateCCW => {
                self.checkpoint();
                ops::rotate_about(&mut self.doc, &sel, if a == Action::RotateCW { -90.0 } else { 90.0 }, self.rotate_about);
            }
            Action::ConvertToPath => {
                self.checkpoint();
                ops::convert_to_path(&mut self.doc, &sel);
            }
            Action::BreakApart => {
                self.checkpoint();
                self.sel = ops::break_apart(&mut self.doc, &sel);
            }
            Action::Offset => self.win_offset = true,
            Action::GridArray => {
                self.array_edit = None;
                self.win_array = true;
            }
            Action::EditArray => self.edit_array(),
            Action::Weld | Action::BoolUnion | Action::BoolSubtract | Action::BoolIntersect => {
                let op = match a {
                    Action::BoolSubtract => ops::BoolOp::Subtract,
                    Action::BoolIntersect => ops::BoolOp::Intersect,
                    _ => ops::BoolOp::Union,
                };
                let before = self.doc.clone();
                match ops::boolean(&mut self.doc, &sel, op) {
                    Ok(i) => {
                        self.undo.push((before, sel));
                        self.redo.clear();
                        self.touch();
                        self.sel = vec![i];
                    }
                    Err(e) => self.error(e, now),
                }
            }
            Action::AlignLeft | Action::AlignRight | Action::AlignTop | Action::AlignBottom | Action::AlignHCenter | Action::AlignVCenter => {
                self.checkpoint();
                let al = match a {
                    Action::AlignLeft => ops::Align::Left,
                    Action::AlignRight => ops::Align::Right,
                    Action::AlignTop => ops::Align::Top,
                    Action::AlignBottom => ops::Align::Bottom,
                    Action::AlignHCenter => ops::Align::HCenter,
                    _ => ops::Align::VCenter,
                };
                ops::align(&mut self.doc, &sel, al);
            }
            Action::DistributeH | Action::DistributeV => {
                self.checkpoint();
                ops::distribute(&mut self.doc, &sel, a == Action::DistributeH);
            }
            Action::PageCenter => {
                self.checkpoint();
                let c = p2(self.settings.device.width / 2.0, self.settings.device.height / 2.0);
                ops::move_to(&mut self.doc, &sel, c);
            }
            Action::PushFront => {
                self.checkpoint();
                self.sel = ops::push_front(&mut self.doc, &sel);
            }
            Action::PushBack => {
                self.checkpoint();
                self.sel = ops::push_back(&mut self.doc, &sel);
            }
            Action::ZoomIn => self.canvas.zoom_step = Some(1.25),
            Action::ZoomOut => self.canvas.zoom_step = Some(0.8),
            Action::ZoomPage => {
                let d = &self.settings.device;
                self.canvas.pending_fit = Some(BBox { min: p2(0.0, 0.0), max: p2(d.width, d.height) });
            }
            Action::ZoomSelection => {
                let b = ops::selection_bbox(&self.doc, &sel);
                if !b.is_empty() {
                    self.canvas.pending_fit = Some(b.expand(b.width().max(b.height()) * 0.1 + 2.0));
                }
            }
            Action::ToggleFilled => {
                self.settings.filled_view = !self.settings.filled_view;
                self.save_settings(now);
            }
            Action::ToggleGrid => {
                self.settings.show_grid = !self.settings.show_grid;
                self.save_settings(now);
            }
            Action::Preview => {
                self.preview_on = !self.preview_on;
                if self.preview_on {
                    self.job = Some(gcode::build_job(&self.doc, &self.settings.device, None));
                    self.preview_t = 1.0;
                    self.side_tab = SideTab::Laser;
                }
            }
            Action::TraceImage => {
                if let Some(i) = self.selected_bitmap() {
                    self.trace.target = Some(self.doc.shapes[i].id);
                    self.trace.preview_for = None;
                    self.trace.layer = self.current_layer_for_vectors();
                    self.win_trace = true;
                }
            }
            Action::TraceMcp => {
                if let Some(i) = self.selected_bitmap() {
                    self.trace.target = Some(self.doc.shapes[i].id);
                    self.trace.layer = self.current_layer_for_vectors();
                    self.win_mcp = true;
                }
            }
            Action::TextOnPath | Action::TextOnCircle => match self.text_on_path(a == Action::TextOnCircle) {
                Ok(()) => self.toast("The text follows the curve. Change the text, its shift, side and direction in Properties.", now),
                Err(e) => self.error(e, now),
            },
            Action::ImageMask | Action::ImageMaskOut => match self.mask_picture(a == Action::ImageMask) {
                Ok(n) => self.toast(format!("Masked: {n} pixel(s) of the picture won't burn now."), now),
                Err(e) => self.error(e, now),
            },
            Action::ImageInvert | Action::ImageReset => {
                if let Some(i) = self.selected_bitmap() {
                    self.checkpoint();
                    if let ShapeKind::Bitmap { adjust, .. } = &mut self.doc.shapes[i].kind {
                        if a == Action::ImageInvert {
                            adjust.invert = !adjust.invert;
                        } else {
                            *adjust = ImageAdjust::default();
                        }
                    }
                }
            }
            Action::LaserConnect => {
                self.side_tab = SideTab::Laser;
                self.show_right = true;
                if self.laser.connected {
                    self.laser.send(Cmd::Disconnect);
                } else {
                    let port = self.settings.device.port.clone();
                    if port.is_empty() {
                        self.ports = Laser::ports();
                        self.toast("Choose the laser's port in the Laser panel, then press Connect.", now);
                    } else {
                        self.laser.send(Cmd::Connect { port, baud: self.settings.device.baud });
                    }
                }
            }
            // (K40 and galvo lasers have their own Home and Frame further down)
            Action::LaserHome if !self.settings.device.controller.is_usb_laser() => {
                self.laser.send(Cmd::Send(vec!["$H".into()]));
                self.laser.homed = true;
            }
            Action::LaserFrame if !self.settings.device.controller.is_usb_laser() => {
                if !self.laser.connected {
                    self.toast("Connect to a laser first (Laser panel).", now);
                    return;
                }
                // frame exactly what Start would burn (shapes off the bed, hidden layers and construction lines
                // are left out; with a rotary, where the rotary will actually move; placed as Start from says)
                let place = match self.job_placement(false) {
                    Ok(p) => p,
                    Err(e) => {
                        self.error(e, now);
                        return;
                    }
                };
                let job = self.machine_job_at(&place);
                let Some(mut lines) = gcode::frame_gcode(&job, self.settings.device.rapid_speed) else {
                    self.job = Some(job);
                    self.error("Nothing to frame: no layer with Burn switched on has shapes inside the bed.", now);
                    return;
                };
                if self.rotary_on() && self.settings.device.rotary.axis == crate::rotary::RotaryAxis::A {
                    lines = lines.iter().map(|l| crate::rotary::y_to_a(l).trim_end().to_string()).collect();
                }
                self.warn_if_not_homed(now);
                self.send_placed(lines, &job, &place, now);
                self.job = Some(job);
            }
            #[cfg(not(usb_lasers))]
            Action::LaserStart | Action::LaserFrame | Action::LaserHome | Action::LaserStop if matches!(self.settings.device.controller, gcode::Controller::EzCad2 | gcode::Controller::K40Nano) => {
                let why = if cfg!(target_arch = "wasm32") { "in the browser" } else { "on this system yet" };
                self.error(format!("USB lasers (K40, galvo) aren't supported {why}. Save G-code still makes a job file for the K40 (.egv)."), now);
            }
            #[cfg(usb_lasers)]
            Action::LaserStart | Action::LaserFrame | Action::LaserStop if self.settings.device.controller == gcode::Controller::EzCad2 => {
                use crate::galvo;
                let busy = self.usb_job.as_ref().map(|r| r.result.lock().map(|g| g.is_none()).unwrap_or(false)).unwrap_or(false);
                if a == Action::LaserStop {
                    match &self.usb_job {
                        Some(r) if busy => r.stop.store(true, std::sync::atomic::Ordering::Relaxed),
                        _ => self.usb_job = Some(galvo::usb::stop()),
                    }
                    return;
                }
                if busy {
                    // a running red-pointer frame stops when the next action starts
                    if let Some(r) = &self.usb_job {
                        r.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    self.toast("Stopping what the galvo is doing first; press again in a moment.", now);
                    return;
                }
                let job = gcode::build_job(&self.doc, &self.settings.device, None);
                if job.moves.iter().all(|m| m.power <= 0.0) {
                    self.job = Some(job);
                    self.error("Nothing to mark: no layer with Burn switched on has shapes inside the lens field.", now);
                    return;
                }
                let s = self.settings.device.galvo.clone();
                if a == Action::LaserFrame {
                    let mut b = BBox::EMPTY;
                    for m in job.moves.iter().filter(|m| m.power > 0.0) {
                        b.add(m.from);
                        b.add(m.to);
                    }
                    // about 20 seconds of tracing, or until Stop
                    let perim = 2.0 * (b.width() + b.height());
                    let loops = ((20.0 * s.light_speed / perim.max(1.0)) as usize).clamp(3, 2000);
                    self.usb_job = Some(galvo::usb::run(galvo::frame_list(b, &s, loops), s, true, "Framing done".into()));
                    self.toast("Tracing the job outline with the red pointer (Stop ends it)", now);
                    self.job = Some(job);
                    return;
                }
                if let Some(e) = galvo::pulse_width_problem(&job, &self.settings.device.galvo) {
                    self.job = Some(job);
                    self.error(e, now);
                    return;
                }
                let (entries, outside) = galvo::job_list(&job, &self.doc, &self.settings.device);
                if outside > 0 || job.skipped_outside > 0 {
                    self.error(format!("{} piece(s) outside the lens field are left out.", outside + job.skipped_outside), now);
                }
                self.usb_job = Some(galvo::usb::run(entries, s, false, "Galvo job finished".into()));
                self.toast(format!("Marking — about {}", fmt_time(job.seconds)), now);
                self.job_started = Some(now);
                self.job_estimate = job.seconds;
                self.job_finished_after = None;
                self.job = Some(job);
                self.advance_variable_text(now);
            }
            #[cfg(usb_lasers)]
            Action::LaserStart | Action::LaserFrame | Action::LaserHome | Action::LaserStop if self.settings.device.controller == gcode::Controller::K40Nano => {
                use crate::k40;
                if a == Action::LaserStop {
                    match &self.usb_job {
                        Some(r) if r.result.lock().map(|g| g.is_none()).unwrap_or(false) => r.stop.store(true, std::sync::atomic::Ordering::Relaxed),
                        _ => self.usb_job = Some(k40::usb::command("I", "K40 stopped".into())),
                    }
                    return;
                }
                if self.usb_job.as_ref().map(|r| r.result.lock().map(|g| g.is_none()).unwrap_or(false)).unwrap_or(false) {
                    self.error("The K40 is still busy with the last job. Press Stop to cancel it.", now);
                    return;
                }
                if a == Action::LaserHome {
                    self.usb_job = Some(k40::usb::command("IPP", "K40 homed".into()));
                    return;
                }
                let job = self.machine_job();
                if job.moves.iter().all(|m| m.power <= 0.0) {
                    self.job = Some(job);
                    self.error("Nothing to burn: no layer with Burn switched on has shapes inside the bed.", now);
                    return;
                }
                if a == Action::LaserFrame {
                    let mut b = BBox::EMPTY;
                    for m in job.moves.iter().filter(|m| m.power > 0.0) {
                        b.add(m.from);
                        b.add(m.to);
                    }
                    let data = k40::frame_egv(b, &self.settings.device);
                    self.usb_job = Some(k40::usb::run(data, "Framing done".into(), true));
                    self.job = Some(job);
                    return;
                }
                if job.skipped_outside > 0 {
                    self.error(format!("{} shape(s) outside the bed are left out of this job.", job.skipped_outside), now);
                }
                let data = k40::job_egv(&job, &self.settings.device);
                self.usb_job = Some(k40::usb::run(data, "K40 job finished".into(), true));
                self.toast(format!("Sending the job to the K40 — about {}. Set the power with the knob on the laser.", fmt_time(job.seconds)), now);
                self.job_started = Some(now);
                self.job_estimate = job.seconds;
                self.job_finished_after = None;
                self.job = Some(job);
                self.advance_variable_text(now);
            }
            // galvo Home (switched off: a galvo has nothing to home)
            Action::LaserHome | Action::LaserFrame => {}
            Action::LaserStart if self.settings.device.controller == gcode::Controller::Ruida => {
                let job = self.machine_job();
                if self.settings.device.ip.trim().is_empty() {
                    self.job = Some(job);
                    self.preview_on = true;
                    self.error("Enter the Ruida controller's IP address in Settings → Laser, or use Save G-code to make an .rd file.", now);
                    return;
                }
                self.net_job = Some(crate::ruida::send(&job, &self.settings.device, &self.settings.device.ip));
                self.toast(format!("Sending the job to {} …", self.settings.device.ip), now);
                self.job = Some(job);
                self.advance_variable_text(now);
            }
            Action::LaserStart => {
                let place = if self.laser.connected { self.job_placement(false) } else { Ok(gcode::Placement::absolute(&self.settings.device)) };
                let place = match place {
                    Ok(p) => p,
                    Err(e) => {
                        self.error(e, now);
                        return;
                    }
                };
                let job = self.machine_job_at(&place);
                if self.rotary_on() {
                    let now2 = now;
                    for w in crate::rotary::warnings(&self.doc, &self.settings.device.rotary, self.settings.units) {
                        self.error(w, now2);
                    }
                }
                if !self.laser.connected {
                    self.job = Some(job);
                    self.preview_on = true;
                    self.side_tab = SideTab::Laser;
                    self.toast("No laser connected — showing the preview instead. Connect in the Laser panel, or Save G-code.", now);
                    return;
                }
                if job.skipped_outside > 0 {
                    self.error(format!("{} shape(s) outside the bed are left out of this job.", job.skipped_outside), now);
                }
                self.warn_if_not_homed(now);
                if !self.send_placed(job.gcode.lines().map(|l| l.to_string()).collect(), &job, &place, now) {
                    self.job = Some(job);
                    return;
                }
                self.toast(format!("Job started — about {}", fmt_time(job.seconds)), now);
                self.job_started = Some(now);
                self.job_estimate = job.seconds;
                self.job_finished_after = None;
                self.laser.progress = (0, job.gcode.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with(';')).count());
                self.job = Some(job);
                self.advance_variable_text(now);
            }
            Action::LaserPause => {
                let paused = self.laser.state.starts_with("Hold");
                self.laser.send(Cmd::Realtime(if paused { b'~' } else { b'!' }));
            }
            Action::LaserStop => {
                self.job_started = None;
                self.laser.progress = (0, 0);
                self.laser.send(Cmd::Realtime(b'!'));
                self.laser.send(Cmd::Realtime(0x18));
                self.laser.send(Cmd::ClearQueue);
            }
            Action::ToolPen => self.set_tool(Tool::Pen),
            Action::ToolArc => self.set_tool(Tool::Arc),
            Action::ToolStar => self.set_tool(Tool::Star),
            Action::ToolTrim => self.set_tool(Tool::Trim),
            Action::ToolTabs => self.set_tool(Tool::Tabs),
            Action::ToolWarp => {
                if self.sel.is_empty() {
                    self.error("Select the shapes to warp first.", now);
                } else {
                    self.warp = crate::warp::Warp { kind: self.warp.kind, ..Default::default() };
                    self.set_tool(Tool::Warp);
                }
            }
            Action::ToolFillet => self.set_tool(Tool::Fillet),
            Action::ToolWand => self.set_tool(Tool::Wand),
            Action::ToolLasso => self.set_tool(Tool::Lasso),
            Action::ToolDimension => self.set_tool(Tool::Dimension),
            Action::ToolRelate => self.set_tool(Tool::Relate),
            Action::JoinPaths => {
                self.checkpoint();
                let joined = crate::join::join(&mut self.doc, &sel, crate::join::AUTO_JOIN_TOL);
                if joined.is_empty() {
                    self.toast("Nothing to join: no open ends within 0.05 mm of each other.", now);
                } else {
                    self.sel = joined;
                    self.touch();
                    self.toast("Joined into continuous paths.", now);
                }
            }
            Action::ToolMirrorLine => {
                // LightBurn's way: the last selected shape is a single straight segment → mirror the rest about it
                let line = (sel.len() >= 2).then(|| self.doc.shapes.get(*sel.last().unwrap()).and_then(crate::sketch::mirror_line_of)).flatten();
                match line {
                    Some((p, q)) => {
                        self.checkpoint();
                        let rest: Vec<usize> = sel[..sel.len() - 1].to_vec();
                        let made = crate::sketch::mirror_copy(&mut self.doc, &rest, p, q);
                        if !made.is_empty() {
                            self.sel = made;
                        }
                        self.touch();
                    }
                    None => self.set_tool(Tool::MirrorLine),
                }
            }
            Action::ToggleConstruction => self.toggle_construction(),
            Action::PathPattern | Action::AlignToPath => {
                self.array_edit = None;
                self.curve_dlg.0 = a == Action::AlignToPath;
                self.win_curve = true;
            }
            Action::Barcode => self.win_barcode = true,
            Action::NodeSmooth | Action::NodeCorner | Action::NodeLine | Action::NodeCurve | Action::NodeInsert | Action::NodeBreak => self.node_op(a),
            Action::CircularArray => {
                self.array_edit = None;
                let b = ops::selection_bbox(&self.doc, &sel);
                self.circular.3 = p2(b.center().x, b.min.y - b.height().max(10.0));
                self.win_circular = true;
            }
            Action::SelectionToPath => {
                if let Some(s) = self.image_sel.clone() {
                    if let Some(i) = self.index_of_id(s.shape_id) {
                        let shape = self.doc.shapes[i].clone();
                        if let ShapeKind::Bitmap { w, h, data, .. } = &shape.kind {
                            // exactly the selection you see: the marching ants use the same refined outline
                            let px = crate::wand::outlines(&s.mask, s.w, s.h, &self.opts.wand_refine);
                            let lines = crate::imaging::pixels_to_shape_space(px, *w, *h, data.width, data.height, &shape.xform);
                            self.trace.layer = self.current_layer_for_vectors();
                            self.add_outlines(lines, None);
                            self.image_sel = None;
                            self.set_tool(Tool::Select);
                        }
                    }
                }
            }
            Action::ClearSelection => self.image_sel = None,
            Action::PreviewPlay => {
                if !self.preview_on || self.job.is_none() {
                    self.job = Some(gcode::build_job(&self.doc, &self.settings.device, None));
                    self.preview_on = true;
                    self.side_tab = SideTab::Laser;
                }
                if self.preview_t >= 0.999 {
                    self.preview_t = 0.0;
                }
                self.preview_playing = !self.preview_playing;
            }
            Action::InvertBed => self.invert_bed = !self.invert_bed,
            Action::SaveLayerDefaults => {
                self.doc.sync_cuts();
                self.settings.layer_defaults = self.doc.cuts.clone();
                set_layer_defaults(&self.settings.layer_defaults);
                self.save_settings(now);
                self.toast(format!("Saved {} layer(s) as defaults for new projects", self.doc.cuts.len()), now);
            }
            Action::NewTab => self.new_tab(None),
            Action::DuplicateTab => {
                let d = self.doc.clone();
                self.new_tab(Some(d));
            }
            Action::CloseTab => {
                if self.dirty && !confirm_discard() {
                    return;
                }
                self.close_tab(self.active_tab);
            }
            Action::CommandSearch => {
                self.win_search = true;
                self.search_text.clear();
                self.search_index = 0;
            }
            Action::Shortcuts => self.win_shortcuts = true,
            Action::Settings => self.win_settings = true,
            Action::Notes => self.win_notes = !self.win_notes,
            Action::Help => self.win_help = !self.win_help,
            Action::VariableText => self.win_vartext = !self.win_vartext,
            Action::Camera => self.win_camera = !self.win_camera,
            Action::Rotary => self.win_rotary = !self.win_rotary,
            Action::TestCalibrate => self.win_test = !self.win_test,
            Action::Bolden | Action::StencilBridges => self.win_shape_tools = true,
            Action::Materials => self.win_materials = !self.win_materials,
            Action::Nest => self.win_nest = true,
            Action::WelcomeTour => {
                self.win_settings = false;
                self.win_help = false;
                self.tour = Some(0);
            }
        }
        self.sel_valid();
    }

    pub fn node_op(&mut self, a: Action) {
        let Some(&si) = self.sel.first() else { return };
        let Some(ShapeKind::Path(mut pd)) = self.doc.shapes.get(si).map(|s| s.kind.clone()) else { return };
        let nodes = self.canvas.nodes.clone();
        self.checkpoint();
        match a {
            Action::NodeSmooth => crate::nodes::smooth(&mut pd, &nodes),
            Action::NodeCorner => crate::nodes::corner(&mut pd, &nodes),
            Action::NodeLine => crate::nodes::to_lines(&mut pd, &nodes),
            Action::NodeCurve => crate::nodes::to_curves(&mut pd, &nodes),
            Action::NodeInsert => {
                let added = crate::nodes::insert_midpoints(&mut pd, &nodes);
                self.canvas.nodes.extend(added);
            }
            Action::NodeBreak => crate::nodes::break_at(&mut pd, &nodes),
            _ => {}
        }
        self.doc.shapes[si].kind = ShapeKind::Path(pd);
    }

    /// Construction lines: move the selection onto (or back off) the construction layer; with nothing
    /// selected, switch new drawing to (or from) the construction layer.
    pub fn toggle_construction(&mut self) {
        let c = crate::model::CONSTRUCTION_LAYER;
        if self.sel.is_empty() {
            match self.construction_prev.take() {
                Some(prev) => self.current_layer = prev,
                None => {
                    self.construction_prev = Some(if self.current_layer == c { 0 } else { self.current_layer });
                    self.current_layer = c;
                }
            }
            return;
        }
        self.checkpoint();
        let all_construction = self.sel.iter().all(|i| self.doc.shapes.get(*i).map(|s| s.layer == c).unwrap_or(false));
        let back = self.construction_prev.filter(|l| *l != c).unwrap_or(if self.current_layer == c { 0 } else { self.current_layer });
        for &i in &self.sel.clone() {
            if let Some(s) = self.doc.shapes.get_mut(i) {
                if all_construction {
                    let prev = s.extra_attrs.iter().find(|(k, _)| k == "BcPrevLayer").and_then(|(_, v)| v.parse().ok()).unwrap_or(back);
                    s.extra_attrs.retain(|(k, _)| k != "BcPrevLayer");
                    s.set_layer(prev);
                } else if s.layer != c {
                    s.extra_attrs.retain(|(k, _)| k != "BcPrevLayer");
                    s.extra_attrs.push(("BcPrevLayer".into(), s.layer.to_string()));
                    s.set_layer(c);
                }
            }
        }
        self.doc.sync_cuts();
        self.touch();
    }

    pub fn current_layer_for_vectors(&self) -> usize {
        if self.doc.cut(self.current_layer).map(|c| c.mode == CutMode::Image).unwrap_or(false) {
            0
        } else {
            self.current_layer
        }
    }

    /// Assign the selection to a layer (palette click), like LightBurn.
    pub fn assign_layer(&mut self, layer: usize) {
        self.current_layer = layer;
        if !self.sel.is_empty() {
            self.checkpoint();
            for &i in &self.sel.clone() {
                self.doc.shapes[i].set_layer(layer);
            }
            self.doc.sync_cuts();
        }
    }

    /// Open the dialog of the array a selected shape belongs to, filled in with how it was made.
    pub fn edit_array(&mut self) {
        let Some(ai) = self.sel.iter().find_map(|i| self.doc.shapes.get(*i).and_then(|s| crate::relations::array_of(&self.doc, s.id))) else { return };
        self.array_edit = Some(ai);
        match self.doc.arrays[ai].kind.clone() {
            crate::relations::ArrayKind::Grid { cols, rows, gap_x, gap_y } => {
                self.array = (cols, rows, gap_x, gap_y);
                self.win_array = true;
            }
            crate::relations::ArrayKind::Circular { count, span, rotate, center } => {
                self.circular = (count, span, rotate, center);
                self.win_circular = true;
            }
            crate::relations::ArrayKind::Path { opts, .. } => {
                self.path_copy = opts;
                self.curve_dlg.0 = false;
                self.win_curve = true;
            }
        }
    }

    /// Make (or remake, when editing) an array from `sources`; remembers it so it can be edited later.
    pub fn make_array(&mut self, kind: crate::relations::ArrayKind, sources: &[usize]) {
        use crate::relations::{ArrayKind, ArrayLink};
        self.checkpoint();
        let edit = self.array_edit.filter(|ai| *ai < self.doc.arrays.len());
        let (src_ids, mut idx): (Vec<u64>, Vec<usize>) = match edit {
            Some(ai) => {
                // remove the old copies, then start again from the same originals
                let old = self.doc.arrays[ai].clone();
                let mut gone: Vec<usize> = old.copies.iter().filter_map(|id| self.doc.shapes.iter().position(|s| s.id == *id)).collect();
                gone.sort_unstable_by(|a, b| b.cmp(a));
                for g in gone {
                    self.doc.shapes.remove(g);
                }
                let idx = old.sources.iter().filter_map(|id| self.doc.shapes.iter().position(|s| s.id == *id)).collect();
                (old.sources, idx)
            }
            None => (sources.iter().filter_map(|i| self.doc.shapes.get(*i).map(|s| s.id)).collect(), sources.to_vec()),
        };
        idx.retain(|i| *i < self.doc.shapes.len());
        let made: Vec<usize> = match &kind {
            ArrayKind::Grid { cols, rows, gap_x, gap_y } => ops::grid_array(&mut self.doc, &idx, *cols, *rows, *gap_x, *gap_y),
            ArrayKind::Circular { count, span, rotate, center } => crate::sketch::circular_array(&mut self.doc, &idx, *center, *count, *span, *rotate),
            ArrayKind::Path { curve, opts } => match self.doc.shapes.iter().position(|s| s.id == *curve) {
                Some(ci) => {
                    let mut with_curve = idx.clone();
                    with_curve.push(ci);
                    crate::sketch::path_pattern(&mut self.doc, &with_curve, ci, opts)
                }
                None => Vec::new(),
            },
        };
        let copies: Vec<u64> = made.iter().filter(|i| !idx.contains(i)).filter_map(|i| self.doc.shapes.get(*i).map(|s| s.id)).collect();
        let link = ArrayLink { kind, sources: src_ids, copies };
        match edit {
            Some(ai) => self.doc.arrays[ai] = link,
            None => self.doc.arrays.push(link),
        }
        let mut sel = idx;
        sel.extend(made.into_iter().filter(|i| !sel.contains(i)).collect::<Vec<_>>());
        self.sel = sel;
        self.array_edit = None;
        self.touch();
    }

    /// Pasted SVG (from a drawing program, a browser, or BurnCraft itself), centred on the mouse.
    pub fn paste_svg(&mut self, text: &str, now: f64) {
        let at = self.canvas.pointer_world.unwrap_or(p2(self.settings.device.width / 2.0, self.settings.device.height / 2.0));
        self.add_svg(text.as_bytes(), Some(at), "Pasted", now);
    }

    /// Add an SVG's shapes: outlines on the layer nearest their colour (curves kept), embedded pictures as
    /// images. Centred on `at`, or else where they sit on the SVG's page (the page's top-left at the bed's
    /// top-left), moved to the middle of the bed if that would leave them off it.
    pub fn add_svg(&mut self, data: &[u8], at: Option<P2>, what: &str, now: f64) {
        match crate::svg::import(data, self.settings.device.height) {
            Ok(imp) => self.add_imported(imp, at, &format!("{what} SVG"), now),
            Err(e) => self.error(e, now),
        }
    }

    /// Import page `page` of a PDF.
    pub fn import_pdf_page(&mut self, data: &[u8], page: usize, now: f64) {
        match crate::pdf::import(data, page, self.settings.device.height) {
            Ok(imp) => self.add_imported(imp, None, "Imported PDF", now),
            Err(e) => self.error(e, now),
        }
    }

    /// Add what an importer read (SVG, DXF, PDF): outlines on the layer nearest their colour (or the layer
    /// they name), pictures as images. Centred on `at`, or else where the drawing put them, moved to the
    /// middle of the bed if that would leave them off it.
    pub fn add_imported(&mut self, imp: crate::svg::Imported, at: Option<P2>, what: &str, now: f64) {
        let bed_h = self.settings.device.height;
        let mut shapes: Vec<Shape> = Vec::new();
        for (pd, rgb) in imp.paths {
            shapes.push(Shape::new(ShapeKind::Path(pd), Xform::IDENTITY, crate::svg::nearest_layer(rgb)));
        }
        for (pd, layer) in imp.on_layers {
            shapes.push(Shape::new(ShapeKind::Path(pd), Xform::IDENTITY, layer));
        }
        let image_layer = self.image_layer();
        let n_images = imp.images.len();
        for (bd, w, h, x) in imp.images {
            shapes.push(Shape::new(ShapeKind::Bitmap { w, h, data: bd, adjust: ImageAdjust::default() }, x, image_layer));
        }
        let b = shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        let bed = BBox { min: p2(0.0, 0.0), max: p2(self.settings.device.width, bed_h) };
        let target = at.or_else(|| (!bed.contains_box(&b)).then(|| bed.center()));
        if let Some(t) = target {
            let d = t - b.center();
            for s in &mut shapes {
                s.xform = Xform::translate(d.x, d.y).then_after(s.xform);
            }
        }
        self.checkpoint();
        let start = self.doc.shapes.len();
        let n = shapes.len();
        self.doc.shapes.extend(shapes);
        if n_images > 0 {
            self.doc.cut_mut(image_layer).mode = CutMode::Image;
        }
        self.doc.sync_cuts();
        self.sel = (start..self.doc.shapes.len()).collect();
        self.touch();
        let mut msg = format!("{what}: {n} shape(s), on the layers nearest their colours");
        if imp.skipped_text > 0 {
            msg.push_str(&format!(". {} text item(s) left out: convert text to outlines in the program that made it first", imp.skipped_text));
        }
        for note in &imp.notes {
            let mut c = note.chars();
            let first: String = c.next().map(|f| f.to_uppercase().collect()).unwrap_or_default();
            msg.push_str(&format!(". {first}{}", c.as_str()));
        }
        self.toast(msg, now);
    }

    /// Files copied in the file manager and pasted: each is imported as if dropped on the window.
    pub fn import_paths(&mut self, paths: Vec<PathBuf>, now: f64) {
        for p in paths.into_iter().take(20) {
            match std::fs::read(&p) {
                Ok(bytes) => self.import_bytes(&p.to_string_lossy(), bytes, now),
                Err(e) => self.error(format!("Couldn't read {}: {e}", p.display()), now),
            }
        }
    }

    /// Copy as Picture: the selection as a picture on the clipboard (for documents and chats). BurnCraft
    /// remembers it, so pasting it back here gives the shapes, not a picture.
    pub fn copy_picture(&mut self, now: f64) {
        let shapes: Vec<Shape> = self.sel.iter().filter_map(|i| self.doc.shapes.get(*i)).cloned().collect();
        let b = shapes.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        if b.is_empty() {
            return;
        }
        let frame = BBox { min: b.min - p2(1.0, 1.0), max: b.max + p2(1.0, 1.0) };
        // up to 300 DPI, at most 2000 pixels across
        let dpi = (2000.0 * 25.4 / frame.width().max(frame.height())).min(300.0);
        let fill = |l: usize| self.doc.cut(l).is_some_and(|c| matches!(c.mode, CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill));
        match crate::raster::render(&shapes, frame, dpi, true, &fill) {
            Ok((w, h, rgba)) => {
                let fp = crate::raster::fingerprint(w as usize, h as usize, &rgba);
                if crate::platform::clipboard_set_image(w as usize, h as usize, rgba) {
                    self.clipboard = shapes;
                    self.clip_picture = Some(fp);
                    self.toast(format!("Copied as a {w} × {h} picture"), now);
                } else {
                    self.error("The picture couldn't be put on the clipboard.", now);
                }
            }
            Err(e) => self.error(e, now),
        }
    }

    /// Save the export chosen in the Export window.
    pub fn export_now(&mut self, ctx: &egui::Context, now: f64) {
        let o = self.export_opts;
        if o.format == crate::export::Format::Gcode {
            self.run(Action::ExportGcode, ctx);
            return;
        }
        self.doc.sync_cuts();
        let shapes = crate::export::gather(&self.doc, &self.sel, &o);
        if shapes.is_empty() {
            self.error(if o.selection { "Nothing selected to export." } else { "Nothing to export (only layers that don't burn? Tick \u{201c}Include layers that don't burn\u{201d})." }, now);
            return;
        }
        let bed = BBox { min: p2(0.0, 0.0), max: p2(self.settings.device.width, self.settings.device.height) };
        let (bytes, note) = match crate::export::bytes(&self.doc, &shapes, bed, &o) {
            Ok(r) => r,
            Err(e) => {
                self.error(e, now);
                return;
            }
        };
        let stem = self.doc.title.clone().or_else(|| self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "design".into());
        let name = format!("{stem}.{}", o.format.ext());
        match self.save_file(&format!("{} file", o.format.label()), &[o.format.ext()], &name, &bytes) {
            Ok(_) => {
                self.win_export = false;
                let mut msg = format!("Exported {} shape(s) as {}", shapes.len(), o.format.label());
                if let Some(n) = note {
                    msg.push_str(&format!(" ({n})"));
                }
                self.toast(msg, now);
            }
            Err(Some(e)) => self.error(format!("Could not export: {e}"), now),
            Err(None) => {}
        }
    }

    /// BurnCraft's clipboard as an SVG document (framed tightly around the shapes).
    fn clipboard_svg(&self) -> String {
        let frame = self.clipboard.iter().fold(BBox::EMPTY, |b, s| b.union(&s.bbox()));
        crate::svg::export(&self.clipboard, frame)
    }

    /// Pasted plain text becomes a Text shape at the mouse (LightBurn's "Paste Text").
    pub fn paste_text(&mut self, text: &str) {
        let text: String = text.trim().chars().take(2000).collect();
        let at = self.canvas.pointer_world.unwrap_or(p2(self.settings.device.width / 2.0, self.settings.device.height / 2.0));
        let layer = self.current_layer_for_vectors();
        self.add_shape(Shape::new(ShapeKind::Text { text, font: crate::fonts::DEFAULT_FONT.into(), height: 10.0, style: Default::default(), backup: None }, Xform::translate(at.x, at.y), layer));
        self.side_tab = SideTab::Properties;
    }

    /// Paste (or duplicate) shapes from BurnCraft's own clipboard.
    pub fn paste_shapes(&mut self, a: Action) {
        let sel = self.sel.clone();
        let src: Vec<Shape> = if a == Action::Duplicate { sel.iter().map(|i| self.doc.shapes[*i].clone()).collect() } else { self.clipboard.clone() };
        self.checkpoint();
        let start = self.doc.shapes.len();
        // Like LightBurn, Paste centres the copy on the mouse when it is over
        // the canvas, otherwise offsets it by 5 mm; Paste in Place keeps the position
        let shift = if a == Action::Paste {
            let b = src.iter().fold(crate::geom::BBox::EMPTY, |b, s| b.union(&s.bbox()));
            match self.canvas.pointer_world {
                Some(m) if !b.is_empty() => Some(m - b.center()),
                _ => Some(p2(PASTE_OFFSET, -PASTE_OFFSET)),
            }
        } else {
            None
        };
        for mut s in src {
            s.renew_ids();
            if let Some(d) = shift {
                s.xform = Xform::translate(d.x, d.y).then_after(s.xform);
            }
            self.doc.shapes.push(s);
        }
        self.doc.sync_cuts();
        self.sel = (start..self.doc.shapes.len()).collect();
        if self.canvas.pointer_world.is_none() && a == Action::Paste {
            self.clipboard.iter_mut().for_each(|s| s.xform = Xform::translate(PASTE_OFFSET, -PASTE_OFFSET).then_after(s.xform));
        }
    }

    pub fn add_shape(&mut self, s: Shape) {
        self.checkpoint();
        self.doc.shapes.push(s);
        self.doc.sync_cuts();
        self.sel = vec![self.doc.shapes.len() - 1];
        // drawn onto the end of another open line or curve: they become one path (closed if it closes)
        let last = self.doc.shapes.len() - 1;
        let joined = crate::join::join_new(&mut self.doc, last);
        if !joined.is_empty() {
            self.sel = joined;
        }
    }

    /// Add traced/derived outlines as one path on the trace layer.
    pub fn add_outlines(&mut self, lines: Vec<Vec<P2>>, delete_image_id: Option<u64>) {
        if lines.is_empty() {
            return;
        }
        self.checkpoint();
        let layer = self.trace.layer;
        self.doc.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&lines)), Xform::IDENTITY, layer));
        let new_idx = self.doc.shapes.len() - 1;
        let new_id = self.doc.shapes[new_idx].id;
        if let Some(id) = delete_image_id {
            if let Some(i) = self.index_of_id(id) {
                self.doc.shapes.remove(i);
            }
        }
        self.doc.sync_cuts();
        self.sel = self.index_of_id(new_id).into_iter().collect();
    }
}

/// Copied files as some file managers put them on the clipboard as text: every line a `file://` address.
/// Anything else (ordinary text that mentions a file) gives nothing.
pub fn file_uris(text: &str) -> Vec<PathBuf> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    if lines.is_empty() || !lines.iter().all(|l| l.starts_with("file://")) {
        return Vec::new();
    }
    lines
        .iter()
        .filter_map(|l| {
            // file:///home/me/a%20b.svg → /home/me/a b.svg (and file://host/… is skipped)
            let rest = l.strip_prefix("file://")?;
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            if !path.starts_with('/') {
                return None;
            }
            let bytes = path.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' && i + 2 < bytes.len() {
                    // (read the two digits as bytes: slicing the text could land inside a character and crash)
                    if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            let s = String::from_utf8(out).ok()?;
            // Windows: file:///C:/dir/file
            let s = if s.len() > 3 && s.as_bytes()[2] == b':' { s[1..].to_string() } else { s };
            Some(PathBuf::from(s))
        })
        .collect()
}

/// Text that is an SVG document: longer than 10 characters, starts with '<', contains "<svg " and the SVG namespace.
pub fn is_svg_text(t: &str) -> bool {
    let t = t.trim();
    t.chars().count() > 10 && t.starts_with('<') && t.to_ascii_lowercase().contains("<svg ") && t.to_ascii_lowercase().contains("xmlns=\"http://www.w3.org/2000/svg\"")
}

/// Time format m:ss / h:mm:ss (hours only when there are any).
pub fn fmt_time(secs: f64) -> String {
    let total = secs.max(0.0) as i64;
    let mins = total / 60;
    let s = total - mins * 60;
    let h = mins / 60;
    let m = mins % 60;
    if h <= 0 {
        format!("{}:{:02}", m, s)
    } else {
        format!("{}:{:02}:{:02}", h, m, s)
    }
}

/// Make `defaults` the starting settings for layers in new projects.
pub fn set_layer_defaults(defaults: &[CutSetting]) {
    *crate::model::LAYER_DEFAULTS.write().unwrap() = defaults.to_vec();
}

pub fn confirm_discard() -> bool {
    matches!(
        crate::platform::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("This project has changes that aren't saved. Discard them?")
            .set_buttons(crate::platform::MessageButtons::YesNo)
            .show(),
        crate::platform::MessageDialogResult::Yes
    )
}

/// A small example project, used for the first look and for screenshots.
pub fn demo_document(dev: &gcode::Device) -> Document {
    let mut d = Document::default();
    let cx = dev.width / 2.0;
    let cy = dev.height / 2.0;
    // a coaster: rounded square cut line, engraved ring and text
    d.shapes.push(Shape::new(ShapeKind::Rect { w: 120.0, h: 120.0, cr: 14.0 }, Xform::translate(cx - 70.0, cy + 10.0), 2));
    d.shapes.push(Shape::new(ShapeKind::Ellipse { rx: 46.0, ry: 46.0 }, Xform::translate(cx - 70.0, cy + 10.0), 1));
    d.shapes.push(Shape::new(ShapeKind::Text { text: "BurnCraft".into(), font: String::new(), height: 12.0, style: Default::default(), backup: None }, Xform::translate(cx - 70.0, cy + 4.0), 0));
    let star: Vec<P2> = (0..=10)
        .map(|i| {
            let a = std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.0;
            let r = if i % 2 == 0 { 16.0 } else { 7.0 };
            p2(cx - 70.0 + r * a.cos(), cy + 40.0 + r * a.sin())
        })
        .collect();
    d.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&[star])), Xform::IDENTITY, 0));
    d.shapes.push(Shape::new(ShapeKind::Polygon { n: 6, rx: 30.0, ry: 30.0 }, Xform::translate(cx + 80.0, cy + 50.0), 3));
    // a generated gradient "photo" for the Image layer
    let (w, h) = (160u32, 110u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 / w as f64;
            let fy = y as f64 / h as f64;
            let d0 = ((fx - 0.35).powi(2) + (fy - 0.45).powi(2)).sqrt();
            let hill = if fy > 0.55 + 0.15 * (fx * 9.0).sin() * 0.5 { 60.0 } else { 0.0 };
            let v = (255.0 * (0.25 + d0 * 1.1)).min(250.0) - hill;
            let v = v.clamp(0.0, 255.0) as u8;
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut png = Vec::new();
    let img = image::RgbaImage::from_raw(w, h, rgba.clone()).unwrap();
    let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let bd = BitmapData { encoded: Arc::new(png), width: w, height: h, rgba: Arc::new(rgba), source_file: String::new() };
    d.shapes.push(Shape::new(ShapeKind::Bitmap { w: 96.0, h: 66.0, data: bd, adjust: ImageAdjust { dither: Dither::Jarvis, ..Default::default() } }, Xform::translate(cx + 80.0, cy - 60.0), 4));
    d.sync_cuts();
    d.cut_mut(1).mode = CutMode::Fill;
    d.cut_mut(1).max_power = 35.0;
    d.cut_mut(1).speed = 250.0;
    d.cut_mut(2).max_power = 70.0;
    d.cut_mut(2).speed = 12.0;
    d.cut_mut(4).mode = CutMode::Image;
    d.cut_mut(4).speed = 200.0;
    d
}

/// Timing of the job that is burning: (estimated total, elapsed, remaining, fraction done).
pub fn job_timing(app: &App, now: f64) -> Option<(f64, f64, f64, f64)> {
    let start = app.job_started?;
    let (sent, total) = app.laser.progress;
    let elapsed = (now - start).max(0.0);
    let frac = if total > 0 { (sent as f64 / total as f64).clamp(0.0, 1.0) } else { 0.0 };
    // early on trust the estimate; once the job is under way, trust its real pace
    let by_pace = if frac > 0.0 { elapsed / frac - elapsed } else { f64::INFINITY };
    let by_estimate = (app.job_estimate - elapsed).max(0.0);
    let w = ((frac - 0.02) / 0.2).clamp(0.0, 1.0);
    let remaining = if by_pace.is_finite() { by_estimate * (1.0 - w) + by_pace * w } else { by_estimate };
    Some((app.job_estimate, elapsed, remaining.max(0.0), frac))
}

#[cfg(test)]
mod paste_tests {
    #[test]
    fn svg_text_is_recognised_like_lightburn() {
        assert!(super::is_svg_text(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M0 0 L10 0"/></svg>"#));
        assert!(!super::is_svg_text("<svg>"));
        assert!(!super::is_svg_text("just some words to paste"));
    }

    #[test]
    fn copied_files_as_text_are_recognised() {
        use std::path::PathBuf;
        assert_eq!(super::file_uris("file:///home/me/My%20Sign.svg\r\nfile:///home/me/b.dxf\n"), vec![PathBuf::from("/home/me/My Sign.svg"), PathBuf::from("/home/me/b.dxf")]);
        assert_eq!(super::file_uris("file:///C:/Users/me/a.pdf"), vec![PathBuf::from("C:/Users/me/a.pdf")]);
        // ordinary text that mentions a file isn't a file list
        assert!(super::file_uris("see file:///home/me/a.svg").is_empty());
        assert!(super::file_uris("file:///home/me/a.svg\nand some words").is_empty());
        assert!(super::file_uris("").is_empty());
        // another computer's files aren't fetched
        assert!(super::file_uris("file://server/share/a.svg").is_empty());
        // a % followed by a letter that takes several bytes doesn't crash
        assert_eq!(super::file_uris("file:///home/me/%aé.svg"), vec![PathBuf::from("/home/me/%aé.svg")]);
    }
}
