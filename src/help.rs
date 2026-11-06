//! The Help window: topics written in simple Markdown (help/*.md, built into the program),
//! plus a live list of the keyboard shortcuts as they are set right now.

use crate::actions::Action;
use crate::app::App;
use crate::theme::{self, ACCENT};
use egui::text::{LayoutJob, TextFormat};
use egui::{vec2, Color32, Context, FontId, RichText};

const TOPICS: &[&str] = &[
    include_str!("../help/01-getting-started.md"),
    include_str!("../help/02-drawing.md"),
    include_str!("../help/03-guides-relations-dimensions.md"),
    include_str!("../help/04-arranging-arrays.md"),
    include_str!("../help/05-images.md"),
    include_str!("../help/06-layers-burning.md"),
    include_str!("../help/07-devices-settings.md"),
    include_str!("../help/08-troubleshooting.md"),
];

/// The topic after the written ones: the shortcut list.
const SHORTCUTS_TOPIC: usize = TOPICS.len();

pub fn title(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim_start_matches('#').trim()
}

fn topic_title(i: usize) -> &'static str {
    if i == SHORTCUTS_TOPIC {
        "Keyboard shortcuts"
    } else {
        title(TOPICS[i])
    }
}

/// The shortcut list as Markdown, using the keys currently set.
fn shortcuts_text(app: &App) -> String {
    let mut out = String::from("# Keyboard shortcuts\n\nThese are your current keys. Change them in Keyboard Shortcuts (View tab).\n");
    let mut groups: Vec<&str> = Vec::new();
    for a in Action::ALL {
        if !groups.contains(&a.group()) {
            groups.push(a.group());
        }
    }
    for g in groups {
        out.push_str(&format!("\n## {g}\n\n"));
        for a in Action::ALL.iter().filter(|a| a.group() == g) {
            let keys = app.settings.keymap.label(*a);
            if !keys.is_empty() {
                out.push_str(&format!("- **{}** — `{}`\n", a.label(), keys));
            }
        }
    }
    out
}

fn topic_text(app: &App, i: usize) -> String {
    if i == SHORTCUTS_TOPIC {
        shortcuts_text(app)
    } else {
        TOPICS[i].to_string()
    }
}

/// Topics whose text contains every word of the search.
pub fn matching(app: &App, query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    (0..=SHORTCUTS_TOPIC)
        .filter(|&i| {
            let t = topic_text(app, i).to_lowercase();
            words.iter().all(|w| t.contains(w.as_str()))
        })
        .collect()
}

/// One line of text with **bold** and `code` spans.
fn inline(ui: &egui::Ui, text: &str, size: f32, color: Color32, query: &[String]) -> LayoutJob {
    let strong = ui.visuals().strong_text_color();
    let code_bg = ui.visuals().code_bg_color;
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let (mut bold, mut code) = (false, false);
    let mut rest = text;
    while !rest.is_empty() {
        let next = [rest.find("**"), rest.find('`')].into_iter().flatten().min();
        let (piece, tail) = match next {
            Some(n) => rest.split_at(n),
            None => (rest, ""),
        };
        if !piece.is_empty() {
            let hit = !query.is_empty() && query.iter().any(|w| piece.to_lowercase().contains(w.as_str()));
            let fmt = TextFormat {
                font_id: if code { FontId::monospace(size - 1.0) } else { FontId::proportional(size) },
                color: if bold || code { strong } else { color },
                background: if code { code_bg } else if hit { ACCENT.gamma_multiply(0.25) } else { Color32::TRANSPARENT },
                ..Default::default()
            };
            job.append(piece, 0.0, fmt);
        }
        if tail.starts_with("**") {
            bold = !bold;
            rest = &tail[2..];
        } else if tail.starts_with('`') {
            code = !code;
            rest = &tail[1..];
        } else {
            rest = tail;
        }
    }
    job
}

