//! The Welcome Tour: a short walk through the window that dims everything except the part being explained.
//! It starts by itself the first time BurnCraft runs and can be replayed from Settings or the View tab.

use crate::app::{App, RibbonTab, SideTab};
use crate::theme::{self, ACCENT};
use egui::{pos2, vec2, Align2, Color32, Context, Id, Key, Order, Rect, RichText, Sense, Stroke, StrokeKind};

struct Step {
    title: &'static str,
    body: &'static str,
    /// Parts of the window to highlight (names recorded in `App::ui_rects`); none = a card in the middle.
    target: &'static [&'static str],
    tab: Option<RibbonTab>,
    side: Option<SideTab>,
}

const STEPS: &[Step] = &[
    Step {
        title: "Welcome to BurnCraft",
        body: "This quick tour shows where everything is — it takes about a minute.\n\nUse Next (or the Right arrow key) to move on, Left arrow to go back, and Esc to skip. You can replay it any time from Settings.",
        target: &[],
        tab: Some(RibbonTab::Home),
        side: None,
    },
    Step {
        title: "Ribbon tabs",
        body: "Commands are grouped by task: Home for everyday drawing, Sketch for CAD-style tools, Modify, Arrange, Image, Laser and View.",
        target: &["ribbon_tabs"],
        tab: Some(RibbonTab::Home),
        side: None,
    },
    Step {
        title: "Ribbon buttons",
        body: "Each tab's commands. Hover a button to see what it does and its shortcut. Right-click a button to pin it to the quick-access bar.",
        target: &["ribbon_groups"],
        tab: Some(RibbonTab::Home),
        side: None,
    },
    Step {
        title: "Quick access and search",
        body: "Buttons you pin sit in the middle of the top bar. On the right: search every command (S or Ctrl+K), keyboard shortcuts, Help (F1) and Settings.",
        target: &["quick_bar", "ribbon_icons"],
        tab: None,
        side: None,
    },
    Step {
        title: "The canvas",
        body: "This is the laser bed; (0, 0) is the front-left corner. Scroll to zoom, drag with the middle or right mouse button to pan.\n\nWhile you draw, smart guides snap to ends, middles, centres and alignments, and show green guide lines.",
        target: &["canvas"],
        tab: None,
        side: None,
    },
    Step {
        title: "Shapes list",
        body: "Every shape in the project. Click one to select it — handy when it's small or hidden under others.",
        target: &["shapes"],
        tab: None,
        side: None,
    },
    Step {
        title: "Layers, Properties and Laser",
        body: "Layers: each colour has its own mode (line, fill, image), speed, power and passes.\nProperties: exact size, position and angle of the selection.\nLaser: connect to your machine, jog it, frame and start the job.",
        target: &["side"],
        tab: None,
        side: Some(SideTab::Layers),
    },
    Step {
        title: "Colours, snapping and project tabs",
        body: "Pick a colour to choose the layer for new shapes. Switch smart guides and grid snap, and see the zoom and cursor position.\n\nThe tabs along the bottom hold several projects at once — right-click one to rename, duplicate or close it.",
        target: &["status"],
        tab: None,
        side: None,
    },
    Step {
        title: "CAD-style sketching",
        body: "The Sketch tab: lines and arcs that join into closed shapes, relations (horizontal, parallel, tangent, equal…), dimensions you can type to drive the size, construction lines (Q), mirror about a line and patterns along a curve.",
        target: &["ribbon_groups"],
        tab: Some(RibbonTab::Sketch),
        side: None,
    },
    Step {
        title: "Burning a job",
        body: "The Laser tab: Preview shows the laser's path and how long it will take, Save G-code writes a file, and Start burns on a connected laser.\n\nSet up your machine in Settings — or import a LightBurn device profile (.lbdev).",
        target: &["ribbon_groups"],
        tab: Some(RibbonTab::Laser),
        side: None,
    },
    Step {
        title: "You're ready",
        body: "Press F1 (or the ? button) for help on drawing, sketching, layers, devices and more.\n\nYou can replay this tour from Settings whenever you like. Happy making!",
        target: &["ribbon_icons"],
        tab: Some(RibbonTab::Home),
        side: None,
    },
];

pub fn steps() -> usize {
    STEPS.len()
}

fn finish(app: &mut App) {
    app.tour = None;
    app.ribbon_tab = RibbonTab::Home;
    if !app.settings.tour_done {
        app.settings.tour_done = true;
        let _ = app.settings.save();
    }
}

