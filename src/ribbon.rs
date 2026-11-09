//! The tabbed ribbon (big labelled icon buttons grouped by task), the pinned quick-access bar,
//! and the tool options bar under it.

use crate::actions::Action;
use crate::app::{App, ArcMode, RibbonTab, Tool};
use crate::icons::{self, Icon, Pal};
use crate::theme::{self, ACCENT};
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui};

/// Most commands shown in the quick-access bar before the overflow button.
pub const QUICK_SLOTS: usize = 12;

pub fn groups(tab: RibbonTab) -> Vec<(&'static str, Vec<Action>)> {
    use Action::*;
    match tab {
        RibbonTab::Home => vec![
            ("Project", vec![New, Open, Save, Import]),
            ("Clipboard", vec![Paste, Copy, Cut, Duplicate, Delete]),
            ("Draw", vec![ToolSelect, ToolLine, ToolRect, ToolEllipse, ToolText]),
            ("Arrange", vec![Group, Ungroup, PageCenter]),
            ("Burn", vec![Preview, LaserFrame, LaserStart, ExportGcode]),
        ],
        RibbonTab::Sketch => vec![
            ("Select", vec![ToolSelect, ToolPan, SelectAll]),
            ("Create", vec![ToolLine, ToolPen, ToolArc, ToolRect, ToolEllipse, ToolPolygon, ToolStar, ToolText, Barcode]),
            ("Edit", vec![ToolNodes, ToolTrim, ToolFillet, JoinPaths, ConvertToPath, BreakApart]),
            ("Points", vec![NodeSmooth, NodeCorner, NodeLine, NodeCurve, NodeInsert, NodeBreak]),
            ("Constrain", vec![ToggleConstruction, ToolRelate, ToolDimension]),
            ("Inspect", vec![ToolMeasure]),
        ],
        RibbonTab::Modify => vec![
            ("Transform", vec![FlipH, FlipV, ToolMirrorLine, RotateCCW, RotateCW]),
            ("Combine", vec![Weld, BoolUnion, BoolSubtract, BoolIntersect, JoinPaths]),
            ("Shape", vec![Offset, GridArray, CircularArray, PathPattern, AlignToPath, EditArray, ConvertToPath, BreakApart]),
            ("Group", vec![Group, Ungroup]),
        ],
        RibbonTab::Arrange => vec![
            ("Align", vec![AlignLeft, AlignVCenter, AlignRight, AlignTop, AlignHCenter, AlignBottom]),
            ("Distribute", vec![DistributeH, DistributeV]),
            ("Position", vec![PageCenter, PushFront, PushBack]),
        ],
        RibbonTab::Image => vec![
            ("Bring in", vec![Import]),
            ("Select", vec![ToolWand, ToolLasso, SelectionToPath, ClearSelection]),
            ("Vectorize", vec![TraceImage, TraceMcp]),
            ("Adjust", vec![ImageInvert, ImageReset]),
        ],
        RibbonTab::Laser => vec![
            ("Machine", vec![LaserConnect, LaserHome]),
            ("Job", vec![Preview, PreviewPlay, LaserFrame, LaserStart, LaserPause, LaserStop]),
            ("Output", vec![ExportGcode, SaveLayerDefaults]),
            ("Extras", vec![VariableText, Camera, Rotary]),
        ],
        RibbonTab::View => vec![
            ("Zoom", vec![ZoomIn, ZoomOut, ZoomPage, ZoomSelection]),
            ("Display", vec![ToggleFilled, ToggleGrid, InvertBed]),
            ("Projects", vec![NewTab, DuplicateTab, CloseTab]),
            ("App", vec![CommandSearch, Shortcuts, Settings, Notes, Help, WelcomeTour]),
        ],
    }
}

pub fn pal(ui: &Ui, active: bool, enabled: bool) -> Pal {
    let dark = ui.visuals().dark_mode;
    let c = theme::colors(dark);
    let mut fg = if active { ACCENT } else { c.icon };
    let mut accent = ACCENT;
    if !enabled {
        fg = fg.gamma_multiply(0.35);
        accent = accent.gamma_multiply(0.35);
    }
    Pal { fg, accent, soft: if enabled { c.accent_soft } else { Color32::TRANSPARENT } }
}

