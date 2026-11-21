//! Cached values: numbers, units, colours, style sheets and warnings.

use std::collections::HashMap;

use crate::model::Rgba;
use crate::sheet::{Cell, RowKey, Sheet};
use crate::{Warning, WarningKind};

/// Most warnings kept for one read; one more says how many were dropped.
const MAX_WARNINGS: usize = 1_000;
/// Largest length read, in inches: a larger one would overflow once scaled to points.
const MAX_LENGTH: f64 = 1e12;
/// Most style sheets followed up a parent chain.
const MAX_STYLE_CHAIN: usize = 32;

/// Warnings, capped.
#[derive(Default)]
pub(crate) struct Sink {
    pub list: Vec<Warning>,
    dropped: usize,
}

impl Sink {
    pub fn push(&mut self, kind: WarningKind, place: &str, message: impl Into<String>) {
        if self.list.len() < MAX_WARNINGS {
            self.list.push(Warning { kind, place: place.to_string(), message: message.into() });
        } else {
            self.dropped += 1;
        }
    }

    pub fn finish(mut self) -> Vec<Warning> {
        if self.dropped > 0 {
            self.list.push(Warning { kind: WarningKind::Limit, place: String::new(), message: format!("{} more warnings left out", self.dropped) });
        }
        self.list
    }
}

/// A finite number, or None.
pub(crate) fn number(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Inches per unit for the length units of `U` (MS-VSDX §2.5.8 and the Cell_Type `U` list).
fn inches_per(unit: &str) -> Option<f64> {
    Some(match unit {
        "" | "IN" | "IN_F" | "DL" | "NUM" | "PNT" => 1.0,
        "FT" | "FT_I" => 12.0,
        "YD" => 36.0,
        "MI" => 63_360.0,
        "MM" => 1.0 / 25.4,
        "CM" => 1.0 / 2.54,
        "M" => 1.0 / 0.0254,
        "KM" => 1000.0 / 0.0254,
        "PT" => 1.0 / 72.0,
        "PC" => 1.0 / 6.0,
        _ => return None,
    })
}

/// What reading a cell found.
pub(crate) enum Value {
    Absent,
    Num(f64),
    Bad,
}

/// The cell's `V` as a number in its own unit, with a warning for an unreadable one.
fn raw(cell: Option<&Cell>, sink: &mut Sink, place: &str, name: &str) -> Value {
    let Some(v) = cell.and_then(|c| c.v.as_deref()) else { return Value::Absent };
    match number(v) {
        Some(n) => Value::Num(n),
        None => {
            sink.push(WarningKind::BadValue, place, format!("{name}: unreadable value {:?}; its default is used", clip(v)));
            Value::Bad
        }
    }
}

fn clip(v: &str) -> String {
    v.chars().take(32).collect()
}

/// A length cell in inches. Units this crate does not know are read as inches, with a
/// warning.
pub(crate) fn length(cell: Option<&Cell>, sink: &mut Sink, place: &str, name: &str) -> Option<f64> {
    let Value::Num(n) = raw(cell, sink, place, name) else { return None };
    let unit = cell.and_then(|c| c.u.as_deref()).unwrap_or("");
    match inches_per(unit) {
        Some(k) => Some((n * k).clamp(-MAX_LENGTH, MAX_LENGTH)).filter(|f| f.is_finite()),
        None => {
            sink.push(WarningKind::BadValue, place, format!("{name}: unit {:?} read as inches", clip(unit)));
            Some(n.clamp(-MAX_LENGTH, MAX_LENGTH))
        }
    }
}

/// A plain number (scalar, boolean, index) cell.
pub(crate) fn scalar(cell: Option<&Cell>, sink: &mut Sink, place: &str, name: &str) -> Option<f64> {
    match raw(cell, sink, place, name) {
        Value::Num(n) => Some(n),
        Value::Absent | Value::Bad => None,
    }
}

/// An angle cell in radians: `DEG` is degrees, anything else radians.
pub(crate) fn angle(cell: Option<&Cell>, sink: &mut Sink, place: &str, name: &str) -> Option<f64> {
    let n = scalar(cell, sink, place, name)?;
    Some(if cell.and_then(|c| c.u.as_deref()) == Some("DEG") { n.to_radians() } else { n })
}

/// A colour value.
pub(crate) enum Colour {
    Rgb(Rgba),
    Themed,
    Bad,
}

fn hex(s: &str) -> Option<Rgba> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some(Rgba::rgb(p(0)?, p(2)?, p(4)?))
}

