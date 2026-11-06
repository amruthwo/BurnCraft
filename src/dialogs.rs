//! Floating windows: shortcuts editor, settings, trace, MCP outline, command search, notes, array, offset.

use crate::actions::Action;
use crate::app::{App, McpState};
use crate::geom::*;
use crate::model::*;
use crate::settings::{Theme, Units};
use crate::shortcuts::{Chord, Keymap, Preset};
use crate::theme::{self, ACCENT};
use egui::{vec2, Color32, Context, RichText};

pub fn all(ctx: &Context, app: &mut App, run: &mut Vec<Action>) {
    shortcuts(ctx, app);
    settings(ctx, app);
    trace(ctx, app);
    mcp(ctx, app);
    search(ctx, app, run);
    notes(ctx, app);
    array(ctx, app);
    offset(ctx, app);
    circular(ctx, app);
    along_curve(ctx, app);
    barcode(ctx, app);
    crate::help::window(ctx, app);
    toasts(ctx, app);
}

fn shortcuts(ctx: &Context, app: &mut App) {
    if !app.win_shortcuts {
        return;
    }
    let mut open = true;
    egui::Window::new("Keyboard shortcuts").open(&mut open).default_size(vec2(620.0, 560.0)).resizable(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Starting point:");
            for p in [Preset::Cad, Preset::LightBurn] {
                if ui.selectable_label(app.settings.keymap.preset == p, p.label()).clicked() {
                    app.settings.keymap.preset = p;
                    app.settings.keymap.overrides.clear();
                    let _ = app.settings.save();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Find:");
            ui.add(egui::TextEdit::singleline(&mut app.shortcut_filter).hint_text("command or key").desired_width(200.0));
            if ui.button("Reset all").clicked() {
                app.settings.keymap.overrides.clear();
                let _ = app.settings.save();
            }
            if ui.button("Export…").clicked() {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("Shortcuts", &["json"]).set_file_name("burncraft-shortcuts.json").save_file() {
                    let _ = std::fs::write(p, serde_json::to_string_pretty(&app.settings.keymap.to_json()).unwrap());
                }
            }
            if ui.button("Import…").clicked() {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("Shortcuts", &["json"]).pick_file() {
                    if let Ok(v) = std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
                        app.settings.keymap = Keymap::from_json(&v);
                        let _ = app.settings.save();
                    }
                }
            }
        });
        if let Some(a) = app.capture_for {
            ui.label(RichText::new(format!("Press the new keys for “{}”… (Esc cancels)", a.label())).color(ACCENT).strong());
        } else {
            ui.label(RichText::new("Click a shortcut to change it. Single letters work while the canvas has focus, just like CAD apps.").size(11.5));
        }
        ui.separator();
        let filter = app.shortcut_filter.to_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut last_group = "";
            egui::Grid::new("keys").striped(true).num_columns(4).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                for a in Action::ALL.iter().copied() {
                    let keys = app.settings.keymap.chords(a);
                    let key_text = keys.iter().map(|c| c.display()).collect::<Vec<_>>().join(", ");
                    if !filter.is_empty() && !a.label().to_lowercase().contains(&filter) && !key_text.to_lowercase().contains(&filter) && !a.group().to_lowercase().contains(&filter) {
                        continue;
                    }
                    if a.group() != last_group {
                        last_group = a.group();
                        ui.label(RichText::new(a.group()).strong().color(ACCENT));
                        ui.end_row();
                    }
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), r, a.icon(), &crate::ribbon::pal(ui, false, true));
                        ui.label(a.label());
                    });
                    let capturing = app.capture_for == Some(a);
                    let txt = if capturing { "press keys…".to_string() } else if key_text.is_empty() { "—".to_string() } else { key_text };
                    if ui.add(egui::Button::new(RichText::new(txt).monospace()).selected(capturing).min_size(vec2(150.0, 22.0))).on_hover_text(a.tip()).clicked() {
                        app.capture_for = Some(a);
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Clear").clicked() {
                            app.settings.keymap.set(a, Vec::new());
                            let _ = app.settings.save();
                        }
                        if app.settings.keymap.overrides.contains_key(&a) && ui.small_button("Default").clicked() {
                            app.settings.keymap.reset(a);
                            let _ = app.settings.save();
                        }
                    });
                    let conflicts = app.settings.keymap.conflicts(a);
                    if conflicts.is_empty() {
                        ui.label("");
                    } else {
                        ui.label(RichText::new(format!("Also used by: {}", conflicts.iter().map(|(_, o)| o.label()).collect::<Vec<_>>().join(", "))).color(Color32::from_rgb(240, 170, 40)).size(11.0));
                    }
                    ui.end_row();
                }
            });
        });
    });
    if !open {
        app.win_shortcuts = false;
        app.capture_for = None;
    }
}

/// Called from the key handler while capturing a new shortcut.
pub fn capture_key(app: &mut App, c: Chord) {
    if let Some(a) = app.capture_for.take() {
        if c.key.name() == "Escape" && !c.has_modifier() && !c.shift {
            return;
        }
        let mut list = app.settings.keymap.chords(a);
        // replace the main shortcut, keep alternates
        if list.is_empty() {
            list.push(c);
        } else {
            list[0] = c;
        }
        list.dedup();
        app.settings.keymap.set(a, list);
        let _ = app.settings.save();
    }
}

