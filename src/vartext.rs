//! Variable text: text shapes marked "variable" fill in placeholders from a .csv / .txt file, a serial number
//! and the date, so the same design can burn a different name, number or line on every job.
//!
//! Placeholders: `%0`, `%1`… a column of the current row (from 0); `{Name}` a column by its heading;
//! `{serial}` the serial number; `{row}` the row number (from 1); `{date}` and `{time}`; `%%` a plain %.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VarText {
    /// Where the rows came from (shown, and reloaded when the project opens if it's still there).
    pub file: String,
    pub has_headers: bool,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Current row (0-based).
    pub row: usize,
    pub serial: i64,
    pub serial_step: i64,
    /// Pad the serial number with zeros to this many digits (0 = no padding).
    pub serial_digits: usize,
    /// Move to the next row and serial number after each job is started.
    pub advance: bool,
}

impl Default for VarText {
    fn default() -> Self {
        VarText { file: String::new(), has_headers: true, headers: Vec::new(), rows: Vec::new(), row: 0, serial: 1, serial_step: 1, serial_digits: 0, advance: true }
    }
}

/// Most rows kept (and saved in the project).
pub const MAX_ROWS: usize = 20_000;

/// Split text into rows: .txt = one value per line; .csv = comma, semicolon or tab separated, with "quotes".
pub fn parse(text: &str, csv: bool) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !csv {
        return text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty()).map(|l| vec![l.to_string()]).take(MAX_ROWS).collect();
    }
    let first = text.lines().next().unwrap_or("");
    let sep = [',', ';', '\t'].into_iter().max_by_key(|c| first.matches(*c).count()).filter(|c| first.contains(*c)).unwrap_or(',');
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == sep {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            if row.iter().any(|f| !f.trim().is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
            if rows.len() >= MAX_ROWS {
                return rows;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|f| !f.trim().is_empty()) {
            rows.push(row);
        }
    }
    rows
}

impl VarText {
    /// Load rows from a file's text. `name` decides .csv or .txt.
    pub fn load(&mut self, name: &str, text: &str) {
        let csv = !name.to_ascii_lowercase().ends_with(".txt");
        let mut rows = parse(text, csv);
        self.headers.clear();
        if !csv {
            self.has_headers = false; // a .txt is just lines
        }
        if csv && self.has_headers && !rows.is_empty() {
            self.headers = rows.remove(0);
        }
        self.rows = rows;
        self.file = name.to_string();
        self.row = 0;
    }

    /// Re-split headers when "first row has headings" is switched.
    pub fn set_has_headers(&mut self, on: bool) {
        if on == self.has_headers {
            return;
        }
        self.has_headers = on;
        if on && self.headers.is_empty() && !self.rows.is_empty() {
            self.headers = self.rows.remove(0);
        } else if !on && !self.headers.is_empty() {
            let h = std::mem::take(&mut self.headers);
            self.rows.insert(0, h);
        }
        self.row = self.row.min(self.rows.len().saturating_sub(1));
    }

    pub fn columns(&self) -> usize {
        self.headers.len().max(self.rows.iter().map(|r| r.len()).max().unwrap_or(0))
    }

    pub fn serial_text(&self) -> String {
        if self.serial_digits > 0 {
            format!("{:0width$}", self.serial, width = self.serial_digits)
        } else {
            self.serial.to_string()
        }
    }

    /// Fill in the placeholders of `text` for the current row.
    pub fn resolve(&self, text: &str) -> String {
        let row = self.rows.get(self.row);
        let col = |i: usize| row.and_then(|r| r.get(i)).cloned().unwrap_or_default();
        let mut out = String::with_capacity(text.len());
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '%' && i + 1 < chars.len() {
                if chars[i + 1] == '%' {
                    out.push('%');
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() && j - i <= 3 {
                    j += 1;
                }
                if j > i + 1 {
                    let n: usize = chars[i + 1..j].iter().collect::<String>().parse().unwrap_or(0);
                    out.push_str(&col(n));
                    i = j;
                    continue;
                }
            }
            if c == '{' {
                if let Some(end) = chars[i + 1..].iter().position(|&c| c == '}') {
                    let key: String = chars[i + 1..i + 1 + end].iter().collect();
                    let k = key.trim().to_lowercase();
                    let value = match k.as_str() {
                        "serial" => Some(self.serial_text()),
                        "row" => Some((self.row + 1).to_string()),
                        "date" => Some(today().0),
                        "time" => Some(today().1),
                        _ => self.headers.iter().position(|h| h.trim().to_lowercase() == k).map(col),
                    };
                    if let Some(v) = value {
                        out.push_str(&v);
                        i += end + 2;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// After a job: next row (wrapping to the start) and next serial number.
    pub fn advance_row(&mut self) {
        if !self.rows.is_empty() {
            self.row = (self.row + 1) % self.rows.len();
        }
        self.serial += self.serial_step;
    }
}

/// ("YYYY-MM-DD", "HH:MM") in UTC from the system clock.
fn today() -> (String, String) {
    #[cfg(not(target_arch = "wasm32"))]
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    #[cfg(target_arch = "wasm32")]
    let secs = (js_sys::Date::now() / 1000.0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}", rem / 3600, rem % 3600 / 60))
}

/// The variable data the text outlines are drawn with (the active project's).
static CURRENT: RwLock<Option<VarText>> = RwLock::new(None);

pub fn set_current(v: &VarText) {
    if let Ok(mut c) = CURRENT.write() {
        if c.as_ref() != Some(v) {
            *c = Some(v.clone());
        }
    }
}

/// Fill in a variable text shape's placeholders with the current row.
pub fn resolve(text: &str) -> String {
    match CURRENT.read() {
        Ok(c) => match c.as_ref() {
            Some(v) => v.resolve(text),
            None => VarText::default().resolve(text),
        },
        Err(_) => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_rows_and_placeholders() {
        let mut v = VarText::default();
        v.load("names.csv", "Name,Town\r\n\"Smith, Jo\",Leeds\nAl,\"York \"\"North\"\"\"\n\n");
        assert_eq!(v.headers, vec!["Name", "Town"]);
        assert_eq!(v.rows.len(), 2);
        assert_eq!(v.resolve("%0 from %1"), "Smith, Jo from Leeds");
        v.row = 1;
        assert_eq!(v.resolve("{name} – {Town} #{serial} row {row} 100%%"), "Al – York \"North\" #1 row 2 100%");
        v.serial_digits = 4;
        v.advance_row();
        assert_eq!(v.row, 0, "wraps to the first row");
        assert_eq!(v.resolve("{serial}"), "0002");
        assert_eq!(v.resolve("{unknown} %9"), "{unknown} ");
    }

    #[test]
    fn txt_lines_and_semicolons() {
        let mut v = VarText::default();
        v.load("list.TXT", "first\n\nsecond line\n");
        assert_eq!(v.rows, vec![vec!["first".to_string()], vec!["second line".to_string()]]);
        assert!(v.headers.is_empty());
        let rows = parse("a;b;c\n1;2;3", true);
        assert_eq!(rows[1], vec!["1", "2", "3"]);
        v.set_has_headers(true);
        assert_eq!(v.headers, vec!["first"]);
        v.set_has_headers(false);
        assert_eq!(v.rows.len(), 2);
        let (d, t) = today();
        assert_eq!(d.len(), 10);
        assert_eq!(t.len(), 5);
    }
}
