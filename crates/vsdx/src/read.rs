//! Reading drawings and stencils.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use craft_opc::Package;
use craft_xml::{El, Options};

use crate::geom::{Affine, Ctx, PT, paths, vertices};
use crate::model::{ConnectionPoint, Connector, Drawing, Master, Page, Point, Rgba, Shape, Stencil};
use crate::sheet::{Cell, Cuts, MAX_NAME, RawShape, RowKey, Sheet, merge_shape, raw_shape, sheet};
use crate::values::{Colour, Sink, Style, Styles, View, angle, colour, length, scalar, short};
use crate::{Error, Limits, Warning, WarningKind};

/// Reads a `.vsdx` drawing with [`Limits::DEFAULT`].
pub fn read(bytes: &[u8]) -> Result<(Drawing, Vec<Warning>), Error> {
    read_with(bytes, &Limits::DEFAULT)
}

/// Reads a `.vsdx` drawing.
pub fn read_with(bytes: &[u8], limits: &Limits) -> Result<(Drawing, Vec<Warning>), Error> {
    let mut doc = Doc::open(bytes, limits)?;
    let drawing = doc.pages()?;
    Ok((drawing, doc.sink.finish()))
}

/// Reads the masters of a `.vssx` stencil (or of any package with a Masters part) with
/// [`Limits::DEFAULT`].
pub fn read_stencil(bytes: &[u8]) -> Result<(Stencil, Vec<Warning>), Error> {
    read_stencil_with(bytes, &Limits::DEFAULT)
}

/// Reads the masters of a `.vssx` stencil.
pub fn read_stencil_with(bytes: &[u8], limits: &Limits) -> Result<(Stencil, Vec<Warning>), Error> {
    let mut doc = Doc::open(bytes, limits)?;
    let mut stencil = Stencil::default();
    let mut count = 0;
    for id in doc.master_order.clone() {
        let Some(m) = doc.masters.get(&id) else { continue };
        let (name, local_name, unique_id, part) = (m.name.clone(), m.local_name.clone(), m.unique_id.clone(), m.part.clone());
        let Some(base) = m.base.clone() else { continue };
        let height = m.page_height.unwrap_or(0.0);
        let icon_png = part.and_then(|p| doc.icon(&p, &name));
        let mut flat = Flat {
            doc: &mut doc,
            page: short(&name),
            page_h: height,
            shapes: Vec::new(),
            connectors: Vec::new(),
            count: &mut count,
            glue: HashMap::new(),
            rows: HashMap::new(),
        };
        flat.walk(&base, &Affine::IDENTITY, 0)?;
        let shapes = flat.shapes;
        let (shapes, width, height) = normalise(shapes);
        stencil.masters.push(Master { name, local_name, unique_id, width, height, shapes, icon_png });
    }
    Ok((stencil, doc.sink.finish()))
}