fn settings(ctx: &Context, app: &mut App) {
    if !app.win_settings {
        return;
    }
    let mut open = true;
    let mut save = false;
    let mut retheme = false;
    let mut replay = false;
    egui::Window::new("Settings").open(&mut open).default_width(460.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Getting started").strong());
            replay = ui.button("Replay Welcome Tour").on_hover_text("Walk through the window again, step by step").clicked();
            if ui.button("Help…").on_hover_text("How to use BurnCraft (F1)").clicked() {
                app.win_help = true;
            }
        });
        ui.separator();
        ui.label(RichText::new("Look and feel").strong());
        egui::Grid::new("s1").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label("Theme");
            ui.horizontal(|ui| {
                retheme |= ui.selectable_value(&mut app.settings.theme, Theme::Dark, "Dark").changed();
                retheme |= ui.selectable_value(&mut app.settings.theme, Theme::Light, "Light").changed();
            });
            ui.end_row();
            ui.label("Size");
            ui.horizontal(|ui| {
                for (v, l) in [(0.9, "Compact"), (1.0, "Normal"), (1.3, "Large"), (1.6, "Touch")] {
                    retheme |= ui.selectable_value(&mut app.settings.ui_scale, v, l).changed();
                }
            });
            ui.end_row();
            ui.label("Ribbon labels");
            save |= ui.checkbox(&mut app.settings.ribbon_labels, "Show text under icons").changed();
            ui.end_row();
            ui.label("Ribbon layout");
            save |= ui.checkbox(&mut app.settings.ribbon_compact, "Compact: two rows of small buttons (CAD-style)").changed();
            ui.end_row();
            ui.label("Units");
            ui.horizontal(|ui| {
                save |= ui.selectable_value(&mut app.settings.units, Units::Mm, "mm").changed();
                save |= ui.selectable_value(&mut app.settings.units, Units::Inch, "inch").changed();
            });
            ui.end_row();
            ui.label("Grid spacing");
            save |= ui.add(egui::DragValue::new(&mut app.settings.grid_step).range(1.0..=100.0).suffix(" mm")).changed();
            ui.end_row();
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Laser").strong());
            if ui.button("Import LightBurn device (.lbdev)…").on_hover_text("Read the laser's name and work area from a LightBurn device profile").clicked() {
                if let Some(p) = crate::platform::FileDialog::new().add_filter("LightBurn device", &["lbdev"]).pick_file() {
                    let now = ui.input(|i| i.time);
                    app.import_lbdev(&p, now);
                }
            }
        });
        let d = &mut app.settings.device;
        egui::Grid::new("s2").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label("Name");
            save |= ui.text_edit_singleline(&mut d.name).changed();
            ui.end_row();
            ui.label("Controller");
            let before = d.controller;
            egui::ComboBox::from_id_salt("controller").selected_text(d.controller.label()).width(260.0).show_ui(ui, |ui| {
                use crate::gcode::{Family, Support};
                for (fam, title) in [(Family::GCode, "G-code controllers"), (Family::Dsp, "DSP controllers"), (Family::Galvo, "Galvo controllers")] {
                    ui.label(RichText::new(title).strong().size(11.5));
                    for c in crate::gcode::Controller::ALL.into_iter().filter(|c| c.family() == fam) {
                        let note = match c.support() {
                            Support::Full => "",
                            Support::FileAndNetwork => "  · .rd files + network",
                            Support::NotYet => "  · not yet",
                        };
                        let r = ui.add_enabled(c.support() != Support::NotYet, egui::Button::selectable(d.controller == c, format!("{}{}", c.label(), note)));
                        if r.clicked() {
                            d.controller = c;
                        }
                    }
                }
            });
            if d.controller != before {
                // switch to that controller's usual power range and serial speed
                d.s_max = d.controller.default_s_max();
                d.baud = d.controller.default_baud();
                d.dynamic_power = d.dynamic_power && d.controller.has_dynamic();
                save = true;
            }
            ui.end_row();
            ui.label("");
            ui.label(RichText::new("G-code controllers: send over USB/serial or save G-code. Ruida (DSP): save .rd files or send over the network. Trocen, TopWisdom and galvo (EZCad, BSL) controllers use closed protocols that BurnCraft doesn't speak yet.").size(11.0).weak());
            ui.end_row();
            if d.controller == crate::gcode::Controller::Ruida {
                ui.label("IP address");
                save |= ui.add(egui::TextEdit::singleline(&mut d.ip).hint_text("e.g. 192.168.1.100")).on_hover_text("The address shown on the controller's panel (Ruida listens on UDP port 50200)").changed();
                ui.end_row();
            }
            ui.label("Work area");
            ui.horizontal(|ui| {
                save |= ui.add(egui::DragValue::new(&mut d.width).range(10.0..=5000.0).suffix(" mm")).changed();
                ui.label("×");
                save |= ui.add(egui::DragValue::new(&mut d.height).range(10.0..=5000.0).suffix(" mm")).changed();
            });
            ui.end_row();
            ui.label("Max S value");
            save |= ui.add(egui::DragValue::new(&mut d.s_max).range(1.0..=100000.0)).on_hover_text("GRBL $30 — usually 1000").changed();
            ui.end_row();
            ui.label("Travel speed");
            save |= ui.add(egui::DragValue::new(&mut d.rapid_speed).range(1.0..=2000.0).suffix(" mm/s")).changed();
            ui.end_row();
            ui.label("Power mode");
            save |= ui.add_enabled(d.controller.has_dynamic(), egui::Checkbox::new(&mut d.dynamic_power, "Dynamic (M4) — lower power in corners")).changed();
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("Cut optimization").strong()).on_hover_text("The order the laser cuts shapes on the same layer (LightBurn's defaults)");
        let o = &mut app.settings.device.opt;
        save |= ui.checkbox(&mut o.reduce_travel, "Reduce travel moves (go to the nearest shape next)").changed();
        save |= ui.checkbox(&mut o.inner_first, "Cut inner shapes first").changed();
        save |= ui.checkbox(&mut o.best_direction, "Choose best direction (open lines from either end)").changed();
        save |= ui.checkbox(&mut o.best_start, "Choose best starting point (closed shapes)").changed();
        ui.label(RichText::new("Layers are cut in the order of the Layers list; within a layer the options above apply.").size(11.0).weak());
        ui.separator();
        ui.label(RichText::new("AI outlines").strong());
        save |= ai_settings_ui(ui, app);
        ui.separator();
        ui.label(RichText::new(format!("Settings file: {}", crate::settings::Settings::path().map(|p| p.display().to_string()).unwrap_or_default())).size(10.5));
    });
    if retheme {
        app.apply_theme(ctx);
        save = true;
    }
    if save {
        let _ = app.settings.save();
        app.view.fitted = app.view.fitted && true;
    }
    if !open {
        app.win_settings = false;
    }
    if replay {
        app.win_settings = false;
        app.tour = Some(0);
    }
}

