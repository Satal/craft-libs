//! Writing a minimal drawing package (MS-VSDX §3.1).

use std::collections::{HashMap, HashSet};

use craft_opc::{PackageOut, RelsOut};
use craft_xml::Writer;

use crate::geom::PT;
use crate::model::{ConnectionPoint, Connector, Drawing, GeomPath, Page, Point, Rgba, Segment, Shape};
use crate::{Error, Warning, WarningKind};

const NS_CORE: &str = "http://schemas.microsoft.com/office/visio/2011/1/core";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RT_VISIO: &str = "http://schemas.microsoft.com/visio/2010/relationships/";
const CT_DOCUMENT: &str = "application/vnd.ms-visio.drawing.main+xml";
const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml";
const CT_PAGE: &str = "application/vnd.ms-visio.page+xml";
/// The first shape ID the writer hands out when it renumbers (conflict c2 in the crate docs).
const FIRST_ID: u32 = 4;

/// Writes a drawing as a `.vsdx` package, with a warning for each connector end it could not
/// glue: one glued to a shape that is not on its page, or to a connection point the shape
/// does not have, is written free.
pub fn write(drawing: &Drawing) -> Result<(Vec<u8>, Vec<Warning>), Error> {
    check(drawing)?;
    let mut warnings = Vec::new();
    let mut pkg = PackageOut::default();
    let mut root = RelsOut::default();
    root.add(&format!("{RT_VISIO}document"), "visio/document.xml", false);
    pkg.rels("", &root);
    pkg.xml_part("visio/document.xml", CT_DOCUMENT, document());
    let mut doc_rels = RelsOut::default();
    doc_rels.add(&format!("{RT_VISIO}pages"), "pages/pages.xml", false);
    pkg.rels("visio/document.xml", &doc_rels);
    let mut pages_rels = RelsOut::default();
    let mut entries = Vec::new();
    for (i, page) in drawing.pages.iter().enumerate() {
        let file = format!("page{}.xml", i + 1);
        let rid = pages_rels.add(&format!("{RT_VISIO}page"), &file, false);
        let (w, h) = page_size(page);
        entries.push((page_name(page, i), w, h, rid));
        pkg.xml_part(&format!("visio/pages/{file}"), CT_PAGE, page_contents(page, h, &mut warnings)?);
    }
    pkg.xml_part("visio/pages/pages.xml", CT_PAGES, pages(&entries));
    pkg.rels("visio/pages/pages.xml", &pages_rels);
    Ok((pkg.finish()?, warnings))
}

fn finite(v: f64, what: &str) -> Result<(), Error> {
    if v.is_finite() { Ok(()) } else { Err(Error::Invalid(format!("{what} is not a finite number"))) }
}

fn check_points(points: &[Point], what: &str) -> Result<(), Error> {
    points.iter().try_for_each(|p| finite(p.0, what).and_then(|_| finite(p.1, what)))
}

fn segment_points(s: &Segment) -> Vec<Point> {
    match *s {
        Segment::MoveTo(p) | Segment::LineTo(p) => vec![p],
        Segment::CubicTo { c1, c2, to } => vec![c1, c2, to],
        Segment::ArcTo { to, via, rotation, ratio } => vec![to, via, (rotation, ratio)],
        Segment::Ellipse { centre, a, b } => vec![centre, a, b],
    }
}

fn check(drawing: &Drawing) -> Result<(), Error> {
    for p in &drawing.pages {
        check_points(&[(p.width, p.height)], "a page size")?;
        for s in &p.shapes {
            let what = format!("a value of shape {}", s.id);
            check_points(&[(s.x, s.y), (s.width, s.height), (s.angle, s.line_weight)], &what)?;
            check_points(&s.connections, &what)?;
            for g in &s.geometry {
                for seg in &g.segments {
                    check_points(&segment_points(seg), &what)?;
                }
            }
        }
        for c in &p.connectors {
            let what = format!("a value of connector {}", c.id);
            check_points(&c.points, &what)?;
            finite(c.line_weight, &what)?;
            if c.points.len() < 2 {
                return Err(Error::Invalid(format!("connector {} has fewer than two points", c.id)));
            }
        }
    }
    Ok(())
}

