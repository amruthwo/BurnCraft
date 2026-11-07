//! Side panels: shape list (left), Layers / Properties / Laser (right), palette and status bar (bottom).

use crate::actions::Action;
use crate::app::{fmt_time, App, SideTab, Tool};
use crate::canvas::display_color;
use crate::geom::*;
use crate::icons::Icon;
use crate::laser::{Cmd, Laser};
use crate::model::*;
use crate::ribbon::icon_label;
use crate::theme::{self, ACCENT};
use egui::{vec2, Color32, RichText, Sense, Stroke, StrokeKind, Ui};

fn swatch(ui: &mut Ui, layer: usize, size: f32, selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let [cr, cg, cb] = layer_rgb(layer);
    ui.painter().rect_filled(r, 3.0, Color32::from_rgb(cr, cg, cb));
    if selected {
        ui.painter().rect_stroke(r.expand(2.0), 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Outside);
    } else {
        ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(80)), StrokeKind::Inside);
    }
    resp
}

pub fn left_panel(ui: &mut Ui, app: &mut App) {
    icon_label(ui, Icon::Layers, "Shapes");
    ui.label(RichText::new("Top of the list burns first").size(11.0).color(theme::colors(ui.visuals().dark_mode).weak));
    ui.separator();
    let mut select: Option<(usize, bool)> = None;
    let mut toggle_lock: Option<usize> = None;
    if app.doc.shapes.is_empty() {
        ui.label(RichText::new("Nothing here yet. Draw a shape or import an image.").italics());
    }
    let mut sel_mask = vec![false; app.doc.shapes.len()];
    for &i in &app.sel {
        if let Some(m) = sel_mask.get_mut(i) {
            *m = true;
        }
    }
    // only the rows on screen are built, so a project with thousands of shapes stays quick
    let row_h = ui.spacing().interact_size.y.max(18.0);
    let n = app.doc.shapes.len();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, rows| {
        for i in rows {
            let s = &app.doc.shapes[i];
            let selected = sel_mask[i];
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, display_color(s.layer, ui.visuals().dark_mode));
                let resp = ui.selectable_label(selected, format!("{}  ·  {}", s.display_name(), layer_name(s.layer)));
                if resp.clicked() {
                    select = Some((i, ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl)));
                }
                if icon_button(ui, if s.locked { Icon::Lock } else { Icon::Unlock }, 14.0).on_hover_text("Lock / unlock (locked shapes can't move)").clicked() {
                    toggle_lock = Some(i);
                }
            });
        }
    });
    if let Some((i, add)) = select {
        if add {
            if let Some(p) = app.sel.iter().position(|x| *x == i) {
                app.sel.remove(p);
            } else {
                app.sel.push(i);
            }
        } else {
            app.sel = vec![i];
        }
    }
    if let Some(i) = toggle_lock {
        app.checkpoint();
        app.doc.shapes[i].locked = !app.doc.shapes[i].locked;
    }
}

