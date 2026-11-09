//! User preferences, saved as JSON in the platform's config folder.

use crate::gcode::Device;
use crate::shortcuts::Keymap;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Mm,
    Inch,
}

impl Units {
    pub fn factor(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Inch => 1.0 / 25.4,
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Inch => "in",
        }
    }
    /// Decimals the measure readout uses: 2 for mm, 4 for inches.
    pub fn decimals(self) -> i32 {
        match self {
            Units::Mm => 2,
            Units::Inch => 4,
        }
    }
    /// Square millimetres to these units squared (÷ 645.16 for in²).
    pub fn area_factor(self) -> f64 {
        match self {
            Units::Mm => 1.0,
            Units::Inch => 1.0 / 645.16,
        }
    }
    /// A length in mm shown in these units, rounded to `decimals`.
    pub fn round(self, mm: f64) -> f64 {
        let k = 10f64.powi(self.decimals());
        (mm * self.factor() * k).round() / k
    }
}

#[derive(Clone, Debug)]
pub struct McpConfig {
    /// Command that starts an MCP server speaking JSON-RPC over stdio.
    pub command: String,
    pub args: String,
    /// Tool to call; it receives `image` (base64 PNG), `mime_type`, `width_px`, `height_px` and `instructions`.
    pub tool: String,
    pub instructions: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig {
            command: String::new(),
            args: String::new(),
            tool: "trace_outlines".into(),
            instructions: "Return the main outlines of the subject as SVG path data in pixel coordinates.".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub theme: Theme,
    pub ui_scale: f32,
    pub units: Units,
    pub keymap: Keymap,
    pub device: Device,
    pub mcp: McpConfig,
    pub recent: Vec<String>,
    pub snap_grid: bool,
    pub snap_points: bool,
    pub grid_step: f64,
    pub show_grid: bool,
    pub filled_view: bool,
    pub ribbon_labels: bool,
    /// Two rows of small buttons per group (CAD-style) instead of one row of large ones.
    pub ribbon_compact: bool,
    /// The Welcome Tour has been seen (finished or skipped), so it doesn't start by itself again.
    pub tour_done: bool,
    /// Camera source and alignment.
    pub camera: crate::camera::CameraSettings,
    /// Ask GitHub for a newer version when BurnCraft starts.
    pub check_updates: bool,
    pub selection_cad_style: bool,
    /// Layer settings applied to layers of new projects.
    pub layer_defaults: Vec<crate::model::CutSetting>,
    /// Commands pinned to the quick-access bar (action ids, in order).
    pub pinned: Vec<String>,
    pub ai: AiConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiProvider {
    Mcp,
    Anthropic,
    OpenAiCompatible,
}

impl AiProvider {
    pub const ALL: [AiProvider; 3] = [AiProvider::Anthropic, AiProvider::OpenAiCompatible, AiProvider::Mcp];
    pub fn label(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "Claude (Anthropic API)",
            AiProvider::OpenAiCompatible => "OpenAI-compatible (OpenAI, Ollama, LM Studio, OpenRouter…)",
            AiProvider::Mcp => "MCP server (any tool)",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            AiProvider::Anthropic => "anthropic",
            AiProvider::OpenAiCompatible => "openai",
            AiProvider::Mcp => "mcp",
        }
    }
    pub fn from_id(s: &str) -> AiProvider {
        match s {
            "anthropic" => AiProvider::Anthropic,
            "openai" => AiProvider::OpenAiCompatible,
            _ => AiProvider::Mcp,
        }
    }
}

/// The user's AI tool of choice for deriving outlines from images.
#[derive(Clone, Debug)]
pub struct AiConfig {
    pub provider: AiProvider,
    /// Saved key; when empty, ANTHROPIC_API_KEY / OPENAI_API_KEY from the environment are used.
    pub api_key: String,
    pub anthropic_model: String,
    pub openai_base_url: String,
    pub openai_model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: AiProvider::Mcp,
            api_key: String::new(),
            anthropic_model: "claude-opus-5-5".into(),
            openai_base_url: "http://localhost:11434/v1".into(),
            openai_model: "llava".into(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Dark,
            ui_scale: 1.0,
            units: Units::Mm,
            keymap: Keymap::default(),
            device: Device::default(),
            mcp: McpConfig::default(),
            recent: Vec::new(),
            snap_grid: true,
            snap_points: true,
            grid_step: 10.0,
            show_grid: true,
            filled_view: false,
            ribbon_labels: true,
            ribbon_compact: true,
            tour_done: false,
            camera: Default::default(),
            check_updates: true,
            selection_cad_style: true,
            layer_defaults: Vec::new(),
            pinned: ["Save", "Undo", "Redo", "ToolSelect", "ToolLine", "ToolRect", "ToolEllipse", "ToolText", "TraceImage", "Preview", "LaserStart"].iter().map(|s| s.to_string()).collect(),
            ai: AiConfig::default(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
const WEB_KEY: &str = "burncraft.settings";

#[cfg(target_arch = "wasm32")]
fn web_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "BurnCraft", "BurnCraft").map(|p| p.config_dir().to_path_buf())
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        #[cfg(target_arch = "wasm32")]
        let text = web_storage().and_then(|s| s.get_item(WEB_KEY).ok().flatten());
        #[cfg(not(target_arch = "wasm32"))]
        let text = Self::path().and_then(|p| std::fs::read_to_string(p).ok());
        let Some(text) = text else { return Settings::default() };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return Settings::default() };
        Self::from_json(&v)
    }