fn trace(ctx: &Context, app: &mut App) {
    if !app.win_trace {
        return;
    }
    let Some(idx) = app.trace.target.and_then(|id| app.index_of_id(id)) else {
        app.win_trace = false;
        return;
    };
    // recompute the preview when the settings change
    let key = (app.doc.shapes[idx].id, app.trace.params.clone());
    if app.trace.preview_for.as_ref() != Some(&key) {
        let s = app.doc.shapes[idx].clone();
        if let ShapeKind::Bitmap { w, h, data, adjust } = &s.kind {
            let gray = crate::imaging::adjusted_gray(data, adjust);
            let (iw, ih) = (data.width as usize, data.height as usize);
            let px = crate::imaging::trace(&gray, iw, ih, &app.trace.params);
            app.trace.preview = crate::imaging::pixels_to_shape_space(px.clone(), *w, *h, data.width, data.height, &s.xform);
            app.trace.preview_px = px;
            // thumbnail: what gets traced (black = ink) or the adjusted original
            let step = (iw.max(ih) / 900).max(1);
            let (tw, th) = (iw.div_ceil(step), ih.div_ceil(step));
            let mask = crate::imaging::ink_mask(&gray, &app.trace.params);
            let mut pix = Vec::with_capacity(tw * th);
            for y in 0..th {
                for x in 0..tw {
                    let i = (y * step).min(ih - 1) * iw + (x * step).min(iw - 1);
                    pix.push(if app.trace.show_original { gray[i] } else if mask[i] { 30 } else { 245 });
                }
            }
            let img = egui::ColorImage::from_gray([tw, th], &pix);
            app.trace.thumb = Some(ctx.load_texture("trace_thumb", img, egui::TextureOptions::LINEAR));
            app.trace.thumb_size = (iw, ih);
        }
        app.trace.preview_for = Some(key);
    }
    let mut open = true;
    let mut accept = false;
    egui::Window::new("Trace Image").open(&mut open).default_width(380.0).show(ctx, |ui| {
        // live preview inside the window
        if let Some(tex) = &app.trace.thumb {
            let (iw, ih) = app.trace.thumb_size;
            let avail = ui.available_width().min(380.0);
            let scale = (avail / iw.max(1) as f32).min(280.0 / ih.max(1) as f32);
            let size = vec2(iw as f32 * scale, ih as f32 * scale);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            for l in &app.trace.preview_px {
                let pts: Vec<egui::Pos2> = l.iter().map(|p| rect.min + vec2(p.x as f32 * scale, p.y as f32 * scale)).collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, Color32::from_rgb(30, 200, 110))));
            }
            ui.horizontal(|ui| {
                if ui.selectable_label(!app.trace.show_original, "What gets traced").clicked() {
                    app.trace.show_original = false;
                    app.trace.preview_for = None;
                }
                if ui.selectable_label(app.trace.show_original, "Original").clicked() {
                    app.trace.show_original = true;
                    app.trace.preview_for = None;
                }
            });
        }
        let p = &mut app.trace.params;
        egui::Grid::new("tr").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Method");
            egui::ComboBox::from_id_salt("engine").selected_text(p.engine.label()).show_ui(ui, |ui| {
                for e in crate::imaging::TraceEngine::ALL {
                    ui.selectable_value(&mut p.engine, e, e.label());
                }
            });
            ui.end_row();
            ui.label("Threshold");
            ui.add(egui::Slider::new(&mut p.threshold, 0..=255)).on_hover_text("Pixels darker than this are traced");
            ui.end_row();
            ui.label("Cutoff");
            ui.add(egui::Slider::new(&mut p.cutoff, 0..=255)).on_hover_text("Pixels darker than this are ignored (trace a band of greys)");
            ui.end_row();
            ui.label("Ignore less than");
            ui.add(egui::Slider::new(&mut p.ignore_less_than, 0.0..=50.0).suffix(" px")).on_hover_text("Drop specks smaller than this");
            ui.end_row();
            ui.label("Smoothness");
            ui.add(egui::Slider::new(&mut p.smoothness, 0.0..=4.0));
            ui.end_row();
            ui.label("Optimize");
            ui.add(egui::Slider::new(&mut p.optimize, 0.0..=4.0)).on_hover_text("Higher = fewer points");
            ui.end_row();
            ui.label("Trace light parts");
            ui.checkbox(&mut p.invert, "");
            ui.end_row();
            ui.label("Put on layer");
            egui::ComboBox::from_id_salt("trace_layer").selected_text(layer_name(app.trace.layer)).show_ui(ui, |ui| {
                for l in 0..COLOR_LAYERS {
                    ui.selectable_value(&mut app.trace.layer, l, layer_name(l));
                }
            });
            ui.end_row();
            ui.label("Delete image after");
            ui.checkbox(&mut app.trace.delete_image, "");
            ui.end_row();
        });
        let pts: usize = app.trace.preview.iter().map(|l| l.len()).sum();
        ui.label(RichText::new(format!("{} outlines, {} points (also shown in green on the bed)", app.trace.preview.len(), pts)).size(11.5));
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Add outlines").strong()).fill(ACCENT.gamma_multiply(0.8))).clicked() {
                accept = true;
            }
            if ui.button("Defaults").clicked() {
                app.trace.params = Default::default();
            }
            if ui.button("Ask AI instead…").on_hover_text("Use your AI tool of choice to find outlines").clicked() {
                app.win_mcp = true;
            }
        });
    });
    if accept {
        let lines = std::mem::take(&mut app.trace.preview);
        let del = if app.trace.delete_image { app.trace.target } else { None };
        app.add_outlines(lines, del);
        open = false;
    }
    if !open {
        app.win_trace = false;
        app.trace.preview.clear();
        app.trace.preview_px.clear();
        app.trace.preview_for = None;
    }
}