pub fn right_panel(ui: &mut Ui, app: &mut App, run: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        for (t, label) in [(SideTab::Layers, "Layers"), (SideTab::Properties, "Properties"), (SideTab::Laser, "Laser")] {
            let sel = app.side_tab == t;
            if ui.add(egui::Button::new(RichText::new(label).color(if sel { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() })).selected(sel).min_size(vec2(84.0, 28.0))).clicked() {
                app.side_tab = t;
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.side_tab {
        SideTab::Layers => layers(ui, app),
        SideTab::Properties => properties(ui, app),
        SideTab::Laser => laser(ui, app, run),
    });
}

fn layers(ui: &mut Ui, app: &mut App) {
    // bring the layer list up to date only after something changed, not every frame
    if app.cuts_synced_rev != app.rev {
        app.doc.sync_cuts();
        app.cuts_synced_rev = app.rev;
    }
    if app.doc.cuts.is_empty() {
        ui.label("Layers appear here when you draw. Each colour in the bottom palette is a layer with its own speed and power.");
        return;
    }
    let mut changed = false;
    let mut choose: Option<usize> = None;
    egui::Grid::new("cuts").striped(true).num_columns(5).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        ui.label(RichText::new("Layer").strong());
        ui.label(RichText::new("Mode").strong());
        ui.label(RichText::new("Speed/Power").strong());
        ui.label(RichText::new("Burn").strong()).on_hover_text("Include this layer in the job");
        ui.label(RichText::new("Show").strong());
        ui.end_row();
        for c in app.doc.cuts.iter_mut() {
            let cur = app.current_layer == c.index;
            ui.horizontal(|ui| {
                if swatch(ui, c.index, 16.0, cur).clicked() {
                    choose = Some(c.index);
                }
                if ui.selectable_label(cur, &c.name).clicked() {
                    choose = Some(c.index);
                }
            });
            ui.label(c.mode.label());
            ui.label(format!("{} / {}%", fmt_num(c.speed), fmt_num(c.max_power)));
            changed |= ui.checkbox(&mut c.output, "").changed();
            let mut show = !c.hide;
            if ui.checkbox(&mut show, "").changed() {
                c.hide = !show;
                changed = true;
            }
            ui.end_row();
        }
    });
    if let Some(i) = choose {
        app.current_layer = i;
    }
    ui.add_space(8.0);
    let idx = app.current_layer;
    if app.doc.cut(idx).is_some() {
        ui.separator();
        ui.horizontal(|ui| {
            swatch(ui, idx, 18.0, false);
            ui.label(RichText::new(format!("{} settings", layer_name(idx))).strong());
        });
        let before = app.doc.cut(idx).cloned();
        let c = app.doc.cut_mut(idx);
        egui::Grid::new("cut_edit").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut c.name);
            ui.end_row();
            ui.label("Mode");
            egui::ComboBox::from_id_salt("mode").selected_text(c.mode.label()).show_ui(ui, |ui| {
                for m in CutMode::ALL {
                    ui.selectable_value(&mut c.mode, m, m.label());
                }
            });
            ui.end_row();
            ui.label("Speed");
            ui.add(egui::DragValue::new(&mut c.speed).range(0.1..=2000.0).speed(1.0).suffix(" mm/s"));
            ui.end_row();
            ui.label("Max power");
            ui.add(egui::Slider::new(&mut c.max_power, 0.0..=100.0).suffix(" %"));
            ui.end_row();
            ui.label("Min power");
            ui.add(egui::Slider::new(&mut c.min_power, 0.0..=100.0).suffix(" %"));
            ui.end_row();
            ui.label("Passes");
            ui.add(egui::DragValue::new(&mut c.passes).range(1..=100));
            ui.end_row();
            if matches!(c.mode, CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill | CutMode::Image) {
                ui.label("Line interval");
                ui.add(egui::DragValue::new(&mut c.interval).range(0.01..=5.0).speed(0.005).suffix(" mm"));
                ui.end_row();
                ui.label("");
                ui.label(RichText::new(format!("≈ {:.0} lines per inch", 25.4 / c.interval.max(0.001))).size(11.0));
                ui.end_row();
                ui.label("Bi-directional");
                ui.checkbox(&mut c.bidir, "");
                ui.end_row();
            }
            if matches!(c.mode, CutMode::Fill | CutMode::FillLine | CutMode::OffsetFill) {
                ui.label("Scan angle");
                ui.add(egui::DragValue::new(&mut c.angle).range(-180.0..=180.0).suffix("°"));
                ui.end_row();
                ui.label("Cross-hatch");
                ui.checkbox(&mut c.cross_hatch, "");
                ui.end_row();
                ui.label("Fill shapes individually");
                ui.checkbox(&mut c.fill_individually, "").on_hover_text("Finish each shape before moving on, instead of sweeping across every shape on the layer. Much faster when shapes are far apart.");
                ui.end_row();
                ui.label("Constant Power Mode");
                ui.checkbox(&mut c.force_constant_power, "").on_hover_text("Burn this layer at constant power (M3) instead of dynamic power (M4), as LightBurn's option of the same name. Useful for cutting thick material evenly.");
                ui.end_row();
                if c.mode == CutMode::OffsetFill {
                    ui.label("");
                    ui.label(RichText::new("Offset Fill engraves as a regular fill in this version.").size(11.0));
                    ui.end_row();
                }
            }
            ui.label("Air assist");
            ui.checkbox(&mut c.air_assist, "");
            ui.end_row();
            ui.label("Frequency");
            ui.add(egui::DragValue::new(&mut c.frequency).range(1.0..=1_000_000.0).speed(100.0).suffix(" Hz")).on_hover_text("Pulse frequency, for fiber and PWM sources that use it");
            ui.end_row();
            ui.label("Order");
            ui.add(egui::DragValue::new(&mut c.priority).range(0..=99)).on_hover_text("Lower numbers burn first");
            ui.end_row();
        });
        let after = app.doc.cut(idx).cloned();
        if before.map(|b| format!("{:?}", b)) != after.map(|a| format!("{:?}", a)) {
            changed = true;
        }
    }
    if changed {
        app.touch();
    }
    if !app.warnings.is_empty() {
        ui.separator();
        ui.label(RichText::new("Notes from opening the project").strong());
        for w in &app.warnings {
            ui.label(RichText::new(format!("• {w}")).size(11.5));
        }
    }
}

