//! Sheets as read: cells, sections and rows by name, with master inheritance (MS-VSDX
//! §2.2.5.4) applied by merging a local sheet over its base.

use std::collections::{BTreeMap, HashMap, HashSet};

use craft_xml::El;

/// A cell's cached value and its unit.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Cell {
    pub v: Option<String>,
    pub u: Option<String>,
    pub f: Option<String>,
}

/// A row key: its `IX`, or its name in a named section.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RowKey {
    Ix(u32),
    Name(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub t: Option<String>,
    pub del: bool,
    pub cells: BTreeMap<String, Cell>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Section {
    pub del: bool,
    pub cells: BTreeMap<String, Cell>,
    pub rows: BTreeMap<RowKey, Row>,
}

/// The cells and sections of a shape, style or page sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Sheet {
    pub cells: BTreeMap<String, Cell>,
    /// By section name and `IX` (0 for sections that do not repeat).
    pub sections: BTreeMap<(String, u32), Section>,
}

/// A shape as read from a page or master part, before inheritance.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawShape {
    pub id: u32,
    pub kind: String,
    pub name: String,
    pub master: Option<u32>,
    pub master_shape: Option<u32>,
    pub del: bool,
    pub line_style: Option<u32>,
    pub fill_style: Option<u32>,
    pub text_style: Option<u32>,
    pub sheet: Sheet,
    pub text: Option<String>,
    pub subs: Vec<RawShape>,
}

fn cell(el: &El) -> Option<(String, Cell)> {
    let n = el.attr("N")?;
    Some((n.to_string(), Cell { v: el.attr("V").map(str::to_string), u: el.attr("U").map(str::to_string), f: el.attr("F").map(str::to_string) }))
}

fn row(el: &El) -> Row {
    Row { t: el.attr("T").map(str::to_string), del: el.flag("Del", false), cells: el.children("Cell").filter_map(cell).collect() }
}

fn section(el: &El) -> Section {
    let mut s = Section { del: el.flag("Del", false), ..Section::default() };
    let mut next: u32 = 0;
    for c in el.elements() {
        match c.local() {
            "Cell" => {
                if let Some((n, v)) = cell(c) {
                    s.cells.insert(n, v);
                }
            }
            "Row" => {
                // MS-VSDX Row_Type: IX for indexed sections, implicit by counting when absent;
                // N for named sections.
                let key = match (c.attr("IX").and_then(|v| v.trim().parse::<u32>().ok()), c.attr("N")) {
                    (Some(ix), _) => RowKey::Ix(ix),
                    (None, Some(n)) => RowKey::Name(n.to_string()),
                    (None, None) => RowKey::Ix(next),
                };
                if let RowKey::Ix(ix) = key {
                    next = ix.saturating_add(1);
                }
                s.rows.insert(key, row(c));
            }
            _ => {}
        }
    }
    s
}

/// Reads the `Cell` and `Section` children of a sheet element.
pub(crate) fn sheet(el: &El) -> Sheet {
    let mut sh = Sheet::default();
    let mut next_geometry: u32 = 0;
    for c in el.elements() {
        match c.local() {
            "Cell" => {
                if let Some((n, v)) = cell(c) {
                    sh.cells.insert(n, v);
                }
            }
            "Section" => {
                let Some(name) = c.attr("N") else { continue };
                let ix = match c.attr("IX").and_then(|v| v.trim().parse::<u32>().ok()) {
                    Some(ix) => ix,
                    None if name == "Geometry" => next_geometry,
                    None => 0,
                };
                if name == "Geometry" {
                    next_geometry = ix.saturating_add(1);
                }
                sh.sections.insert((name.to_string(), ix), section(c));
            }
            _ => {}
        }
    }
    sh
}

/// The plain text of a `Text` element: its text with the `cp`, `pp` and `tp` markers left out
/// and the text of `fld` fields kept.
pub(crate) fn text(el: &El) -> String {
    el.deep_text()
}

fn u32_attr(el: &El, name: &str) -> Option<u32> {
    el.attr(name).and_then(|v| v.trim().parse().ok())
}

/// Longest shape name kept, in characters: a longer one is cut, because every instance of a
/// master copies the names of the master's shapes.
pub(crate) const MAX_NAME: usize = 256;

/// What reading cut short.
#[derive(Default)]
pub(crate) struct Cuts {
    /// Sub-shapes left out below the group depth limit.
    pub depth: bool,
    /// A shape name cut to [`MAX_NAME`].
    pub name: bool,
}

/// Reads a `Shape` element and its sub-shapes. `depth` counts the groups above it; past
/// `max_depth` sub-shapes are left out and `cuts.depth` is set.
pub(crate) fn raw_shape(el: &El, depth: usize, max_depth: usize, cuts: &mut Cuts) -> RawShape {
    let mut name = el.attr("NameU").or_else(|| el.attr("Name")).unwrap_or("").to_string();
    if let Some((at, _)) = name.char_indices().nth(MAX_NAME) {
        name.truncate(at);
        cuts.name = true;
    }
    let mut s = RawShape {
        id: u32_attr(el, "ID").unwrap_or(0),
        kind: el.attr("Type").unwrap_or("Shape").to_string(),
        name,
        master: u32_attr(el, "Master"),
        master_shape: u32_attr(el, "MasterShape"),
        del: el.flag("Del", false),
        line_style: u32_attr(el, "LineStyle"),
        fill_style: u32_attr(el, "FillStyle"),
        text_style: u32_attr(el, "TextStyle"),
        sheet: sheet(el),
        text: el.child("Text").map(text),
        subs: Vec::new(),
    };
    if let Some(shapes) = el.child("Shapes") {
        if depth >= max_depth {
            cuts.depth = true;
        } else {
            s.subs = shapes.children("Shape").map(|c| raw_shape(c, depth + 1, max_depth, cuts)).collect();
        }
    }
    s
}