/// AI provider settings, shared by Settings and the AI Outline window. Returns true if anything changed.
pub fn ai_settings_ui(ui: &mut egui::Ui, app: &mut App) -> bool {
    use crate::settings::AiProvider;
    let mut changed = false;
    ui.label(RichText::new("Optional. The built-in tracers are open source and work offline; an AI tool can help with photos where a brightness trace isn't enough.").size(11.5));
    let ai = &mut app.settings.ai;
    egui::ComboBox::from_id_salt("ai_provider").selected_text(ai.provider.label()).width(320.0).show_ui(ui, |ui| {
        for p in AiProvider::ALL {
            changed |= ui.selectable_value(&mut ai.provider, p, p.label()).changed();
        }
    });
    egui::Grid::new("ai_cfg").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
        match ai.provider {
            AiProvider::Anthropic => {
                ui.label("API key");
                changed |= ui.add(egui::TextEdit::singleline(&mut ai.api_key).password(true).hint_text("leave empty to use ANTHROPIC_API_KEY")).changed();
                ui.end_row();
                ui.label("Model");
                changed |= ui.add(egui::TextEdit::singleline(&mut ai.anthropic_model).hint_text("claude-opus-5-5")).changed();
                ui.end_row();
            }
            AiProvider::OpenAiCompatible => {
                ui.label("Server");
                changed |= ui.add(egui::TextEdit::singleline(&mut ai.openai_base_url).hint_text("https://api.openai.com/v1  or  http://localhost:11434/v1")).changed();
                ui.end_row();
                ui.label("Vision model");
                changed |= ui.add(egui::TextEdit::singleline(&mut ai.openai_model).hint_text("e.g. gpt-5, llava, qwen2.5-vl")).changed();
                ui.end_row();
                ui.label("API key");
                changed |= ui.add(egui::TextEdit::singleline(&mut ai.api_key).password(true).hint_text("not needed for local servers")).changed();
                ui.end_row();
            }
            AiProvider::Mcp => {
                let m = &mut app.settings.mcp;
                ui.label("Command");
                changed |= ui.add(egui::TextEdit::singleline(&mut m.command).hint_text("e.g. npx, uvx, /path/to/server")).changed();
                ui.end_row();
                ui.label("Arguments");
                changed |= ui.text_edit_singleline(&mut m.args).changed();
                ui.end_row();
                ui.label("Tool name");
                changed |= ui.text_edit_singleline(&mut m.tool).changed();
                ui.end_row();
            }
        }
    });
    if app.settings.ai.provider != AiProvider::Mcp && !app.settings.ai.api_key.is_empty() {
        ui.label(RichText::new("The key is saved in your BurnCraft settings file on this computer.").size(10.5));
    }
    ui.label(RichText::new("BurnCraft is also an MCP server: run “burncraft --mcp” so an assistant can trace images and add outlines to projects.").size(10.5));
    changed
}

fn mcp(ctx: &Context, app: &mut App) {
    if !app.win_mcp {
        return;
    }
    let now = ctx.input(|i| i.time);
    // collect a finished background job
    if let McpState::Running { target, rx, .. } = &app.mcp {
        if let Ok(res) = rx.try_recv() {
            let target = *target;
            app.mcp = match res {
                Ok(px_lines) => {
                    let lines = match app.index_of_id(target).map(|i| app.doc.shapes[i].clone()) {
                        Some(s) => match &s.kind {
                            ShapeKind::Bitmap { w, h, data, .. } => crate::imaging::pixels_to_shape_space(px_lines, *w, *h, data.width, data.height, &s.xform),
                            _ => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    McpState::Done { target, lines }
                }
                Err(e) => McpState::Failed(e),
            };
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
    let mut open = true;
    let mut start = false;
    let mut accept = false;
    let mut save = false;
    egui::Window::new("AI Outline").open(&mut open).default_width(420.0).show(ctx, |ui| {
        ui.label("Send the selected image to your AI tool and get back vector outlines.");
        ui.add_space(4.0);
        save |= ai_settings_ui(ui, app);
        ui.separator();
        ui.label("What to look for");
        save |= ui.add(egui::TextEdit::multiline(&mut app.settings.mcp.instructions).desired_rows(2).desired_width(f32::INFINITY)).changed();
        ui.horizontal(|ui| {
            ui.label("Put on layer");
            egui::ComboBox::from_id_salt("mcp_layer").selected_text(layer_name(app.trace.layer)).show_ui(ui, |ui| {
                for l in 0..COLOR_LAYERS {
                    ui.selectable_value(&mut app.trace.layer, l, layer_name(l));
                }
            });
        });
        match &app.mcp {
            McpState::Idle => {}
            McpState::Running { started, .. } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Waiting for the AI… {:.0}s", now - started));
                });
            }
            McpState::Done { lines, .. } => {
                ui.label(RichText::new(format!("Got {} outlines (green on the bed).", lines.len())).color(Color32::from_rgb(70, 200, 110)));
            }
            McpState::Failed(e) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(235, 90, 80)));
            }
        }
        ui.horizontal(|ui| {
            let running = matches!(app.mcp, McpState::Running { .. });
            let ready = app.settings.ai.provider != crate::settings::AiProvider::Mcp || !app.settings.mcp.command.trim().is_empty();
            if ui.add_enabled(!running && ready, egui::Button::new("Ask")).clicked() {
                start = true;
            }
            if matches!(app.mcp, McpState::Done { .. }) && ui.add(egui::Button::new(RichText::new("Add outlines").strong()).fill(ACCENT.gamma_multiply(0.8))).clicked() {
                accept = true;
            }
        });
    });
    if save {
        let _ = app.settings.save();
    }
    if start {
        if let Some(i) = app.trace.target.and_then(|id| app.index_of_id(id)) {
            if let ShapeKind::Bitmap { data, adjust, .. } = &app.doc.shapes[i].kind {
                let gray = crate::imaging::adjusted_gray(data, adjust);
                let img = image::GrayImage::from_raw(data.width, data.height, gray).unwrap();
                let mut png = Vec::new();
                let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
                let (w, h) = (data.width, data.height);
                let ai = app.settings.ai.clone();
                let mcp_cfg = app.settings.mcp.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::ai::derive_outlines(&ai, &mcp_cfg, &png, w, h));
                    ctx2.request_repaint();
                });
                app.mcp = McpState::Running { target: app.doc.shapes[i].id, rx, started: now };
            }
        }
    }
    if accept {
        if let McpState::Done { lines, .. } = std::mem::replace(&mut app.mcp, McpState::Idle) {
            app.add_outlines(lines, None);
        }
        open = false;
    }
    if !open {
        app.win_mcp = false;
        if !matches!(app.mcp, McpState::Running { .. }) {
            app.mcp = McpState::Idle;
        }
    }
}