fn properties(ui: &mut Ui, app: &mut App) {
    app.canvas.font_hover = None;
    let u = app.settings.units;
    let f = u.factor();
    if app.sel.is_empty() {
        ui.label(RichText::new("Select a shape to see its size, position and options.").italics());
        ui.add_space(8.0);
        ui.label(RichText::new("Current tool").strong());
        ui.label(app.tool.hint());
        return;
    }
    let bb = app.selection_box();
    let mut cx = bb.center().x * f;
    let mut cy = bb.center().y * f;
    let mut w = bb.width() * f;
    let mut h = bb.height() * f;
    let (ocx, ocy, ow, oh) = (cx, cy, w, h);
    let mut rot = if app.sel.len() == 1 { app.doc.shapes[app.sel[0]].xform.rotation_deg() } else { 0.0 };
    let orot = rot;
    ui.label(RichText::new(if app.sel.len() == 1 { app.doc.shapes[app.sel[0]].display_name() } else { format!("{} shapes", app.sel.len()) }).strong());
    // part of an array: say so, and offer to change it
    if let Some(ai) = app.sel.iter().find_map(|i| app.doc.shapes.get(*i).and_then(|s| crate::relations::array_of(&app.doc, s.id))) {
        let a = &app.doc.arrays[ai];
        let what = match &a.kind {
            crate::relations::ArrayKind::Grid { cols, rows, .. } => format!("Grid array, {cols} × {rows}"),
            crate::relations::ArrayKind::Circular { count, .. } => format!("Circular array, {count} copies"),
            crate::relations::ArrayKind::Path { .. } => "Pattern on path".to_string(),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(what).color(ACCENT));
            if ui.button("Edit array…").on_hover_text("Change the number of copies, spacing or angle (or double-click any copy)").clicked() {
                app.edit_array();
            }
        });
    }
    let mut any_drag = false;
    let step = if u == crate::settings::Units::Inch { 0.01 } else { 1.0 };
    egui::Grid::new("xf").num_columns(2).spacing(vec2(6.0, 6.0)).show(ui, |ui| {
        let sfx = format!(" {}", u.suffix());
        ui.label("X");
        any_drag |= num_field(ui, "px", &mut cx, step, &sfx, None);
        ui.end_row();
        ui.label("Y");
        any_drag |= num_field(ui, "py", &mut cy, step, &sfx, None);
        ui.end_row();
        ui.label("W");
        any_drag |= num_field(ui, "pw", &mut w, step, &sfx, Some(0.001));
        ui.end_row();
        ui.label("H");
        any_drag |= num_field(ui, "ph", &mut h, step, &sfx, Some(0.001));
        ui.end_row();
        ui.label("Rotate");
        any_drag |= num_field(ui, "pr", &mut rot, 15.0, "°", None);
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut app.lock_aspect, "Lock ratio");
        ui.end_row();
    });
    ui.label(RichText::new("Use − / +, the mouse wheel, or ↑ ↓ (Shift = ×10, Ctrl = ÷10)").size(10.5).color(theme::colors(ui.visuals().dark_mode).weak));
    let moved = cx != ocx || cy != ocy;
    let mut resized = w != ow || h != oh;
    if resized && app.lock_aspect && ow > 0.0 && oh > 0.0 {
        if w != ow {
            h = oh * w / ow;
        } else {
            w = ow * h / oh;
        }
    }
    resized = resized && bb.width() > 0.0 && bb.height() > 0.0;
    if moved || resized || rot != orot {
        if !app.canvas.prop_drag {
            app.checkpoint();
            app.canvas.prop_drag = any_drag;
        }
        let c = bb.center();
        let sel = app.sel.clone();
        if resized {
            let t = Xform::about(c, Xform::scale((w / f) / bb.width().max(1e-9), (h / f) / bb.height().max(1e-9)));
            crate::ops::transform_selection(&mut app.doc, &sel, t);
        }
        if rot != orot {
            crate::ops::transform_selection(&mut app.doc, &sel, Xform::about(c, Xform::rotate((rot - orot).to_radians())));
        }
        if moved {
            crate::ops::transform_selection(&mut app.doc, &sel, Xform::translate(cx / f - c.x, cy / f - c.y));
        }
        app.touch();
    }
    if !any_drag && !ui.input(|i| i.pointer.any_down()) {
        app.canvas.prop_drag = false;
    }
    if app.sel.len() != 1 {
        return;
    }
    ui.separator();
    let i = app.sel[0];
    let mut kind = app.doc.shapes[i].kind.clone();
    let mut changed = false;
    let mut layer = app.doc.shapes[i].layer;
    ui.horizontal(|ui| {
        ui.label("Layer");
        egui::ComboBox::from_id_salt("shape_layer").selected_text(layer_name(layer)).show_ui(ui, |ui| {
            for l in 0..COLOR_LAYERS + 2 {
                ui.horizontal(|ui| {
                    swatch(ui, l, 12.0, false);
                    if ui.selectable_label(layer == l, layer_name(l)).clicked() {
                        layer = l;
                    }
                });
            }
        });
    });
    match &mut kind {
        ShapeKind::Rect { cr, w, h } => {
            ui.horizontal(|ui| {
                ui.label("Corner radius");
                changed |= ui.add(egui::DragValue::new(cr).range(0.0..=w.min(*h) / 2.0).speed(0.2).suffix(" mm")).changed();
            });
        }
        ShapeKind::Polygon { n, .. } => {
            ui.horizontal(|ui| {
                ui.label("Sides");
                changed |= ui.add(egui::DragValue::new(n).range(3..=crate::sketch::POLYGON_MAX_SIDES)).changed();
            });
        }
        ShapeKind::Text { text, height, backup, font, style } => {
            ui.label("Text");
            let resp = ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY));
            if app.canvas.focus_text {
                resp.request_focus();
                app.canvas.focus_text = false;
            }
            let mut redraw = resp.changed();
            let mut spec = crate::fonts::FontSpec::parse(font);
            let before = spec.clone();
            ui.horizontal(|ui| {
                ui.label("Font");
                if ui.button(format!("{}  ▾", spec.label())).on_hover_text("Choose from the fonts installed on this computer").clicked() {
                    app.canvas.font_list_open = !app.canvas.font_list_open;
                }
                ui.toggle_value(&mut spec.bold, RichText::new(" B ").strong()).on_hover_text("Bold");
                ui.toggle_value(&mut spec.italic, RichText::new(" I ").italics()).on_hover_text("Italic");
            });
            if app.canvas.font_list_open {
                ui.add(egui::TextEdit::singleline(&mut app.canvas.font_filter).hint_text("Search fonts…").desired_width(f32::INFINITY));
                let filter = app.canvas.font_filter.to_lowercase();
                let fams = crate::fonts::families();
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("fontlist").max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                        let mut entries: Vec<(&str, &str)> = vec![("", "Default (built-in)")];
                        entries.extend(fams.iter().filter(|f| filter.is_empty() || f.to_lowercase().contains(&filter)).map(|f| (f.as_str(), f.as_str())));
                        for (fam, label) in entries {
                            let r = ui.selectable_label(spec.family == fam, label);
                            if r.hovered() {
                                let hs = crate::fonts::FontSpec { family: fam.to_string(), ..spec.clone() };
                                app.canvas.font_hover = Some(hs.to_qt());
                            }
                            if r.clicked() {
                                spec.family = fam.to_string();
                                app.canvas.font_list_open = false;
                            }
                        }
                        if fams.is_empty() {
                            ui.label("No installed fonts were found.");
                        }
                    });
                });
                ui.label(RichText::new("Point at a font to preview it on the bed.").size(11.0));
            }
            if spec != before {
                *font = spec.to_qt();
                redraw = true;
            }
            egui::Grid::new("textopts").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                ui.label("Height");
                redraw |= ui.add(egui::DragValue::new(height).range(0.5..=500.0).speed(0.2).suffix(" mm")).changed();
                ui.end_row();
                ui.label("Letter spacing");
                redraw |= ui.add(egui::DragValue::new(&mut style.letter_spacing).range(-50.0..=200.0).speed(0.1).suffix(" mm")).changed();
                ui.end_row();
                ui.label("Case");
                egui::ComboBox::from_id_salt("textcase").selected_text(style.case.label()).show_ui(ui, |ui| {
                    for c in crate::model::TextCase::ALL {
                        redraw |= ui.selectable_value(&mut style.case, c, c.label()).changed();
                    }
                });
                ui.end_row();
                ui.label("Weld letters");
                redraw |= ui.checkbox(&mut style.weld, "").on_hover_text("Merge overlapping letters into one outline so the laser doesn't burn the overlaps twice").changed();
                ui.end_row();
                ui.label("Variable text");
                ui.horizontal(|ui| {
                    redraw |= ui
                        .checkbox(&mut style.variable, "")
                        .on_hover_text("Fill in %0, %1… / {Column} from a .csv or .txt row, {serial}, {date} and {time}")
                        .changed();
                    if ui.small_button("Rows…").on_hover_text("Load the rows and choose the current one").clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("run_action"), crate::actions::Action::VariableText));
                    }
                });
                ui.end_row();
            });
            if style.variable {
                ui.label(RichText::new(format!("Burns as: {}", crate::vartext::resolve(text))).size(11.0));
            }
            if redraw {
                *backup = None;
                changed = true;
            }
            if backup.is_some() {
                ui.label(RichText::new("Showing the outline saved in the project. Change any option to redraw it with the chosen font.").size(11.0));
            }
        }
        ShapeKind::Bitmap { adjust, data, w, h } => {
            ui.label(RichText::new(format!("Image {} × {} px  ·  {:.0} DPI", data.width, data.height, data.width as f64 / (*w / 25.4).max(1e-6))).size(11.5));
            let _ = h;
            egui::Grid::new("img").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                ui.label("Brightness");
                changed |= ui.add(egui::Slider::new(&mut adjust.brightness, -100.0..=100.0)).changed();
                ui.end_row();
                ui.label("Contrast");
                changed |= ui.add(egui::Slider::new(&mut adjust.contrast, -100.0..=100.0)).changed();
                ui.end_row();
                ui.label("Gamma");
                changed |= ui.add(egui::Slider::new(&mut adjust.gamma, 0.2..=3.0)).changed();
                ui.end_row();
                ui.label("Invert");
                changed |= ui.checkbox(&mut adjust.invert, "").changed();
                ui.end_row();
                ui.label("Dithering");
                egui::ComboBox::from_id_salt("dither").selected_text(adjust.dither.label()).show_ui(ui, |ui| {
                    for d in Dither::ALL {
                        changed |= ui.selectable_value(&mut adjust.dither, d, d.label()).changed();
                    }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Trace Image…").clicked() {
                    app.trace.target = Some(app.doc.shapes[i].id);
                    app.trace.preview_for = None;
                    app.trace.layer = app.current_layer_for_vectors();
                    app.win_trace = true;
                }
                if ui.button("AI Outline (MCP)…").clicked() {
                    app.trace.target = Some(app.doc.shapes[i].id);
                    app.win_mcp = true;
                }
            });
        }
        ShapeKind::Group(ch) => {
            ui.label(format!("{} shapes inside. Ungroup to edit them one by one.", ch.len()));
        }
        ShapeKind::Path(pd) => {
            ui.label(format!("{} points, {} segments", pd.verts.len(), pd.prims.len()));
            if ui.button("Edit points").clicked() {
                app.tool = Tool::Nodes;
            }
        }
        _ => {}
    }
    if changed {
        if !app.canvas.prop_drag {
            app.checkpoint();
            app.canvas.prop_drag = true;
        }
        app.doc.shapes[i].kind = kind;
        app.touch();
    }
    if layer != app.doc.shapes[i].layer {
        app.checkpoint();
        app.doc.shapes[i].set_layer(layer);
        app.doc.sync_cuts();
    }
}