/// Draw the tour on top of the window (call after every panel has been drawn this frame).
pub fn show(ctx: &Context, app: &mut App) {
    let Some(i) = app.tour else { return };
    if i >= STEPS.len() {
        finish(app);
        return;
    }
    let step = &STEPS[i];
    if let Some(t) = step.tab {
        app.ribbon_tab = t;
    }
    if let Some(s) = step.side {
        app.side_tab = s;
    }
    let screen = ctx.content_rect();
    let target = step.target.iter().filter_map(|k| app.ui_rects.get(k)).filter(|r| r.is_positive()).fold(Rect::NOTHING, |a, r| a.union(*r));
    let target = (target.is_positive()).then(|| target.expand(4.0).intersect(screen));

    // keys: → / Enter next, ← back, Esc skip
    let (next, back, skip) = ctx.input_mut(|inp| {
        let n = inp.consume_key(egui::Modifiers::NONE, Key::ArrowRight) | inp.consume_key(egui::Modifiers::NONE, Key::Enter);
        let b = inp.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
        let s = inp.consume_key(egui::Modifiers::NONE, Key::Escape);
        (n, b, s)
    });
    let mut go: Option<isize> = if next { Some(1) } else if back { Some(-1) } else { None };
    let mut quit = skip;

    // dim everything except the highlighted part, and catch clicks so the app underneath stays still
    let dark = ctx.global_style().visuals.dark_mode;
    let col = theme::colors(dark);
    egui::Area::new(Id::new("tour_dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.allocate_rect(screen, Sense::click_and_drag());
        let shade = Color32::from_black_alpha(if dark { 150 } else { 110 });
        let p = ui.painter();
        match target {
            Some(t) => {
                p.rect_filled(Rect::from_min_max(screen.min, pos2(screen.max.x, t.min.y)), 0.0, shade);
                p.rect_filled(Rect::from_min_max(pos2(screen.min.x, t.max.y), screen.max), 0.0, shade);
                p.rect_filled(Rect::from_min_max(pos2(screen.min.x, t.min.y), pos2(t.min.x, t.max.y)), 0.0, shade);
                p.rect_filled(Rect::from_min_max(pos2(t.max.x, t.min.y), pos2(screen.max.x, t.max.y)), 0.0, shade);
                p.rect_stroke(t, 6.0, Stroke::new(2.5, ACCENT), StrokeKind::Outside);
            }
            None => {
                p.rect_filled(screen, 0.0, shade);
            }
        }
    });

    // the card: beside the highlighted part, on the side with the most room
    const W: f32 = 340.0;
    const GAP: f32 = 14.0;
    let (anchor, pivot) = match target {
        Some(t) if t.area() < screen.area() * 0.35 => {
            let below = screen.bottom() - t.bottom();
            let above = t.top() - screen.top();
            let right = screen.right() - t.right();
            let left = t.left() - screen.left();
            let cx = t.center().x.clamp(screen.left() + W / 2.0 + 8.0, screen.right() - W / 2.0 - 8.0);
            let cy = t.center().y.clamp(screen.top() + 130.0, screen.bottom() - 130.0);
            if below >= 230.0 {
                (pos2(cx, t.bottom() + GAP), Align2::CENTER_TOP)
            } else if above >= 230.0 {
                (pos2(cx, t.top() - GAP), Align2::CENTER_BOTTOM)
            } else if right >= left {
                (pos2(t.right() + GAP, cy), Align2::LEFT_CENTER)
            } else {
                (pos2(t.left() - GAP, cy), Align2::RIGHT_CENTER)
            }
        }
        Some(t) => (t.center(), Align2::CENTER_CENTER),
        None => (screen.center(), Align2::CENTER_CENTER),
    };
    egui::Area::new(Id::new("tour_card")).order(Order::Tooltip).fixed_pos(anchor).pivot(pivot).constrain(true).show(ctx, |ui| {
        egui::Frame::NONE.fill(col.panel).stroke(Stroke::new(1.5, ACCENT)).corner_radius(12.0).inner_margin(16.0).shadow(ui.visuals().popup_shadow).show(ui, |ui| {
            ui.set_width(W - 32.0);
            ui.horizontal(|ui| {
                // progress dots
                for k in 0..STEPS.len() {
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                    let c = if k == i { ACCENT } else if k < i { ACCENT.gamma_multiply(0.45) } else { col.border };
                    ui.painter().circle_filled(r.center(), if k == i { 4.0 } else { 3.0 }, c);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} of {}", i + 1, STEPS.len())).size(11.0).color(col.weak));
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new(step.title).size(17.0).strong());
            ui.add_space(6.0);
            ui.label(RichText::new(step.body).size(13.5).color(col.text));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let last = i + 1 == STEPS.len();
                if !last && ui.link(RichText::new("Skip tour").color(col.weak)).clicked() {
                    quit = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if last { "Finish" } else if i == 0 { "Start tour" } else { "Next" };
                    let next = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong()).fill(ACCENT).min_size(vec2(84.0, 28.0));
                    if ui.add(next).clicked() {
                        go = Some(1);
                    }
                    if i > 0 && ui.add(egui::Button::new("Back").min_size(vec2(64.0, 28.0))).clicked() {
                        go = Some(-1);
                    }
                });
            });
        });
    });

    if quit {
        finish(app);
    } else if let Some(d) = go {
        let n = i as isize + d;
        if n < 0 {
            app.tour = Some(0);
        } else if n as usize >= STEPS.len() {
            finish(app);
        } else {
            app.tour = Some(n as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_words_and_known_targets() {
        let known = ["ribbon_tabs", "ribbon_groups", "quick_bar", "ribbon_icons", "canvas", "shapes", "side", "status"];
        for s in STEPS {
            assert!(!s.title.is_empty() && s.body.len() > 40);
            assert!(s.target.iter().all(|t| known.contains(t)), "unknown target in {}", s.title);
        }
        assert!(steps() >= 8);
    }
}