fn search(ctx: &Context, app: &mut App, run: &mut Vec<Action>) {
    if !app.win_search {
        return;
    }
    let q = app.search_text.to_lowercase();
    let mut hits: Vec<Action> = Action::ALL
        .iter()
        .copied()
        .filter(|a| q.is_empty() || a.label().to_lowercase().contains(&q) || a.group().to_lowercase().contains(&q) || a.tip().to_lowercase().contains(&q))
        .collect();
    hits.sort_by_key(|a| !a.label().to_lowercase().starts_with(&q));
    let screen = ctx.content_rect();
    let mut close = false;
    egui::Area::new(egui::Id::new("cmd_search")).fixed_pos(egui::pos2(screen.center().x - 220.0, screen.top() + 110.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        let col = theme::colors(ui.visuals().dark_mode);
        egui::Frame::popup(ui.style()).fill(col.panel).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(440.0);
            let r = ui.add(egui::TextEdit::singleline(&mut app.search_text).hint_text("Search commands…").desired_width(f32::INFINITY).font(egui::TextStyle::Heading));
            r.request_focus();
            let (down, up, enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if r.changed() {
                app.search_index = 0;
            }
            if down {
                app.search_index = (app.search_index + 1).min(hits.len().saturating_sub(1));
            }
            if up {
                app.search_index = app.search_index.saturating_sub(1);
            }
            if esc {
                close = true;
            }
            ui.add_space(4.0);
            for (k, a) in hits.iter().take(9).enumerate() {
                let sel = k == app.search_index;
                let resp = ui.horizontal(|ui| {
                    let (rr, _) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::hover());
                    crate::icons::paint(ui.painter(), rr, a.icon(), &crate::ribbon::pal(ui, sel, app.enabled(*a)));
                    let r = ui.selectable_label(sel, format!("{}   ·  {}", a.label(), a.group()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(app.settings.keymap.label(*a)).monospace().color(col.weak));
                    });
                    r
                });
                if resp.inner.clicked() || (sel && enter) {
                    run.push(*a);
                    close = true;
                }
            }
            if hits.is_empty() {
                ui.label("No matching command");
            }
        });
    });
    if close {
        app.win_search = false;
    }
}

fn notes(ctx: &Context, app: &mut App) {
    if !app.win_notes {
        return;
    }
    let mut open = true;
    egui::Window::new("Project notes").open(&mut open).default_size(vec2(360.0, 240.0)).show(ctx, |ui| {
        if ui.add(egui::TextEdit::multiline(&mut app.doc.notes).desired_width(f32::INFINITY).desired_rows(10)).changed() {
            app.dirty = true;
        }
    });
    if !open {
        app.win_notes = false;
    }
}