fn laser(ui: &mut Ui, app: &mut App, run: &mut Vec<Action>) {
    app.laser.poll();
    let col = theme::colors(ui.visuals().dark_mode);
    // job summary / preview
    if app.preview_on {
        if app.job.is_none() {
            app.job = Some(crate::gcode::build_job(&app.doc, &app.settings.device, None));
        }
        if let Some(job) = &app.job {
            ui.label(RichText::new("Preview").strong());
            // same summary as LightBurn's preview window
            ui.label(format!("Cut distance: {:.0} mm (~{})", job.cut_length, fmt_time(job.cut_seconds)));
            ui.label(format!("Rapid moves: {:.0} mm (~{})", job.travel_length, fmt_time(job.travel_seconds)));
            ui.label(RichText::new(format!("Total time estimated: {}", fmt_time(job.seconds))).strong());
            if job.skipped_outside > 0 {
                ui.label(RichText::new(format!("{} shape(s) outside the bed are left out of the job.", job.skipped_outside)).color(Color32::from_rgb(235, 90, 80)));
            }
            ui.horizontal(|ui| {
                let label = if app.preview_playing { "Pause" } else { "Play" };
                if ui.add(egui::Button::new(RichText::new(label).strong()).min_size(vec2(64.0, 26.0))).on_hover_text("Simulate the laser moving along the path").clicked() {
                    if !app.preview_playing && app.preview_t >= 0.999 {
                        app.preview_t = 0.0;
                    }
                    app.preview_playing = !app.preview_playing;
                }
                ui.add(egui::Slider::new(&mut app.preview_t, 0.0..=1.0).show_value(false));
            });
            ui.horizontal(|ui| {
                ui.label("Speed");
                for s in [1.0f32, 10.0, 50.0, 200.0] {
                    ui.selectable_value(&mut app.preview_speed, s, format!("{}×", s));
                }
            });
            ui.label(RichText::new(format!("{} of {}", fmt_time(job.seconds * app.preview_t as f64), fmt_time(job.seconds))).size(11.0));
            ui.horizontal(|ui| {
                if ui.button("Close preview").clicked() {
                    app.preview_on = false;
                }
                if ui.button("Save G-code…").clicked() {
                    run.push(Action::ExportGcode);
                }
            });
        }
        ui.separator();
    }
    ui.label(RichText::new("Laser").strong());
    let dev = &mut app.settings.device;
    if dev.controller == crate::gcode::Controller::Ruida {
        ui.horizontal(|ui| {
            ui.label("Ruida IP");
            ui.add(egui::TextEdit::singleline(&mut dev.ip).hint_text("192.168.1.100").desired_width(130.0));
        });
        ui.label(RichText::new("Start sends the job over the network (UDP 50200). Save G-code makes an .rd file for a USB stick.").size(11.0).color(col.weak));
        return;
    }
    if dev.controller == crate::gcode::Controller::EzCad2 {
        ui.label(format!("{} galvo, {} mm lens, over USB", dev.galvo.source.label(), crate::geom::fmt_num(dev.galvo.lens)));
        #[cfg(not(usb_lasers))]
        ui.colored_label(egui::Color32::from_rgb(230, 150, 60), "USB lasers aren't supported on this system yet.");
        #[cfg(usb_lasers)]
        {
            if let Some(r) = &app.usb_job {
                let p = r.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(p).text(if p < 1.0 { format!("Sending {:.0}%", p * 100.0) } else { "Working…".to_string() }));
            }
            ui.horizontal(|ui| {
                for (a, label, tip) in [
                    (Action::LaserFrame, "Frame", "Trace the job's outline with the red pointer"),
                    (Action::LaserStart, "Start", "Mark the job"),
                    (Action::LaserStop, "Stop", "Stop marking or framing"),
                ] {
                    if ui.button(label).on_hover_text(tip).clicked() {
                        run.push(a);
                    }
                }
            });
        }
        ui.label(RichText::new("The work area is the lens field. Layer speed, power and frequency (Hz) are used. Set the source, lens, orientation and correction file in Settings. Not yet tested on a real machine: start at low power.").size(11.0).color(col.weak));
        return;
    }
    if dev.controller == crate::gcode::Controller::K40Nano {
        ui.label(format!("K40 with a {} board, over USB", dev.board));
        #[cfg(not(usb_lasers))]
        ui.colored_label(egui::Color32::from_rgb(230, 150, 60), "USB lasers aren't supported on this system yet; Save G-code still makes .egv files.");
        #[cfg(usb_lasers)]
        {
            if let Some(r) = &app.usb_job {
                let p = r.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
                ui.add(egui::ProgressBar::new(p).text(if p < 1.0 { format!("Sending {:.0}%", p * 100.0) } else { "Burning…".to_string() }));
            }
            ui.horizontal(|ui| {
                for (a, label) in [(Action::LaserHome, "Home"), (Action::LaserFrame, "Frame"), (Action::LaserStart, "Start"), (Action::LaserStop, "Stop")] {
                    if ui.button(label).clicked() {
                        run.push(a);
                    }
                }
                if ui.button("Unlock rail").on_hover_text("Let the head be moved by hand").clicked() {
                    app.usb_job = Some(crate::k40::usb::command("IS2P", "Rail unlocked: the head can be moved by hand".into()));
                }
            });
        }
        ui.label(RichText::new("Home before the first job: jobs start from the back-left corner. Set the power with the knob on the laser; layer speeds are used, layer power isn't. Save G-code makes an .egv file.").size(11.0).color(col.weak));
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Port");
        egui::ComboBox::from_id_salt("port").selected_text(if dev.port.is_empty() { "choose…".to_string() } else { dev.port.clone() }).show_ui(ui, |ui| {
            if app.ports.is_empty() {
                ui.label("No serial ports found");
            }
            for p in &app.ports {
                ui.selectable_value(&mut dev.port, p.clone(), p);
            }
        });
        if icon_button(ui, Icon::Refresh, 16.0).on_hover_text("Look for ports again").clicked() {
            app.ports = Laser::ports();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Baud");
        egui::ComboBox::from_id_salt("baud").selected_text(dev.baud.to_string()).show_ui(ui, |ui| {
            for b in [9600, 57600, 115200, 230400, 250000] {
                ui.selectable_value(&mut dev.baud, b, b.to_string());
            }
        });
        let label = if app.laser.connected { "Disconnect" } else { "Connect" };
        if ui.add_enabled(!dev.port.is_empty() || app.laser.connected, egui::Button::new(label)).clicked() {
            if app.laser.connected {
                app.laser.send(Cmd::Disconnect);
            } else {
                app.laser.send(Cmd::Connect { port: dev.port.clone(), baud: dev.baud });
            }
        }
    });
    let state_color = if app.laser.connected { Color32::from_rgb(70, 200, 110) } else { col.weak };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, state_color);
        ui.label(RichText::new(format!("{}   X {:.2}  Y {:.2}", app.laser.state, app.laser.pos.0, app.laser.pos.1)).color(state_color));
    });
    let (sent, total) = app.laser.progress;
    let now = ui.input(|i| i.time);
    if let Some((est, elapsed, remaining, frac)) = crate::app::job_timing(app, now) {
        let done = total > 0 && sent >= total;
        ui.add(egui::ProgressBar::new(frac as f32).text(format!("{:.0}%", frac * 100.0)));
        egui::Grid::new("job_time").num_columns(2).spacing(vec2(10.0, 2.0)).show(ui, |ui| {
            ui.label("Estimated");
            ui.label(fmt_time(est));
            ui.end_row();
            ui.label("Elapsed time");
            ui.label(fmt_time(elapsed));
            ui.end_row();
            ui.label("Remaining");
            ui.label(if done { "finishing…".to_string() } else { format!("about {}", fmt_time(remaining)) });
            ui.end_row();
        });
        if done && app.laser.state.starts_with("Idle") {
            app.job_finished_after = Some(elapsed);
            app.job_started = None;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    } else if let Some(t) = app.job_finished_after {
        ui.label(RichText::new(format!("Last job finished in {}", fmt_time(t))).color(Color32::from_rgb(70, 200, 110)));
    } else if total > 0 {
        ui.add(egui::ProgressBar::new(sent as f32 / total as f32).text(format!("{sent} / {total} lines")));
    }
    ui.add_space(4.0);
    // jog pad
    ui.label(RichText::new("Move").strong());
    let step = app.jog_step;
    let mut jog: Option<(f64, f64)> = None;
    egui::Grid::new("jog").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        ui.label("");
        if jog_button(ui, Icon::ArrowUp).clicked() {
            jog = Some((0.0, step));
        }
        ui.label("");
        ui.end_row();
        if jog_button(ui, Icon::ArrowLeft).clicked() {
            jog = Some((-step, 0.0));
        }
        if jog_button(ui, Icon::Home).on_hover_text("Home ($H)").clicked() {
            run.push(Action::LaserHome);
        }
        if jog_button(ui, Icon::ArrowRight).clicked() {
            jog = Some((step, 0.0));
        }
        ui.end_row();
        ui.label("");
        if jog_button(ui, Icon::ArrowDown).clicked() {
            jog = Some((0.0, -step));
        }
        ui.label("");
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.label("Step");
        for s in [0.1, 1.0, 10.0, 50.0] {
            ui.selectable_value(&mut app.jog_step, s, fmt_num(s));
        }
        ui.label("mm");
    });
    if let Some((dx, dy)) = jog {
        app.laser.send(Cmd::Send(vec![format!("$J=G91 G21 X{dx:.3} Y{dy:.3} F3000")]));
    }
    ui.horizontal(|ui| {
        for a in [Action::LaserFrame, Action::LaserStart, Action::LaserPause, Action::LaserStop] {
            if ui.add_enabled(app.enabled(a), egui::Button::new(a.label())).on_hover_text(app.tooltip(a)).clicked() {
                run.push(a);
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Set origin here").on_hover_text("G92 X0 Y0").clicked() {
            app.laser.send(Cmd::Send(vec!["G92 X0 Y0".into()]));
        }
        if ui.button("Unlock").on_hover_text("$X — clear an alarm").clicked() {
            app.laser.send(Cmd::Send(vec!["$X".into()]));
        }
    });
    ui.separator();
    ui.label(RichText::new("Console").strong());
    egui::ScrollArea::vertical().id_salt("console").max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
        for l in &app.laser.console {
            ui.label(RichText::new(l).monospace().size(11.0));
        }
    });
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut app.console_input).hint_text("Type a command, e.g. $$").desired_width(180.0));
        if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button("Send").clicked() {
            let cmd = std::mem::take(&mut app.console_input);
            if !cmd.trim().is_empty() {
                app.laser.log(format!("> {cmd}"));
                app.laser.send(Cmd::Send(vec![cmd]));
            }
        }
    });
}

