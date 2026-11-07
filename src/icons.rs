//! Simple line icons painted with vectors (no image files), readable at any size.

use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // a few icons are drawn but not used by any command yet
pub enum Icon {
    New, Open, Import, Save, SaveAs, Export, Undo, Redo, Copy, Cut, Paste, Duplicate, Delete, SelectAll,
    Select, Pan, Line, Rect, Ellipse, Polygon, Text, Nodes, Measure,
    Group, Ungroup, FlipH, FlipV, RotateCw, RotateCcw, ToPath, Offset, Weld, Union, Subtract, Intersect, BreakApart, Array,
    AlignLeft, AlignRight, AlignTop, AlignBottom, AlignHCenter, AlignVCenter, DistributeH, DistributeV, PageCenter, Front, Back,
    ZoomIn, ZoomOut, ZoomFit, ZoomSel, Filled, Grid, Preview,
    Trace, Mcp, Invert, Reset,
    Connect, Home, Frame, Start, Pause, Stop,
    Search, Keyboard, Settings, Notes, Exit, Layers, Laser, Image,
    Lock, Unlock, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Dot, Refresh,
    Pen, Arc, Star, Trim, Fillet, Barcode, NodeSmooth, NodeCorner, NodeLine, NodeCurve, NodeInsert, NodeBreak,
    CircArray, Wand, Lasso, SelOutline, NewTab, DuplicateTab, CloseTab, Dimension,
    Relate, Construction, MirrorLine, PathPattern, AlignPath, Join, Help, Tour, VarText, Camera,
}