fn array(ctx: &Context, app: &mut App) {
    if !app.win_array {
        return;
    }
    let mut open = true;
    let mut go = false;
    let editing = app.array_edit.is_some();
    egui::Window::new(if editing { "Edit Grid Array" } else { "Grid Array" }).open(&mut open).show(ctx, |ui| {
        // the size of what is being copied (the array's originals when editing)
        let src: Vec<usize> = match app.array_edit.and_then(|ai| app.doc.arrays.get(ai)) {
            Some(a) => a.sources.iter().filter_map(|id| app.doc.shapes.iter().position(|s| s.id == *id)).collect(),
            None => app.sel.clone(),
        };
        let bb = crate::ops::selection_bbox(&app.doc, &src);
        let (bw, bh) = if bb.is_empty() { (0.0, 0.0) } else { (bb.width(), bb.height()) };
        let c2c = &mut app.array_c2c;
        let (c, r, gx, gy) = &mut app.array;
        // Grid Array dialog (same options as LightBurn's): spacing either
        // "Distance from Center to Center" or "Padding Between Edges"
        ui.horizontal(|ui| {
            ui.selectable_value(c2c, false, "Padding Between Edges");
            ui.selectable_value(c2c, true, "Distance from Center to Center");
        });
        egui::Grid::new("arr").num_columns(2).show(ui, |ui| {
            ui.label("X Columns");
            ui.add(egui::DragValue::new(c).range(1..=100));
            ui.end_row();
            ui.label("Y Rows");
            ui.add(egui::DragValue::new(r).range(1..=100));
            ui.end_row();
            let off = |c2c: bool, size: f64| if c2c { size } else { 0.0 };
            ui.label("X Spacing");
            let mut sx = *gx + off(*c2c, bw);
            if ui.add(egui::DragValue::new(&mut sx).speed(0.5).suffix(" mm")).changed() {
                *gx = sx - off(*c2c, bw);
            }
            ui.end_row();
            ui.label("Y Spacing");
            let mut sy = *gy + off(*c2c, bh);
            if ui.add(egui::DragValue::new(&mut sy).speed(0.5).suffix(" mm")).changed() {
                *gy = sy - off(*c2c, bh);
            }
            ui.end_row();
        });
        // "Total Size: %1 %3 x %2 %3  (Count: %4)"
        let tw = *c as f64 * bw + (*c as f64 - 1.0).max(0.0) * *gx;
        let th = *r as f64 * bh + (*r as f64 - 1.0).max(0.0) * *gy;
        ui.label(RichText::new(format!("Total Size: {} mm x {} mm  (Count: {})", fmt_num((tw * 100.0).round() / 100.0), fmt_num((th * 100.0).round() / 100.0), *c * *r)).size(11.5));
        go = ui.button(if editing { "Update array" } else { "Make copies" }).clicked();
    });
    if go && (editing || !app.sel.is_empty()) {
        let (cols, rows, gap_x, gap_y) = app.array;
        let sel = app.sel.clone();
        app.make_array(crate::relations::ArrayKind::Grid { cols, rows, gap_x, gap_y }, &sel);
        open = false;
    }
    if !open {
        app.win_array = false;
        app.array_edit = None;
    }
}

fn offset(ctx: &Context, app: &mut App) {
    if !app.win_offset {
        app.offset_preview.clear();
        return;
    }
    use crate::ops::{OffsetDir, OffsetJoin};
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut go = false;
    let before = app.offset_opts;
    egui::Window::new("Offset Shapes").open(&mut open).default_width(300.0).show(ctx, |ui| {
        let o = &mut app.offset_opts;
        egui::Grid::new("off").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Distance");
            ui.add(egui::DragValue::new(&mut o.dist).speed(crate::ops::OffsetOpts::DIST_STEP * 0.1).range(0.0..=crate::ops::OffsetOpts::MAX_DIST).max_decimals(2).suffix(" mm")).on_hover_text("How far the generated offset curve will be from the source shapes");
            ui.end_row();
            ui.label("Direction");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut o.dir, OffsetDir::Outward, "Outward");
                ui.selectable_value(&mut o.dir, OffsetDir::Inward, "Inward");
                ui.selectable_value(&mut o.dir, OffsetDir::Both, "Both");
            });
            ui.end_row();
            ui.label("Corners");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut o.join, OffsetJoin::Round, "Round");
                ui.selectable_value(&mut o.join, OffsetJoin::Bevel, "Bevel");
                ui.selectable_value(&mut o.join, OffsetJoin::Corner, "Corner");
            });
            ui.end_row();
            ui.label("Outer Shapes Only");
            ui.checkbox(&mut o.outer_only, "").on_hover_text("Only offset the outer shapes - useful when you just want an outline");
            ui.end_row();
            ui.label("Select Resulting Objects");
            ui.checkbox(&mut o.select_result, "").on_hover_text("After offsetting, the resulting shapes will replace the current selection");
            ui.end_row();
            ui.label("Delete Original Objects");
            ui.checkbox(&mut o.replace, "").on_hover_text("After offsetting, the original shapes will be deleted");
            ui.end_row();
            ui.label("Optimize / Simplify Results");
            ui.checkbox(&mut o.optimize, "").on_hover_text("After offsetting, the resulting curves are simplified to reduce the complexity of the output");
            ui.end_row();
            if o.optimize {
                ui.label("Tolerance");
                ui.add(egui::DragValue::new(&mut o.simplify).range(0.0..=crate::ops::OffsetOpts::MAX_TOL).speed(crate::ops::OffsetOpts::TOL_STEP).max_decimals(4).suffix(" mm"));
                ui.end_row();
            }
        });
        if crate::ops::selection_overlaps(&app.doc, &app.sel) {
            ui.label(RichText::new("Intersecting shapes detected: some selected shapes overlap. Offsets are applied to each shape on its own, which may give unexpected results — consider combining them first (Modify → Union).").size(11.5).color(Color32::from_rgb(240, 170, 40)));
        }
        ui.label(RichText::new("The result is previewed in orange on the bed.").size(11.5));
        go = ui.add(egui::Button::new(RichText::new("Offset").strong()).fill(ACCENT.gamma_multiply(0.8))).clicked();
    });
    if before != app.offset_opts || app.offset_preview.is_empty() {
        app.offset_preview = crate::ops::offset_preview(&app.doc, &app.sel, &app.offset_opts);
    }
    if go {
        let before = app.doc.clone();
        let sel = app.sel.clone();
        match crate::ops::offset(&mut app.doc, &sel, &app.offset_opts) {
            Ok(s) => {
                app.undo.push((before, sel));
                app.redo.clear();
                app.touch();
                if app.offset_opts.select_result {
                    app.sel = s;
                }
                app.sel_valid();
                open = false;
            }
            Err(e) => app.error(e, now),
        }
    }
    if !open {
        app.win_offset = false;
        app.offset_preview.clear();
    }
}

