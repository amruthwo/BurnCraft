//! Windows for Test & Calibrate, the material library, and saving a material.

use crate::app::App;
use crate::calibrate::{Range, TestKind};
use crate::materials::Material;
use crate::model::*;
use egui::{vec2, Color32, Context, RichText};

/// "Save as material": the settings to keep, and the name, thickness and notes being typed.
#[derive(Clone, Debug)]
pub struct SaveMaterial {
    pub cut: CutSetting,
    pub pulse_width: Option<u16>,
    pub name: String,
    pub thickness: f64,
    pub notes: String,
    /// The layer to link to the new preset afterwards.
    pub link_layer: Option<usize>,
}

pub fn all(ctx: &Context, app: &mut App) {
    test_window(ctx, app);
    shape_tools_window(ctx, app);
    materials_window(ctx, app);
    save_window(ctx, app);
}

fn range_row(ui: &mut egui::Ui, label: &str, r: &mut Range, lo: f64, hi: f64, unit: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(crate::settings::num(&mut r.min).range(lo..=hi).suffix(unit));
        ui.label("to");
        ui.add(crate::settings::num(&mut r.max).range(lo..=hi).suffix(unit));
        ui.label("in");
        ui.add(crate::settings::num(&mut r.steps).range(1..=20));
        ui.label("steps");
    });
    ui.end_row();
}