impl RawShape {
    /// The work copying this shape and its sub-shapes costs: one per shape, one per geometry
    /// or other section row, and one per 256 bytes of text.
    pub fn weight(&self) -> usize {
        let rows: usize = self.sheet.sections.values().map(|s| s.rows.len()).sum();
        let text = self.text.as_ref().map_or(0, |t| t.len() / 256);
        self.subs.iter().fold(1usize.saturating_add(rows).saturating_add(text), |w, c| w.saturating_add(c.weight()))
    }
}

fn merge_cells(local: &BTreeMap<String, Cell>, base: &BTreeMap<String, Cell>) -> BTreeMap<String, Cell> {
    let mut out = base.clone();
    for (n, c) in local {
        out.insert(n.clone(), c.clone());
    }
    out
}

fn merge_section(local: &Section, base: &Section) -> Section {
    let mut rows = BTreeMap::new();
    for (k, b) in &base.rows {
        match local.rows.get(k) {
            Some(l) if l.del => {}
            Some(l) => {
                rows.insert(k.clone(), Row { t: l.t.clone().or_else(|| b.t.clone()), del: false, cells: merge_cells(&l.cells, &b.cells) });
            }
            None => {
                rows.insert(k.clone(), b.clone());
            }
        }
    }
    for (k, l) in &local.rows {
        if !l.del && !base.rows.contains_key(k) {
            rows.insert(k.clone(), l.clone());
        }
    }
    Section { del: false, cells: merge_cells(&local.cells, &base.cells), rows }
}

/// `local` over `base`: a local cell, section or row replaces the inherited one by name and
/// IX, and a local section or row with `Del="1"` removes it.
pub(crate) fn merge(local: &Sheet, base: &Sheet) -> Sheet {
    let mut sections = BTreeMap::new();
    for (k, b) in &base.sections {
        match local.sections.get(k) {
            Some(l) if l.del => {}
            Some(l) => {
                sections.insert(k.clone(), merge_section(l, b));
            }
            None => {
                sections.insert(k.clone(), b.clone());
            }
        }
    }
    for (k, l) in &local.sections {
        if !l.del && !base.sections.contains_key(k) {
            let mut l = l.clone();
            l.rows.retain(|_, r| !r.del);
            sections.insert(k.clone(), l);
        }
    }
    Sheet { cells: merge_cells(&local.cells, &base.cells), sections }
}

/// A master sub-shape taken whole: its IDs are the master's, not the page's, so they become 0.
fn inherited(b: &RawShape) -> RawShape {
    RawShape { id: 0, subs: b.subs.iter().map(inherited).collect(), ..b.clone() }
}

/// Merges a shape over the master shape it inherits from. Sub-shapes match by `MasterShape`;
/// master sub-shapes the instance does not name are inherited whole.
pub(crate) fn merge_shape(local: &RawShape, base: &RawShape) -> RawShape {
    let by_id: HashMap<u32, &RawShape> = base.subs.iter().map(|b| (b.id, b)).collect();
    let named: HashSet<u32> = local.subs.iter().filter(|l| l.master.is_none()).filter_map(|l| l.master_shape).collect();
    let mut subs = Vec::new();
    for l in &local.subs {
        match l.master_shape.and_then(|id| by_id.get(&id)) {
            Some(b) if l.master.is_none() => subs.push(merge_shape(l, b)),
            _ => subs.push(l.clone()),
        }
    }
    for b in &base.subs {
        if !named.contains(&b.id) {
            subs.push(inherited(b));
        }
    }
    subs.retain(|s| !s.del);
    RawShape {
        id: local.id,
        kind: if local.subs.is_empty() && base.subs.is_empty() { local.kind.clone() } else { "Group".to_string() },
        name: if local.name.is_empty() { base.name.clone() } else { local.name.clone() },
        master: local.master,
        master_shape: local.master_shape,
        del: local.del,
        line_style: local.line_style.or(base.line_style),
        fill_style: local.fill_style.or(base.fill_style),
        text_style: local.text_style.or(base.text_style),
        sheet: merge(&local.sheet, &base.sheet),
        text: local.text.clone().or_else(|| base.text.clone()),
        subs,
    }
}

impl Sheet {
    pub fn cell(&self, n: &str) -> Option<&Cell> {
        self.cells.get(n)
    }

    pub fn section(&self, n: &str, ix: u32) -> Option<&Section> {
        self.sections.get(&(n.to_string(), ix))
    }

    /// The Geometry sections in IX order.
    pub fn geometry(&self) -> impl Iterator<Item = &Section> {
        self.sections.iter().filter(|((n, _), _)| n == "Geometry").map(|(_, s)| s)
    }
}