pub fn bottom_bar(ui: &mut Ui, app: &mut App) {
    let col = theme::colors(ui.visuals().dark_mode);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let size = if ui.available_width() < 900.0 { 16.0 } else { 20.0 };
        egui::ScrollArea::horizontal().id_salt("palette").max_width(ui.available_width() - 460.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for l in 0..COLOR_LAYERS + 2 {
                    let resp = swatch(ui, l, size, app.current_layer == l);
                    if l >= COLOR_LAYERS {
                        ui.painter().text(resp.rect.center(), egui::Align2::CENTER_CENTER, layer_name(l), egui::FontId::proportional(9.0), Color32::BLACK);
                    }
                    let resp = resp.on_hover_text(format!("{} — click to put the selection on this layer", layer_name(l)));
                    if resp.clicked() {
                        app.assign_layer(l);
                    }
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let u = app.settings.units;
            if let Some(w) = app.canvas.hover_world {
                ui.label(RichText::new(format!("X {:>8.2}  Y {:>8.2} {}", w.x * u.factor(), w.y * u.factor(), u.suffix())).monospace().color(col.weak));
            }
            ui.label(RichText::new(format!("{:.0}%", app.view.zoom / 3.78 * 100.0)).color(col.weak)).on_hover_text("Zoom (100% ≈ real size on a 96 DPI screen)");
            let mut sg = app.settings.snap_grid;
            if ui.toggle_value(&mut sg, "Grid snap").changed() {
                app.settings.snap_grid = sg;
            }
            let mut sp = app.settings.snap_points;
            if ui.toggle_value(&mut sp, "Smart guides").on_hover_text("Snap to endpoints, midpoints and centres; line up horizontally/vertically with points you rest on; parallel/perpendicular to lines you rest on").changed() {
                app.settings.snap_points = sp;
            }
        });
    });
    ui.horizontal(|ui| {
        let now = ui.input(|i| i.time);
        if let Some((_, elapsed, remaining, frac)) = crate::app::job_timing(app, now) {
            ui.label(RichText::new(format!("Burning {:.0}% · {} elapsed · about {} left", frac * 100.0, fmt_time(elapsed), fmt_time(remaining))).size(11.5).color(ACCENT));
            ui.separator();
        }
        ui.label(RichText::new(app.tool.hint()).size(11.5).color(col.weak));
    });
}