fn test_window(ctx: &Context, app: &mut App) {
    if !app.win_test {
        return;
    }
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut create = false;
    let mut apply_kerf: Option<(usize, f64)> = None;
    let galvo = app.settings.device.controller == crate::gcode::Controller::EzCad2;
    egui::Window::new("Test & Calibrate").open(&mut open).vscroll(true).max_height(crate::dialogs::fit_h(ctx)).max_width(crate::dialogs::fit_w(ctx)).default_width(480.0).show(ctx, |ui| {
        let p = &mut app.test_params;
        egui::ComboBox::from_id_salt("test_kind").selected_text(p.kind.label()).width(260.0).show_ui(ui, |ui| {
            for k in TestKind::ALL {
                ui.selectable_value(&mut p.kind, k, k.label());
            }
        });
        ui.label(RichText::new(p.kind.hint()).size(11.5));
        if p.kind == TestKind::Color && !galvo {
            ui.colored_label(Color32::from_rgb(230, 150, 60), "This test is for galvo lasers with a MOPA fiber source. Your laser in Settings isn't a galvo.");
        }
        ui.add_space(4.0);
        egui::Grid::new("test_params").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            match p.kind {
                TestKind::PowerSpeed => {
                    ui.label("Burn as");
                    ui.horizontal(|ui| {
                        for m in [CutMode::Fill, CutMode::Line, CutMode::Image] {
                            ui.selectable_value(&mut p.mode, m, m.label());
                        }
                    });
                    ui.end_row();
                    range_row(ui, "Power", &mut p.power, 0.0, 100.0, " %");
                    range_row(ui, "Speed", &mut p.speed, 0.1, 100_000.0, " mm/s");
                }
                TestKind::CutThrough => {
                    range_row(ui, "Speed", &mut p.speed, 0.1, 100_000.0, " mm/s");
                    range_row(ui, "Passes", &mut p.passes, 1.0, 50.0, "");
                    ui.label("Power");
                    ui.add(crate::settings::num(&mut p.fixed_power).range(0.0..=100.0).suffix(" %"));
                    ui.end_row();
                }
                TestKind::Interval => {
                    range_row(ui, "Line interval", &mut p.interval, 0.005, 2.0, " mm");
                }
                TestKind::Color => {
                    range_row(ui, "Frequency", &mut p.frequency, 1.0, 4000.0, " kHz");
                    range_row(ui, "Pulse width", &mut p.pulse, 1.0, 1000.0, " ns");
                }
                TestKind::KerfFit => {
                    ui.label("Material thickness");
                    ui.add(app.settings.units.len(&mut p.thickness, 0.2, 50.0, 0.05));
                    ui.end_row();
                }
                _ => {}
            }
            if p.kind == TestKind::BurnAnt {
                ui.label("");
                ui.label(RichText::new("The card's layers start with ordinary settings, named \"(check)\": set them for your material in the Layers panel.").size(11.0));
                ui.end_row();
            }
            if !matches!(p.kind, TestKind::PowerSpeed | TestKind::CutThrough | TestKind::BurnAnt) {
                ui.label("Speed");
                ui.add(crate::settings::num(&mut p.fixed_speed).range(0.1..=100_000.0).suffix(" mm/s"));
                ui.end_row();
                if p.kind != TestKind::Dither {
                    ui.label("Power");
                } else {
                    ui.label("Max power");
                }
                ui.add(crate::settings::num(&mut p.fixed_power).range(0.0..=100.0).suffix(" %"));
                ui.end_row();
            }
            if matches!(p.kind, TestKind::PowerSpeed | TestKind::CutThrough | TestKind::Interval | TestKind::Color) {
                ui.label("Square size");
                ui.add(app.settings.units.len(&mut p.cell, 2.0, 100.0, 0.5));
                ui.end_row();
                ui.label("Gap");
                ui.add(app.settings.units.len(&mut p.gap, 0.5, 50.0, 0.5));
                ui.end_row();
            }
        });
        if p.kind == TestKind::KerfFit {
            ui.separator();
            ui.label(RichText::new("After cutting the kerf gauge").strong());
            ui.horizontal(|ui| {
                ui.label("Measured gap");
                ui.add(app.settings.units.len(&mut app.kerf_gap, 0.0, 20.0, 0.01));
            });
            let k = crate::calibrate::kerf_from_gap(app.kerf_gap);
            ui.label(format!("Kerf offset to use: {} mm (cut width {} mm)", fmt3(k), fmt3(k * 2.0)));
            ui.horizontal(|ui| {
                let layer = app.current_layer;
                if ui.add_enabled(app.kerf_gap > 0.0, egui::Button::new(format!("Use it on layer {}", layer_name(layer)))).clicked() {
                    apply_kerf = Some((layer, k));
                }
            });
        }
        ui.add_space(6.0);
        let kind = p.kind;
        if kind != TestKind::BurnAnt {
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.test_on_tile, "On a Colony tile").on_hover_text("Put the test on a collectible tile with the ant mark, its serial number, the date and a QR code holding the test's settings, on the smallest standard card it fits");
                if app.test_on_tile {
                    ui.label("dated");
                    ui.add(egui::TextEdit::singleline(&mut app.tile_date).desired_width(90.0));
                }
            });
        }
        ui.label(RichText::new("The test opens in a new tab. Labels burn lightly on their own layer; check the settings there. Cells each have their own settings: select one to see them.").size(11.0).weak());
        if ui.button(RichText::new("Create the test").strong()).clicked() {
            create = true;
        }
    });
    if create {
        let p = app.test_params.clone();
        let mut doc = crate::calibrate::generate(&p, &app.settings.device);
        let mut note = "Test created in a new tab. Check the settings, Frame it, then burn.".to_string();
        if app.test_on_tile && p.kind != TestKind::BurnAnt {
            let serial = app.next_serial(now);
            let date = app.tile_date.clone();
            let (tile, standard) = crate::colony::on_tile(doc, p.kind.label(), &crate::calibrate::params_code(&p, serial, &date), serial, &date);
            doc = tile;
            note = format!("Ant #{serial}: the test is on a Colony tile in a new tab. Check the settings, Frame it, then burn.");
            if !standard {
                note.push_str(" (It's too big for the standard tiles: use fewer steps or smaller squares for one.)");
            }
            crate::calibrate::center_on_bed(&mut doc, &app.settings.device);
        }
        let b = doc.shapes.iter().fold(crate::geom::BBox::EMPTY, |b, s| b.union(&s.bbox()));
        if !crate::gcode::bed_box(&app.settings.device).contains_box(&b) {
            note.push_str(" It's bigger than the work area: parts off the bed won't burn. Use fewer steps or smaller squares.");
        }
        app.new_tab(Some(doc));
        app.toast(note, now);
    }
    if let Some((layer, k)) = apply_kerf {
        if app.doc.cut(layer).is_some() {
            app.checkpoint();
            app.doc.cut_mut(layer).kerf = k;
            app.touch();
            app.toast(format!("Kerf offset {} mm set on layer {}.", fmt3(k), layer_name(layer)), now);
        }
    }
    if !open {
        app.win_test = false;
    }
}

fn fmt3(v: f64) -> String {
    crate::geom::fmt_num((v * 1000.0).round() / 1000.0)
}

