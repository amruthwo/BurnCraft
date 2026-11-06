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
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click to select · drag left-to-right: shapes fully inside · right-to-left: anything touched · Shift/Ctrl adds",
            Tool::Pan => "Drag to move the view · scroll to zoom",
            Tool::Line => "Click to add points · rest on the last point to switch the next segment to an arc · Shift locks horizontal / 45° / vertical · double-click or Enter finishes · click the first point to close",
            Tool::Relate => "Click nodes, lines or circles (one or two), then pick a relation above · click a relation tag to remove it",
            Tool::MirrorLine => "Click a straight edge or construction line to mirror the selection about it",
            Tool::Rect => "Drag to draw · Shift = square · Ctrl = from centre",
            Tool::Ellipse => "Drag to draw · Shift = circle · Ctrl = from centre",
            Tool::Polygon => "Drag to draw · Ctrl = from centre · set sides in Properties",
            Tool::Text => "Click where the text should go, then type in Properties",
            Tool::Nodes => "Drag points to reshape · double-click a point to remove it",
            Tool::Measure => "Drag to measure distance and angle",
            Tool::Pen => "Click for a sharp point · click-and-drag for a smooth curve · Shift snaps angles · Enter or double-click finishes · click the first point to close",
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
    Running { target: u64, rx: Receiver<Result<Vec<Vec<P2>>, String>>, started: f64 },
    Done { target: u64, lines: Vec<Vec<P2>> },
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
    pub win_about: bool,
    pub search_text: String,
    pub search_index: usize,
    pub array: (usize, usize, f64, f64),
    pub offset_dist: f64,
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
    /// The welcome card was dismissed with "Start drawing" (until the next new project).
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
            Action::Copy | Action::Cut | Action::Duplicate | Action::Delete | Action::FlipH | Action::FlipV | Action::RotateCW | Action::RotateCCW | Action::PageCenter | Action::PushFront | Action::PushBack | Action::ZoomSelection | Action::GridArray | Action::ConvertToPath | Action::Offset | Action::BreakApart => has_sel,
            Action::Group | Action::Weld | Action::BoolUnion | Action::BoolSubtract | Action::BoolIntersect | Action::AlignLeft | Action::AlignRight | Action::AlignTop | Action::AlignBottom | Action::AlignHCenter | Action::AlignVCenter => self.sel.len() >= 2,
            Action::DistributeH | Action::DistributeV => self.sel.len() >= 3,
            Action::Ungroup => self.sel.iter().any(|i| matches!(self.doc.shapes.get(*i).map(|s| &s.kind), Some(ShapeKind::Group(_)))),
            // a picture copied in another app can always be pasted
            Action::Paste => true,
            Action::PasteInPlace => !self.clipboard.is_empty(),
            Action::Undo => !self.undo.is_empty(),
            Action::Redo => !self.redo.is_empty(),
            Action::TraceImage | Action::TraceMcp | Action::ImageInvert | Action::ImageReset => self.selected_bitmap().is_some(),
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
        if dirty && self.screenshot.is_none() {
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
    pub fn import_lbdev(&mut self, p: &std::path::Path, now: f64) {
        match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| crate::gcode::devices_from_lbdev(&t)) {
            Ok((devs, notes)) => {
                let d = devs[0].clone();
                let keep_port = std::mem::take(&mut self.settings.device.port);
                self.settings.device = crate::gcode::Device { port: keep_port, ..d };
                self.save_settings(now);
                self.touch();
                let more = if devs.len() > 1 { format!(" ({} more in the file)", devs.len() - 1) } else { String::new() };
                self.toast(format!("Laser set up: {} · {} × {} mm{more}", self.settings.device.name, fmt_num(self.settings.device.width), fmt_num(self.settings.device.height)), now);
                for n in notes {
                    self.warnings.push(n);
                }
            }
            Err(e) => self.error(format!("Could not read this device profile: {e}"), now),
        }
    }

    pub fn open_path(&mut self, p: PathBuf, now: f64) {
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "lbdev" {
            self.import_lbdev(&p, now);
            return;
        }
        if ext != "lbrn2" && ext != "lbrn" && ext != "burn" && ext != "xml" {
            self.import_path(p, now);
            return;
        }
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| crate::lbrn::load_str(&t)) {
            Ok(l) => {
                if !self.doc.shapes.is_empty() || self.dirty || self.path.is_some() {
                    self.new_tab(None);
                }
                self.new_doc();
                self.doc = l.doc;
                self.kept = l.kept;
                self.doc.sync_cuts();
                self.settings.add_recent(&p.to_string_lossy());
                self.save_settings(now);
                let n = self.doc.shapes.len();
                self.path = Some(p);
                self.dirty = false;
                self.warnings = l.warnings;
                self.zoom_to_fit_all();
                let w = self.warnings.len();
                self.toast(format!("Opened project with {n} shape{}{}", if n == 1 { "" } else { "s" }, if w > 0 { format!(" ({w} note(s) in the Layers panel)") } else { String::new() }), now);
            }
            Err(e) => self.error(format!("Could not open this project: {e}"), now),
        }
    }

    pub fn import_path(&mut self, p: PathBuf, now: f64) {
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "lbrn2" || ext == "lbrn" {
            // merge another project
            match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| crate::lbrn::load_str(&t)) {
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
        match std::fs::read(&p) {
            Ok(bytes) => self.add_image_bytes(bytes, &p.to_string_lossy(), None, now),
            Err(e) => self.error(format!("Could not read the file: {e}"), now),
        }
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

    pub fn save_to(&mut self, p: PathBuf, now: f64) {
        let mut p = p;
        if p.extension().is_none() {
            p.set_extension("lbrn2");
        }
        self.doc.sync_cuts();
        let text = crate::lbrn::save_string(&self.doc, &self.kept);
        match std::fs::write(&p, text) {
            Ok(_) => {
                self.settings.add_recent(&p.to_string_lossy());
                self.save_settings(now);
                self.path = Some(p);
                self.dirty = false;
                self.toast("Saved", now);
            }
            Err(e) => self.error(format!("Could not save: {e}"), now),
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
            Action::Open => {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("Laser projects", &["lbrn2", "lbrn"]).add_filter("LightBurn device profile", &["lbdev"]).add_filter("All files", &["*"]).pick_file() {
                    self.open_path(p, now);
                }
            }
            Action::Import => {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("Images and projects", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff", "lbrn2", "lbrn"]).pick_file() {
                    self.import_path(p, now);
                }
            }
            Action::Save => match self.path.clone() {
                Some(p) if p.extension().map(|e| e == "lbrn2").unwrap_or(false) => self.save_to(p, now),
                _ => self.run(Action::SaveAs, ctx),
            },
            Action::SaveAs => {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("LightBurn-compatible project", &["lbrn2"]).set_file_name("project.lbrn2").save_file() {
                    self.save_to(p, now);
                }
            }
            Action::ExportGcode => {
                let job = gcode::build_job(&self.doc, &self.settings.device, None);
                let name = self.gcode_file_name(job.seconds);
                let ruida = self.settings.device.controller == gcode::Controller::Ruida;
                let dialog = if ruida {
                    // Ruida: the scrambled .rd job file, for a USB stick or RDWorks
                    crate::platform::FileDialog::new().add_filter("Ruida job", &["rd"]).set_file_name(&name.replace(".gcode", ".rd"))
                } else {
                    crate::platform::FileDialog::new().add_filter("G-code", &["gcode", "gc", "nc"]).set_file_name(&name)
                };
                if let Some(p) = dialog.save_file() {
                    let bytes = if ruida { crate::ruida::rd_file(&job, &self.settings.device) } else { job.gcode.clone().into_bytes() };
                    match std::fs::write(&p, &bytes) {
                        Ok(_) => self.toast(format!("G-code saved — about {}", fmt_time(job.seconds)), now),
                        Err(e) => self.error(format!("Could not save G-code: {e}"), now),
                    }
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
                crate::platform::clipboard_set_text(crate::platform::CLIP_MARKER);
                self.toast(format!("Copied {}", sel.len()), now);
            }
            Action::Cut => {
                self.clipboard = sel.iter().map(|i| self.doc.shapes[*i].clone()).collect();
                crate::platform::clipboard_set_text(crate::platform::CLIP_MARKER);
                self.checkpoint();
                ops::delete(&mut self.doc, &sel);
                self.sel.clear();
            }
            // our own shapes were copied last (the system clipboard still holds our marker) → paste those
            Action::Paste if crate::platform::clipboard_text().as_deref() != Some(crate::platform::CLIP_MARKER) => {
                // something newer than our own shapes is on the clipboard: paste, in this order,
                // SVG, then a picture, then plain text as a Text shape
                let text = crate::platform::clipboard_text();
                if let Some(t) = text.as_deref().filter(|t| is_svg_text(t)) {
                    self.paste_svg(t, now);
                } else if let Some(png) = crate::platform::clipboard_image() {
                    let at = self.canvas.pointer_world;
                    self.add_image_bytes(png, "pasted.png", at, now);
                } else if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
                    self.paste_text(&t);
                } else if !self.clipboard.is_empty() {
                    self.paste_shapes(a);
                } else {
                    self.toast("Nothing to paste: copy shapes, a picture, SVG or text in another app.", now);
                }
            }
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
                ops::rotate(&mut self.doc, &sel, if a == Action::RotateCW { -90.0 } else { 90.0 });
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
            Action::LaserHome => self.laser.send(Cmd::Send(vec!["$H".into()])),
            Action::LaserFrame => {
                let b = self.doc.bbox();
                if b.is_empty() {
                    return;
                }
                if !self.laser.connected {
                    self.toast("Connect to a laser first (Laser panel).", now);
                    return;
                }
                let f = self.settings.device.rapid_speed * 60.0;
                let pts = [b.min, p2(b.max.x, b.min.y), b.max, p2(b.min.x, b.max.y), b.min];
                let mut lines = vec!["G90".to_string(), "G21".to_string()];
                for p in pts {
                    lines.push(format!("G1 X{:.3} Y{:.3} F{:.0} S0", p.x, p.y, f));
                }
                self.laser.send(Cmd::Send(lines));
            }
            Action::LaserStart if self.settings.device.controller == gcode::Controller::Ruida => {
                let job = gcode::build_job(&self.doc, &self.settings.device, None);
                if self.settings.device.ip.trim().is_empty() {
                    self.job = Some(job);
                    self.preview_on = true;
                    self.error("Enter the Ruida controller's IP address in Settings → Laser, or use Save G-code to make an .rd file.", now);
                    return;
                }
                self.net_job = Some(crate::ruida::send(&job, &self.settings.device, &self.settings.device.ip));
                self.toast(format!("Sending the job to {} …", self.settings.device.ip), now);
                self.job = Some(job);
            }
            Action::LaserStart => {
                let job = gcode::build_job(&self.doc, &self.settings.device, None);
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
                self.laser.send(Cmd::Send(job.gcode.lines().map(|l| l.to_string()).collect()));
                self.toast(format!("Job started — about {}", fmt_time(job.seconds)), now);
                self.job_started = Some(now);
                self.job_estimate = job.seconds;
                self.job_finished_after = None;
                self.laser.progress = (0, job.gcode.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with(';')).count());
                self.job = Some(job);
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

    /// Pasted SVG: its outlines as one path (SVG's Y runs down, so it's flipped), centred on the mouse.
    pub fn paste_svg(&mut self, text: &str, now: f64) {
        let lines: Vec<Vec<P2>> = crate::mcp::outlines_from_svg_text(text).into_iter().map(|l| l.into_iter().map(|p| p2(p.x, -p.y)).collect()).collect();
        if lines.is_empty() {
            self.toast("The pasted SVG has no outlines BurnCraft can read.", now);
            return;
        }
        let b = lines.iter().flatten().fold(BBox::EMPTY, |mut b, p| {
            b.add(*p);
            b
        });
        let at = self.canvas.pointer_world.unwrap_or(p2(self.settings.device.width / 2.0, self.settings.device.height / 2.0));
        let d = at - b.center();
        let lines: Vec<Vec<P2>> = lines.into_iter().map(|l| l.into_iter().map(|p| p + d).collect()).collect();
        self.checkpoint();
        let layer = self.current_layer_for_vectors();
        self.doc.shapes.push(Shape::new(ShapeKind::Path(PathData::from_polylines(&lines)), Xform::IDENTITY, layer));
        self.doc.sync_cuts();
        self.sel = vec![self.doc.shapes.len() - 1];
        self.touch();
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
}