/// Project tabs along the bottom edge (like Onshape). Returns a tab to close, if asked.
pub fn project_tabs(ui: &mut Ui, app: &mut App) -> Option<usize> {
    let col = theme::colors(ui.visuals().dark_mode);
    let mut switch = None;
    let mut close = None;
    let mut dup = None;
    let mut new_tab = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..app.tabs.len() {
            let (name, dirty) = app.tab_title(i);
            let active = i == app.active_tab;
            let text = format!("{}{}", if dirty { "• " } else { "" }, name);
            let fill = if active { col.bed } else { col.panel };
            egui::Frame::NONE.fill(fill).corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 6, se: 6 }).stroke(Stroke::new(1.0, if active { ACCENT } else { col.border })).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    // renaming happens right in the tab: Rename… (right-click) or double-click turns the name into a
                    // text box; Enter or clicking elsewhere keeps it, Escape cancels
                    if let Some((j, buf)) = app.tab_rename.as_mut().filter(|(j, _)| *j == i) {
                        let _ = j;
                        let te = ui.add(egui::TextEdit::singleline(buf).desired_width(140.0).font(egui::TextStyle::Body));
                        if !te.has_focus() && !te.lost_focus() {
                            te.request_focus();
                        }
                        let esc = ui.input(|k| k.key_pressed(egui::Key::Escape));
                        if esc {
                            app.tab_rename = None;
                        } else if te.lost_focus() {
                            let name = buf.clone();
                            app.rename_tab(i, &name);
                            app.tab_rename = None;
                        }
                        return;
                    }
                    let r = ui.add(egui::Label::new(RichText::new(text).color(if active { col.text } else { col.weak })).sense(Sense::click())).on_hover_text("Double-click or right-click to rename");
                    if r.clicked() {
                        switch = Some(i);
                    }
                    if r.double_clicked() {
                        app.tab_rename = Some((i, app.tab_title(i).0));
                    }
                    // the same menu as LightBurn's page tabs: Insert, Duplicate, Rename…, —, Remove
                    r.context_menu(|ui| {
                        if ui.button("New Tab").clicked() {
                            new_tab = true;
                            ui.close();
                        }
                        if ui.button("Duplicate Tab").clicked() {
                            dup = Some(i);
                            ui.close();
                        }
                        if ui.button("Rename Tab…").on_hover_text("Leave blank for the file name / default name").clicked() {
                            app.tab_rename = Some((i, app.tab_title(i).0));
                            ui.close();
                        }
                        ui.separator();
                        if ui.add_enabled(app.tabs.len() > 1, egui::Button::new("Close Tab")).clicked() {
                            close = Some(i);
                            ui.close();
                        }
                    });
                    if app.tabs.len() > 1 && ui.add(egui::Button::new(RichText::new("×").size(12.0)).frame(false)).on_hover_text("Close tab").clicked() {
                        close = Some(i);
                    }
                });
            });
        }
        if ui.add(egui::Button::new(RichText::new("+").size(14.0)).frame(false)).on_hover_text("New project tab (copy and paste works between tabs)").clicked() {
            app.new_tab(None);
        }
    });
    if new_tab {
        app.new_tab(None);
    }
    if let Some(i) = switch {
        app.switch_tab(i);
    }
    if let Some(i) = dup {
        app.switch_tab(i);
        let d = app.doc.clone();
        app.new_tab(Some(d));
    }
    close
}