fn rgb_fn(s: &str) -> Option<Rgba> {
    let inner = s.trim().strip_prefix("RGB(")?.strip_suffix(')')?;
    let mut it = inner.split(',').map(|p| number(p).filter(|n| (0.0..=255.0).contains(n)).map(|n| n.round() as u8));
    let c = Rgba::rgb(it.next()??, it.next()??, it.next()??);
    if it.next().is_some() { None } else { Some(c) }
}

/// Reads a colour: `#RRGGBB`, `RGB(r,g,b)` or an index into the document's colour table
/// (0 black and 1 white when the table lacks them, as style 0 uses them in §3.1.1).
pub(crate) fn colour(v: &str, table: &HashMap<u32, Rgba>) -> Colour {
    let v = v.trim();
    if v.eq_ignore_ascii_case("themed") {
        return Colour::Themed;
    }
    if let Some(c) = hex(v).or_else(|| rgb_fn(v)) {
        return Colour::Rgb(c);
    }
    match v.parse::<u32>() {
        Ok(i) => match table.get(&i) {
            Some(c) => Colour::Rgb(*c),
            None if i == 0 => Colour::Rgb(Rgba::BLACK),
            None if i == 1 => Colour::Rgb(Rgba::WHITE),
            None => Colour::Bad,
        },
        Err(_) => Colour::Bad,
    }
}

/// Which style chain a cell inherits through.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    Line,
    Fill,
    Text,
}

/// A style sheet and the parents of each kind.
pub(crate) struct Style {
    pub sheet: Sheet,
    pub line: Option<u32>,
    pub fill: Option<u32>,
    pub text: Option<u32>,
}

/// The document's style sheets and colour table.
#[derive(Default)]
pub(crate) struct Styles {
    pub sheets: HashMap<u32, Style>,
    pub colors: HashMap<u32, Rgba>,
}

/// The values of style 0 "No Style" in the sample of §3.1.1, used when the document has none.
fn builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "LineWeight" => "0.01041666666666667",
        "LineColor" => "0",
        "LinePattern" => "1",
        "FillForegnd" => "1",
        "FillPattern" => "1",
        "Color" => "0",
        _ => return None,
    })
}

impl Styles {
    fn chain(&self, start: Option<u32>, cat: Category) -> impl Iterator<Item = &Style> {
        let mut next = start;
        std::iter::from_fn(move || {
            let s = self.sheets.get(&next?)?;
            next = match cat {
                Category::Line => s.line,
                Category::Fill => s.fill,
                Category::Text => s.text,
            };
            Some(s)
        })
        .take(MAX_STYLE_CHAIN)
    }

    /// A shape-level cell from the style chain, or the built-in style 0 value.
    pub fn cell(&self, start: Option<u32>, cat: Category, name: &str) -> Option<Cell> {
        self.chain(start, cat)
            .find_map(|s| s.sheet.cell(name).cloned())
            .or_else(|| builtin(name).map(|v| Cell { v: Some(v.to_string()), u: None, f: None }))
    }

    /// A cell of row `ix` of `section` from the style chain, or the built-in value.
    pub fn row_cell(&self, start: Option<u32>, cat: Category, section: &str, ix: u32, name: &str) -> Option<Cell> {
        self.chain(start, cat)
            .find_map(|s| s.sheet.section(section, 0).and_then(|sec| sec.rows.get(&RowKey::Ix(ix))).and_then(|r| r.cells.get(name)).cloned())
            .or_else(|| builtin(name).map(|v| Cell { v: Some(v.to_string()), u: None, f: None }))
    }
}

/// A resolved shape sheet with its styles: the shape's own cells first, then the style chain.
pub(crate) struct View<'a> {
    pub sheet: &'a Sheet,
    pub styles: &'a Styles,
    pub line: Option<u32>,
    pub fill: Option<u32>,
    pub text: Option<u32>,
}

fn category(name: &str) -> Option<Category> {
    if name.starts_with("Line") || name.ends_with("Arrow") || name.ends_with("ArrowSize") || name == "Rounding" {
        Some(Category::Line)
    } else if name.starts_with("Fill") {
        Some(Category::Fill)
    } else {
        None
    }
}

impl View<'_> {
    pub fn cell(&self, name: &str) -> Option<Cell> {
        if let Some(c) = self.sheet.cell(name) {
            return Some(c.clone());
        }
        let cat = category(name)?;
        let start = if cat == Category::Line { self.line } else { self.fill };
        self.styles.cell(start, cat, name)
    }

    /// The text colour: Character row 0 `Color`, from the shape or its text style.
    pub fn text_color(&self) -> Option<Cell> {
        let own = self.sheet.section("Character", 0).and_then(|s| s.rows.get(&RowKey::Ix(0))).and_then(|r| r.cells.get("Color"));
        own.cloned().or_else(|| self.styles.row_cell(self.text, Category::Text, "Character", 0, "Color"))
    }
}