/// Moves shapes so their bounding box starts at the origin; returns its size.
fn normalise(mut shapes: Vec<Shape>) -> (Vec<Shape>, f64, f64) {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for s in &shapes {
        for (x, y) in corners(s) {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    let Some((x0, y0, x1, y1)) = b else { return (shapes, 0.0, 0.0) };
    for s in &mut shapes {
        s.x -= x0;
        s.y -= y0;
    }
    (shapes, x1 - x0, y1 - y0)
}

/// The corners of a shape's rotated box.
fn corners(s: &Shape) -> [Point; 4] {
    let (cx, cy) = (s.x + s.width / 2.0, s.y + s.height / 2.0);
    let (sin, cos) = s.angle.to_radians().sin_cos();
    let at = |dx: f64, dy: f64| (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
    let (w, h) = (s.width / 2.0, s.height / 2.0);
    [at(-w, -h), at(w, -h), at(w, h), at(-w, h)]
}

struct MasterData {
    name: String,
    local_name: String,
    unique_id: Option<String>,
    page_height: Option<f64>,
    base: Option<RawShape>,
    /// The master part, for its icon.
    part: Option<String>,
    /// What copying `base` costs: see [`RawShape::weight`].
    weight: usize,
}

/// Largest icon image kept, in bytes.
const MAX_ICON: usize = 1024 * 1024;
/// Most icon bytes kept across a stencil; parts several masters share count once.
const MAX_ICON_TOTAL: usize = 32 * 1024 * 1024;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

struct Doc {
    pkg: Package,
    limits: Limits,
    doc_part: String,
    styles: Styles,
    masters: HashMap<u32, MasterData>,
    master_order: Vec<u32>,
    sink: Sink,
    /// Work left for copying masters into instances: shapes and rows.
    work: usize,
    /// Icon parts already read, by lower-case part name.
    icons: HashMap<String, Option<Arc<[u8]>>>,
    icon_bytes: usize,
}

fn u32_attr(el: &El, name: &str) -> Option<u32> {
    el.attr(name).and_then(|v| v.trim().parse().ok())
}

fn name_of(el: &El) -> (String, String) {
    let local = el.attr("Name").unwrap_or("").to_string();
    let universal = el.attr("NameU").map(str::to_string).unwrap_or_else(|| local.clone());
    (universal, local)
}

impl Doc {
    fn open(bytes: &[u8], limits: &Limits) -> Result<Doc, Error> {
        let pkg = Package::open(bytes, &limits.package)?;
        let rel = pkg.rels("").first("document").filter(|r| !r.external).cloned().ok_or_else(|| Error::NotVsdx("no document relationship".into()))?;
        let mut doc = Doc {
            pkg,
            limits: *limits,
            doc_part: rel.target,
            styles: Styles::default(),
            masters: HashMap::new(),
            master_order: Vec::new(),
            sink: Sink::default(),
            work: limits.max_shapes.saturating_add(limits.max_rows).saturating_mul(2),
            icons: HashMap::new(),
            icon_bytes: 0,
        };
        let root = doc.xml(&doc.doc_part.clone())?.ok_or_else(|| Error::NotVsdx("the document part is missing".into()))?;
        if root.local() != "VisioDocument" {
            return Err(Error::NotVsdx(format!("the document part's root is {:?}", root.local())));
        }
        doc.read_styles(&root);
        doc.read_masters()?;
        Ok(doc)
    }

    fn options(&self) -> Options<'static> {
        let max_input = usize::try_from(self.limits.package.max_part).unwrap_or(usize::MAX);
        let xml =
            craft_xml::Limits { max_input, max_depth: self.limits.max_xml_depth, max_nodes: self.limits.max_xml_nodes, ..craft_xml::Limits::DEFAULT };
        Options::local().with_limits(xml)
    }

    fn xml(&self, part: &str) -> Result<Option<El>, Error> {
        Ok(self.pkg.xml(part, &self.options())?)
    }

    fn read_styles(&mut self, root: &El) {
        for e in root.child("Colors").into_iter().flat_map(|c| c.children("ColorEntry")) {
            let ix = u32_attr(e, "IX");
            let rgb = e.attr("RGB").map(|v| colour(v, &HashMap::new()));
            if let (Some(ix), Some(Colour::Rgb(c))) = (ix, rgb) {
                self.styles.colors.insert(ix, c);
            }
        }
        for s in root.child("StyleSheets").into_iter().flat_map(|c| c.children("StyleSheet")) {
            let Some(id) = u32_attr(s, "ID") else { continue };
            let style = Style { sheet: sheet(s), line: u32_attr(s, "LineStyle"), fill: u32_attr(s, "FillStyle"), text: u32_attr(s, "TextStyle") };
            self.styles.sheets.insert(id, style);
        }
    }

    /// The part a `Rel r:id` child of `el` names, through the relationships of `source`.
    fn rel_target(&self, el: &El, source: &str) -> Option<String> {
        let id = el.child("Rel").and_then(|r| r.attr("id"))?;
        self.pkg.rels(source).by_id(id).filter(|r| !r.external).map(|r| r.target.clone())
    }

    fn read_masters(&mut self) -> Result<(), Error> {
        let Some(part) = self.pkg.rels(&self.doc_part).first("masters").filter(|r| !r.external).map(|r| r.target.clone()) else { return Ok(()) };
        let Some(root) = self.xml(&part)? else {
            self.sink.push(WarningKind::Missing, "masters", "the masters part is missing");
            return Ok(());
        };
        let mut seen = HashSet::new();
        for m in root.children("Master") {
            if self.master_order.len() >= self.limits.max_masters {
                return Err(Error::TooMany { what: "masters", limit: self.limits.max_masters });
            }
            let Some(id) = u32_attr(m, "ID") else { continue };
            let (name, local_name) = name_of(m);
            let place = format!("master {}", short(&name));
            let page_height = m.child("PageSheet").map(sheet).and_then(|s| length(s.cell("PageHeight"), &mut self.sink, &place, "PageHeight"));
            let target = self.rel_target(m, &part);
            let (base, part) = match target {
                Some(t) if seen.insert(t.to_ascii_lowercase()) => (self.master_base(&t, &place)?, Some(t)),
                _ => {
                    self.sink.push(WarningKind::Missing, &place, "no master part (or one another master already uses)");
                    (None, None)
                }
            };
            let weight = base.as_ref().map_or(0, RawShape::weight);
            let unique_id = m.attr("UniqueID").map(str::to_string);
            self.masters.insert(id, MasterData { name, local_name, unique_id, page_height, base, part, weight });
            self.master_order.push(id);
        }
        Ok(())
    }

    fn master_base(&mut self, part: &str, place: &str) -> Result<Option<RawShape>, Error> {
        let Some(root) = self.xml(part)? else {
            self.sink.push(WarningKind::Missing, place, "the master part is missing");
            return Ok(None);
        };
        let mut tops = self.raw_shapes(&root, place);
        tops.iter_mut().for_each(as_master_shape);
        Ok(match tops.len() {
            0 => None,
            1 => tops.pop(),
            _ => Some(RawShape { kind: "Group".into(), subs: tops, ..RawShape::default() }),
        })
    }

    /// The PNG image a master part relates to: one at most [`MAX_ICON`] bytes long that starts
    /// like a PNG, and within [`MAX_ICON_TOTAL`] across the stencil. Masters that relate to the
    /// same image share it.
    fn icon(&mut self, part: &str, name: &str) -> Option<Arc<[u8]>> {
        let rels = self.pkg.rels(part);
        let ct = self.pkg.content_types();
        let target = rels.all("image").filter(|r| !r.external).find_map(|r| {
            let png = ct.of(&r.target).is_some_and(|c| c.eq_ignore_ascii_case("image/png")) || r.target.to_ascii_lowercase().ends_with(".png");
            png.then(|| r.target.clone())
        })?;
        let key = target.to_ascii_lowercase();
        if let Some(known) = self.icons.get(&key) {
            return known.clone();
        }
        let place = format!("master {}", short(name));
        let bytes = self.pkg.get(&target)?;
        let found = if !bytes.starts_with(PNG_MAGIC) {
            self.sink.push(WarningKind::BadValue, &place, "icon is not a PNG image; left out");
            None
        } else if bytes.len() > MAX_ICON {
            self.sink.push(WarningKind::Limit, &place, format!("icon over {MAX_ICON} bytes left out"));
            None
        } else if self.icon_bytes.saturating_add(bytes.len()) > MAX_ICON_TOTAL {
            self.sink.push(WarningKind::Limit, &place, format!("icon left out: icons are over {MAX_ICON_TOTAL} bytes in all"));
            None
        } else {
            self.icon_bytes += bytes.len();
            Some(Arc::<[u8]>::from(bytes))
        };
        self.icons.insert(key, found.clone());
        found
    }

    /// The top-level shapes of a `PageContents` or `MasterContents` element.
    fn raw_shapes(&mut self, root: &El, place: &str) -> Vec<RawShape> {
        let mut cuts = Cuts::default();
        let shapes = root
            .child("Shapes")
            .into_iter()
            .flat_map(|s| s.children("Shape"))
            .map(|s| raw_shape(s, 0, self.limits.max_group_depth, &mut cuts))
            .collect();
        if cuts.depth {
            self.sink.push(WarningKind::Limit, place, format!("groups nested deeper than {} left out", self.limits.max_group_depth));
        }
        if cuts.name {
            self.sink.push(WarningKind::Limit, place, format!("shape names longer than {MAX_NAME} characters cut"));
        }
        shapes
    }

    /// A shape with its master applied, and its sub-shapes' masters.
    ///
    /// Copying a master costs its shapes and rows against a budget shared by the whole read,
    /// charged before the copy: a master with many parts instanced many times is refused
    /// rather than multiplied in memory.
    fn resolve(&mut self, s: &RawShape, place: &str) -> Result<RawShape, Error> {
        let mut out = match s.master {
            Some(m) => match self.masters.get(&m).and_then(|d| d.base.as_ref().map(|b| (b, d.weight))) {
                Some((base, weight)) => {
                    let Some(left) = self.work.checked_sub(weight) else {
                        return Err(Error::TooMany {
                            what: "shapes and rows copied from masters",
                            limit: self.limits.max_shapes.saturating_add(self.limits.max_rows).saturating_mul(2),
                        });
                    };
                    let merged = merge_shape(s, base);
                    self.work = left;
                    merged
                }
                None => {
                    self.sink.push(WarningKind::Missing, place, format!("shape {}: master {m} not found; drawn from its own cells", s.id));
                    s.clone()
                }
            },
            None => s.clone(),
        };
        out.subs = out.subs.iter().map(|c| self.resolve(c, place)).collect::<Result<_, _>>()?;
        Ok(out)
    }

    fn pages(&mut self) -> Result<Drawing, Error> {
        let mut drawing = Drawing::default();
        let Some(part) = self.pkg.rels(&self.doc_part).first("pages").filter(|r| !r.external).map(|r| r.target.clone()) else {
            self.sink.push(WarningKind::Missing, "document", "no pages part");
            return Ok(drawing);
        };
        let Some(root) = self.xml(&part)? else {
            self.sink.push(WarningKind::Missing, "document", "the pages part is missing");
            return Ok(drawing);
        };
        let mut count = 0usize;
        let mut seen = HashSet::new();
        for p in root.children("Page") {
            if drawing.pages.len() >= self.limits.max_pages {
                return Err(Error::TooMany { what: "pages", limit: self.limits.max_pages });
            }
            let (name, _) = name_of(p);
            match self.rel_target(p, &part) {
                Some(t) if seen.insert(t.to_ascii_lowercase()) => {
                    let page = self.page(p, &t, name, &mut count)?;
                    drawing.pages.push(page);
                }
                _ => self.sink.push(WarningKind::Missing, &short(&name), "no page part (or one another page already uses); page left out"),
            }
        }
        Ok(drawing)
    }

    fn page(&mut self, entry: &El, part: &str, name: String, count: &mut usize) -> Result<Page, Error> {
        let ps = entry.child("PageSheet").map(sheet).unwrap_or_default();
        let place = short(&name);
        let size = |n: &str, default: f64, sink: &mut Sink| match length(ps.cell(n), sink, &place, n) {
            Some(v) if v > 0.0 => v,
            _ => {
                sink.push(WarningKind::BadValue, &place, format!("no usable {n}; {default} inches used"));
                default
            }
        };
        let (w, h) = (size("PageWidth", 8.5, &mut self.sink), size("PageHeight", 11.0, &mut self.sink));
        let mut page = Page { name, width: w * PT, height: h * PT, ..Page::default() };
        let Some(root) = self.xml(part)? else {
            self.sink.push(WarningKind::Missing, &place, "the page part is missing");
            return Ok(page);
        };
        let tops = self.raw_shapes(&root, &place);
        let glue = connects(&root);
        let mut flat = Flat { doc: self, page: place, page_h: h, shapes: Vec::new(), connectors: Vec::new(), count, glue, rows: HashMap::new() };
        for t in tops.iter().filter(|t| !t.del) {
            let resolved = flat.doc.resolve(t, &flat.page)?;
            flat.walk(&resolved, &Affine::IDENTITY, 0)?;
        }
        flat.fix_glue();
        page.shapes = flat.shapes;
        page.connectors = flat.connectors;
        Ok(page)
    }
}

/// Prepares a shape of a master to be inherited: it is not an instance of a master itself
/// (following such a reference could never end), and shapes it deletes are dropped here so
/// that each instance does not copy them.
fn as_master_shape(s: &mut RawShape) {
    s.master = None;
    s.subs.retain(|c| !c.del);
    s.subs.iter_mut().for_each(as_master_shape);
}

/// The glue target a `ToCell` names: `Connections.X<n>` is point `n`, anything else the shape.
fn glue_point(to_cell: Option<&str>) -> ConnectionPoint {
    to_cell.and_then(|c| c.strip_prefix("Connections.X")).and_then(|n| n.parse().ok()).map_or(ConnectionPoint::Shape, ConnectionPoint::Point)
}

/// `Connects`: per connector ID, the glue of its begin and end.
type Glue = HashMap<u32, (Option<(u32, ConnectionPoint)>, Option<(u32, ConnectionPoint)>)>;

fn connects(root: &El) -> Glue {
    let mut out = Glue::new();
    for c in root.child("Connects").into_iter().flat_map(|c| c.children("Connect")) {
        let (Some(from), Some(to)) = (u32_attr(c, "FromSheet"), u32_attr(c, "ToSheet")) else { continue };
        let target = Some((to, glue_point(c.attr("ToCell"))));
        let entry = out.entry(from).or_default();
        match c.attr("FromCell") {
            Some(f) if f.starts_with("Begin") => entry.0 = target,
            Some(f) if f.starts_with("End") => entry.1 = target,
            _ => {}
        }
    }
    out
}

/// Glue from an endpoint formula: the first `Sheet.<id>!` reference, and the connection
/// point it names.
fn formula_glue(cell: Option<&Cell>) -> Option<(u32, ConnectionPoint)> {
    let f = cell?.f.as_deref()?;
    let at = f.find("Sheet.")?;
    let rest = f.get(at + 6..)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let id = digits.parse().ok()?;
    let cell = rest.get(digits.len()..)?.strip_prefix('!')?;
    let name: String = cell.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    Some((id, glue_point(Some(&name))))
}

/// Flattens one page (or master) into shapes and connectors.
struct Flat<'a> {
    doc: &'a mut Doc,
    page: String,
    /// Page height in inches, to turn y up into y down.
    page_h: f64,
    shapes: Vec<Shape>,
    connectors: Vec<Connector>,
    count: &'a mut usize,
    glue: Glue,
    /// The `IX` of each connection row of the shapes by ID, in the order of
    /// [`Shape::connections`].
    rows: HashMap<u32, Vec<u32>>,
}

/// A shape's transform cells, in inches and radians.
struct Xf {
    w: f64,
    h: f64,
    pin: Point,
    loc: Point,
    angle: f64,
    flip_x: bool,
    flip_y: bool,
}

impl Flat<'_> {
    fn len(&mut self, sh: &Sheet, n: &str, place: &str) -> Option<f64> {
        length(sh.cell(n), &mut self.doc.sink, place, n)
    }

    fn xf(&mut self, sh: &Sheet, place: &str) -> Xf {
        let w = self.len(sh, "Width", place).unwrap_or(0.0);
        let h = self.len(sh, "Height", place).unwrap_or(0.0);
        let pin = (self.len(sh, "PinX", place).unwrap_or(0.0), self.len(sh, "PinY", place).unwrap_or(0.0));
        let loc = (self.len(sh, "LocPinX", place).unwrap_or(w / 2.0), self.len(sh, "LocPinY", place).unwrap_or(h / 2.0));
        let sink = &mut self.doc.sink;
        let angle = angle(sh.cell("Angle"), sink, place, "Angle").unwrap_or(0.0);
        let flip_x = scalar(sh.cell("FlipX"), sink, place, "FlipX").is_some_and(|v| v != 0.0);
        let flip_y = scalar(sh.cell("FlipY"), sink, place, "FlipY").is_some_and(|v| v != 0.0);
        Xf { w, h, pin, loc, angle, flip_x, flip_y }
    }

    /// A page point (inches, y up) in model coordinates.
    fn model(&self, (x, y): Point) -> Point {
        (x * PT, (self.page_h - y) * PT)
    }

    fn walk(&mut self, s: &RawShape, parent: &Affine, depth: usize) -> Result<(), Error> {
        if s.del {
            return Ok(());
        }
        *self.count += 1;
        if *self.count > self.doc.limits.max_shapes {
            return Err(Error::TooMany { what: "shapes", limit: self.doc.limits.max_shapes });
        }
        let place = format!("{} shape {}", self.page, s.id);
        if s.kind == "Guide" {
            self.doc.sink.push(WarningKind::Unsupported, &place, "guide left out");
            return Ok(());
        }
        let xf = self.xf(&s.sheet, &place);
        let full = Affine::local_to_parent(xf.pin, xf.loc, xf.angle, xf.flip_x, xf.flip_y).then(parent);
        if s.sheet.cell("BeginX").is_some() && s.sheet.cell("EndX").is_some() {
            self.connector(s, &xf, &full, parent, &place);
        } else {
            let has_text = s.text.as_deref().is_some_and(|t| !t.is_empty());
            if s.kind != "Group" || s.sheet.geometry().next().is_some() || has_text {
                if s.kind == "Foreign" {
                    self.doc.sink.push(WarningKind::Unsupported, &place, "image or embedded object: only its box and text are read");
                }
                self.shape(s, &xf, &full, &place);
            }
        }
        if depth >= self.doc.limits.max_group_depth && !s.subs.is_empty() {
            self.doc.sink.push(WarningKind::Limit, &place, "groups nested too deep left out");
            return Ok(());
        }
        for c in &s.subs {
            self.walk(c, &full, depth + 1)?;
        }
        Ok(())
    }

    fn view<'v>(&'v self, s: &'v RawShape) -> View<'v> {
        View { sheet: &s.sheet, styles: &self.doc.styles, line: s.line_style, fill: s.fill_style, text: s.text_style }
    }

    /// A colour cell with its transparency cell; `themed` is the colour a theme value becomes.
    fn colour(&mut self, c: Option<Cell>, trans: Option<Cell>, themed: Rgba, place: &str, name: &str) -> Option<Rgba> {
        let v = c?.v?;
        let mut rgba = match colour(&v, &self.doc.styles.colors) {
            Colour::Rgb(c) => c,
            Colour::Themed => {
                self.doc.sink.push(
                    WarningKind::Themed,
                    place,
                    format!("{name}: theme colour drawn as #{:02X}{:02X}{:02X}", themed.r, themed.g, themed.b),
                );
                themed
            }
            Colour::Bad => {
                let shown: String = v.chars().take(32).collect();
                self.doc.sink.push(
                    WarningKind::BadValue,
                    place,
                    format!("{name}: unreadable colour {shown:?} drawn as #{:02X}{:02X}{:02X}", themed.r, themed.g, themed.b),
                );
                themed
            }
        };
        let t = scalar(trans.as_ref(), &mut self.doc.sink, place, name).unwrap_or(0.0).clamp(0.0, 1.0);
        rgba.a = ((1.0 - t) * 255.0).round() as u8;
        Some(rgba)
    }

    /// Line colour, weight in points and dashing.
    fn line(&mut self, s: &RawShape, place: &str) -> (Option<Rgba>, f64, bool) {
        let v = self.view(s);
        let (pattern, color, trans, weight) = (v.cell("LinePattern"), v.cell("LineColor"), v.cell("LineColorTrans"), v.cell("LineWeight"));
        let sink = &mut self.doc.sink;
        let pattern = scalar(pattern.as_ref(), sink, place, "LinePattern").unwrap_or(1.0);
        let weight = length(weight.as_ref(), sink, place, "LineWeight").unwrap_or(0.0).max(0.0) * PT;
        if pattern == 0.0 {
            return (None, weight, false);
        }
        (self.colour(color, trans, Rgba::BLACK, place, "LineColor"), weight, pattern >= 2.0)
    }

    fn fill(&mut self, s: &RawShape, place: &str) -> Option<Rgba> {
        let v = self.view(s);
        let (pattern, color, trans) = (v.cell("FillPattern"), v.cell("FillForegnd"), v.cell("FillForegndTrans"));
        let pattern = scalar(pattern.as_ref(), &mut self.doc.sink, place, "FillPattern").unwrap_or(1.0);
        if pattern == 0.0 {
            return None;
        }
        if pattern != 1.0 {
            self.doc.sink.push(WarningKind::Unsupported, place, format!("fill pattern {pattern} drawn as a solid fill"));
        }
        self.colour(color, trans, Rgba::WHITE, place, "FillForegnd")
    }

    fn shape(&mut self, s: &RawShape, xf: &Xf, full: &Affine, place: &str) {
        let (theta, mirror) = full.rotation();
        let centre = self.model(full.apply((xf.w / 2.0, xf.h / 2.0)));
        let (w, h) = (xf.w * PT, xf.h * PT);
        let mut ctx = Ctx { sink: &mut self.doc.sink, place, max_rows: self.doc.limits.max_rows };
        let geometry = paths(&s.sheet, (xf.w, xf.h), mirror, &mut ctx);
        let (connections, ixs) = self.connections(&s.sheet, (xf.w, xf.h), mirror, place);
        if s.id != 0 {
            self.rows.entry(s.id).or_insert(ixs);
        }
        let (line, line_weight, dashed) = self.line(s, place);
        let fill = self.fill(s, place);
        let tc = self.view(s).text_color();
        let text_color = self.colour(tc, None, Rgba::BLACK, place, "Color");
        let mut angle = -theta.to_degrees();
        if angle.abs() < 1e-9 {
            angle = 0.0;
        }
        self.shapes.push(Shape {
            id: s.id,
            name: s.name.clone(),
            master: s.master.and_then(|m| self.doc.masters.get(&m)).map(|m| m.name.clone()),
            x: centre.0 - w / 2.0,
            y: centre.1 - h / 2.0,
            width: w,
            height: h,
            angle,
            geometry,
            text: s.text.clone().unwrap_or_default(),
            fill,
            line,
            line_weight,
            dashed,
            text_color,
            connections,
        });
    }

    /// Connection section rows in the model's box space.
    /// The rows in `IX` order, with the `IX` of each: a gap in the numbering is kept in the
    /// `IX`es, so that glue to `Connections.X<n>` can be matched to its row.
    fn connections(&mut self, sh: &Sheet, (w, h): Point, mirror: bool, place: &str) -> (Vec<Point>, Vec<u32>) {
        let Some(sec) = sh.section("Connection", 0) else { return (Vec::new(), Vec::new()) };
        let rows: Vec<_> = sec
            .rows
            .iter()
            .filter(|(_, r)| !r.del)
            .filter_map(|(k, r)| if let RowKey::Ix(ix) = k { Some((*ix, r)) } else { None })
            .take(self.doc.limits.max_rows)
            .collect();
        rows.into_iter()
            .map(|(ix, r)| {
                let x = length(r.cells.get("X"), &mut self.doc.sink, place, "X").unwrap_or(0.0);
                let y = length(r.cells.get("Y"), &mut self.doc.sink, place, "Y").unwrap_or(0.0);
                (((if mirror { w - x } else { x }) * PT, (h - y) * PT), ix)
            })
            .unzip()
    }

    /// Makes the glue say what the model has: a connector end glued to a shape that is not
    /// in the model (a guide, a skipped group, a deleted shape) is left free, and glue to
    /// `Connections.X<n>` becomes the position of row `IX = n - 1` among the target's rows,
    /// or the whole shape when it has no such row. Each is reported.
    fn fix_glue(&mut self) {
        let known: HashSet<u32> = self.shapes.iter().map(|s| s.id).filter(|&id| id != 0).collect();
        let rows = &self.rows;
        let sink = &mut self.doc.sink;
        let place = &self.page;
        for c in &mut self.connectors {
            for (end, glue) in [("begin", &mut c.from), ("end", &mut c.to)] {
                let Some((target, at)) = *glue else { continue };
                if !known.contains(&target) {
                    sink.push(
                        WarningKind::Missing,
                        place,
                        format!("connector {}: its {end} is glued to shape {target}, which is not drawn; left free", c.id),
                    );
                    *glue = None;
                    continue;
                }
                let ConnectionPoint::Point(n) = at else { continue };
                let position = n.checked_sub(1).and_then(|ix| rows.get(&target)?.iter().position(|&r| r == ix));
                match position.and_then(|p| u32::try_from(p + 1).ok()) {
                    Some(p) => *glue = Some((target, ConnectionPoint::Point(p))),
                    None => {
                        sink.push(
                            WarningKind::Missing,
                            place,
                            format!(
                                "connector {}: its {end} is glued to connection point {n} of shape {target}, which has none; glued to the shape",
                                c.id
                            ),
                        );
                        *glue = Some((target, ConnectionPoint::Shape));
                    }
                }
            }
        }
    }

    fn connector(&mut self, s: &RawShape, xf: &Xf, full: &Affine, parent: &Affine, place: &str) {
        let mut end = |bx: &str, by: &str| {
            let x = self.len(&s.sheet, bx, place).unwrap_or(0.0);
            let y = self.len(&s.sheet, by, place).unwrap_or(0.0);
            parent.apply((x, y))
        };
        let (begin, finish) = (end("BeginX", "BeginY"), end("EndX", "EndY"));
        let mut ctx = Ctx { sink: &mut self.doc.sink, place, max_rows: self.doc.limits.max_rows };
        let route: Vec<Point> = vertices(&s.sheet, (xf.w, xf.h), &mut ctx).into_iter().map(|p| full.apply(p)).collect();
        let tol = (0.01f64).max(0.01 * dist(begin, finish));
        let follows =
            matches!((route.first(), route.last()), (Some(&a), Some(&b)) if route.len() >= 2 && dist(a, begin) < tol && dist(b, finish) < tol);
        let mut points: Vec<Point> = if follows { route } else { vec![begin, finish] };
        if let Some(first) = points.first_mut() {
            *first = begin;
        }
        if let Some(last) = points.last_mut() {
            *last = finish;
        }
        let points = points.into_iter().map(|p| self.model(p)).collect();
        let (line, line_weight, dashed) = self.line(s, place);
        let v = self.view(s);
        let (ba, ea) = (v.cell("BeginArrow"), v.cell("EndArrow"));
        let arrow = |c: Option<Cell>, sink: &mut Sink, n: &str| scalar(c.as_ref(), sink, place, n).map_or(0, |a| a.clamp(0.0, 255.0) as u8);
        let begin_arrow = arrow(ba, &mut self.doc.sink, "BeginArrow");
        let end_arrow = arrow(ea, &mut self.doc.sink, "EndArrow");
        let (from_c, to_c) = self.glue.get(&s.id).copied().unwrap_or_default();
        let from = from_c.or_else(|| formula_glue(s.sheet.cell("BeginX")));
        let to = to_c.or_else(|| formula_glue(s.sheet.cell("EndX")));
        self.connectors.push(Connector {
            id: s.id,
            from,
            to,
            points,
            begin_arrow,
            end_arrow,
            line,
            line_weight,
            dashed,
            label: s.text.clone().unwrap_or_default(),
        });
    }
}

fn dist(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}