fn icon_button(ui: &mut Ui, icon: Icon, size: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size + 6.0, size + 6.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, theme::colors(ui.visuals().dark_mode).hover);
    }
    crate::icons::paint(ui.painter(), r.shrink(3.0), icon, &crate::ribbon::pal(ui, false, true));
    resp
}

fn jog_button(ui: &mut Ui, icon: Icon) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(44.0, 34.0), Sense::click());
    let c = theme::colors(ui.visuals().dark_mode);
    ui.painter().rect_filled(r, 6.0, if resp.hovered() { c.hover } else { ui.visuals().widgets.inactive.bg_fill });
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, &crate::ribbon::pal(ui, false, true));
    resp
}

/// A number field with − / + buttons; the mouse wheel over it and ↑/↓ while it has focus also step it.
/// Shift = 10× steps, Ctrl = ÷10. Returns true while it's being changed continuously (drag/scroll).
fn num_field(ui: &mut Ui, id: &str, v: &mut f64, step: f64, suffix: &str, min: Option<f64>) -> bool {
    let mut busy = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let mods = ui.input(|i| i.modifiers);
        let k = if mods.shift { 10.0 } else if mods.ctrl || mods.command { 0.1 } else { 1.0 };
        let st = step * k;
        let lo = min.unwrap_or(f64::NEG_INFINITY);
        if ui.add(egui::Button::new("−").min_size(vec2(22.0, 22.0))).clicked() {
            *v = (*v - st).max(lo);
        }
        let resp = ui.add(egui::DragValue::new(v).speed(st * 0.5).max_decimals(3).suffix(suffix).range(lo..=f64::INFINITY)).on_hover_text("Scroll or use ↑/↓ to adjust");
        ui.push_id(id, |_| {});
        if ui.add(egui::Button::new("+").min_size(vec2(22.0, 22.0))).clicked() {
            *v += st;
        }
        if resp.hovered() {
            let sc = ui.input(|i| i.smooth_scroll_delta.y);
            if sc.abs() > 0.0 {
                // one step per notch of the wheel (smooth scrolling arrives spread over frames)
                let acc_id = egui::Id::unique(("num_scroll", id));
                let acc = ui.ctx().data_mut(|d| {
                    let a = d.get_temp_mut_or_default::<f32>(acc_id);
                    *a += sc;
                    let out = *a;
                    if out.abs() >= 30.0 {
                        *a = 0.0;
                    }
                    out
                });
                if acc.abs() >= 30.0 {
                    *v = (*v + st * acc.signum() as f64).max(lo);
                }
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
                busy = true;
            }
        }
        if resp.has_focus() {
            let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
            if up {
                *v += st;
            }
            if down {
                *v = (*v - st).max(lo);
            }
        }
        busy |= resp.dragged();
    });
    busy
}