/// Payload carried while dragging a command onto (or within) the quick-access bar.
#[derive(Clone, Copy, Debug)]
pub struct PinPayload {
    pub action: Action,
    pub from: Option<usize>,
}

#[derive(Default)]
pub struct ButtonOut {
    pub clicked: bool,
    pub hovered: bool,
    pub toggle_pin: bool,
}

/// One ribbon button. Can be clicked, or dragged up to the quick-access bar.
pub fn big_button(ui: &mut Ui, app: &App, a: Action, labels: bool, size: f32) -> ButtonOut {
    let enabled = app.enabled(a);
    let active = app.is_active(a);
    let text_w = if labels { ui.painter().layout_no_wrap(a.label().to_string(), FontId::proportional(11.5), Color32::WHITE).size().x } else { 0.0 };
    let longest_word = a.label().split(' ').map(|w| ui.painter().layout_no_wrap(w.to_string(), FontId::proportional(11.5), Color32::WHITE).size().x).fold(0.0, f32::max);
    let w = if labels { (size * 1.9).max(longest_word + 10.0).max(if text_w < size * 2.6 { text_w + 10.0 } else { 0.0 }) } else { size + 10.0 };
    // tall enough for the label however many lines it wraps to, so it never runs into the group name below
    let label_h = if labels { ui.painter().layout(a.label().to_string(), FontId::proportional(11.5), Color32::WHITE, w - 4.0).size().y } else { 0.0 };
    let h = if labels { size + 12.0 + label_h } else { size + 10.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let c = theme::colors(ui.visuals().dark_mode);
    if active {
        ui.painter().rect_filled(rect, 7.0, c.accent_soft);
        ui.painter().rect_stroke(rect, 7.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 7.0, c.hover);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 5.0 + size / 2.0), vec2(size, size));
    icons::paint(ui.painter(), icon_rect, a.icon(), &pal(ui, active, enabled));
    if labels {
        let color = if enabled { c.text } else { c.weak.gamma_multiply(0.6) };
        let galley = ui.painter().layout(a.label().to_string(), FontId::proportional(11.5), color, w - 4.0);
        let pos = pos2(rect.center().x - galley.size().x / 2.0, icon_rect.bottom() + 3.0);
        ui.painter().galley(pos, galley, color);
    }
    resp.dnd_set_drag_payload(PinPayload { action: a, from: None });
    let mut out = ButtonOut { clicked: enabled && resp.clicked(), hovered: resp.hovered(), toggle_pin: false };
    let pinned = app.settings.pinned.iter().any(|p| p == a.id());
    resp.context_menu(|ui| {
        if ui.button(if pinned { "Remove from quick bar" } else { "Pin to quick bar" }).clicked() {
            out.toggle_pin = true;
            ui.close();
        }
    });
    let _ = resp.on_hover_text(format!("{}\n(drag to the top bar to pin it)", app.tooltip(a)));
    out
}

/// A small ribbon button: icon with its label beside it, one line (CAD-style two-row ribbon).
pub fn row_button(ui: &mut Ui, app: &App, a: Action, with_label: bool) -> ButtonOut {
    let enabled = app.enabled(a);
    let active = app.is_active(a);
    let c = theme::colors(ui.visuals().dark_mode);
    let color = if enabled { c.text } else { c.weak.gamma_multiply(0.6) };
    let galley = ui.painter().layout_no_wrap(a.label().to_string(), FontId::proportional(11.5), color);
    let w = if with_label { 26.0 + galley.size().x + 8.0 } else { 28.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click_and_drag());
    if active {
        ui.painter().rect_filled(rect, 5.0, c.accent_soft);
        ui.painter().rect_stroke(rect, 5.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 5.0, c.hover);
    }
    let ir = Rect::from_center_size(pos2(rect.left() + if with_label { 13.0 } else { 14.0 }, rect.center().y), vec2(17.0, 17.0));
    icons::paint(ui.painter(), ir, a.icon(), &pal(ui, active, enabled));
    if with_label {
        ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0), galley, color);
    }
    resp.dnd_set_drag_payload(PinPayload { action: a, from: None });
    let mut out = ButtonOut { clicked: enabled && resp.clicked(), hovered: resp.hovered(), toggle_pin: false };
    let pinned = app.settings.pinned.iter().any(|p| p == a.id());
    resp.context_menu(|ui| {
        if ui.button(if pinned { "Remove from quick bar" } else { "Pin to quick bar" }).clicked() {
            out.toggle_pin = true;
            ui.close();
        }
    });
    let _ = resp.on_hover_text(format!("{}\n(drag to the top bar to pin it)", app.tooltip(a)));
    out
}

