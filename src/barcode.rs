//! QR / Data Matrix / 1D barcodes as engravable vector rectangles.

use crate::geom::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Qr,
    DataMatrix,
    Code128,
    Ean13,
    UpcA,
    Code39,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Qr, Kind::DataMatrix, Kind::Code128, Kind::Ean13, Kind::UpcA, Kind::Code39];
    pub fn label(self) -> &'static str {
        match self {
            Kind::Qr => "QR code",
            Kind::DataMatrix => "Data Matrix",
            Kind::Code128 => "Code 128",
            Kind::Ean13 => "EAN-13",
            Kind::UpcA => "UPC-A",
            Kind::Code39 => "Code 39",
        }
    }
    pub fn is_2d(self) -> bool {
        matches!(self, Kind::Qr | Kind::DataMatrix)
    }
    pub fn hint(self) -> &'static str {
        match self {
            Kind::Qr => "Any text or a web address",
            Kind::DataMatrix => "Any text; small and robust, common on parts",
            Kind::Code128 => "Letters, digits and symbols",
            Kind::Ean13 => "12 digits (the check digit is added)",
            Kind::UpcA => "11 digits (the check digit is added)",
            Kind::Code39 => "Capital letters, digits, - . $ / + % and space",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BarcodeState {
    pub kind: Kind,
    pub text: String,
    /// Overall width in mm.
    pub width: f64,
    /// Bar height in mm (1D codes).
    pub height: f64,
    pub quiet_zone: bool,
    /// Burn the light modules instead of the dark ones (for dark materials such as slate or anodised metal).
    pub invert: bool,
    pub human_text: bool,
}

impl Default for BarcodeState {
    fn default() -> Self {
        BarcodeState { kind: Kind::Qr, text: "https://github.com/".into(), width: 30.0, height: 15.0, quiet_zone: true, invert: false, human_text: true }
    }
}

/// Dark modules on a grid (row-major), or a 1D bar pattern as a single row.
pub struct Matrix {
    pub w: usize,
    pub h: usize,
    pub dark: Vec<bool>,
}

pub fn encode(kind: Kind, text: &str) -> Result<Matrix, String> {
    let bars = |v: Vec<u8>| Matrix { w: v.len(), h: 1, dark: v.into_iter().map(|b| b == 1).collect() };
    match kind {
        Kind::Qr => {
            let q = qrcode::QrCode::new(text.as_bytes()).map_err(|e| format!("Can't make a QR code: {e:?}"))?;
            let w = q.width();
            Ok(Matrix { w, h: w, dark: q.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect() })
        }
        Kind::DataMatrix => {
            let dm = datamatrix::DataMatrix::encode_str(text, datamatrix::SymbolList::default()).map_err(|e| format!("Can't make a Data Matrix: {e:?}"))?;
            let bm = dm.bitmap();
            let (w, h) = (bm.width(), bm.height());
            let mut dark = vec![false; w * h];
            for (x, y) in bm.pixels() {
                if x < w && y < h {
                    dark[y * w + x] = true;
                }
            }
            Ok(Matrix { w, h, dark })
        }
        Kind::Code128 => {
            let c = barcoders::sym::code128::Code128::new(format!("\u{0181}{text}")).map_err(|e| format!("Code 128 can't hold that text: {e:?}"))?;
            Ok(bars(c.encode()))
        }
        Kind::Ean13 => {
            let digits: String = text.chars().filter(|c| c.is_ascii_digit()).take(12).collect();
            let c = barcoders::sym::ean13::EAN13::new(&digits).map_err(|e| format!("EAN-13 needs 12 digits: {e:?}"))?;
            Ok(bars(c.encode()))
        }
        Kind::UpcA => {
            let digits: String = text.chars().filter(|c| c.is_ascii_digit()).take(11).collect();
            if digits.len() != 11 {
                return Err("UPC-A needs 11 digits.".into());
            }
            // UPC-A is EAN-13 with a leading zero
            let c = barcoders::sym::ean13::EAN13::new(format!("0{digits}")).map_err(|e| format!("UPC-A: {e:?}"))?;
            Ok(bars(c.encode()))
        }
        Kind::Code39 => {
            let c = barcoders::sym::code39::Code39::new(text.to_uppercase()).map_err(|e| format!("Code 39 can't hold that text: {e:?}"))?;
            Ok(bars(c.encode()))
        }
    }
}

/// Closed rectangles (mm, centred on the origin, y up) for the modules to burn.
pub fn outlines(st: &BarcodeState, m: &Matrix) -> Vec<Vec<P2>> {
    let quiet = if st.quiet_zone { if st.kind.is_2d() { 2 } else { 10 } } else { 0 };
    let cols = m.w + quiet * 2;
    let module = st.width / cols.max(1) as f64;
    let (rows, row_h) = if st.kind.is_2d() { (m.h + quiet * 2, module) } else { (1, st.height) };
    let total_h = rows as f64 * row_h;
    let x0 = -st.width / 2.0;
    let y_top = total_h / 2.0;
    let dark_at = |x: isize, y: isize| -> bool {
        let (mx, my) = (x - quiet as isize, if st.kind.is_2d() { y - quiet as isize } else { 0 });
        mx >= 0 && my >= 0 && (mx as usize) < m.w && (my as usize) < m.h && m.dark[my as usize * m.w + mx as usize]
    };
    let mut out = Vec::new();
    for r in 0..rows {
        let mut c = 0;
        while c < cols {
            let burn = |c: usize| dark_at(c as isize, r as isize) != st.invert;
            if !burn(c) {
                c += 1;
                continue;
            }
            let start = c;
            while c < cols && burn(c) {
                c += 1;
            }
            let (xa, xb) = (x0 + start as f64 * module, x0 + c as f64 * module);
            let (ya, yb) = (y_top - (r + 1) as f64 * row_h, y_top - r as f64 * row_h);
            out.push(vec![p2(xa, ya), p2(xb, ya), p2(xb, yb), p2(xa, yb), p2(xa, ya)]);
        }
    }
    // merge vertically stacked identical runs into taller rectangles (fewer, cleaner fills)
    merge_columns(out)
}

fn merge_columns(rects: Vec<Vec<P2>>) -> Vec<Vec<P2>> {
    let mut boxes: Vec<BBox> = rects.iter().map(|r| BBox::from_points(r)).collect();
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..boxes.len() {
            for j in 0..boxes.len() {
                if i == j {
                    continue;
                }
                let (a, b) = (boxes[i], boxes[j]);
                if (a.min.x - b.min.x).abs() < 1e-9 && (a.max.x - b.max.x).abs() < 1e-9 && (a.min.y - b.max.y).abs() < 1e-9 {
                    boxes[i] = BBox { min: b.min, max: a.max };
                    boxes.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    boxes.into_iter().map(|b| vec![b.min, p2(b.max.x, b.min.y), b.max, p2(b.min.x, b.max.y), b.min]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qr_and_bars() {
        let st = BarcodeState::default();
        let m = encode(Kind::Qr, "hello").unwrap();
        assert!(m.w >= 21);
        let o = outlines(&st, &m);
        assert!(!o.is_empty());
        let b = o.iter().fold(BBox::EMPTY, |b, r| b.union(&BBox::from_points(r)));
        assert!(b.width() <= st.width + 1e-6);
        for k in [Kind::Code128, Kind::Code39, Kind::DataMatrix] {
            assert!(encode(k, "ABC123").is_ok(), "{:?}", k);
        }
        assert!(encode(Kind::Ean13, "590123412345").is_ok());
        assert!(encode(Kind::UpcA, "03600029145").is_ok());
    }
}