/// A number as written: Rust's shortest round-trip form, without `-0`.
fn n(v: f64) -> String {
    if v == 0.0 { "0".into() } else { v.to_string() }
}

fn hex(c: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn trans(c: Rgba) -> String {
    n(1.0 - f64::from(c.a) / 255.0)
}

fn root(w: &mut Writer, name: &str) {
    w.open(name, &[("xmlns", NS_CORE), ("xmlns:r", NS_R), ("xml:space", "preserve")]);
}

fn cell(w: &mut Writer, name: &str, v: &str) {
    w.tag("Cell").attr("N", name).attr("V", v).empty();
}

/// The Document part: the §3.1.1 sample.
fn document() -> Vec<u8> {
    let mut w = Writer::document();
    root(&mut w, "VisioDocument");
    w.open("FaceNames", &[]);
    w.empty("FaceName", &[("NameU", "Calibri")]);
    w.close();
    w.open("StyleSheets", &[]);
    w.open("StyleSheet", &[("ID", "0"), ("NameU", "No Style"), ("Name", "No Style")]);
    for (k, v) in [
        ("EnableLineProps", "1"),
        ("EnableFillProps", "1"),
        ("EnableTextProps", "1"),
        ("LineWeight", "0.01041666666666667"),
        ("LineColor", "0"),
        ("LinePattern", "1"),
        ("FillForegnd", "1"),
        ("FillPattern", "1"),
        ("VerticalAlign", "1"),
    ] {
        cell(&mut w, k, v);
    }
    w.open("Section", &[("N", "Character")]);
    w.open("Row", &[("IX", "0")]);
    for (k, v) in [("Font", "Calibri"), ("Color", "0"), ("FontScale", "1"), ("Size", "0.1666666666666667")] {
        cell(&mut w, k, v);
    }
    w.close();
    w.close();
    w.open("Section", &[("N", "Paragraph")]);
    w.open("Row", &[("IX", "0")]);
    for (k, v) in [
        ("IndFirst", "0"),
        ("IndLeft", "0"),
        ("IndRight", "0"),
        ("SpLine", "-1.2"),
        ("HorzAlign", "1"),
        ("Bullet", "0"),
        ("BulletStr", ""),
        ("BulletFont", "0"),
        ("BulletFontSize", "-1"),
        ("TextPosAfterBullet", "0"),
        ("Flags", "0"),
    ] {
        cell(&mut w, k, v);
    }
    w.close();
    w.close();
    w.open("Section", &[("N", "Tabs")]);
    w.empty("Row", &[("IX", "0")]);
    w.close();
    w.close();
    w.close();
    w.open("DocumentSheet", &[("NameU", "TheDoc"), ("Name", "TheDoc"), ("LineStyle", "0"), ("FillStyle", "0"), ("TextStyle", "0")]);
    cell(&mut w, "DocLangID", "en-US");
    w.close();
    w.close();
    w.into_bytes()
}

fn page_name(page: &Page, i: usize) -> String {
    if page.name.trim().is_empty() { format!("Page-{}", i + 1) } else { page.name.clone() }
}

/// The page size in inches; a size that is not positive is taken from the content.
fn page_size(page: &Page) -> (f64, f64) {
    if page.width > 0.0 && page.height > 0.0 {
        return (page.width / PT, page.height / PT);
    }
    let mut max = (0.0f64, 0.0f64);
    for s in &page.shapes {
        max = (max.0.max(s.x + s.width), max.1.max(s.y + s.height));
    }
    for p in page.connectors.iter().flat_map(|c| &c.points) {
        max = (max.0.max(p.0), max.1.max(p.1));
    }
    let (w, h) = ((max.0 + PT / 2.0) / PT, (max.1 + PT / 2.0) / PT);
    (if w > 0.0 { w } else { 8.5 }, if h > 0.0 { h } else { 11.0 })
}

fn pages(entries: &[(String, f64, f64, String)]) -> Vec<u8> {
    let mut w = Writer::document();
    root(&mut w, "Pages");
    for (i, (name, pw, ph, rid)) in entries.iter().enumerate() {
        w.tag("Page").attr("ID", i).attr("NameU", name).attr("Name", name).open();
        w.open("PageSheet", &[("LineStyle", "0"), ("FillStyle", "0"), ("TextStyle", "0")]);
        cell(&mut w, "PageWidth", &n(*pw));
        cell(&mut w, "PageHeight", &n(*ph));
        cell(&mut w, "PageScale", "1");
        cell(&mut w, "DrawingScale", "1");
        w.close();
        w.tag("Rel").attr("r:id", rid).empty();
        w.close();
    }
    w.close();
    w.into_bytes()
}

/// Model IDs when they are unique and non-zero; otherwise new ones from [`FIRST_ID`].
/// The IDs written: per shape, per connector, and from each model ID.
struct Ids {
    shapes: Vec<u32>,
    connectors: Vec<u32>,
    map: HashMap<u32, u32>,
}

fn ids(page: &Page) -> Result<Ids, Error> {
    let given: Vec<u32> = page.shapes.iter().map(|s| s.id).chain(page.connectors.iter().map(|c| c.id)).collect();
    let mut seen = HashSet::new();
    let keep = given.iter().all(|&id| id != 0 && seen.insert(id));
    let mut map = HashMap::new();
    let mut out = Vec::with_capacity(given.len());
    for (i, &id) in given.iter().enumerate() {
        let new = if keep {
            id
        } else {
            u32::try_from(i).ok().and_then(|i| i.checked_add(FIRST_ID)).ok_or_else(|| Error::Invalid("too many shapes on a page".into()))?
        };
        map.entry(id).or_insert(new);
        out.push(new);
    }
    let connectors = out.split_off(page.shapes.len());
    Ok(Ids { shapes: out, connectors, map })
}

fn page_contents(page: &Page, ph: f64, warnings: &mut Vec<Warning>) -> Result<Vec<u8>, Error> {
    let Ids { shapes: shape_ids, connectors: connector_ids, map } = ids(page)?;
    let points: HashMap<u32, usize> = page.shapes.iter().zip(&shape_ids).map(|(s, &id)| (id, s.connections.len())).collect();
    let mut glue = Vec::new();
    for c in &page.connectors {
        let mut resolve = |g: &Option<(u32, ConnectionPoint)>, end: &str| -> Option<(u32, ConnectionPoint)> {
            let (id, at) = (*g)?;
            let mut free = |why: String| {
                let place = crate::values::short(&page.name);
                warnings.push(Warning {
                    kind: WarningKind::Missing,
                    place,
                    message: format!("connector {}: its {end} is glued to {why}; written free", c.id),
                });
            };
            let Some((new, count)) = map.get(&id).and_then(|n| Some((*n, *points.get(n)?))) else {
                free(format!("shape {id}, which is not on its page"));
                return None;
            };
            if let ConnectionPoint::Point(k) = at
                && !usize::try_from(k).is_ok_and(|k| (1..=count).contains(&k))
            {
                free(format!("connection point {k} of shape {id}, which has {count}"));
                return None;
            }
            Some((new, at))
        };
        glue.push((resolve(&c.from, "begin"), resolve(&c.to, "end")));
    }
    let mut w = Writer::document();
    root(&mut w, "PageContents");
    w.open("Shapes", &[]);
    for (s, &id) in page.shapes.iter().zip(&shape_ids) {
        shape(&mut w, s, id, ph);
    }
    for ((c, &id), g) in page.connectors.iter().zip(&connector_ids).zip(&glue) {
        connector(&mut w, c, id, *g, ph);
    }
    w.close();
    if glue.iter().any(|(a, b)| a.is_some() || b.is_some()) {
        w.open("Connects", &[]);
        for (&id, (from, to)) in connector_ids.iter().zip(&glue) {
            for (cell, g) in [("BeginX", from), ("EndX", to)] {
                if let Some((sheet, at)) = g {
                    w.tag("Connect").attr("FromSheet", id).attr("FromCell", cell).attr("ToSheet", sheet).attr("ToCell", to_cell(*at)).empty();
                }
            }
        }
        w.close();
    }
    w.close();
    Ok(w.into_bytes())
}

fn to_cell(at: ConnectionPoint) -> String {
    match at {
        ConnectionPoint::Shape => "PinX".into(),
        ConnectionPoint::Point(k) => format!("Connections.X{k}"),
    }
}

fn shape_attrs(w: &mut Writer, id: u32, kind: &str, name: &str) {
    let name = (!name.is_empty()).then_some(name);
    w.tag("Shape")
        .attr("ID", id)
        .attr_opt("NameU", name)
        .attr_opt("Name", name)
        .attr("Type", kind)
        .attr("LineStyle", 0)
        .attr("FillStyle", 0)
        .attr("TextStyle", 0)
        .open();
}

fn fill_line(w: &mut Writer, fill: Option<Rgba>, line: Option<Rgba>, weight: f64, dashed: bool) {
    match fill {
        Some(c) => {
            cell(w, "FillForegnd", &hex(c));
            cell(w, "FillForegndTrans", &trans(c));
            cell(w, "FillPattern", "1");
        }
        None => cell(w, "FillPattern", "0"),
    }
    match line {
        Some(c) => {
            cell(w, "LineColor", &hex(c));
            cell(w, "LineColorTrans", &trans(c));
            cell(w, "LinePattern", if dashed { "2" } else { "1" });
            cell(w, "LineWeight", &n(weight.max(0.0) / PT));
        }
        None => cell(w, "LinePattern", "0"),
    }
}

fn text(w: &mut Writer, t: &str) {
    if !t.is_empty() {
        w.tag("Text").text(t);
    }
}

fn shape(w: &mut Writer, s: &Shape, id: u32, ph: f64) {
    shape_attrs(w, id, "Shape", &s.name);
    let (iw, ih) = (s.width / PT, s.height / PT);
    cell(w, "PinX", &n((s.x + s.width / 2.0) / PT));
    cell(w, "PinY", &n(ph - (s.y + s.height / 2.0) / PT));
    cell(w, "Width", &n(iw));
    cell(w, "Height", &n(ih));
    cell(w, "LocPinX", &n(iw / 2.0));
    cell(w, "LocPinY", &n(ih / 2.0));
    cell(w, "Angle", &n(-s.angle.to_radians()));
    fill_line(w, s.fill, s.line, s.line_weight, s.dashed);
    if let Some(c) = s.text_color {
        w.open("Section", &[("N", "Character")]);
        w.open("Row", &[("IX", "0")]);
        cell(w, "Color", &hex(c));
        w.close();
        w.close();
    }
    let local = |(x, y): Point| (x / PT, ih - y / PT);
    for (i, g) in s.geometry.iter().enumerate() {
        geometry(w, i, g, (iw, ih), &local);
    }
    if !s.connections.is_empty() {
        w.open("Section", &[("N", "Connection")]);
        for (i, &p) in s.connections.iter().enumerate() {
            let (x, y) = local(p);
            w.tag("Row").attr("IX", i).open();
            cell(w, "X", &n(x));
            cell(w, "Y", &n(y));
            w.close();
        }
        w.close();
    }
    text(w, &s.text);
    w.close();
}

fn row(w: &mut Writer, ix: usize, t: &str, cells: &[(&str, f64)]) {
    w.tag("Row").attr("T", t).attr("IX", ix).open();
    for (k, v) in cells {
        cell(w, k, &n(*v));
    }
    w.close();
}

fn geometry(w: &mut Writer, i: usize, g: &GeomPath, (iw, ih): Point, local: &dyn Fn(Point) -> Point) {
    w.tag("Section").attr("N", "Geometry").attr("IX", i).open();
    cell(w, "NoFill", if g.no_fill { "1" } else { "0" });
    cell(w, "NoLine", if g.no_line { "1" } else { "0" });
    cell(w, "NoShow", "0");
    for (k, seg) in g.segments.iter().enumerate() {
        let ix = k + 1;
        match *seg {
            Segment::MoveTo(p) => {
                let (x, y) = local(p);
                row(w, ix, "MoveTo", &[("X", x), ("Y", y)]);
            }
            Segment::LineTo(p) => {
                let (x, y) = local(p);
                row(w, ix, "LineTo", &[("X", x), ("Y", y)]);
            }
            Segment::CubicTo { c1, c2, to } => cubic(w, ix, [local(c1), local(c2), local(to)], (iw, ih)),
            Segment::ArcTo { to, via, rotation, ratio } => {
                let ((x, y), (a, b)) = (local(to), local(via));
                row(w, ix, "EllipticalArcTo", &[("X", x), ("Y", y), ("A", a), ("B", b), ("C", -rotation.to_radians()), ("D", ratio)]);
            }
            Segment::Ellipse { centre, a, b } => {
                let ((x, y), (a1, b1), (c, d)) = (local(centre), local(a), local(b));
                row(w, ix, "Ellipse", &[("X", x), ("Y", y), ("A", a1), ("B", b1), ("C", c), ("D", d)]);
            }
        }
    }
    w.close();
}

/// A cubic as `RelCubBezTo`, whose cells are fractions of the shape's size; a shape with no
/// width or height cannot hold one, so it gets a line to the end instead.
fn cubic(w: &mut Writer, ix: usize, [c1, c2, to]: [Point; 3], (iw, ih): Point) {
    if iw <= 0.0 || ih <= 0.0 {
        row(w, ix, "LineTo", &[("X", to.0), ("Y", to.1)]);
        return;
    }
    let cells = [("X", to.0 / iw), ("Y", to.1 / ih), ("A", c1.0 / iw), ("B", c1.1 / ih), ("C", c2.0 / iw), ("D", c2.1 / ih)];
    row(w, ix, "RelCubBezTo", &cells);
}

/// The endpoint formula for glue to a connection point (O6 in the crate docs); glue to a
/// whole shape gets none.
fn glue_formula(g: Option<(u32, ConnectionPoint)>) -> Option<String> {
    match g? {
        (id, ConnectionPoint::Point(k)) => Some(format!("PAR(PNT(Sheet.{id}!Connections.X{k},Sheet.{id}!Connections.Y{k}))")),
        (_, ConnectionPoint::Shape) => None,
    }
}

fn end_cells(w: &mut Writer, prefix: &str, p: Point, g: Option<(u32, ConnectionPoint)>) {
    let f = glue_formula(g);
    w.tag("Cell").attr("N", format!("{prefix}X")).attr("V", n(p.0)).attr_opt("F", f.as_deref()).empty();
    w.tag("Cell").attr("N", format!("{prefix}Y")).attr("V", n(p.1)).attr_opt("F", f.as_deref()).empty();
}

type Glue = (Option<(u32, ConnectionPoint)>, Option<(u32, ConnectionPoint)>);

fn connector(w: &mut Writer, c: &Connector, id: u32, (from, to): Glue, ph: f64) {
    shape_attrs(w, id, "Shape", "");
    let pts: Vec<Point> = c.points.iter().map(|&(x, y)| (x / PT, ph - y / PT)).collect();
    let (Some(&b), Some(&e)) = (pts.first(), pts.last()) else {
        w.close();
        return;
    };
    let (dx, dy) = (e.0 - b.0, e.1 - b.1);
    let len = dx.hypot(dy);
    let angle = if len > 0.0 { dy.atan2(dx) } else { 0.0 };
    end_cells(w, "Begin", b, from);
    end_cells(w, "End", e, to);
    let pin = ((b.0 + e.0) / 2.0, (b.1 + e.1) / 2.0);
    cell(w, "PinX", &n(pin.0));
    cell(w, "PinY", &n(pin.1));
    cell(w, "Width", &n(len));
    cell(w, "Height", "0");
    cell(w, "LocPinX", &n(len / 2.0));
    cell(w, "LocPinY", "0");
    cell(w, "Angle", &n(angle));
    cell(w, "BeginArrow", &c.begin_arrow.to_string());
    cell(w, "EndArrow", &c.end_arrow.to_string());
    fill_line(w, None, c.line, c.line_weight, c.dashed);
    let (sin, cos) = (-angle).sin_cos();
    let local = |(x, y): Point| {
        let (rx, ry) = (x - pin.0, y - pin.1);
        (rx * cos - ry * sin + len / 2.0, rx * sin + ry * cos)
    };
    w.tag("Section").attr("N", "Geometry").attr("IX", 0).open();
    cell(w, "NoFill", "1");
    cell(w, "NoLine", "0");
    cell(w, "NoShow", "0");
    for (k, &p) in pts.iter().enumerate() {
        let (x, y) = local(p);
        row(w, k + 1, if k == 0 { "MoveTo" } else { "LineTo" }, &[("X", x), ("Y", y)]);
    }
    w.close();
    text(w, &c.label);
    w.close();
}