pub fn small_icon_button(ui: &mut Ui, app: &App, a: Action, size: f32) -> bool {
    let enabled = app.enabled(a);
    let (rect, resp) = ui.allocate_exact_size(vec2(size + 8.0, size + 8.0), if enabled { Sense::click() } else { Sense::hover() });
    let c = theme::colors(ui.visuals().dark_mode);
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 6.0, c.hover);
    }
    icons::paint(ui.painter(), rect.shrink(4.0), a.icon(), &pal(ui, app.is_active(a), enabled));
    resp.on_hover_text(app.tooltip(a)).clicked()
}

fn toggle_pin(app: &mut App, a: Action) {
    if let Some(i) = app.settings.pinned.iter().position(|p| p == a.id()) {
        app.settings.pinned.remove(i);
    } else {
        app.settings.pinned.push(a.id().to_string());
    }
    let _ = app.settings.save();
}

/// The pinned quick-access bar, centred in the tab row. Drop ribbon buttons here to pin them;
/// drag a pinned button away to remove it, or along the bar to reorder.
fn quick_bar(ui: &mut Ui, app: &mut App, row_y: f32, left_end: f32, right_start: f32, run: &mut Vec<Action>) {
    let ctx = ui.ctx().clone();
    let c = theme::colors(ui.visuals().dark_mode);
    let pinned: Vec<Action> = app.settings.pinned.iter().filter_map(|id| Action::from_id(id)).collect();
    let slot = 30.0;
    let room = ((right_start - left_end - 24.0) / slot).floor().max(1.0) as usize;
    let shown = pinned.len().min(QUICK_SLOTS).min(room.saturating_sub(1).max(1));
    let overflow = pinned.len() > shown;
    let dragging = egui::DragAndDrop::has_payload_of_type::<PinPayload>(&ctx);
    let slots = shown + overflow as usize;
    let bar_w = (slots as f32 * slot + 12.0).max(if dragging || pinned.is_empty() { 150.0 } else { 0.0 });
    let full = ui.max_rect();
    let cx = full.center().x.clamp(left_end + bar_w / 2.0 + 8.0, (right_start - bar_w / 2.0 - 8.0).max(left_end + bar_w / 2.0 + 8.0));
    let bar = Rect::from_center_size(pos2(cx, row_y), vec2(bar_w, 30.0));
    let bar_resp = ui.interact(bar, Id::unique("quick_bar"), Sense::hover());
    let hovering_drop = bar_resp.dnd_hover_payload::<PinPayload>().is_some();
    ui.painter().rect_filled(bar, 8.0, if hovering_drop { c.accent_soft } else { c.panel });
    if dragging {
        ui.painter().rect_stroke(bar, 8.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }
    if pinned.is_empty() {
        ui.painter().text(bar.center(), Align2::CENTER_CENTER, "drag tools here", FontId::proportional(11.0), c.weak);
    }
    let x0 = bar.center().x - (slots as f32 * slot) / 2.0;
    for (i, a) in pinned.iter().take(shown).enumerate() {
        let r = Rect::from_min_size(pos2(x0 + i as f32 * slot, bar.top()), vec2(slot, 30.0)).shrink(2.0);
        let resp = ui.interact(r, Id::unique(("pin", i)), Sense::click_and_drag());
        let enabled = app.enabled(*a);
        if resp.hovered() && enabled {
            ui.painter().rect_filled(r, 5.0, c.hover);
        }
        icons::paint(ui.painter(), r.shrink(4.0), a.icon(), &pal(ui, app.is_active(*a), enabled));
        resp.dnd_set_drag_payload(PinPayload { action: *a, from: Some(i) });
        if resp.clicked() && enabled {
            run.push(*a);
        }
        if resp.hovered() {
            app.hover_action = Some(*a);
        }
        resp.context_menu(|ui| {
            if ui.button("Remove from quick bar").clicked() {
                toggle_pin(app, *a);
                ui.close();
            }
        });
        resp.on_hover_text(app.tooltip(*a));
    }
    // overflow: everything past the visible slots, shown while pointing at »
    if overflow {
        let r = Rect::from_min_size(pos2(x0 + shown as f32 * slot, bar.top()), vec2(slot, 30.0)).shrink(2.0);
        let resp = ui.interact(r, Id::unique("pin_more"), Sense::hover());
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "»", FontId::proportional(16.0), c.text);
        let open_id = Id::unique("pin_more_open");
        let mut open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false) || resp.hovered();
        if open {
            let area = egui::Area::new(Id::unique("pin_more_area")).fixed_pos(pos2(r.left() - 80.0, r.bottom() + 2.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    for a in pinned.iter().skip(shown) {
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            icons::paint(ui.painter(), ir, a.icon(), &pal(ui, false, app.enabled(*a)));
                            if ui.add_enabled(app.enabled(*a), egui::Button::new(a.label()).frame(false)).clicked() {
                                run.push(*a);
                            }
                        });
                    }
                });
            });
            open = resp.hovered() || area.response.contains_pointer() || ctx.pointer_hover_pos().map(|p| area.response.rect.expand(6.0).contains(p)).unwrap_or(false);
        }
        ctx.data_mut(|d| d.insert_temp(open_id, open));
    }
    // drop handling
    if let Some(p) = bar_resp.dnd_release_payload::<PinPayload>() {
        let px = ctx.pointer_interact_pos().map(|q| q.x).unwrap_or(bar.right());
        let mut idx = (((px - x0) / slot).round().max(0.0) as usize).min(app.settings.pinned.len());
        let id = p.action.id().to_string();
        if let Some(from) = p.from {
            if from < app.settings.pinned.len() {
                app.settings.pinned.remove(from);
                if idx > from {
                    idx -= 1;
                }
            }
        } else if let Some(existing) = app.settings.pinned.iter().position(|x| *x == id) {
            app.settings.pinned.remove(existing);
            if idx > existing {
                idx -= 1;
            }
        }
        app.settings.pinned.insert(idx.min(app.settings.pinned.len()), id);
        let _ = app.settings.save();
    } else if ctx.input(|i| i.pointer.any_released()) {
        // a pinned button dropped somewhere else: unpin it
        if let Some(p) = egui::DragAndDrop::payload::<PinPayload>(&ctx) {
            if let Some(from) = p.from {
                if from < app.settings.pinned.len() {
                    app.settings.pinned.remove(from);
                    let _ = app.settings.save();
                }
            }
            egui::DragAndDrop::clear_payload(&ctx);
        }
    }
}

