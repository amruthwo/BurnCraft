//! Rotary attachments: burning around cylinders (tumblers, bottles, pens) with a roller or chuck rotary.
//!
//! The design is drawn flat, as if the object's surface were unrolled: its height is distance around the
//! object. For the job, distances along Y are turned into rotary motor travel. Either the rotary replaces the
//! Y motor (the usual way on GRBL and K40 machines), or it's a separate A axis (G-code controllers with four
//! axes, e.g. FluidNC), in which case the G-code says A instead of Y.
//!
//! Travel per mm of the object's surface:
//! - chuck (the object turns with the chuck): units per turn ÷ (π × object diameter);
//! - roller (the object rests on rollers): units per turn ÷ (π × roller diameter), whatever the object's size.

use crate::geom::*;
use crate::model::Document;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RotaryKind {
    #[default]
    Roller,
    Chuck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RotaryAxis {
    /// The rotary is plugged in instead of the Y motor.
    #[default]
    Y,
    /// A fourth axis (G-code "A").
    A,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotarySettings {
    pub enabled: bool,
    pub kind: RotaryKind,
    pub axis: RotaryAxis,
    /// How far the controller has to move its axis for one full turn of the chuck or roller, in its own units
    /// (mm on a Y axis; often 360 degrees on an A axis). Measure it once: mark the roller, jog until it has
    /// turned exactly once, read the distance.
    pub units_per_turn: f64,
    /// Roller rotaries: diameter of the roller that turns the object (mm).
    pub roller_diameter: f64,
    /// Diameter of the object being burned (mm).
    pub object_diameter: f64,
    /// Turn the other way (if burns come out upside down).
    pub reverse: bool,
}

impl Default for RotarySettings {
    fn default() -> Self {
        RotarySettings { enabled: false, kind: RotaryKind::Roller, axis: RotaryAxis::Y, units_per_turn: 100.0, roller_diameter: 50.0, object_diameter: 80.0, reverse: false }
    }
}

impl RotarySettings {
    /// Distance once around the object (mm): the most the design can be tall.
    pub fn circumference(&self) -> f64 {
        PI * self.object_diameter.max(0.1)
    }

    /// Controller travel per mm of the object's surface.
    pub fn scale(&self) -> f64 {
        let d = match self.kind {
            RotaryKind::Chuck => self.object_diameter,
            RotaryKind::Roller => self.roller_diameter,
        };
        let s = self.units_per_turn / (PI * d.max(0.1));
        if self.reverse {
            -s
        } else {
            s
        }
    }

    /// Bed position (flat design) → where the controller's axes have to go, turning about the line `y0`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_machine(&self, p: P2, y0: f64) -> P2 {
        p2(p.x, y0 + (p.y - y0) * self.scale())
    }
}

/// The line the object turns about: the middle of the shapes that will burn (else the middle of the bed).
pub fn centre_line(doc: &Document, bed_h: f64) -> f64 {
    let b = doc.bbox();
    if b.is_empty() {
        bed_h / 2.0
    } else {
        b.center().y
    }
}

/// The document as the controller has to see it: Y distances turned into rotary travel.
pub fn machine_doc(doc: &Document, r: &RotarySettings, bed_h: f64) -> Document {
    let y0 = centre_line(doc, bed_h);
    let k = r.scale();
    // scale Y about y0: translate down, scale, translate back
    let x = Xform::translate(0.0, y0).then_after(Xform::scale(1.0, k)).then_after(Xform::translate(0.0, -y0));
    let mut d = doc.clone();
    for s in &mut d.shapes {
        s.xform = x.then_after(s.xform);
    }
    d
}

/// For an A-axis rotary: G-code with the Y words turned into A words (the values are already rotary travel).
pub fn y_to_a(gcode: &str) -> String {
    let mut out = String::with_capacity(gcode.len());
    for line in gcode.lines() {
        let t = line.trim_start();
        let is_move = t.starts_with("G0 ") || t.starts_with("G1 ") || t.starts_with("G00 ") || t.starts_with("G01 ") || t.starts_with("G0\t") || t.starts_with("G1\t");
        if is_move && !t.starts_with(';') {
            let words: Vec<String> = line.split(' ').map(|w| if let Some(v) = w.strip_prefix('Y') { format!("A{v}") } else { w.to_string() }).collect();
            out.push_str(&words.join(" "));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Problems worth telling the user before a rotary job.
pub fn warnings(doc: &Document, r: &RotarySettings) -> Vec<String> {
    let mut w = Vec::new();
    let b = doc.bbox();
    if !b.is_empty() && b.height() > r.circumference() + 1e-6 {
        w.push(format!("The design is {:.1} mm tall but the object is only {:.1} mm around: it would overlap itself.", b.height(), r.circumference()));
    }
    if r.units_per_turn <= 0.0 {
        w.push("Set how far the rotary axis moves for one turn (Rotary window).".into());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn travel_per_surface_mm() {
        // chuck: one turn = 100 mm of Y travel; an 80 mm object is 251.3 mm around
        let c = RotarySettings { kind: RotaryKind::Chuck, units_per_turn: 100.0, object_diameter: 80.0, ..Default::default() };
        assert!((c.scale() - 100.0 / (PI * 80.0)).abs() < 1e-12);
        assert!((c.circumference() - 251.327).abs() < 1e-3);
        // going once around the object moves the axis exactly one turn
        let m = c.to_machine(p2(10.0, 50.0 + c.circumference()), 50.0);
        assert!((m.y - 150.0).abs() < 1e-9 && m.x == 10.0);
        // roller: the object's size doesn't matter, the roller's does
        let r = RotarySettings { kind: RotaryKind::Roller, units_per_turn: 100.0, roller_diameter: 50.0, object_diameter: 300.0, ..Default::default() };
        assert!((r.scale() - 100.0 / (PI * 50.0)).abs() < 1e-12);
        assert_eq!(RotarySettings { reverse: true, ..r.clone() }.scale(), -r.scale());
    }

    #[test]
    fn machine_doc_scales_about_the_design_middle_and_a_axis_words() {
        let mut doc = Document::default();
        doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Rect { w: 20.0, h: 40.0, cr: 0.0 }, Xform::translate(30.0, 60.0), 0));
        let r = RotarySettings { kind: RotaryKind::Chuck, units_per_turn: 360.0, object_diameter: 360.0 / PI / 2.0, ..Default::default() };
        // scale = 360 / (π · d) = 2
        let m = machine_doc(&doc, &r, 200.0);
        let b = m.bbox();
        assert!((b.height() - 80.0).abs() < 1e-6 && (b.width() - 20.0).abs() < 1e-6);
        assert!((b.center().y - 60.0).abs() < 1e-6, "turns about the design's middle");
        assert_eq!(y_to_a("G0 X1.000 Y2.000\nG1 X3 Y4 S100\n; Y5 comment\nM5\n"), "G0 X1.000 A2.000\nG1 X3 A4 S100\n; Y5 comment\nM5\n");
        // 40 mm tall fits around a 57.3 mm object (180 mm around), but not around a 10 mm one (31.4 mm)
        assert!(warnings(&doc, &r).is_empty());
        assert_eq!(warnings(&doc, &RotarySettings { object_diameter: 10.0, ..r }).len(), 1);
    }
}