fn circular(ctx: &Context, app: &mut App) {
    if !app.win_circular {
        return;
    }
    let mut open = true;
    let mut go = false;
    let editing = app.array_edit.is_some();
    egui::Window::new(if editing { "Edit Circular Array" } else { "Circular Array" }).open(&mut open).default_width(300.0).show(ctx, |ui| {
        let (count, span, rotate, center) = &mut app.circular;
        egui::Grid::new("circ").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Copies (total)");
            ui.add(egui::DragValue::new(count).range(2..=360));
            ui.end_row();
            ui.label("Angle");
            ui.add(egui::DragValue::new(span).range(-360.0..=360.0).suffix("°")).on_hover_text("360° spreads the copies evenly all the way round");
            ui.end_row();
            ui.label("Centre X");
            ui.add(egui::DragValue::new(&mut center.x).speed(0.5).suffix(" mm"));
            ui.end_row();
            ui.label("Centre Y");
            ui.add(egui::DragValue::new(&mut center.y).speed(0.5).suffix(" mm"));
            ui.end_row();
            ui.label("Rotate copies");
            ui.checkbox(rotate, "").on_hover_text("Turn each copy to face the centre, like spokes");
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Centre of bed").clicked() {
                *center = p2(app.settings.device.width / 2.0, app.settings.device.height / 2.0);
            }
            if ui.button("Below selection").clicked() {
                let b = crate::ops::selection_bbox(&app.doc, &app.sel);
                *center = p2(b.center().x, b.min.y - b.height().max(10.0));
            }
            // LightBurn: "Use Last Selected Object Position As Center"
            if ui.add_enabled(app.sel.len() >= 2, egui::Button::new("Last selected shape")).on_hover_text("Use the centre of the last selected shape (it isn't copied)").clicked() {
                if let Some(last) = app.sel.pop() {
                    *center = app.doc.shapes[last].bbox().center();
                }
            }
        });
        ui.label(RichText::new("The centre is marked with a cross on the bed.").size(11.5));
        go = ui.add(egui::Button::new(RichText::new(if editing { "Update array" } else { "Make copies" }).strong()).fill(ACCENT.gamma_multiply(0.8))).clicked();
    });
    if go && (editing || !app.sel.is_empty()) {
        let (count, span, rotate, center) = app.circular;
        let sel = app.sel.clone();
        app.make_array(crate::relations::ArrayKind::Circular { count, span, rotate, center }, &sel);
        open = false;
    }
    if !open {
        app.win_circular = false;
        app.array_edit = None;
    }
}

fn along_curve(ctx: &Context, app: &mut App) {
    if !app.win_curve {
        return;
    }
    let mut open = true;
    let mut go = false;
    let title = if app.curve_dlg.0 { "Align to Path" } else { "Pattern on Path" };
    egui::Window::new(title).open(&mut open).default_width(320.0).show(ctx, |ui| {
        let (align, _, rotate, anchor) = &mut app.curve_dlg;
        let pc = &mut app.path_copy;
        ui.horizontal(|ui| {
            ui.selectable_value(align, false, "Copy along the curve");
            ui.selectable_value(align, true, "Move onto the curve");
        });
        egui::Grid::new("curve").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            if !*align {
                ui.radio_value(&mut pc.by_spacing, false, "Number of Copies");
                ui.add_enabled(!pc.by_spacing, egui::DragValue::new(&mut pc.copies).range(1..=crate::sketch::PATH_COPIES_MAX));
                ui.end_row();
                ui.radio_value(&mut pc.by_spacing, true, "Space Between Copies");
                ui.add_enabled(pc.by_spacing, egui::DragValue::new(&mut pc.spacing).range(crate::sketch::PATH_SPACING_MIN..=crate::sketch::PATH_SPACING_MAX).speed(0.5).suffix(" mm"));
                ui.end_row();
                ui.label("Rotate Copies");
                ui.checkbox(&mut pc.rotate, "").on_hover_text("Turn each copy by how much the path has turned since the start");
                ui.end_row();
                ui.checkbox(&mut pc.scale, "Scale Copies");
                ui.add_enabled(pc.scale, egui::DragValue::new(&mut pc.scale_pct).range(1.0..=2000.0).suffix(" %")).on_hover_text("Size of the last copy; the ones between grow or shrink evenly");
                ui.end_row();
            } else {
                ui.label("Follow the curve");
                ui.checkbox(rotate, "").on_hover_text("Turn each piece to follow the direction of the curve");
                ui.end_row();
                ui.label("Sit on the curve by");
                ui.horizontal(|ui| {
                    ui.selectable_value(anchor, crate::sketch::CurveAnchor::Centre, "Centre");
                    ui.selectable_value(anchor, crate::sketch::CurveAnchor::Bottom, "Bottom");
                });
                ui.end_row();
            }
        });
        let ok = app.sel.len() >= 2 || app.array_edit.is_some();
        ui.label(RichText::new(if ok { "The last selected shape is the curve. Copies start where the curve passes nearest the shapes and keep their offset from it." } else { "Select the shapes, then Shift-click the curve last." }).size(11.5));
        let label = if *align { "Align" } else if app.array_edit.is_some() { "Update array" } else { "Make copies" };
        go = ui.add_enabled(ok, egui::Button::new(RichText::new(label).strong()).fill(ACCENT.gamma_multiply(0.8))).clicked();
    });
    if go {
        let sel = app.sel.clone();
        let (align, _, rotate, anchor) = app.curve_dlg;
        if align {
            app.checkpoint();
            let curve = *sel.last().unwrap();
            crate::sketch::align_to_path(&mut app.doc, &sel, curve, rotate, anchor);
            app.touch();
        } else {
            let curve_id = match app.array_edit.and_then(|ai| app.doc.arrays.get(ai)).map(|a| a.kind.clone()) {
                Some(crate::relations::ArrayKind::Path { curve, .. }) => Some(curve),
                _ => sel.last().and_then(|i| app.doc.shapes.get(*i)).map(|s| s.id),
            };
            if let Some(curve) = curve_id {
                let sources: Vec<usize> = sel.iter().copied().filter(|i| app.doc.shapes.get(*i).map(|s| s.id != curve).unwrap_or(false)).collect();
                app.make_array(crate::relations::ArrayKind::Path { curve, opts: app.path_copy }, &sources);
            }
        }
        open = false;
    }
    if !open {
        app.win_curve = false;
        app.array_edit = None;
    }
}