/// Draw the ribbon; returns actions to run.
pub fn ribbon(ui: &mut Ui, app: &mut App) -> Vec<Action> {
    let mut run = Vec::new();
    let c = theme::colors(ui.visuals().dark_mode);
    let narrow = ui.available_width() < 900.0;
    app.hover_action = None;
    // tab row
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
        let logo = logo_texture(ui.ctx());
        ui.painter().image(logo.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        let row_y = r.center().y;
        ui.add_space(4.0);
        if !narrow {
            ui.label(egui::RichText::new("BurnCraft").strong().size(15.0));
            ui.add_space(10.0);
        }
        let mut tabs_rect = Rect::NOTHING;
        for t in RibbonTab::ALL {
            let sel = app.ribbon_tab == t;
            let text = egui::RichText::new(t.label()).size(13.5).color(if sel { ACCENT } else { c.text });
            let resp = ui.add(egui::Button::new(text).frame(false).min_size(vec2(if narrow { 44.0 } else { 62.0 }, 26.0)));
            tabs_rect = tabs_rect.union(resp.rect);
            if sel {
                let r = resp.rect;
                ui.painter().line_segment([pos2(r.left() + 8.0, r.bottom()), pos2(r.right() - 8.0, r.bottom())], Stroke::new(2.5, ACCENT));
            }
            if resp.clicked() {
                app.ribbon_tab = t;
            }
        }
        app.ui_rects.insert("ribbon_tabs", tabs_rect);
        let left_end = ui.cursor().left();
        let right_w = if narrow { 240.0 } else { 140.0 };
        let right_start = ui.max_rect().right() - right_w;
        app.ui_rects.insert("quick_bar", Rect::from_x_y_ranges(left_end..=right_start, (row_y - 15.0)..=(row_y + 15.0)));
        quick_bar(ui, app, row_y, left_end, right_start, &mut run);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut icons_rect = Rect::NOTHING;
            for a in [Action::Settings, Action::Help, Action::Shortcuts, Action::CommandSearch] {
                if small_icon_button(ui, app, a, 18.0) {
                    run.push(a);
                }
                icons_rect = icons_rect.union(ui.min_rect());
            }
            app.ui_rects.insert("ribbon_icons", icons_rect);
            if narrow {
                if ui.selectable_label(app.show_right, "Panels").clicked() {
                    app.show_right = !app.show_right;
                }
                if ui.selectable_label(app.show_left, "Shapes").clicked() {
                    app.show_left = !app.show_left;
                }
            }
        });
    });
    // groups
    let labels = app.settings.ribbon_labels;
    let compact = app.settings.ribbon_compact;
    let size = if narrow { 24.0 } else { 28.0 };
    let mut pin_toggles = Vec::new();
    egui::ScrollArea::horizontal().id_salt("ribbon_scroll").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (gi, (name, acts)) in groups(app.ribbon_tab).into_iter().enumerate() {
                if gi > 0 {
                    ui.add_space(6.0);
                    let (r, _) = ui.allocate_exact_size(vec2(1.0, if labels { size + 52.0 } else { size + 20.0 }), Sense::hover());
                    ui.painter().rect_filled(r.shrink2(vec2(0.0, 6.0)), 0.0, c.border);
                    ui.add_space(6.0);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let mut handle = |o: ButtonOut, a: Action, run: &mut Vec<Action>, app: &mut App| {
                            if o.clicked {
                                run.push(a);
                            }
                            if o.hovered {
                                app.hover_action = Some(a);
                            }
                            if o.toggle_pin {
                                pin_toggles.push(a);
                            }
                        };
                        // node operations, alignment and distribution are well known by their icons: small, no text
                        let icons_only = compact && matches!(name, "Points" | "Align" | "Distribute");
                        if icons_only {
                            let n = acts.len();
                            let per_row = n.div_ceil(2);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add_space(3.0);
                                for row in acts.chunks(per_row.max(1)) {
                                    ui.horizontal(|ui| {
                                        for a in row {
                                            let o = row_button(ui, app, *a, false);
                                            handle(o, *a, &mut run, app);
                                        }
                                    });
                                }
                            });
                        } else if compact && labels {
                            // CAD-style: the group's main command large, the rest small in two rows
                            let mut it = acts.into_iter();
                            if let Some(first) = it.next() {
                                let o = big_button(ui, app, first, labels, size);
                                handle(o, first, &mut run, app);
                            }
                            let rest: Vec<Action> = it.collect();
                            for pair in rest.chunks(2) {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.add_space(3.0);
                                    for a in pair {
                                        let o = row_button(ui, app, *a, true);
                                        handle(o, *a, &mut run, app);
                                    }
                                });
                            }
                        } else {
                            for a in acts {
                                let o = big_button(ui, app, a, labels, size);
                                handle(o, a, &mut run, app);
                            }
                        }
                    });
                    if labels {
                        let (r, _) = ui.allocate_exact_size(vec2(ui.min_rect().width(), 14.0), Sense::hover());
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, name, FontId::proportional(10.5), c.weak);
                    }
                });
            }
            if app.ribbon_tab == RibbonTab::Image {
                if let Some(i) = app.selected_bitmap() {
                    ui.add_space(12.0);
                    image_quick_adjust(ui, app, i);
                }
            }
        });
    });
    for a in pin_toggles {
        toggle_pin(app, a);
    }
    tool_options(ui, app);
    run
}

