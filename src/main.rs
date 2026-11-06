#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod barcode;
mod canvas;
mod dialogs;
mod dims;
mod fonts;
mod gcode;
mod help;
mod geom;
mod icons;
mod imaging;
mod join;
mod laser;
mod lbrn;
mod mcp;
mod nodes;
mod model;
mod ops;
mod panels;
mod platform;
mod relations;
mod ruida;
mod ribbon;
mod settings;
mod shortcuts;
mod sketch;
mod snap;
mod wand;
mod ai;
mod text;
mod theme;
mod tour;
mod xml;

use app::{App, Screenshot};
use egui::{Event, Key};
use shortcuts::Chord;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const HELP: &str = "BurnCraft — laser design and control

Usage:
  burncraft [project.lbrn2 | image.png]   open a project or image
  burncraft --screenshot <file.png>       draw ~120 frames, save a picture of the window, exit
  burncraft --mcp                         run as an MCP server on stdin/stdout (for AI assistants)
  burncraft --vectorize <in.png> <out.svg> turn a colour picture into an SVG (VTracer)
  burncraft --help
";

struct Root {
    app: App,
    started: bool,
    last_title: String,
    open_on_start: Option<PathBuf>,
}

impl Root {
    fn new(screenshot: Option<PathBuf>, open: Option<PathBuf>) -> Root {
        let mut app = App::new(settings::Settings::load());
        if let Some(path) = screenshot {
            app.screenshot = Some(Screenshot { path, frame: 0, requested: false, result: Arc::new(Mutex::new(None)) });
            app.doc = app::demo_document(&app.settings.device);
            app.sel = vec![1];
            // BURNCRAFT_TAB=sketch (etc.) shows another ribbon tab in the screenshot
            let tab = std::env::var("BURNCRAFT_TAB").unwrap_or_default().to_ascii_lowercase();
            app.ribbon_tab = app::RibbonTab::ALL.into_iter().find(|t| t.label().to_ascii_lowercase() == tab).unwrap_or(app::RibbonTab::Home);
            app.settings.theme = settings::Theme::Dark;
            app.settings.ui_scale = 1.0;
            app.settings.filled_view = true;
            app.side_tab = app::SideTab::Layers;
            app.current_layer = 1;
            // BURNCRAFT_TOUR=<step> shows that tour step, BURNCRAFT_HELP=<topic> the Help window
            app.tour = std::env::var("BURNCRAFT_TOUR").ok().and_then(|v| v.parse().ok());
            if let Some(t) = std::env::var("BURNCRAFT_HELP").ok().and_then(|v| v.parse().ok()) {
                app.win_help = true;
                app.help_topic = t;
            }
        } else if !app.settings.tour_done {
            // the first time BurnCraft runs, show the Welcome Tour
            app.tour = Some(0);
        }
        Root { app, started: false, last_title: String::new(), open_on_start: open }
    }