fn materials_window(ctx: &Context, app: &mut App) {
    if !app.win_materials {
        return;
    }
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut delete: Option<String> = None;
    let mut tile: Option<Material> = None;
    let mut apply: Option<Material> = None;
    let mut export = false;
    let mut import = false;
    let mut changed = false;
    let laser = app.settings.device.name.clone();
    egui::Window::new("Materials").open(&mut open).vscroll(true).max_height(crate::dialogs::fit_h(ctx)).max_width(crate::dialogs::fit_w(ctx)).default_width(520.0).show(ctx, |ui| {
        ui.label(RichText::new("Saved settings for your materials. Apply one to the current layer, or save a layer's settings (Layers panel) or a test square's (Properties) as a new one.").size(11.5));
        ui.checkbox(&mut app.materials_this_laser, format!("Only for this laser ({laser})"));
        ui.horizontal(|ui| {
            ui.label("Colony tiles:");
            egui::ComboBox::from_id_salt("tile_size").selected_text(app.tile_size.label()).show_ui(ui, |ui| {
                for t in crate::colony::TileSize::ALL {
                    ui.selectable_value(&mut app.tile_size, t, t.label());
                }
            });
            ui.label("dated");
            ui.add(egui::TextEdit::singleline(&mut app.tile_date).desired_width(90.0));
        });
        ui.separator();
        let mut list: Vec<usize> = (0..app.materials.materials.len()).filter(|i| !app.materials_this_laser || app.materials.materials[*i].for_laser(&laser)).collect();
        if list.is_empty() {
            ui.label(RichText::new(if app.materials.materials.is_empty() { "No materials yet." } else { "None for this laser: untick the box above to see them all." }).italics());
        }
        list.sort_by_key(|i| app.materials.materials[*i].title().to_lowercase());
        for i in list {
            let m = &mut app.materials.materials[i];
            ui.push_id(m.id.clone(), |ui| {
                egui::CollapsingHeader::new(RichText::new(m.title()).strong()).show(ui, |ui| {
                    ui.label(RichText::new(m.summary()).size(11.5));
                    ui.label(RichText::new(format!("Laser: {}", if m.laser.is_empty() { "any" } else { &m.laser })).size(11.0).weak());
                    egui::Grid::new("m").num_columns(2).show(ui, |ui| {
                        ui.label("Name");
                        changed |= ui.text_edit_singleline(&mut m.name).changed();
                        ui.end_row();
                        ui.label("Thickness");
                        changed |= ui.add(crate::settings::num(&mut m.thickness).range(0.0..=1000.0).suffix(" mm")).changed();
                        ui.end_row();
                        ui.label("Notes");
                        changed |= ui.text_edit_multiline(&mut m.notes).changed();
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if ui.button(format!("Apply to layer {}", layer_name(app.current_layer))).clicked() {
                            apply = Some(m.clone());
                        }
                        if ui.button("Colony tile").on_hover_text("Make a collectible tile with these settings: the ant mark, a serial number, the material, the laser, the date, a QR code holding the settings and a sample burned with them").clicked() {
                            tile = Some(m.clone());
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(m.id.clone());
                        }
                    });
                });
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Export…").on_hover_text("Save the library as a file to share or keep").clicked() {
                export = true;
            }
            if ui.button("Import…").on_hover_text("Add the materials from a shared file").clicked() {
                import = true;
            }
        });
    });
    if let Some(m) = tile {
        let serial = app.next_serial(now);
        let mut doc = crate::colony::material_tile(&m, app.tile_size, serial, &app.tile_date.clone());
        crate::calibrate::center_on_bed(&mut doc, &app.settings.device);
        app.new_tab(Some(doc));
        app.toast(format!("Ant #{serial}: a Colony tile for {} in a new tab. Check the cut and engrave layers' settings, then burn.", m.title()), now);
    }
    if let Some(id) = delete {
        app.materials.materials.retain(|m| m.id != id);
        changed = true;
    }
    if let Some(m) = apply {
        let layer = app.current_layer;
        app.checkpoint();
        m.apply(app.doc.cut_mut(layer));
        app.touch();
        app.toast(format!("{} applied to layer {}.", m.title(), layer_name(layer)), now);
    }
    if changed {
        app.materials_dirty = true;
    }
    if app.materials_dirty && !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
        app.save_materials(now);
    }
    if export {
        let bytes = app.materials.to_json().into_bytes();
        match app.save_file("BurnCraft materials", &["json"], "burncraft-materials.json", &bytes) {
            Ok(_) => app.toast("Materials exported", now),
            Err(Some(e)) => app.error(format!("Could not export the materials: {e}"), now),
            Err(None) => {}
        }
    }
    if import {
        app.pick_file(crate::app::Pick::Materials, "BurnCraft materials", &["json"], now);
    }
    if !open {
        app.win_materials = false;
        if app.materials_dirty {
            app.save_materials(now);
        }
    }
}