/// Options for the active tool, in a slim bar under the ribbon (like Photoshop's options bar).
pub fn tool_options(ui: &mut Ui, app: &mut App) {
    let tool = app.tool;
    let has = matches!(tool, Tool::Rect | Tool::Polygon | Tool::Star | Tool::Arc | Tool::Fillet | Tool::Wand | Tool::Lasso | Tool::Nodes | Tool::Line | Tool::Pen | Tool::Trim | Tool::Relate | Tool::MirrorLine);
    if !has {
        return;
    }
    let c = theme::colors(ui.visuals().dark_mode);
    ui.add_space(2.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment([pos2(r.left(), r.top()), pos2(r.right(), r.top())], Stroke::new(1.0, c.border));
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), ir, tool.action().icon(), &pal(ui, true, true));
        ui.label(egui::RichText::new(tool.action().label()).strong());
        ui.separator();
        let o = &mut app.opts;

        match tool {
            Tool::Rect => {
                ui.label("Corner radius");
                ui.add(mm(&mut o.rect_radius, 500.0));
                ui.label(egui::RichText::new("Shift = square · Ctrl = from centre").color(c.weak).size(11.0));
            }
            Tool::Polygon => {
                ui.label("Sides");
                ui.add(egui::DragValue::new(&mut o.poly_sides).range(3..=crate::sketch::POLYGON_MAX_SIDES));
                ui.label("Corner radius");
                ui.add(mm(&mut o.poly_radius, 500.0));
            }
            Tool::Star => {
                ui.label("Points");
                ui.add(egui::DragValue::new(&mut o.star_points).range(3..=crate::sketch::STAR_MAX_POINTS));
                ui.label("Ratio");
                ui.add(egui::DragValue::new(&mut o.star_ratio).range(0.05..=1.0).speed(0.05).max_decimals(2));
                ui.label("Bulge");
                ui.add(egui::DragValue::new(&mut o.star_bulge).range(0.0..=1.0).speed(0.05).max_decimals(2));
                ui.label("Corner radius");
                ui.add(mm(&mut o.star_radius, 200.0));
            }
            Tool::Arc => {
                for m in ArcMode::ALL {
                    ui.selectable_value(&mut o.arc_mode, m, m.label());
                }
                if o.arc_mode == ArcMode::Center {
                    ui.checkbox(&mut o.arc_ccw, "Counter-clockwise");
                }
                let hint = match o.arc_mode {
                    ArcMode::ThreePoint => "Click start, end, then a point the arc passes through",
                    ArcMode::Center => "Click centre, start, then end",
                    ArcMode::Tangent => "Click the end of a line to continue it smoothly, then the arc end",
                };
                ui.label(egui::RichText::new(hint).color(c.weak).size(11.0));
            }
            Tool::Fillet => {
                egui::ComboBox::from_id_salt("fillet_kind").selected_text(o.fillet.kind.label()).show_ui(ui, |ui| {
                    for k in crate::sketch::FilletKind::ALL {
                        ui.selectable_value(&mut o.fillet.kind, k, k.label());
                    }
                });
                use crate::sketch::FilletKind::*;
                match o.fillet.kind {
                    Round => {
                        ui.label("Radius");
                        ui.add(mm(&mut o.fillet.d1, 500.0));
                    }
                    Conic => {
                        ui.label("Size");
                        ui.add(mm(&mut o.fillet.d1, 500.0));
                        ui.label("Shape (ρ)");
                        ui.add(egui::Slider::new(&mut o.fillet.rho, 0.05..=0.95));
                    }
                    ChamferEqual => {
                        ui.label("Distance");
                        ui.add(mm(&mut o.fillet.d1, 500.0));
                    }
                    ChamferTwo => {
                        ui.label("Distance 1");
                        ui.add(mm(&mut o.fillet.d1, 500.0));
                        ui.label("Distance 2");
                        ui.add(mm(&mut o.fillet.d2, 500.0));
                    }
                    ChamferAngle => {
                        ui.label("Distance");
                        ui.add(mm(&mut o.fillet.d1, 500.0));
                        ui.label("Angle");
                        ui.add(egui::DragValue::new(&mut o.fillet.angle_deg).range(1.0..=89.0).suffix("°"));
                    }
                }
            }
            Tool::Wand | Tool::Lasso => {
                for m in crate::wand::SelMode::ALL {
                    ui.selectable_value(&mut o.sel_mode, m, m.label());
                }
                ui.separator();
                if tool == Tool::Wand {
                    ui.label("Tolerance");
                    ui.add(egui::Slider::new(&mut o.wand_tol, 0..=255));
                    ui.checkbox(&mut o.wand_contiguous, "Contiguous");
                } else {
                    ui.checkbox(&mut o.lasso_magnetic, "Magnetic (snap to edges)");
                }
                ui.separator();
                // edge clean-up, applied live to the selection you see and to Selection to Outline
                let before = o.wand_refine;
                ui.spacing_mut().slider_width = 70.0;
                let r = &mut o.wand_refine;
                ui.label("Smooth").on_hover_text("Round off ragged, noisy edges (blur radius in pixels)");
                ui.add(egui::Slider::new(&mut r.smooth, 0.0..=12.0).step_by(1.0).max_decimals(0));
                ui.label("Grow").on_hover_text("Grow (+) or shrink (−) the selection, in pixels");
                ui.add(egui::Slider::new(&mut r.grow, -20..=20));
                ui.label("Specks").on_hover_text("Ignore islands and fill holes smaller than this many pixels across");
                ui.add(egui::Slider::new(&mut r.speckle, 0.0..=40.0).step_by(1.0).max_decimals(0));
                ui.label("Curves").on_hover_text("Smooth the outline into curves (0 = pixel steps)");
                ui.add(egui::Slider::new(&mut r.curves, 0.0..=4.0).step_by(1.0).max_decimals(0));
                if ui.small_button("Reset").on_hover_text("Back to the default clean-up").clicked() {
                    *r = Default::default();
                }
                if ui.small_button("Raw").on_hover_text("Exactly the selected pixels, no clean-up").clicked() {
                    *r = crate::wand::Refine::RAW;
                }
                if o.wand_refine != before {
                    let refine = o.wand_refine;
                    if let Some(sel) = app.image_sel.as_mut() {
                        sel.update_outline(&refine);
                    }
                }
                ui.separator();
                let has_sel = app.image_sel.is_some();
                if ui.add_enabled(has_sel, egui::Button::new("Selection to Outline")).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(Id::unique("run_action"), Action::SelectionToPath));
                }
                if ui.add_enabled(has_sel, egui::Button::new("Clear")).clicked() {
                    app.image_sel = None;
                }
            }
            Tool::Nodes => {
                for a in [Action::NodeSmooth, Action::NodeCorner, Action::NodeLine, Action::NodeCurve, Action::NodeInsert, Action::NodeBreak, Action::Delete] {
                    if small_icon_button(ui, app, a, 16.0) {
                        ui.ctx().data_mut(|d| d.insert_temp(Id::unique("run_action"), a));
                    }
                }
                ui.label(egui::RichText::new(format!("{} selected · Shift-click adds · Alt-drag lasso · scroll over a point to grow", app.canvas.nodes.len())).color(c.weak).size(11.0));
            }
            Tool::Line | Tool::Pen => {
                let on = app.current_layer == crate::model::CONSTRUCTION_LAYER;
                if ui.selectable_label(on, "Construction").on_hover_text("Draw construction lines: used for sketching, snaps, relations and mirror lines, never burned (Q)").clicked() {
                    app.toggle_construction();
                }
                ui.label(egui::RichText::new("Shift locks horizontal / 45° / vertical · smart-guide snaps become relations · Enter or double-click finishes").color(c.weak).size(11.0));
            }
            Tool::Relate => {
                let picks = app.canvas.rel_picks.clone();
                let names: Vec<&str> = picks
                    .iter()
                    .map(|e| match e {
                        crate::relations::Ent::Pt { .. } => "node",
                        crate::relations::Ent::Center { .. } => "centre",
                        crate::relations::Ent::Line { .. } => "line",
                        crate::relations::Ent::Circle { .. } => "circle",
                    })
                    .collect();
                ui.label(if names.is_empty() { "Pick one or two items".to_string() } else { format!("Picked: {}", names.join(" + ")) });
                let kinds = crate::relations::applicable(&picks);
                for k in crate::relations::RelKind::ALL.into_iter().filter(|k| !k.is_dimension()) {
                    if ui.add_enabled(kinds.contains(&k), egui::Button::new(format!("{} {}", k.glyph(), k.label()))).clicked() {
                        app.add_relation(k);
                    }
                }
                if !picks.is_empty() && ui.button("Clear").clicked() {
                    app.canvas.rel_picks.clear();
                }
                ui.label(egui::RichText::new(format!("{} relation(s) in this project", app.doc.relations.len())).color(c.weak).size(11.0));
            }
            Tool::MirrorLine => {
                ui.label(egui::RichText::new(if app.sel.is_empty() { "Select the shapes to mirror first (Select tool), then come back and click the mirror line" } else { "Click a straight edge or construction line to make mirrored copies (or, like LightBurn, select the shapes and then the line last before pressing Mirror Line)" }).color(c.weak).size(11.0));
            }
            Tool::Trim => {
                ui.label(egui::RichText::new("The red piece is what will be removed").color(c.weak).size(11.0));
            }
            _ => {}
        }
    });
}