    fn handle_keys(&mut self, ctx: &egui::Context, run: &mut Vec<actions::Action>) {
        let app = &mut self.app;
        if app.tour.is_some() {
            // the tour has the keyboard (→ next, ← back, Esc skip)
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let typing = ctx.egui_wants_keyboard_input();
        for e in events {
            // Ctrl+V with only a picture on the clipboard (browser, screenshot tool): paste it as an image
            if let Event::PasteImage(img) = &e {
                if !typing && !app.win_search && app.capture_for.is_none() {
                    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                    if let Some(png) = platform::png_bytes(img.size[0] as u32, img.size[1] as u32, rgba) {
                        let now = ctx.input(|i| i.time);
                        let at = app.canvas.pointer_world;
                        app.add_image_bytes(png, "pasted.png", at, now);
                    }
                }
                continue;
            }
            // egui turns Ctrl+C / Ctrl+X / Ctrl+V into clipboard events on most desktops (often without a key
            // event), so treat those as the Copy / Cut / Paste commands when no text field has the keyboard
            let clip = match &e {
                Event::Copy => Some(actions::Action::Copy),
                Event::Cut => Some(actions::Action::Cut),
                Event::Paste(_) => Some(actions::Action::Paste),
                _ => None,
            };
            if let Some(a) = clip {
                if !typing && !app.win_search && app.capture_for.is_none() && !run.contains(&a) {
                    run.push(a);
                }
                continue;
            }
            let Event::Key { key, pressed: true, repeat, modifiers, .. } = e else { continue };
            let Some(chord) = Chord::from_event(key, &modifiers) else { continue };
            if app.capture_for.is_some() {
                if !repeat {
                    dialogs::capture_key(app, chord);
                    ctx.input_mut(|i| i.consume_key(modifiers, key));
                }
                continue;
            }
            if typing || app.win_search {
                continue;
            }
            let arrow = matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown);
            let node_key = if app.tool == app::Tool::Nodes && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                match key {
                    Key::S => Some(actions::Action::NodeSmooth),
                    Key::C => Some(actions::Action::NodeCorner),
                    Key::L => Some(actions::Action::NodeLine),
                    Key::U => Some(actions::Action::NodeCurve),
                    Key::I | Key::M => Some(actions::Action::NodeInsert),
                    Key::B => Some(actions::Action::NodeBreak),
                    Key::D => Some(actions::Action::Delete),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(a) = node_key {
                if !repeat {
                    run.push(a);
                }
                ctx.input_mut(|i| i.consume_key(modifiers, key));
            } else if let Some(a) = app.chord_action(&chord) {
                if run.contains(&a) && matches!(a, actions::Action::Copy | actions::Action::Cut | actions::Action::Paste) {
                    // already queued from the clipboard event this frame
                } else if !repeat || matches!(a, actions::Action::ZoomIn | actions::Action::ZoomOut) {
                    // Escape first cancels whatever is in progress
                    if a == actions::Action::ToolSelect && app.canvas.busy() {
                        app.canvas.cancel();
                    } else {
                        run.push(a);
                    }
                }
                ctx.input_mut(|i| i.consume_key(modifiers, key));
            } else if arrow && !app.sel.is_empty() && !modifiers.command && !modifiers.alt {
                let step = if modifiers.shift { 10.0 } else { 1.0 };
                let (dx, dy) = match key {
                    Key::ArrowLeft => (-step, 0.0),
                    Key::ArrowRight => (step, 0.0),
                    Key::ArrowUp => (0.0, step),
                    _ => (0.0, -step),
                };
                if !repeat {
                    app.checkpoint();
                }
                let sel = app.sel.clone();
                ops::transform_selection(&mut app.doc, &sel, geom::Xform::translate(dx, dy));
                app.touch_xform();
            }
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(sh) = self.app.screenshot.as_mut() else { return };
        sh.frame += 1;
        ctx.request_repaint();
        if sh.frame >= 120 && !sh.requested {
            sh.requested = true;
            let slot = sh.result.clone();
            ctx.request_screenshot(move |img| {
                *slot.lock().unwrap() = Some(img);
            });
        }
        let got = sh.result.lock().unwrap().take();
        if let Some(img) = got {
            let [w, h] = img.size;
            let mut rgba = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                rgba.extend_from_slice(&p.to_array());
            }
            if let Some(parent) = sh.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                Some(im) => match im.save(&sh.path) {
                    Ok(_) => eprintln!("screenshot saved to {}", sh.path.display()),
                    Err(e) => eprintln!("could not save screenshot: {e}"),
                },
                None => eprintln!("screenshot had an unexpected size"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if sh.frame > 600 {
            eprintln!("screenshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for Root {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.started {
            self.started = true;
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            self.app.apply_theme(&ctx);
            if ctx.content_rect().width() < 900.0 {
                self.app.show_left = false;
            }
            if let Some(p) = self.open_on_start.take() {
                self.app.open_path(p, 0.0);
            }
            self.app.ports = laser::Laser::ports();
            app::set_layer_defaults(&self.app.settings.layer_defaults);
        }
        self.screenshot_step(&ctx);
        let mut run = Vec::new();
        self.handle_keys(&ctx, &mut run);
        let app = &mut self.app;

        let title = app.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        let col = theme::colors(ui.visuals().dark_mode);
        let narrow = ctx.content_rect().width() < 900.0;

        let r = egui::Panel::top("ribbon").frame(egui::Frame::NONE.fill(col.ribbon).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            run.extend(ribbon::ribbon(ui, app));
        });
        let mut groups = r.response.rect;
        if let Some(t) = app.ui_rects.get("ribbon_tabs") {
            groups.min.y = t.bottom() + 2.0;
        }
        app.ui_rects.insert("ribbon_groups", groups);
        let mut close_tab = None;
        let r = egui::Panel::bottom("status").frame(egui::Frame::NONE.fill(col.panel).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            panels::bottom_bar(ui, app);
            close_tab = panels::project_tabs(ui, app);
        });
        app.ui_rects.insert("status", r.response.rect);
        if let Some(i) = close_tab {
            let dirty = if i == app.active_tab { app.dirty } else { app.tabs[i].as_ref().map(|t| t.dirty).unwrap_or(false) };
            if !dirty || app::confirm_discard() {
                app.close_tab(i);
            }
        }
        if app.show_left {
            let r = egui::Panel::left("shapes").resizable(true).default_size(if narrow { 180.0 } else { 230.0 }).frame(egui::Frame::NONE.fill(col.panel).inner_margin(8)).show(ui, |ui| {
                panels::left_panel(ui, app);
            });
            app.ui_rects.insert("shapes", r.response.rect);
        } else {
            app.ui_rects.remove("shapes");
        }
        if app.show_right {
            let r = egui::Panel::right("side").resizable(true).default_size(if narrow { 280.0 } else { 330.0 }).frame(egui::Frame::NONE.fill(col.panel).inner_margin(8)).show(ui, |ui| {
                panels::right_panel(ui, app, &mut run);
            });
            app.ui_rects.insert("side", r.response.rect);
        } else {
            app.ui_rects.remove("side");
        }
        let r = egui::CentralPanel::default().frame(egui::Frame::NONE.fill(col.canvas)).show(ui, |ui| {
            run.extend(canvas::canvas_ui(app, ui));
        });
        app.ui_rects.insert("canvas", r.response.rect);
        dialogs::all(&ctx, app, &mut run);
        canvas::dim_editor(&ctx, app);
        tour::show(&ctx, app);

        if let Some(a) = ctx.data_mut(|d| {
            let a = d.get_temp::<actions::Action>(egui::Id::new("run_action"));
            d.remove::<actions::Action>(egui::Id::new("run_action"));
            a
        }) {
            run.push(a);
        }
        for a in run {
            app.run(a, &ctx);
        }
        // preview playback: move the laser head along the job
        if app.preview_playing && app.preview_on {
            if let Some(job) = &app.job {
                let dt = ctx.input(|i| i.stable_dt).min(0.1);
                let dur = (job.seconds as f32 / app.preview_speed).max(0.5);
                app.preview_t = (app.preview_t + dt / dur).min(1.0);
                if app.preview_t >= 1.0 {
                    app.preview_playing = false;
                }
                ctx.request_repaint();
            }
        } else if !app.preview_on {
            app.preview_playing = false;
        }
        if ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("load_demo"))).unwrap_or(false) {
            app.new_doc();
            app.doc = app::demo_document(&app.settings.device);
            app.dirty = true;
            app.zoom_to_fit_all();
        }
        // a network send finished?
        let done = app.net_job.as_ref().and_then(|s| s.lock().ok().and_then(|mut g| g.take()));
        if let Some(r) = done {
            app.net_job = None;
            let now = ctx.input(|i| i.time);
            match r {
                Ok(m) => app.toast(m, now),
                Err(e) => app.error(format!("Could not send the job: {e}"), now),
            }
        } else if app.net_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if app.laser.poll() || app.laser.connected {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        // ask before closing with unsaved work
        if ctx.input(|i| i.viewport().close_requested()) && app.dirty && app.screenshot.is_none() && !app.pending_quit {
            let ok = matches!(
                crate::platform::MessageDialog::new().set_title("Quit BurnCraft?").set_description("You have unsaved changes. Quit anyway?").set_buttons(crate::platform::MessageButtons::YesNo).show(),
                crate::platform::MessageDialogResult::Yes
            );
            if ok {
                app.pending_quit = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--mcp") {
        mcp::run_server();
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--bench") {
        if let Some(p) = args.get(i + 1) {
            canvas::bench(std::path::Path::new(p));
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--vectorize") {
        match (args.get(i + 1), args.get(i + 2)) {
            (Some(inp), Some(out)) => {
                let cfg = vtracer::Config { filter_speckle: 6, color_precision: 6, layer_difference: 24, ..Default::default() };
                match vtracer::convert_image_to_svg(std::path::Path::new(inp), std::path::Path::new(out), cfg) {
                    Ok(_) => eprintln!("wrote {out}"),
                    Err(e) => eprintln!("could not vectorize: {e}"),
                }
            }
            _ => eprintln!("usage: burncraft --vectorize <in.png> <out.svg>"),
        }
        return Ok(());
    }
    let mut screenshot = None;
    let mut open = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--screenshot" {
            screenshot = it.next().map(PathBuf::from);
        } else if !a.starts_with("--") {
            open = Some(PathBuf::from(a));
        }
    }
    let options = eframe::NativeOptions {
        viewport: {
            let vb = egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]).with_min_inner_size([360.0, 480.0]).with_title("BurnCraft").with_app_id("org.burncraft.BurnCraft");
            match ribbon::window_icon() {
                Some(icon) => vb.with_icon(std::sync::Arc::new(icon)),
                None => vb,
            }
        },
        ..Default::default()
    };
    eframe::run_native("BurnCraft", options, Box::new(move |_cc| Ok(Box::new(Root::new(screenshot, open)))))
}

/// Browser / PWA entry point: draws into the `<canvas id="burncraft">` of web/index.html.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("no document");
        let canvas = document.get_element_by_id("burncraft").expect("no canvas with id burncraft").dyn_into::<web_sys::HtmlCanvasElement>().expect("not a canvas");
        let _ = eframe::WebRunner::new().start(canvas, web_options, Box::new(|_cc| Ok(Box::new(Root::new(None, None))))).await;
    });
}