fn save_window(ctx: &Context, app: &mut App) {
    let Some(mut sm) = app.save_material.take() else { return };
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut save = false;
    egui::Window::new("Save as material").open(&mut open).collapsible(false).default_width(380.0).show(ctx, |ui| {
        let m = Material { pulse_width: sm.pulse_width, ..Material::from_cut(&sm.cut, "", 0.0, "") };
        ui.label(RichText::new(m.summary()).strong());
        egui::Grid::new("save_mat").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Material");
            ui.add(egui::TextEdit::singleline(&mut sm.name).hint_text("e.g. Birch plywood"));
            ui.end_row();
            ui.label("Thickness");
            ui.add(app.settings.units.len(&mut sm.thickness, 0.0, 1000.0, 0.1));
            ui.end_row();
            ui.label("Notes");
            ui.add(egui::TextEdit::multiline(&mut sm.notes).desired_rows(2).hint_text("what it's good for, the test it came from…"));
            ui.end_row();
        });
        ui.label(RichText::new(format!("For this laser: {}", app.settings.device.name)).size(11.0).weak());
        if ui.add_enabled(!sm.name.trim().is_empty(), egui::Button::new(RichText::new("Save").strong())).clicked() {
            save = true;
        }
    });
    if save {
        let mut m = Material::from_cut(&sm.cut, &sm.name, sm.thickness, &app.settings.device.name);
        m.pulse_width = sm.pulse_width;
        m.notes = sm.notes.trim().to_string();
        // link the layer only if it still has the settings that were saved
        let same = |c: &CutSetting| format!("{:?}", CutSetting { material: String::new(), ..c.clone() }) == format!("{:?}", CutSetting { material: String::new(), ..sm.cut.clone() });
        if let Some(l) = sm.link_layer.filter(|l| app.doc.cut(*l).is_some_and(same)) {
            app.checkpoint();
            app.doc.cut_mut(l).material = m.id.clone();
            app.touch();
        }
        let title = m.title();
        app.materials.materials.push(m);
        app.save_materials(now);
        app.toast(format!("Saved {title} in Materials"), now);
    } else if open {
        app.save_material = Some(sm);
    }
}

/// Bolden / Thin and Stencil Bridges.
fn shape_tools_window(ctx: &Context, app: &mut App) {
    use crate::stencil::Change;
    if !app.win_shape_tools {
        return;
    }
    let now = ctx.input(|i| i.time);
    let mut open = true;
    let mut go: Option<Change> = None;
    let u = app.settings.units;
    egui::Window::new("Bolden and stencil").open(&mut open).collapsible(false).default_width(380.0).show(ctx, |ui| {
        ui.label(RichText::new("Bolden / Thin").strong());
        ui.horizontal(|ui| {
            ui.label("By");
            ui.add(u.len(&mut app.bolden_amount, -20.0, 20.0, 0.01)).on_hover_text("How much each edge moves out (negative: in)");
            if ui.add_enabled(!app.sel.is_empty(), egui::Button::new("Apply")).clicked() {
                go = Some(Change::Bolden(app.bolden_amount));
            }
        });
        ui.label(RichText::new("Every edge moves out by this much (in, when negative), so strokes grow by twice it.").size(11.0).weak());
        ui.separator();
        ui.label(RichText::new("Stencil bridges").strong());
        ui.horizontal(|ui| {
            ui.label("Bridge width");
            ui.add(u.len(&mut app.bridge_width, 0.05, 50.0, 0.01));
            if ui.add_enabled(!app.sel.is_empty(), egui::Button::new("Apply")).clicked() {
                go = Some(Change::Bridges(app.bridge_width));
            }
        });
        ui.label(RichText::new("For shapes you'll cut out of a sheet as a stencil: a strip is left through every hole (the middle of an O, A, B…) so the piece inside stays joined to the rest.").size(11.0).weak());
        if app.sel.is_empty() {
            ui.colored_label(Color32::from_rgb(230, 150, 60), "Select the shapes or text first.");
        }
    });
    if let Some(change) = go {
        app.checkpoint();
        let sel = app.sel.clone();
        let (n, texts, skipped) = crate::stencil::apply(&mut app.doc, &sel, change);
        app.touch();
        let mut msg = format!("Changed {n} shape(s).");
        if texts > 0 {
            msg.push_str(&format!(" {texts} text(s) became outlines (Undo brings the words back)."));
        }
        if skipped > 0 {
            msg.push_str(&format!(" {skipped} left as they were (open lines, pictures, locked, or thinned away to nothing)."));
        }
        app.toast(msg, now);
    }
    if !open {
        app.win_shape_tools = false;
    }
}
