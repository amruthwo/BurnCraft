//! BurnCraft look: neutral greys with a warm "burn" accent, rounded controls.

use egui::{Color32, CornerRadius, Stroke};

/// BurnCraft burgundy (from the app icon) for fills, and a brighter tint for lines and text.
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(122, 16, 52);
pub const ACCENT: Color32 = Color32::from_rgb(178, 34, 78);
pub const ACCENT_SOFT_DARK: Color32 = Color32::from_rgba_premultiplied(62, 10, 28, 150);
pub const ACCENT_SOFT_LIGHT: Color32 = Color32::from_rgba_premultiplied(242, 208, 220, 210);
pub const SELECT: Color32 = Color32::from_rgb(64, 156, 255);

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Color32,
    pub panel: Color32,
    pub ribbon: Color32,
    pub canvas: Color32,
    pub bed: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub icon: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub border: Color32,
}

pub fn colors(dark: bool) -> Colors {
    if dark {
        Colors {
            bg: Color32::from_rgb(28, 29, 32),
            panel: Color32::from_rgb(36, 38, 42),
            ribbon: Color32::from_rgb(42, 44, 49),
            canvas: Color32::from_rgb(22, 23, 26),
            bed: Color32::from_rgb(46, 48, 53),
            grid_minor: Color32::from_rgb(54, 57, 63),
            grid_major: Color32::from_rgb(70, 74, 82),
            text: Color32::from_rgb(226, 228, 232),
            weak: Color32::from_rgb(150, 155, 165),
            icon: Color32::from_rgb(214, 218, 225),
            accent_soft: ACCENT_SOFT_DARK,
            hover: Color32::from_rgb(58, 61, 68),
            border: Color32::from_rgb(58, 61, 68),
        }
    } else {
        Colors {
            bg: Color32::from_rgb(240, 241, 244),
            panel: Color32::from_rgb(248, 249, 251),
            ribbon: Color32::from_rgb(255, 255, 255),
            canvas: Color32::from_rgb(214, 217, 223),
            bed: Color32::from_rgb(252, 252, 253),
            grid_minor: Color32::from_rgb(232, 234, 238),
            grid_major: Color32::from_rgb(205, 209, 216),
            text: Color32::from_rgb(30, 32, 36),
            weak: Color32::from_rgb(105, 110, 120),
            icon: Color32::from_rgb(52, 56, 64),
            accent_soft: ACCENT_SOFT_LIGHT,
            hover: Color32::from_rgb(230, 233, 238),
            border: Color32::from_rgb(214, 217, 223),
        }
    }
}

pub fn apply(ctx: &egui::Context, dark: bool, scale: f32) {
    let c = colors(dark);
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = c.panel;
    v.window_fill = c.panel;
    v.extreme_bg_color = if dark { Color32::from_rgb(24, 25, 28) } else { Color32::WHITE };
    v.faint_bg_color = if dark { Color32::from_rgb(40, 42, 47) } else { Color32::from_rgb(243, 244, 247) };
    v.selection.bg_fill = if dark { ACCENT_DEEP } else { Color32::from_rgb(240, 200, 214) };
    // text on selected items: white on burgundy (dark), deep burgundy on pink (light)
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::from_rgb(250, 236, 240) } else { ACCENT_DEEP });
    v.hyperlink_color = ACCENT;
    let r = CornerRadius::same(6);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = ACCENT;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, c.border);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_width = 140.0;
    });
    ctx.set_zoom_factor(scale);
}