fn mm(v: &mut f64, max: f64) -> egui::DragValue<'_> {
    egui::DragValue::new(v).range(0.0..=max).speed(0.1).suffix(" mm")
}

fn image_quick_adjust(ui: &mut Ui, app: &mut App, i: usize) {
    let mut changed = false;
    let mut adj = match &app.doc.shapes[i].kind {
        crate::model::ShapeKind::Bitmap { adjust, .. } => adjust.clone(),
        _ => return,
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.label("Brightness");
            changed |= ui.add(egui::Slider::new(&mut adj.brightness, -100.0..=100.0).show_value(false)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Contrast   ");
            changed |= ui.add(egui::Slider::new(&mut adj.contrast, -100.0..=100.0).show_value(false)).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Gamma      ");
            changed |= ui.add(egui::Slider::new(&mut adj.gamma, 0.2..=3.0).show_value(false)).changed();
        });
    });
    if changed {
        if !app.canvas.adjust_drag {
            app.checkpoint();
            app.canvas.adjust_drag = true;
        }
        if let crate::model::ShapeKind::Bitmap { adjust, .. } = &mut app.doc.shapes[i].kind {
            *adjust = adj;
        }
        app.touch();
    } else if !ui.input(|i| i.pointer.any_down()) {
        app.canvas.adjust_drag = false;
    }
}

pub fn icon_label(ui: &mut Ui, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, &pal(ui, false, true));
        ui.label(egui::RichText::new(text).strong());
    });
}

/// The BurnCraft ant icon (packaging/icons), loaded once as a texture.
pub fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = Id::unique("burncraft_logo");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(include_bytes!("../packaging/icons/burncraft-64.png")).map(|i| i.to_rgba8()).ok();
    let color = match img {
        Some(i) => egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()),
        None => egui::ColorImage::new([1, 1], vec![crate::theme::ACCENT_DEEP]),
    };
    let t = ctx.load_texture("burncraft_logo", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Window icon for the title bar / taskbar.
pub fn window_icon() -> Option<egui::IconData> {
    let i = image::load_from_memory(include_bytes!("../packaging/icons/burncraft-256.png")).ok()?.to_rgba8();
    Some(egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() })
}