    /// In the browser, settings live in the page's local storage.
    #[cfg(target_arch = "wasm32")]
    pub fn save(&self) -> Result<(), String> {
        let s = web_storage().ok_or("the browser doesn't allow saving settings (private window?)")?;
        s.set_item(WEB_KEY, &self.to_json().to_string()).map_err(|_| "the browser's storage is full or blocked".to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) -> Result<(), String> {
        let p = Self::path().ok_or("no config folder")?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(p, serde_json::to_string_pretty(&self.to_json()).unwrap()).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> Value {
        let d = &self.device;
        json!({
            "theme": if self.theme == Theme::Dark { "dark" } else { "light" },
            "ui_scale": self.ui_scale,
            "units": if self.units == Units::Mm { "mm" } else { "in" },
            "keymap": self.keymap.to_json(),
            "device": { "name": d.name, "width": d.width, "height": d.height, "s_max": d.s_max, "rapid_speed": d.rapid_speed, "baud": d.baud, "port": d.port, "dynamic_power": d.dynamic_power, "controller": d.controller.id(), "ip": d.ip, "board": d.board, "galvo": serde_json::to_value(&d.galvo).unwrap_or(Value::Null), "rotary": serde_json::to_value(&d.rotary).unwrap_or(Value::Null),
                "inner_first": d.opt.inner_first, "reduce_travel": d.opt.reduce_travel, "best_direction": d.opt.best_direction, "best_start": d.opt.best_start },
            "mcp": { "command": self.mcp.command, "args": self.mcp.args, "tool": self.mcp.tool, "instructions": self.mcp.instructions },
            "recent": self.recent,
            "snap_grid": self.snap_grid,
            "snap_points": self.snap_points,
            "grid_step": self.grid_step,
            "show_grid": self.show_grid,
            "filled_view": self.filled_view,
            "ribbon_labels": self.ribbon_labels,
            "ribbon_compact": self.ribbon_compact,
            "tour_done": self.tour_done,
            "camera": serde_json::to_value(&self.camera).unwrap_or(Value::Null),
            "check_updates": self.check_updates,
            "selection_cad_style": self.selection_cad_style,
            "layer_defaults": self.layer_defaults_xml(),
            "pinned": self.pinned,
            "ai": { "provider": self.ai.provider.id(), "api_key": self.ai.api_key, "anthropic_model": self.ai.anthropic_model, "openai_base_url": self.ai.openai_base_url, "openai_model": self.ai.openai_model },
        })
    }

    fn layer_defaults_xml(&self) -> String {
        let mut root = crate::xml::Node::new("Layers");
        for c in &self.layer_defaults {
            root.push(crate::lbrn::cut_node(c));
        }
        let mut out = String::new();
        root.write(&mut out, 0);
        out
    }

    pub fn from_json(v: &Value) -> Settings {
        let mut s = Settings::default();
        let f = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_f64());
        let b = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_bool());
        let t = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
        if t(v, "theme").as_deref() == Some("light") {
            s.theme = Theme::Light;
        }
        s.ui_scale = f(v, "ui_scale").unwrap_or(1.0).clamp(0.6, 3.0) as f32;
        if t(v, "units").as_deref() == Some("in") {
            s.units = Units::Inch;
        }
        if let Some(k) = v.get("keymap") {
            s.keymap = Keymap::from_json(k);
        }
        if let Some(d) = v.get("device") {
            let dv = &mut s.device;
            dv.name = t(d, "name").unwrap_or(dv.name.clone());
            dv.width = f(d, "width").unwrap_or(dv.width);
            dv.height = f(d, "height").unwrap_or(dv.height);
            dv.s_max = f(d, "s_max").unwrap_or(dv.s_max);
            dv.rapid_speed = f(d, "rapid_speed").unwrap_or(dv.rapid_speed);
            dv.baud = f(d, "baud").unwrap_or(dv.baud as f64) as u32;
            dv.port = t(d, "port").unwrap_or_default();
            dv.dynamic_power = b(d, "dynamic_power").unwrap_or(true);
            dv.controller = crate::gcode::Controller::from_id(&t(d, "controller").unwrap_or_default());
            dv.ip = t(d, "ip").unwrap_or_default();
            if let Some(g) = d.get("galvo").and_then(|g| serde_json::from_value(g.clone()).ok()) {
                dv.galvo = g;
            }
            if let Some(r) = d.get("rotary").and_then(|r| serde_json::from_value(r.clone()).ok()) {
                dv.rotary = r;
            }
            dv.board = t(d, "board").filter(|b| crate::k40::BOARDS.contains(&b.as_str())).unwrap_or_else(|| "M2".into());
            dv.opt.inner_first = b(d, "inner_first").unwrap_or(dv.opt.inner_first);
            dv.opt.reduce_travel = b(d, "reduce_travel").unwrap_or(dv.opt.reduce_travel);
            dv.opt.best_direction = b(d, "best_direction").unwrap_or(dv.opt.best_direction);
            dv.opt.best_start = b(d, "best_start").unwrap_or(dv.opt.best_start);
        }
        if let Some(m) = v.get("mcp") {
            s.mcp.command = t(m, "command").unwrap_or_default();
            s.mcp.args = t(m, "args").unwrap_or_default();
            s.mcp.tool = t(m, "tool").unwrap_or(s.mcp.tool.clone());
            s.mcp.instructions = t(m, "instructions").unwrap_or(s.mcp.instructions.clone());
        }
        if let Some(r) = v.get("recent").and_then(|r| r.as_array()) {
            s.recent = r.iter().filter_map(|x| x.as_str().map(|x| x.to_string())).collect();
        }
        s.snap_grid = b(v, "snap_grid").unwrap_or(s.snap_grid);
        s.snap_points = b(v, "snap_points").unwrap_or(s.snap_points);
        s.grid_step = f(v, "grid_step").unwrap_or(s.grid_step);
        s.show_grid = b(v, "show_grid").unwrap_or(s.show_grid);
        s.filled_view = b(v, "filled_view").unwrap_or(s.filled_view);
        s.ribbon_labels = b(v, "ribbon_labels").unwrap_or(true);
        s.ribbon_compact = b(v, "ribbon_compact").unwrap_or(s.ribbon_compact);
        s.tour_done = b(v, "tour_done").unwrap_or(s.tour_done);
        if let Some(c) = v.get("camera").and_then(|c| serde_json::from_value(c.clone()).ok()) {
            s.camera = c;
        }
        s.check_updates = b(v, "check_updates").unwrap_or(s.check_updates);
        s.selection_cad_style = b(v, "selection_cad_style").unwrap_or(true);
        if let Some(x) = v.get("layer_defaults").and_then(|x| x.as_str()) {
            if let Ok(root) = crate::xml::parse(x) {
                s.layer_defaults = root.children.iter().filter(|c| c.name.starts_with("CutSetting")).map(crate::lbrn::parse_cut).collect();
            }
        }
        if let Some(p) = v.get("pinned").and_then(|p| p.as_array()) {
            s.pinned = p.iter().filter_map(|x| x.as_str().map(|x| x.to_string())).collect();
        }
        if let Some(a) = v.get("ai") {
            s.ai.provider = AiProvider::from_id(&t(a, "provider").unwrap_or_default());
            s.ai.api_key = t(a, "api_key").unwrap_or_default();
            s.ai.anthropic_model = t(a, "anthropic_model").unwrap_or(s.ai.anthropic_model.clone());
            s.ai.openai_base_url = t(a, "openai_base_url").unwrap_or(s.ai.openai_base_url.clone());
            s.ai.openai_model = t(a, "openai_model").unwrap_or(s.ai.openai_model.clone());
        }
        s
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|r| r != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(10);
    }
}