pub struct Pal {
    pub fg: Color32,
    pub accent: Color32,
    pub soft: Color32,
}

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, pal: &Pal) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - egui::vec2(s / 2.0, s / 2.0);
    let p = |x: f32, y: f32| -> Pos2 { pos2(o.x + x * s, o.y + y * s) };
    let w = (s / 14.0).max(1.4);
    let st = Stroke::new(w, pal.fg);
    let ac = Stroke::new(w, pal.accent);
    let thin = Stroke::new(w * 0.7, pal.fg);
    let line = |pts: &[(f32, f32)], stroke: Stroke| {
        painter.add(Shape::line(pts.iter().map(|(x, y)| p(*x, *y)).collect(), stroke));
    };
    let closed = |pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        painter.add(Shape::closed_line(pts.iter().map(|(x, y)| p(*x, *y)).collect(), stroke));
        if fill != Color32::TRANSPARENT {
            painter.add(Shape::convex_polygon(pts.iter().map(|(x, y)| p(*x, *y)).collect(), fill, Stroke::NONE));
        }
    };
    let rect_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(p(x0, y0), p(x1, y1));
    let rs = |r: Rect, stroke: Stroke| {
        painter.rect_stroke(r, 1.5, stroke, StrokeKind::Middle);
    };
    let rf = |r: Rect, c: Color32| {
        painter.rect_filled(r, 1.5, c);
    };
    let circ = |x: f32, y: f32, r: f32, stroke: Stroke| {
        painter.circle_stroke(p(x, y), r * s, stroke);
    };
    let dot = |x: f32, y: f32, r: f32, c: Color32| {
        painter.circle_filled(p(x, y), r * s, c);
    };
    let arrow_head = |tip: (f32, f32), dir: (f32, f32), stroke: Stroke| {
        let (dx, dy) = dir;
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l, dy / l);
        let k = 0.16;
        line(&[(tip.0 - ux * k - uy * k * 0.7, tip.1 - uy * k + ux * k * 0.7), tip, (tip.0 - ux * k + uy * k * 0.7, tip.1 - uy * k - ux * k * 0.7)], stroke);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32, stroke: Stroke| {
        let n = 18;
        let pts: Vec<(f32, f32)> = (0..=n).map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        }).collect();
        line(&pts, stroke);
    };
    use std::f32::consts::PI;
    match icon {
        Icon::New => {
            closed(&[(0.25, 0.1), (0.62, 0.1), (0.78, 0.26), (0.78, 0.9), (0.25, 0.9)], Color32::TRANSPARENT, st);
            line(&[(0.62, 0.1), (0.62, 0.26), (0.78, 0.26)], st);
            line(&[(0.52, 0.48), (0.52, 0.76)], ac);
            line(&[(0.38, 0.62), (0.66, 0.62)], ac);
        }
        Icon::Open => {
            closed(&[(0.1, 0.25), (0.38, 0.25), (0.46, 0.34), (0.88, 0.34), (0.88, 0.82), (0.1, 0.82)], Color32::TRANSPARENT, st);
            closed(&[(0.18, 0.46), (0.96, 0.46), (0.84, 0.82), (0.1, 0.82)], pal.soft, ac);
        }
        Icon::Import => {
            line(&[(0.15, 0.6), (0.15, 0.88), (0.85, 0.88), (0.85, 0.6)], st);
            line(&[(0.5, 0.1), (0.5, 0.66)], ac);
            arrow_head((0.5, 0.68), (0.0, 1.0), ac);
        }
        Icon::Export => {
            line(&[(0.15, 0.6), (0.15, 0.88), (0.85, 0.88), (0.85, 0.6)], st);
            line(&[(0.5, 0.68), (0.5, 0.12)], ac);
            arrow_head((0.5, 0.1), (0.0, -1.0), ac);
        }
        Icon::Save | Icon::SaveAs => {
            closed(&[(0.14, 0.14), (0.72, 0.14), (0.86, 0.28), (0.86, 0.86), (0.14, 0.86)], Color32::TRANSPARENT, st);
            rs(rect_at(0.3, 0.14, 0.64, 0.38), thin);
            rf(rect_at(0.28, 0.56, 0.72, 0.86), pal.soft);
            rs(rect_at(0.28, 0.56, 0.72, 0.86), ac);
            if icon == Icon::SaveAs {
                line(&[(0.62, 0.98), (0.98, 0.62)], ac);
            }
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let fx = |x: f32| if flip { 1.0 - x } else { x };
            let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
                let a = -PI * 0.5 + PI * i as f32 / 16.0;
                (fx(0.5 + 0.3 * a.cos()), 0.55 + 0.25 * a.sin())
            }).collect();
            line(&pts, st);
            line(&[(fx(0.5), 0.3), (fx(0.2), 0.3)], st);
            arrow_head((fx(0.18), 0.3), (if flip { 1.0 } else { -1.0 }, 0.0), ac);
            line(&[(fx(0.5), 0.8), (fx(0.3), 0.8)], st);
        }
        Icon::Copy | Icon::Duplicate => {
            rs(rect_at(0.12, 0.12, 0.6, 0.6), st);
            rf(rect_at(0.38, 0.38, 0.88, 0.88), pal.soft);
            rs(rect_at(0.38, 0.38, 0.88, 0.88), ac);
            if icon == Icon::Duplicate {
                line(&[(0.63, 0.52), (0.63, 0.74)], ac);
                line(&[(0.52, 0.63), (0.74, 0.63)], ac);
            }
        }
        Icon::Cut => {
            circ(0.28, 0.75, 0.12, st);
            circ(0.72, 0.75, 0.12, st);
            line(&[(0.35, 0.65), (0.7, 0.1)], ac);
            line(&[(0.65, 0.65), (0.3, 0.1)], ac);
        }
        Icon::Paste => {
            rs(rect_at(0.18, 0.18, 0.82, 0.9), st);
            rf(rect_at(0.36, 0.1, 0.64, 0.26), pal.accent);
            line(&[(0.32, 0.45), (0.68, 0.45)], thin);
            line(&[(0.32, 0.6), (0.68, 0.6)], thin);
            line(&[(0.32, 0.75), (0.56, 0.75)], thin);
        }
        Icon::Delete => {
            line(&[(0.18, 0.24), (0.82, 0.24)], st);
            line(&[(0.4, 0.24), (0.42, 0.12), (0.58, 0.12), (0.6, 0.24)], st);
            closed(&[(0.26, 0.24), (0.74, 0.24), (0.68, 0.9), (0.32, 0.9)], Color32::TRANSPARENT, st);
            line(&[(0.43, 0.38), (0.44, 0.78)], ac);
            line(&[(0.57, 0.38), (0.56, 0.78)], ac);
        }
        Icon::SelectAll => {
            let d = Stroke::new(w * 0.8, pal.fg);
            for i in 0..4 {
                let t0 = 0.1 + i as f32 * 0.21;
                line(&[(t0, 0.1), (t0 + 0.12, 0.1)], d);
                line(&[(t0, 0.9), (t0 + 0.12, 0.9)], d);
                line(&[(0.1, t0), (0.1, t0 + 0.12)], d);
                line(&[(0.9, t0), (0.9, t0 + 0.12)], d);
            }
            rf(rect_at(0.3, 0.3, 0.7, 0.7), pal.soft);
            rs(rect_at(0.3, 0.3, 0.7, 0.7), ac);
        }
        Icon::Select => {
            closed(&[(0.25, 0.1), (0.25, 0.8), (0.42, 0.64), (0.55, 0.92), (0.66, 0.87), (0.53, 0.6), (0.76, 0.6)], pal.soft, ac);
        }
        Icon::Pan => {
            line(&[(0.5, 0.1), (0.5, 0.9)], st);
            line(&[(0.1, 0.5), (0.9, 0.5)], st);
            arrow_head((0.5, 0.08), (0.0, -1.0), ac);
            arrow_head((0.5, 0.92), (0.0, 1.0), ac);
            arrow_head((0.08, 0.5), (-1.0, 0.0), ac);
            arrow_head((0.92, 0.5), (1.0, 0.0), ac);
        }
        Icon::Line => {
            line(&[(0.15, 0.8), (0.42, 0.3), (0.85, 0.55)], st);
            for (x, y) in [(0.15, 0.8), (0.42, 0.3), (0.85, 0.55)] {
                rf(Rect::from_center_size(p(x, y), egui::vec2(s * 0.14, s * 0.14)), pal.accent);
            }
        }
        Icon::Rect => {
            rf(rect_at(0.14, 0.24, 0.86, 0.76), pal.soft);
            rs(rect_at(0.14, 0.24, 0.86, 0.76), ac);
        }
        Icon::Ellipse => {
            painter.add(egui::epaint::EllipseShape { center: p(0.5, 0.5), radius: egui::vec2(0.38 * s, 0.28 * s), fill: pal.soft, stroke: ac, angle: 0.0 });
        }
        Icon::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6).map(|i| {
                let a = -PI / 2.0 + i as f32 * PI / 3.0;
                (0.5 + 0.38 * a.cos(), 0.52 + 0.38 * a.sin())
            }).collect();
            closed(&pts, pal.soft, ac);
        }
        Icon::Text => {
            line(&[(0.18, 0.2), (0.82, 0.2)], st);
            line(&[(0.5, 0.2), (0.5, 0.86)], st);
            line(&[(0.36, 0.86), (0.64, 0.86)], ac);
        }
        Icon::Nodes => {
            let pts: Vec<(f32, f32)> = (0..=20).map(|i| {
                let t = i as f32 / 20.0;
                (0.12 + 0.76 * t, 0.7 - 0.45 * (t * PI).sin())
            }).collect();
            line(&pts, st);
            line(&[(0.12, 0.7), (0.3, 0.2)], thin);
            line(&[(0.88, 0.7), (0.7, 0.2)], thin);
            for (x, y) in [(0.12, 0.7), (0.88, 0.7)] {
                rf(Rect::from_center_size(p(x, y), egui::vec2(s * 0.15, s * 0.15)), pal.accent);
            }
            dot(0.3, 0.2, 0.06, pal.fg);
            dot(0.7, 0.2, 0.06, pal.fg);
        }
        Icon::Measure => {
            closed(&[(0.08, 0.62), (0.62, 0.08), (0.92, 0.38), (0.38, 0.92)], pal.soft, st);
            for i in 1..6 {
                let t = i as f32 / 6.0;
                let (x, y) = (0.08 + 0.54 * t, 0.62 - 0.54 * t);
                let l = if i % 2 == 0 { 0.16 } else { 0.1 };
                line(&[(x, y), (x + l, y + l)], ac);
            }
        }
        Icon::Group | Icon::Ungroup => {
            let dash = Stroke::new(w * 0.7, pal.accent);
            if icon == Icon::Group {
                for i in 0..5 {
                    let t = 0.06 + i as f32 * 0.19;
                    line(&[(t, 0.06), (t + 0.1, 0.06)], dash);
                    line(&[(t, 0.94), (t + 0.1, 0.94)], dash);
                    line(&[(0.06, t), (0.06, t + 0.1)], dash);
                    line(&[(0.94, t), (0.94, t + 0.1)], dash);
                }
                rs(rect_at(0.2, 0.2, 0.55, 0.55), st);
                circ(0.62, 0.62, 0.18, st);
            } else {
                rs(rect_at(0.08, 0.08, 0.45, 0.45), st);
                circ(0.7, 0.7, 0.18, st);
                arrow_head((0.92, 0.08), (1.0, -1.0), dash);
                arrow_head((0.08, 0.92), (-1.0, 1.0), dash);
            }
        }
        Icon::FlipH => {
            line(&[(0.5, 0.08), (0.5, 0.92)], Stroke::new(w * 0.7, pal.fg));
            closed(&[(0.42, 0.25), (0.42, 0.78), (0.1, 0.78)], pal.soft, st);
            closed(&[(0.58, 0.25), (0.58, 0.78), (0.9, 0.78)], Color32::TRANSPARENT, ac);
        }
        Icon::FlipV => {
            line(&[(0.08, 0.5), (0.92, 0.5)], Stroke::new(w * 0.7, pal.fg));
            closed(&[(0.25, 0.42), (0.78, 0.42), (0.78, 0.1)], pal.soft, st);
            closed(&[(0.25, 0.58), (0.78, 0.58), (0.78, 0.9)], Color32::TRANSPARENT, ac);
        }
        Icon::RotateCw | Icon::RotateCcw => {
            let cw = icon == Icon::RotateCw;
            rs(rect_at(0.34, 0.4, 0.66, 0.72), st);
            if cw {
                arc(0.5, 0.56, 0.36, PI * 1.05, PI * 1.85, ac);
                arrow_head((0.5 + 0.36 * (PI * 1.85).cos(), 0.56 + 0.36 * (PI * 1.85).sin()), (0.7, 0.7), ac);
            } else {
                arc(0.5, 0.56, 0.36, PI * 1.95, PI * 1.15, ac);
                arrow_head((0.5 + 0.36 * (PI * 1.15).cos(), 0.56 + 0.36 * (PI * 1.15).sin()), (-0.7, 0.7), ac);
            }
        }
        Icon::ToPath => {
            painter.add(egui::epaint::EllipseShape { center: p(0.32, 0.5), radius: egui::vec2(0.2 * s, 0.2 * s), fill: Color32::TRANSPARENT, stroke: st, angle: 0.0 });
            arrow_head((0.66, 0.5), (1.0, 0.0), ac);
            line(&[(0.5, 0.5), (0.66, 0.5)], ac);
            line(&[(0.74, 0.3), (0.94, 0.3), (0.94, 0.7), (0.74, 0.7), (0.74, 0.3)], st);
            for (x, y) in [(0.74, 0.3), (0.94, 0.3), (0.94, 0.7), (0.74, 0.7)] {
                dot(x, y, 0.05, pal.accent);
            }
        }
        Icon::Offset => {
            rs(rect_at(0.32, 0.32, 0.68, 0.68), st);
            painter.rect_stroke(rect_at(0.12, 0.12, 0.88, 0.88), 0.16 * s, ac, StrokeKind::Middle);
        }
        Icon::Weld | Icon::Union => {
            painter.circle_filled(p(0.38, 0.5), 0.26 * s, pal.soft);
            painter.circle_filled(p(0.62, 0.5), 0.26 * s, pal.soft);
            arc(0.38, 0.5, 0.26, PI * 0.31, PI * 1.69, ac);
            arc(0.62, 0.5, 0.26, -PI * 0.69, PI * 0.69, ac);
        }
        Icon::Subtract => {
            painter.circle_filled(p(0.38, 0.5), 0.26 * s, pal.soft);
            arc(0.38, 0.5, 0.26, PI * 0.31, PI * 1.69, ac);
            arc(0.62, 0.5, 0.26, PI * 1.31, PI * 0.69, ac);
            circ(0.62, 0.5, 0.26, Stroke::new(w * 0.5, pal.fg));
        }
        Icon::Intersect => {
            circ(0.38, 0.5, 0.26, Stroke::new(w * 0.6, pal.fg));
            circ(0.62, 0.5, 0.26, Stroke::new(w * 0.6, pal.fg));
            arc(0.38, 0.5, 0.26, -PI * 0.31, PI * 0.31, ac);
            arc(0.62, 0.5, 0.26, PI * 0.69, PI * 1.31, ac);
        }
        Icon::BreakApart => {
            line(&[(0.12, 0.45), (0.12, 0.12), (0.45, 0.12)], st);
            line(&[(0.55, 0.12), (0.88, 0.12), (0.88, 0.45)], ac);
            line(&[(0.88, 0.55), (0.88, 0.88), (0.55, 0.88)], st);
            line(&[(0.45, 0.88), (0.12, 0.88), (0.12, 0.55)], ac);
        }
        Icon::Array => {
            for i in 0..3 {
                for j in 0..3 {
                    let r = rect_at(0.1 + i as f32 * 0.29, 0.1 + j as f32 * 0.29, 0.31 + i as f32 * 0.29, 0.31 + j as f32 * 0.29);
                    if i == 0 && j == 0 {
                        rf(r, pal.accent);
                    } else {
                        rs(r, thin);
                    }
                }
            }
        }
        Icon::AlignLeft | Icon::AlignRight | Icon::AlignVCenter => {
            let x = match icon {
                Icon::AlignLeft => 0.12,
                Icon::AlignRight => 0.88,
                _ => 0.5,
            };
            line(&[(x, 0.06), (x, 0.94)], ac);
            let (a0, a1, b0, b1) = match icon {
                Icon::AlignLeft => (0.12, 0.8, 0.12, 0.55),
                Icon::AlignRight => (0.2, 0.88, 0.45, 0.88),
                _ => (0.18, 0.82, 0.3, 0.7),
            };
            rf(rect_at(a0, 0.2, a1, 0.42), pal.soft);
            rs(rect_at(a0, 0.2, a1, 0.42), st);
            rf(rect_at(b0, 0.58, b1, 0.8), pal.soft);
            rs(rect_at(b0, 0.58, b1, 0.8), st);
        }
        Icon::AlignTop | Icon::AlignBottom | Icon::AlignHCenter => {
            let y = match icon {
                Icon::AlignTop => 0.12,
                Icon::AlignBottom => 0.88,
                _ => 0.5,
            };
            line(&[(0.06, y), (0.94, y)], ac);
            let (a0, a1, b0, b1) = match icon {
                Icon::AlignTop => (0.12, 0.8, 0.12, 0.55),
                Icon::AlignBottom => (0.2, 0.88, 0.45, 0.88),
                _ => (0.18, 0.82, 0.3, 0.7),
            };
            rf(rect_at(0.2, a0, 0.42, a1), pal.soft);
            rs(rect_at(0.2, a0, 0.42, a1), st);
            rf(rect_at(0.58, b0, 0.8, b1), pal.soft);
            rs(rect_at(0.58, b0, 0.8, b1), st);
        }
        Icon::DistributeH => {
            line(&[(0.06, 0.1), (0.06, 0.9)], ac);
            line(&[(0.94, 0.1), (0.94, 0.9)], ac);
            rs(rect_at(0.18, 0.3, 0.38, 0.7), st);
            rs(rect_at(0.62, 0.3, 0.82, 0.7), st);
            line(&[(0.42, 0.5), (0.58, 0.5)], thin);
        }
        Icon::DistributeV => {
            line(&[(0.1, 0.06), (0.9, 0.06)], ac);
            line(&[(0.1, 0.94), (0.9, 0.94)], ac);
            rs(rect_at(0.3, 0.18, 0.7, 0.38), st);
            rs(rect_at(0.3, 0.62, 0.7, 0.82), st);
            line(&[(0.5, 0.42), (0.5, 0.58)], thin);
        }
        Icon::PageCenter => {
            rs(rect_at(0.08, 0.08, 0.92, 0.92), thin);
            circ(0.5, 0.5, 0.2, ac);
            line(&[(0.5, 0.2), (0.5, 0.8)], st);
            line(&[(0.2, 0.5), (0.8, 0.5)], st);
        }
        Icon::Front | Icon::Back => {
            let front = icon == Icon::Front;
            let (r1, r2) = (rect_at(0.1, 0.1, 0.62, 0.62), rect_at(0.38, 0.38, 0.9, 0.9));
            let (under, over) = if front { (r1, r2) } else { (r2, r1) };
            rs(under, thin);
            rf(over, pal.soft);
            rs(over, ac);
        }
        Icon::ZoomIn | Icon::ZoomOut | Icon::ZoomFit | Icon::ZoomSel | Icon::Search => {
            circ(0.42, 0.42, 0.28, st);
            line(&[(0.62, 0.62), (0.9, 0.9)], Stroke::new(w * 1.4, pal.fg));
            match icon {
                Icon::ZoomIn => {
                    line(&[(0.28, 0.42), (0.56, 0.42)], ac);
                    line(&[(0.42, 0.28), (0.42, 0.56)], ac);
                }
                Icon::ZoomOut => line(&[(0.28, 0.42), (0.56, 0.42)], ac),
                Icon::ZoomFit => rs(rect_at(0.3, 0.32, 0.54, 0.52), ac),
                Icon::ZoomSel => rf(rect_at(0.32, 0.32, 0.52, 0.52), pal.accent),
                _ => {}
            }
        }
        Icon::Filled => {
            rf(rect_at(0.14, 0.14, 0.62, 0.62), pal.accent);
            circ(0.62, 0.62, 0.26, st);
        }
        Icon::Grid => {
            for i in 0..4 {
                let t = 0.14 + i as f32 * 0.24;
                line(&[(t, 0.1), (t, 0.9)], thin);
                line(&[(0.1, t), (0.9, t)], thin);
            }
            rf(rect_at(0.38, 0.38, 0.62, 0.62), pal.accent);
        }
        Icon::Preview => {
            line(&[(0.1, 0.8), (0.3, 0.35), (0.5, 0.65), (0.72, 0.2), (0.9, 0.5)], Stroke::new(w * 0.8, pal.fg));
            painter.add(Shape::dashed_line(&[p(0.1, 0.2), p(0.9, 0.2)], Stroke::new(w * 0.6, pal.fg), s * 0.06, s * 0.05));
            dot(0.72, 0.2, 0.09, pal.accent);
        }
        Icon::Trace | Icon::Image => {
            rs(rect_at(0.08, 0.14, 0.92, 0.86), st);
            closed(&[(0.14, 0.8), (0.4, 0.42), (0.58, 0.64), (0.7, 0.5), (0.86, 0.8)], pal.soft, Stroke::new(w * 0.7, pal.fg));
            dot(0.7, 0.3, 0.07, pal.accent);
            if icon == Icon::Trace {
                painter.add(Shape::dashed_line(&[p(0.14, 0.8), p(0.4, 0.42), p(0.58, 0.64), p(0.7, 0.5), p(0.86, 0.8)], ac, s * 0.07, s * 0.04));
            }
        }
        Icon::Mcp => {
            rs(rect_at(0.18, 0.3, 0.82, 0.82), st);
            line(&[(0.5, 0.3), (0.5, 0.14)], st);
            dot(0.5, 0.12, 0.06, pal.accent);
            dot(0.36, 0.52, 0.065, pal.accent);
            dot(0.64, 0.52, 0.065, pal.accent);
            line(&[(0.36, 0.68), (0.64, 0.68)], ac);
            line(&[(0.08, 0.5), (0.08, 0.66)], st);
            line(&[(0.92, 0.5), (0.92, 0.66)], st);
        }
        Icon::Invert => {
            circ(0.5, 0.5, 0.36, st);
            let pts: Vec<Pos2> = (0..=20).map(|i| {
                let a = -PI / 2.0 + PI * i as f32 / 20.0;
                p(0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin())
            }).collect();
            painter.add(Shape::convex_polygon(pts, pal.accent, Stroke::NONE));
        }
        Icon::Reset => {
            arc(0.5, 0.52, 0.32, PI * 0.8, PI * 2.6, st);
            arrow_head((0.5 + 0.32 * (PI * 0.8).cos(), 0.52 + 0.32 * (PI * 0.8).sin()), (0.3, 1.0), ac);
        }
        Icon::Connect => {
            rs(rect_at(0.3, 0.38, 0.7, 0.66), st);
            line(&[(0.4, 0.38), (0.4, 0.2)], ac);
            line(&[(0.6, 0.38), (0.6, 0.2)], ac);
            line(&[(0.5, 0.66), (0.5, 0.92)], st);
        }
        Icon::Home => {
            closed(&[(0.12, 0.5), (0.5, 0.14), (0.88, 0.5)], Color32::TRANSPARENT, ac);
            line(&[(0.22, 0.44), (0.22, 0.86), (0.78, 0.86), (0.78, 0.44)], st);
            rs(rect_at(0.42, 0.6, 0.58, 0.86), thin);
        }
        Icon::Frame => {
            painter.add(Shape::dashed_line(&[p(0.12, 0.12), p(0.88, 0.12), p(0.88, 0.88), p(0.12, 0.88), p(0.12, 0.12)], ac, s * 0.08, s * 0.05));
            dot(0.12, 0.12, 0.06, pal.fg);
        }
        Icon::Start => {
            closed(&[(0.28, 0.14), (0.84, 0.5), (0.28, 0.86)], pal.accent, ac);
        }
        Icon::Pause => {
            rf(rect_at(0.24, 0.16, 0.42, 0.84), pal.accent);
            rf(rect_at(0.58, 0.16, 0.76, 0.84), pal.accent);
        }
        Icon::Stop => {
            rf(rect_at(0.2, 0.2, 0.8, 0.8), Color32::from_rgb(220, 60, 60));
        }
        Icon::Keyboard => {
            rs(rect_at(0.06, 0.26, 0.94, 0.76), st);
            for j in 0..2 {
                for i in 0..5 {
                    rf(Rect::from_center_size(p(0.18 + i as f32 * 0.16, 0.4 + j as f32 * 0.14), egui::vec2(s * 0.09, s * 0.08)), pal.fg);
                }
            }
            line(&[(0.3, 0.66), (0.7, 0.66)], ac);
        }
        Icon::Settings => {
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                line(&[(0.5 + 0.26 * a.cos(), 0.5 + 0.26 * a.sin()), (0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin())], Stroke::new(w * 1.6, pal.fg));
            }
            circ(0.5, 0.5, 0.27, st);
            circ(0.5, 0.5, 0.1, ac);
        }
        Icon::Notes => {
            rs(rect_at(0.18, 0.1, 0.82, 0.9), st);
            for i in 0..4 {
                let y = 0.3 + i as f32 * 0.15;
                line(&[(0.3, y), (if i == 3 { 0.55 } else { 0.7 }, y)], if i == 0 { ac } else { thin });
            }
        }
        Icon::Exit => {
            line(&[(0.55, 0.14), (0.16, 0.14), (0.16, 0.86), (0.55, 0.86)], st);
            line(&[(0.38, 0.5), (0.9, 0.5)], ac);
            arrow_head((0.92, 0.5), (1.0, 0.0), ac);
        }
        Icon::Layers => {
            closed(&[(0.5, 0.18), (0.9, 0.38), (0.5, 0.58), (0.1, 0.38)], pal.soft, ac);
            line(&[(0.1, 0.52), (0.5, 0.72), (0.9, 0.52)], st);
            line(&[(0.1, 0.66), (0.5, 0.86), (0.9, 0.66)], st);
        }
        Icon::Lock | Icon::Unlock => {
            painter.rect_filled(rect_at(0.22, 0.46, 0.78, 0.88), 2.0, if icon == Icon::Lock { pal.accent } else { pal.fg.gamma_multiply(0.5) });
            if icon == Icon::Lock {
                arc(0.5, 0.46, 0.18, PI, 2.0 * PI, st);
                line(&[(0.32, 0.46), (0.32, 0.4)], st);
                line(&[(0.68, 0.46), (0.68, 0.4)], st);
            } else {
                arc(0.68, 0.42, 0.16, PI, 2.0 * PI, st);
            }
        }
        Icon::ArrowUp | Icon::ArrowDown | Icon::ArrowLeft | Icon::ArrowRight => {
            let pts: [(f32, f32); 3] = match icon {
                Icon::ArrowUp => [(0.5, 0.2), (0.8, 0.72), (0.2, 0.72)],
                Icon::ArrowDown => [(0.5, 0.8), (0.2, 0.28), (0.8, 0.28)],
                Icon::ArrowLeft => [(0.2, 0.5), (0.72, 0.2), (0.72, 0.8)],
                _ => [(0.8, 0.5), (0.28, 0.8), (0.28, 0.2)],
            };
            closed(&pts, pal.accent, ac);
        }
        Icon::Dot => {
            dot(0.5, 0.5, 0.3, pal.fg);
        }
        Icon::Refresh => {
            arc(0.5, 0.5, 0.3, PI * 0.2, PI * 1.8, st);
            arrow_head((0.5 + 0.3 * (PI * 1.8).cos(), 0.5 + 0.3 * (PI * 1.8).sin()), (0.6, 1.0), ac);
        }
        Icon::Pen => {
            closed(&[(0.5, 0.08), (0.72, 0.48), (0.5, 0.92), (0.28, 0.48)], pal.soft, st);
            line(&[(0.5, 0.08), (0.5, 0.56)], thin);
            dot(0.5, 0.6, 0.06, pal.accent);
            line(&[(0.06, 0.9), (0.3, 0.7)], ac);
            line(&[(0.94, 0.9), (0.7, 0.7)], ac);
        }
        Icon::Arc => {
            arc(0.5, 0.75, 0.38, PI, 2.0 * PI, st);
            for (x, y) in [(0.12, 0.75), (0.5, 0.37), (0.88, 0.75)] {
                rf(Rect::from_center_size(p(x, y), egui::vec2(s * 0.13, s * 0.13)), pal.accent);
            }
        }
        Icon::Star => {
            let pts: Vec<(f32, f32)> = (0..10).map(|i| {
                let a = -PI / 2.0 + i as f32 * PI / 5.0;
                let r = if i % 2 == 0 { 0.42 } else { 0.18 };
                (0.5 + r * a.cos(), 0.54 + r * a.sin())
            }).collect();
            painter.add(Shape::closed_line(pts.iter().map(|(x, y)| p(*x, *y)).collect(), ac));
        }
        Icon::Trim => {
            line(&[(0.08, 0.5), (0.38, 0.5)], st);
            painter.add(Shape::dashed_line(&[p(0.38, 0.5), p(0.66, 0.5)], Stroke::new(w * 0.7, pal.accent), s * 0.05, s * 0.04));
            line(&[(0.66, 0.5), (0.92, 0.5)], st);
            line(&[(0.38, 0.15), (0.38, 0.85)], thin);
            line(&[(0.66, 0.15), (0.66, 0.85)], thin);
        }
        Icon::Fillet => {
            line(&[(0.15, 0.88), (0.15, 0.45)], st);
            arc(0.55, 0.45, 0.4, PI, PI * 1.5, ac);
            line(&[(0.55, 0.05), (0.9, 0.05)], st);
            painter.add(Shape::dashed_line(&[p(0.15, 0.45), p(0.15, 0.05), p(0.55, 0.05)], Stroke::new(w * 0.5, pal.fg), s * 0.04, s * 0.04));
        }
        Icon::Barcode => {
            let bars = [0.1, 0.16, 0.2, 0.3, 0.34, 0.42, 0.5, 0.54, 0.62, 0.7, 0.74, 0.84, 0.9];
            for (i, x) in bars.iter().enumerate() {
                let wdt = if i % 3 == 0 { 0.045 } else { 0.022 };
                rf(rect_at(*x, 0.18, x + wdt, 0.74), if i % 4 == 0 { pal.accent } else { pal.fg });
            }
            line(&[(0.1, 0.86), (0.9, 0.86)], thin);
        }
        Icon::NodeSmooth | Icon::NodeCorner | Icon::NodeLine | Icon::NodeCurve | Icon::NodeInsert | Icon::NodeBreak => {
            match icon {
                Icon::NodeSmooth => {
                    let pts: Vec<(f32, f32)> = (0..=20).map(|i| { let t = i as f32 / 20.0; (0.1 + 0.8 * t, 0.7 - 0.4 * (t * PI).sin()) }).collect();
                    line(&pts, st);
                    line(&[(0.25, 0.3), (0.75, 0.3)], thin);
                    dot(0.25, 0.3, 0.05, pal.fg);
                    dot(0.75, 0.3, 0.05, pal.fg);
                }
                Icon::NodeCorner => line(&[(0.1, 0.75), (0.5, 0.25), (0.9, 0.75)], st),
                Icon::NodeLine => line(&[(0.1, 0.7), (0.9, 0.3)], st),
                Icon::NodeCurve => {
                    let pts: Vec<(f32, f32)> = (0..=20).map(|i| { let t = i as f32 / 20.0; (0.1 + 0.8 * t, 0.7 - 0.4 * t - 0.3 * (t * PI).sin()) }).collect();
                    line(&pts, st);
                }
                Icon::NodeInsert => {
                    line(&[(0.1, 0.6), (0.9, 0.6)], st);
                    line(&[(0.5, 0.15), (0.5, 0.42)], ac);
                    line(&[(0.37, 0.28), (0.63, 0.28)], ac);
                }
                _ => {
                    line(&[(0.08, 0.6), (0.42, 0.6)], st);
                    line(&[(0.58, 0.6), (0.92, 0.6)], st);
                    line(&[(0.44, 0.25), (0.56, 0.4)], ac);
                }
            }
            let (cx, cy) = match icon {
                Icon::NodeSmooth => (0.5, 0.3),
                Icon::NodeCorner => (0.5, 0.25),
                Icon::NodeLine | Icon::NodeCurve => (0.5, 0.5),
                Icon::NodeInsert => (0.5, 0.6),
                _ => (0.42, 0.6),
            };
            let r = Rect::from_center_size(p(cx, cy), egui::vec2(s * 0.15, s * 0.15));
            rf(r, pal.accent);
        }
        Icon::CircArray => {
            circ(0.5, 0.5, 0.32, Stroke::new(w * 0.6, pal.fg));
            for i in 0..6 {
                let a = i as f32 * PI / 3.0 - PI / 2.0;
                let (x, y) = (0.5 + 0.32 * a.cos(), 0.5 + 0.32 * a.sin());
                rf(Rect::from_center_size(p(x, y), egui::vec2(s * 0.14, s * 0.14)), if i == 0 { pal.accent } else { pal.fg });
            }
        }
        Icon::Wand => {
            line(&[(0.15, 0.88), (0.62, 0.4)], Stroke::new(w * 1.4, pal.fg));
            for a in 0..8 {
                let t = a as f32 * PI / 4.0;
                line(&[(0.7 + 0.08 * t.cos(), 0.3 + 0.08 * t.sin()), (0.7 + 0.2 * t.cos(), 0.3 + 0.2 * t.sin())], ac);
            }
        }
        Icon::Lasso => {
            painter.add(egui::epaint::EllipseShape { center: p(0.5, 0.38), radius: egui::vec2(0.38 * s, 0.24 * s), fill: Color32::TRANSPARENT, stroke: Stroke::new(w, pal.accent), angle: 0.0 });
            line(&[(0.3, 0.58), (0.36, 0.78), (0.26, 0.92)], st);
        }
        Icon::SelOutline => {
            painter.add(Shape::dashed_line(&[p(0.1, 0.2), p(0.6, 0.2), p(0.6, 0.7), p(0.1, 0.7), p(0.1, 0.2)], Stroke::new(w * 0.7, pal.fg), s * 0.05, s * 0.04));
            rs(rect_at(0.4, 0.4, 0.9, 0.9), ac);
        }
        Icon::NewTab | Icon::DuplicateTab | Icon::CloseTab => {
            closed(&[(0.08, 0.86), (0.08, 0.34), (0.2, 0.18), (0.55, 0.18), (0.66, 0.34), (0.92, 0.34), (0.92, 0.86)], Color32::TRANSPARENT, st);
            match icon {
                Icon::NewTab => {
                    line(&[(0.5, 0.48), (0.5, 0.76)], ac);
                    line(&[(0.36, 0.62), (0.64, 0.62)], ac);
                }
                Icon::DuplicateTab => {
                    rs(rect_at(0.32, 0.48, 0.56, 0.74), ac);
                    rs(rect_at(0.44, 0.56, 0.68, 0.8), ac);
                }
                _ => {
                    line(&[(0.38, 0.5), (0.62, 0.74)], ac);
                    line(&[(0.62, 0.5), (0.38, 0.74)], ac);
                }
            }
        }
        Icon::Help => {
            circ(0.5, 0.5, 0.38, st);
            arc(0.5, 0.4, 0.12, PI, 2.4 * PI, ac);
            line(&[(0.5 + 0.12 * (0.4 * PI).cos(), 0.4 + 0.12 * (0.4 * PI).sin()), (0.5, 0.6)], ac);
            dot(0.5, 0.72, 0.035, pal.accent);
        }
        Icon::VarText => {
            // a table of rows with a highlighted one, and a T
            rs(rect_at(0.1, 0.2, 0.62, 0.85), thin);
            for y in [0.41, 0.63] {
                line(&[(0.1, y), (0.62, y)], thin);
            }
            rf(rect_at(0.12, 0.43, 0.6, 0.61), pal.soft);
            line(&[(0.66, 0.2), (0.92, 0.2)], ac);
            line(&[(0.79, 0.2), (0.79, 0.62)], ac);
        }
        Icon::Camera => {
            closed(&[(0.12, 0.32), (0.32, 0.32), (0.4, 0.2), (0.6, 0.2), (0.68, 0.32), (0.88, 0.32), (0.88, 0.8), (0.12, 0.8)], Color32::TRANSPARENT, st);
            circ(0.5, 0.56, 0.15, ac);
            dot(0.78, 0.42, 0.03, pal.accent);
        }
        Icon::Tour => {
            // a signpost: pole and two arrow boards
            line(&[(0.5, 0.12), (0.5, 0.9)], st);
            closed(&[(0.5, 0.2), (0.78, 0.2), (0.88, 0.3), (0.78, 0.4), (0.5, 0.4)], pal.soft, ac);
            closed(&[(0.5, 0.48), (0.22, 0.48), (0.12, 0.58), (0.22, 0.68), (0.5, 0.68)], Color32::TRANSPARENT, st);
            line(&[(0.36, 0.9), (0.64, 0.9)], st);
        }
        Icon::Join => {
            line(&[(0.12, 0.7), (0.3, 0.3), (0.48, 0.5)], st);
            line(&[(0.52, 0.5), (0.7, 0.7), (0.88, 0.3)], st);
            dot(0.5, 0.5, 0.08, pal.accent);
        }
        Icon::Relate => {
            line(&[(0.14, 0.78), (0.86, 0.78)], st);
            line(&[(0.2, 0.2), (0.2, 0.62)], st);
            closed(&[(0.52, 0.18), (0.86, 0.18), (0.86, 0.52), (0.52, 0.52)], pal.soft, ac);
            line(&[(0.6, 0.44), (0.78, 0.44)], ac);
            line(&[(0.69, 0.26), (0.69, 0.44)], ac);
        }
        Icon::Construction => {
            let dash = Stroke::new(w * 0.8, pal.accent);
            for i in 0..5 {
                let t0 = i as f32 / 5.0;
                let t1 = t0 + 0.12;
                line(&[(0.12 + 0.76 * t0, 0.82 - 0.64 * t0), (0.12 + 0.76 * t1, 0.82 - 0.64 * t1)], dash);
            }
            dot(0.12, 0.82, 0.06, pal.fg);
            dot(0.88, 0.18, 0.06, pal.fg);
        }
        Icon::MirrorLine => {
            let dash = Stroke::new(w * 0.8, pal.accent);
            for i in 0..4 {
                let y = 0.1 + i as f32 * 0.22;
                line(&[(0.5, y), (0.5, y + 0.12)], dash);
            }
            closed(&[(0.42, 0.25), (0.42, 0.75), (0.14, 0.75)], pal.soft, st);
            closed(&[(0.58, 0.25), (0.58, 0.75), (0.86, 0.75)], Color32::TRANSPARENT, ac);
        }
        Icon::PathPattern | Icon::AlignPath => {
            line(&[(0.1, 0.8), (0.3, 0.55), (0.5, 0.45), (0.7, 0.45), (0.9, 0.3)], thin);
            let marks: &[(f32, f32)] = if matches!(icon, Icon::PathPattern) { &[(0.18, 0.58), (0.5, 0.33), (0.82, 0.24)] } else { &[(0.3, 0.42), (0.6, 0.33)] };
            for (x, y) in marks {
                rf(Rect::from_center_size(p(*x, *y), egui::vec2(s * 0.16, s * 0.16)), pal.accent);
            }
        }
        Icon::Dimension => {
            line(&[(0.14, 0.2), (0.14, 0.62)], thin);
            line(&[(0.86, 0.2), (0.86, 0.62)], thin);
            line(&[(0.16, 0.5), (0.84, 0.5)], ac);
            arrow_head((0.16, 0.5), (-1.0, 0.0), ac);
            arrow_head((0.84, 0.5), (1.0, 0.0), ac);
            line(&[(0.14, 0.82), (0.86, 0.82)], st);
        }
        Icon::Laser => {
            closed(&[(0.3, 0.1), (0.7, 0.1), (0.7, 0.4), (0.3, 0.4)], pal.soft, st);
            line(&[(0.5, 0.4), (0.5, 0.78)], Stroke::new(w * 1.2, Color32::from_rgb(235, 70, 60)));
            for a in [-0.6f32, -0.3, 0.0, 0.3, 0.6] {
                line(&[(0.5, 0.86), (0.5 + 0.3 * a.sin(), 0.86 + 0.1 * a.cos().abs())], ac);
            }
        }
    }
}
