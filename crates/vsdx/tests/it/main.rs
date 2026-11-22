//! The integration tests of craft-vsdx, one binary. Every fixture is built by the test code
//! itself: drawings through [`craft_vsdx::write`], hand-made packages through [`package`].

mod glue;
mod hostile;
mod inherit;
mod roundtrip;

use craft_opc::{PackageOut, RelsOut};
use craft_vsdx::{ARROW, ConnectionPoint, Connector, Drawing, GeomPath, Page, Rgba, Segment, Shape};

pub const NS: &str = "http://schemas.microsoft.com/office/visio/2011/1/core";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RT: &str = "http://schemas.microsoft.com/visio/2010/relationships/";

/// The opening of a part's root element with both namespaces.
pub fn root(name: &str) -> String {
    format!(r#"<{name} xmlns="{NS}" xmlns:r="{NS_R}">"#)
}

/// A page part holding `shapes` (the inner XML of `<Shapes>`) and `connects`.
pub fn page_xml(shapes: &str, connects: &str) -> String {
    format!("{}<Shapes>{shapes}</Shapes>{connects}</PageContents>", root("PageContents"))
}

/// A package with the given page parts, and masters (the `<Master>` attributes and the
/// MasterContents XML of each) when there are any.
pub fn package(document_extra: &str, pages: &[String], masters: &[(&str, String)]) -> Vec<u8> {
    package_with_icons(document_extra, pages, masters, &[])
}

/// [`package`] with images: `(master index, image part name, bytes)`, related to the master part.
/// Masters naming the same part share it.
pub fn package_with_icons(document_extra: &str, pages: &[String], masters: &[(&str, String)], icons: &[(usize, &str, Vec<u8>)]) -> Vec<u8> {
    let mut pkg = PackageOut::default();
    let mut rels = RelsOut::default();
    rels.add(&format!("{RT}document"), "visio/document.xml", false);
    pkg.rels("", &rels);
    pkg.xml_part(
        "visio/document.xml",
        "application/vnd.ms-visio.drawing.main+xml",
        format!("{}{document_extra}</VisioDocument>", root("VisioDocument")).into_bytes(),
    );
    let mut doc_rels = RelsOut::default();
    doc_rels.add(&format!("{RT}pages"), "pages/pages.xml", false);
    let mut pages_xml = root("Pages");
    let mut page_rels = RelsOut::default();
    for (i, p) in pages.iter().enumerate() {
        let rid = page_rels.add(&format!("{RT}page"), &format!("page{i}.xml"), false);
        pages_xml += &format!(
            r#"<Page ID="{i}" NameU="P{i}"><PageSheet><Cell N="PageWidth" V="10"/><Cell N="PageHeight" V="8"/></PageSheet><Rel r:id="{rid}"/></Page>"#
        );
        pkg.xml_part(&format!("visio/pages/page{i}.xml"), "application/vnd.ms-visio.page+xml", p.clone().into_bytes());
    }
    pages_xml += "</Pages>";
    pkg.xml_part("visio/pages/pages.xml", "application/vnd.ms-visio.pages+xml", pages_xml.into_bytes());
    pkg.rels("visio/pages/pages.xml", &page_rels);
    if !masters.is_empty() {
        doc_rels.add(&format!("{RT}masters"), "masters/masters.xml", false);
        let mut list = root("Masters");
        let mut mrels = RelsOut::default();
        for (i, (attrs, contents)) in masters.iter().enumerate() {
            let rid = mrels.add(&format!("{RT}master"), &format!("master{i}.xml"), false);
            list += &format!(r#"<Master {attrs}><Rel r:id="{rid}"/></Master>"#);
            pkg.xml_part(&format!("visio/masters/master{i}.xml"), "application/vnd.ms-visio.master+xml", contents.clone().into_bytes());
            let mut image_rels = RelsOut::default();
            for (_, part, _) in icons.iter().filter(|(m, ..)| *m == i) {
                image_rels.add(&format!("{NS_R}/image"), &format!("../media/{part}"), false);
            }
            pkg.rels(&format!("visio/masters/master{i}.xml"), &image_rels);
        }
        let mut added = std::collections::HashSet::new();
        for (_, part, bytes) in icons {
            if added.insert(*part) {
                pkg.binary(&format!("visio/media/{part}"), "image/png", bytes.clone());
            }
        }
        list += "</Masters>";
        pkg.xml_part("visio/masters/masters.xml", "application/vnd.ms-visio.masters+xml", list.into_bytes());
        pkg.rels("visio/masters/masters.xml", &mrels);
    }
    pkg.rels("visio/document.xml", &doc_rels);
    match pkg.finish() {
        Ok(b) => b,
        Err(e) => panic!("fixture package: {e}"),
    }
}

fn shape(id: u32, x: f64, y: f64, geometry: GeomPath, text: &str) -> Shape {
    let (w, h) = geometry.bounds().map_or((0.0, 0.0), |b| (b.2, b.3));
    Shape {
        id,
        name: format!("Shape {id}"),
        x,
        y,
        width: w,
        height: h,
        geometry: vec![geometry],
        text: text.into(),
        fill: Some(Rgba::rgb(0x20, 0x70, 0xC0)),
        line: Some(Rgba::BLACK),
        line_weight: 1.0,
        text_color: Some(Rgba::WHITE),
        connections: vec![(w / 2.0, 0.0), (w / 2.0, h)],
        ..Shape::default()
    }
}

/// A small flowchart: start, a rotated decision, an end, and connectors glued both ways.
pub fn flowchart() -> Drawing {
    let mut decide = shape(2, 200.0, 140.0, GeomPath::diamond(140.0, 90.0), "Ready?\nCheck twice");
    decide.angle = 30.0;
    let curve = GeomPath {
        segments: vec![
            Segment::MoveTo((0.0, 60.0)),
            Segment::CubicTo { c1: (20.0, 0.0), c2: (100.0, 0.0), to: (120.0, 60.0) },
            Segment::ArcTo { to: (0.0, 60.0), via: (60.0, 50.0), rotation: 0.0, ratio: 1.0 },
        ],
        closed: true,
        ..GeomPath::default()
    };
    let shapes = vec![
        shape(1, 220.0, 20.0, GeomPath::rect(100.0, 50.0), "Start"),
        decide,
        shape(3, 210.0, 300.0, GeomPath::ellipse(120.0, 60.0), "End"),
        shape(5, 40.0, 300.0, curve, ""),
    ];
    let link = |id, from, to, points: Vec<(f64, f64)>, dashed| Connector {
        id,
        from,
        to,
        points,
        end_arrow: ARROW,
        line: Some(Rgba::rgb(0x40, 0x40, 0x40)),
        line_weight: 1.5,
        dashed,
        ..Connector::default()
    };
    let connectors = vec![
        link(10, Some((1, ConnectionPoint::Point(2))), Some((2, ConnectionPoint::Shape)), vec![(270.0, 70.0), (270.0, 140.0)], false),
        link(11, Some((2, ConnectionPoint::Shape)), Some((3, ConnectionPoint::Point(1))), vec![(270.0, 230.0), (300.0, 260.0), (270.0, 300.0)], true),
    ];
    Drawing { pages: vec![Page { name: "Flow".into(), width: 600.0, height: 420.0, shapes, connectors }] }
}