fn barcode(ctx: &Context, app: &mut App) {
    if !app.win_barcode {
        return;
    }
    use crate::barcode::{encode, outlines, Kind};
    let mut open = true;
    let mut add = false;
    egui::Window::new("Barcode / QR Code").open(&mut open).default_width(360.0).show(ctx, |ui| {
        let st = &mut app.barcode;
        ui.horizontal_wrapped(|ui| {
            for k in Kind::ALL {
                ui.selectable_value(&mut st.kind, k, k.label());
            }
        });
        ui.label(RichText::new(st.kind.hint()).size(11.5));
        ui.add(egui::TextEdit::singleline(&mut st.text).desired_width(f32::INFINITY));
        egui::Grid::new("bc").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut st.width).range(5.0..=500.0).speed(0.5).suffix(" mm"));
            ui.end_row();
            if !st.kind.is_2d() {
                ui.label("Bar height");
                ui.add(egui::DragValue::new(&mut st.height).range(2.0..=300.0).speed(0.5).suffix(" mm"));
                ui.end_row();
                ui.label("Digits/text under bars");
                ui.checkbox(&mut st.human_text, "");
                ui.end_row();
            }
            ui.label("Quiet zone");
            ui.checkbox(&mut st.quiet_zone, "").on_hover_text("Blank margin scanners need around the code");
            ui.end_row();
            ui.label("Invert (dark materials)");
            ui.checkbox(&mut st.invert, "").on_hover_text("Burn the light parts instead, for slate, anodised metal or dark wood");
            ui.end_row();
        });
        match encode(st.kind, &st.text) {
            Ok(m) => {
                let lines = outlines(st, &m);
                let b = lines.iter().fold(BBox::EMPTY, |b, l| b.union(&BBox::from_points(l)));
                let avail = ui.available_width().min(340.0);
                let scale = (avail as f64 / b.width().max(1.0)).min(200.0 / b.height().max(1.0));
                let size = vec2((b.width() * scale) as f32 + 16.0, (b.height() * scale) as f32 + 16.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 4.0, if st.invert { Color32::from_rgb(40, 40, 44) } else { Color32::WHITE });
                for l in &lines {
                    let lb = BBox::from_points(l);
                    let tl = rect.min + vec2(8.0 + ((lb.min.x - b.min.x) * scale) as f32, 8.0 + ((b.max.y - lb.max.y) * scale) as f32);
                    let r = egui::Rect::from_min_size(tl, vec2((lb.width() * scale) as f32, (lb.height() * scale) as f32));
                    p.rect_filled(r, 0.0, if st.invert { Color32::from_rgb(235, 225, 200) } else { Color32::BLACK });
                }
                ui.label(RichText::new(format!("{} modules · {} rectangles · {:.1} × {:.1} mm", if st.kind.is_2d() { format!("{}×{}", m.w, m.h) } else { m.w.to_string() }, lines.len(), b.width(), b.height())).size(11.0));
                add = ui.add(egui::Button::new(RichText::new("Add to project").strong()).fill(ACCENT.gamma_multiply(0.8))).clicked();
            }
            Err(e) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(235, 90, 80)));
            }
        }
    });
    if add {
        let st = app.barcode.clone();
        if let Ok(m) = encode(st.kind, &st.text) {
            let lines = outlines(&st, &m);
            let layer = app.current_layer_for_vectors();
            let c = p2(app.settings.device.width / 2.0, app.settings.device.height / 2.0);
            let code = Shape::new(ShapeKind::Path(PathData::from_polylines(&lines)), Xform::translate(c.x, c.y), layer);
            app.checkpoint();
            if !st.kind.is_2d() && st.human_text {
                let b = code.bbox();
                let txt = Shape::new(
                    ShapeKind::Text { text: st.text.clone(), font: String::new(), height: (st.height * 0.18).max(2.0), style: Default::default(), backup: None },
                    Xform::translate(c.x, b.min.y - st.height * 0.25),
                    layer,
                );
                app.doc.shapes.push(Shape::new(ShapeKind::Group(vec![code, txt]), Xform::IDENTITY, layer));
            } else {
                app.doc.shapes.push(code);
            }
            let fresh = app.doc.cut(layer).is_none();
            app.doc.sync_cuts();
            if fresh {
                app.doc.cut_mut(layer).mode = CutMode::Fill;
            }
            app.sel = vec![app.doc.shapes.len() - 1];
            open = false;
        }
    }
    if !open {
        app.win_barcode = false;
    }
}

fn toasts(ctx: &Context, app: &mut App) {
    let now = ctx.input(|i| i.time);
    app.toasts.retain(|t| t.until > now);
    if app.toasts.is_empty() {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -70.0)).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
        let _ = screen;
        for t in &app.toasts {
            let col = theme::colors(ui.visuals().dark_mode);
            egui::Frame::popup(ui.style()).fill(if t.error { Color32::from_rgb(120, 36, 30) } else { col.panel }).show(ui, |ui| {
                ui.label(RichText::new(&t.text).color(if t.error { Color32::WHITE } else { col.text }));
            });
        }
    });
}

#[allow(dead_code)]
fn _unused(_: P2) {}