fn render(ui: &mut egui::Ui, text: &str, query: &[String]) {
    let col = theme::colors(ui.visuals().dark_mode);
    for line in text.lines() {
        let l = line.trim_end();
        if let Some(h) = l.strip_prefix("# ") {
            ui.label(RichText::new(h).size(21.0).strong().color(ACCENT));
            ui.add_space(4.0);
        } else if let Some(h) = l.strip_prefix("## ") {
            ui.add_space(8.0);
            ui.label(RichText::new(h).size(16.0).strong());
            ui.add_space(2.0);
        } else if l.is_empty() {
            ui.add_space(6.0);
        } else {
            // bullets and numbered steps get a hanging indent; indented bullets sit further in
            let depth = (l.len() - l.trim_start().len()) / 2;
            let l = l.trim_start();
            let num = l.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            let (mark, body) = if let Some(b) = l.strip_prefix("- ") {
                (Some("•".to_string()), b)
            } else if let Some((n, b)) = num {
                (Some(format!("{n}.")), b)
            } else {
                (None, l)
            };
            match mark {
                Some(m) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(6.0 + 18.0 * depth as f32);
                        ui.add_sized(vec2(16.0, 18.0), egui::Label::new(RichText::new(m).size(13.5).color(col.weak)));
                        ui.vertical(|ui| {
                            let job = inline(ui, body, 13.5, col.text, query);
                            ui.label(job);
                        });
                    });
                }
                None => {
                    let job = inline(ui, body, 13.5, col.text, query);
                    ui.label(job);
                }
            }
            ui.add_space(2.0);
        }
    }
}

pub fn window(ctx: &Context, app: &mut App) {
    if !app.win_help {
        return;
    }
    let mut open = true;
    let mut tour = false;
    egui::Window::new("Help").open(&mut open).default_size(vec2(780.0, 560.0)).min_width(680.0).resizable(true).collapsible(false).show(ctx, |ui| {
        let col = theme::colors(ui.visuals().dark_mode);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.help_search).hint_text("Search help").desired_width(220.0));
            if !app.help_search.is_empty() && ui.small_button("Clear").clicked() {
                app.help_search.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                tour = ui.button("Welcome Tour").on_hover_text("Walk through the window step by step").clicked();
            });
        });
        ui.separator();
        let query: Vec<String> = app.help_search.split_whitespace().map(|w| w.to_lowercase()).collect();
        let shown = matching(app, &app.help_search);
        if !shown.is_empty() && !shown.contains(&app.help_topic) {
            app.help_topic = shown[0];
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                if shown.is_empty() {
                    ui.label(RichText::new("Nothing matches").color(col.weak));
                }
                for &i in &shown {
                    let sel = app.help_topic == i;
                    let text = RichText::new(topic_title(i)).size(13.5).color(if sel { ACCENT } else { col.text });
                    if ui.add(egui::Button::selectable(sel, text).min_size(vec2(200.0, 28.0))).clicked() {
                        app.help_topic = i;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt(("help_body", app.help_topic)).auto_shrink([false, false]).show(ui, |ui| {
                if !shown.is_empty() {
                    let text = topic_text(app, app.help_topic);
                    ui.vertical(|ui| render(ui, &text, &query));
                }
            });
        });
    });
    if !open {
        app.win_help = false;
    }
    if tour {
        app.win_help = false;
        app.tour = Some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_a_title_and_text() {
        for t in TOPICS {
            assert!(t.starts_with("# "), "topic must start with a title");
            assert!(!title(t).is_empty() && t.len() > 200);
        }
    }

    #[test]
    fn search_finds_topics_and_shortcuts() {
        let app = App::new(crate::settings::Settings::default());
        let r = matching(&app, "driving dimension");
        assert!(r.contains(&2), "dimensions topic should match");
        assert!(matching(&app, "").len() == TOPICS.len() + 1);
        assert!(matching(&app, "zzqqxx").is_empty());
        assert!(shortcuts_text(&app).contains("Ctrl+S"));
    }
}
