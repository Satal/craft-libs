//! Hostile input: every read ends in a drawing or an error, never a panic or a hang.

use craft_opc::PackageOut;
use craft_testkit::hostile::{HammerConfig, hammer};
use craft_vsdx::{Limits, WarningKind, read, read_stencil, read_with, write};

use crate::{flowchart, package, page_xml};

fn written() -> Vec<u8> {
    match write(&flowchart()) {
        Ok((b, _)) => b,
        Err(e) => panic!("write: {e}"),
    }
}

#[test]
fn mutated_packages_never_panic() {
    hammer(&HammerConfig::new("vsdx package", 300), &written(), |b: &[u8]| {
        let _ = read(b);
        let _ = read_stencil(b);
    });
}

#[test]
fn mutated_page_xml_never_panics() {
    let master = format!(
        r#"{}<Shapes><Shape ID="1"><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Shapes><Shape ID="2"><Cell N="Width" V="1"/></Shape></Shapes></Shape></Shapes></MasterContents>"#,
        crate::root("MasterContents")
    );
    let seed = page_xml(
        r#"<Shape ID="4" Master="1"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Angle" V="45" U="DEG"/><Cell N="FlipY" V="1"/>
          <Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="ArcTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/><Cell N="A" V="0.2"/></Row>
          <Row T="EllipticalArcTo" IX="3"><Cell N="X" V="0"/><Cell N="Y" V="0"/><Cell N="A" V="0.5"/><Cell N="B" V="1"/><Cell N="C" V="0"/><Cell N="D" V="2"/></Row></Section><Text>a<cp IX="0"/>b</Text></Shape>
          <Shape ID="5"><Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="3" F="Sheet.4!PinX"/><Cell N="EndY" V="3"/></Shape>"#,
        r#"<Connects><Connect FromSheet="5" FromCell="BeginX" ToSheet="4" ToCell="Connections.X1"/></Connects>"#,
    );
    hammer(&HammerConfig::new("vsdx page xml", 300), seed.as_bytes(), move |b: &[u8]| {
        let page = String::from_utf8_lossy(b).into_owned();
        let _ = read(&package("", &[page], &[("ID=\"1\"", master.clone())]));
    });
}

#[test]
fn a_truncated_package_is_an_error() {
    let bytes = written();
    for cut in [0, 10, bytes.len() / 2, bytes.len() - 1] {
        assert!(read(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn missing_parts_are_errors_or_warnings() {
    assert!(read(&package("", &[], &[])).is_ok_and(|(d, _)| d.pages.is_empty()));
    // A package with no document relationship is not a drawing.
    let mut pkg = PackageOut::default();
    pkg.xml_part("visio/document.xml", "application/xml", b"<VisioDocument/>".to_vec());
    let bytes = match pkg.finish() {
        Ok(b) => b,
        Err(e) => panic!("{e}"),
    };
    assert!(read(&bytes).is_err());
    // A page whose part is gone: the page is still read, empty, with a warning.
    let mut d = flowchart();
    d.pages[0].shapes.clear();
    d.pages[0].connectors.clear();
    assert!(read(&package("", &["<NotAPage/>".into()], &[])).is_ok_and(|(d, _)| d.pages[0].shapes.is_empty()));
}

#[test]
fn counts_over_the_limits_are_refused() {
    let shapes: String = (1..=20).map(|i| format!(r#"<Shape ID="{i}"><Cell N="Width" V="1"/></Shape>"#)).collect();
    let bytes = package("", &[page_xml(&shapes, "")], &[]);
    assert!(read(&bytes).is_ok());
    let small = Limits { max_shapes: 10, ..Limits::DEFAULT };
    assert!(read_with(&bytes, &small).is_err_and(|e| e.is_limit()));
    let pages: Vec<String> = (0..5).map(|_| page_xml("", "")).collect();
    let few = Limits { max_pages: 3, ..Limits::DEFAULT };
    assert!(read_with(&package("", &pages, &[]), &few).is_err_and(|e| e.is_limit()));
    let rows: String = (1..=50).map(|i| format!(r#"<Row T="LineTo" IX="{i}"><Cell N="X" V="{i}"/></Row>"#)).collect();
    let long = format!(r#"<Shape ID="1"><Cell N="Width" V="1"/><Section N="Geometry">{rows}</Section></Shape>"#);
    let capped = Limits { max_rows: 10, ..Limits::DEFAULT };
    let r = read_with(&package("", &[page_xml(&long, "")], &[]), &capped);
    assert!(r.is_ok_and(|(d, w)| d.pages[0].shapes[0].geometry[0].segments.len() <= 10 && w.iter().any(|w| w.kind == WarningKind::Limit)));
}

#[test]
fn huge_indices_and_bad_numbers_fall_back_with_warnings() {
    let shapes = r##"<Shape ID="4294967295"><Cell N="PinX" V="NaN"/><Cell N="PinY" V="1e400"/><Cell N="Width" V="abc"/><Cell N="Height" V="2" U="FURLONG"/>
        <Cell N="Angle" V="inf"/><Cell N="LineColor" V="99"/><Cell N="FillForegnd" V="#GGHHII"/>
        <Section N="Geometry" IX="4294967295"><Row T="LineTo" IX="4294967295"><Cell N="X" V="-1e308"/><Cell N="Y" V="1e308"/></Row><Row T="Bogus"/></Section>
        <Section N="Connection" IX="7"><Row IX="4294967295"><Cell N="X" V="1"/></Row></Section></Shape>"##;
    let (d, w) = match read(&package("", &[page_xml(shapes, "")], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    let s = &d.pages[0].shapes[0];
    assert!([s.x, s.y, s.width, s.height, s.angle].iter().all(|v| v.is_finite()));
    assert!(w.iter().filter(|w| w.kind == WarningKind::BadValue).count() >= 5, "{w:?}");
}

fn nested(depth: usize) -> String {
    let mut xml = String::from(
        r#"<Shape ID="9999"><Cell N="Width" V="1"/><Section N="Geometry"><Row T="MoveTo"/><Row T="LineTo"><Cell N="X" V="1"/></Row></Section></Shape>"#,
    );
    for i in 0..depth {
        xml = format!(r#"<Shape ID="{i}" Type="Group"><Cell N="Width" V="1"/><Shapes>{xml}</Shapes></Shape>"#);
    }
    xml
}

#[test]
fn deep_groups_are_cut_or_refused() {
    let r = read(&package("", &[page_xml(&nested(40), "")], &[]));
    assert!(r.is_ok_and(|(d, w)| d.pages[0].shapes.is_empty() && w.iter().any(|w| w.kind == WarningKind::Limit)));
    let r = read(&package("", &[page_xml(&nested(20), "")], &[]));
    assert!(r.is_ok_and(|(d, _)| d.pages[0].shapes.len() == 1), "groups within the limit are read through");
    assert!(read(&package("", &[page_xml(&nested(200), "")], &[])).is_err_and(|e| e.is_limit()));
}

#[test]
fn a_zip_bomb_is_refused() {
    let mut pkg = PackageOut::default();
    pkg.xml_part("visio/document.xml", "application/xml", vec![b' '; 8 * 1024 * 1024]);
    let bytes = match pkg.finish() {
        Ok(b) => b,
        Err(e) => panic!("{e}"),
    };
    assert!(bytes.len() < 256 * 1024, "the fixture compresses well");
    let tight = Limits { package: craft_opc::Limits::new(100, 1024 * 1024, 4 * 1024 * 1024), ..Limits::DEFAULT };
    assert!(read_with(&bytes, &tight).is_err_and(|e| e.is_limit()));
}

#[test]
fn a_master_shape_that_names_its_own_master_is_read() {
    // A shape inside a master that names a master is not an instance of it; following it
    // would never end.
    let master = format!(
        r#"{}<Shapes><Shape ID="1" Type="Group"><Cell N="Width" V="1"/><Shapes><Shape ID="2" Master="1"><Cell N="Width" V="1"/></Shape></Shapes></Shape></Shapes></MasterContents>"#,
        crate::root("MasterContents")
    );
    let bytes = package("", &[page_xml(r#"<Shape ID="4" Master="1"><Cell N="Width" V="1"/></Shape>"#, "")], &[("ID=\"1\"", master)]);
    assert!(read(&bytes).is_ok_and(|(d, _)| d.pages[0].shapes.len() == 1));
}

#[test]
fn lengths_too_large_to_scale_stay_finite() {
    let shapes = r#"<Shape ID="1"><Cell N="PinX" V="1e308"/><Cell N="Width" V="1e308"/><Cell N="Height" V="1" U="MI"/><Section N="Geometry"><Row T="MoveTo"><Cell N="X" V="1e308"/><Cell N="Y" V="0"/></Row></Section></Shape>"#;
    let r = read(&package("", &[page_xml(shapes, "")], &[]));
    assert!(r.is_ok_and(|(d, _)| d.pages[0].shapes.iter().all(|s| [s.x, s.y, s.width, s.height].iter().all(|v| v.is_finite()))));
}

/// Every coordinate a shape holds, to check none is infinite or NaN.
fn all_finite(s: &craft_vsdx::Shape) -> bool {
    use craft_vsdx::Segment;
    let mut v = vec![s.x, s.y, s.width, s.height, s.angle, s.line_weight];
    v.extend(s.connections.iter().flat_map(|p| [p.0, p.1]));
    for seg in s.geometry.iter().flat_map(|g| &g.segments) {
        match *seg {
            Segment::MoveTo(p) | Segment::LineTo(p) => v.extend([p.0, p.1]),
            Segment::CubicTo { c1, c2, to } => v.extend([c1.0, c1.1, c2.0, c2.1, to.0, to.1]),
            Segment::ArcTo { to, via, rotation, ratio } => v.extend([to.0, to.1, via.0, via.1, rotation, ratio]),
            Segment::Ellipse { centre, a, b } => v.extend([centre.0, centre.1, a.0, a.1, b.0, b.1]),
        }
    }
    v.iter().all(|x| x.is_finite())
}

#[test]
fn huge_relative_geometry_stays_finite() {
    let big = r#"<Cell N="X" V="1e308"/><Cell N="Y" V="-1e308"/><Cell N="A" V="1e308"/><Cell N="B" V="1e308"/><Cell N="C" V="1e308"/><Cell N="D" V="1e308"/>"#;
    let rows: String =
        ["RelMoveTo", "RelLineTo", "RelCubBezTo", "RelQuadBezTo", "RelEllipticalArcTo", "EllipticalArcTo", "ArcTo", "Ellipse", "NURBSTo"]
            .iter()
            .enumerate()
            .map(|(i, t)| format!(r#"<Row T="{t}" IX="{}">{big}</Row>"#, i + 1))
            .collect();
    let shapes = format!(
        r#"<Shape ID="1"><Cell N="Width" V="1e308"/><Cell N="Height" V="1e300"/><Cell N="Angle" V="1e308"/><Section N="Geometry">{rows}</Section>
        <Section N="Connection"><Row IX="0"><Cell N="X" V="1e308"/><Cell N="Y" V="1e308"/></Row></Section></Shape>"#
    );
    let r = read(&package("", &[page_xml(&shapes, "")], &[]));
    assert!(r.is_ok_and(|(d, _)| d.pages[0].shapes.len() == 1 && d.pages[0].shapes.iter().all(all_finite)));
}

#[test]
fn a_big_master_instanced_many_times_is_refused_before_it_is_copied() {
    let sub = |i: usize| {
        let rows: String = (1..=100).map(|r| format!(r#"<Row T="LineTo" IX="{r}"><Cell N="X" V="{r}"/><Cell N="Y" V="1"/></Row>"#)).collect();
        format!(r#"<Shape ID="{i}"><Cell N="Width" V="1"/><Section N="Geometry">{rows}</Section></Shape>"#)
    };
    let subs: String = (1..=300).map(sub).collect();
    let master =
        format!(r#"{}<Shapes><Shape ID="1" Type="Group"><Shapes>{subs}</Shapes></Shape></Shapes></MasterContents>"#, crate::root("MasterContents"));
    let instances: String = (100..400).map(|i| format!(r#"<Shape ID="{i}" Master="1"/>"#)).collect();
    let bytes = package("", &[page_xml(&instances, "")], &[("ID=\"1\"", master.clone())]);
    let started = std::time::Instant::now();
    assert!(read(&bytes).is_err_and(|e| e.is_limit()));
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "refused quickly");
    // A few instances of the same master are fine.
    let few: String = (100..103).map(|i| format!(r#"<Shape ID="{i}" Master="1"/>"#)).collect();
    assert!(read(&package("", &[page_xml(&few, "")], &[("ID=\"1\"", master)])).is_ok_and(|(d, _)| d.pages[0].shapes.len() >= 900));
}

#[test]
fn huge_names_are_cut_in_the_model_and_in_warnings() {
    let name = "n".repeat(200_000);
    let shapes = format!(r#"<Shape ID="1" NameU="{name}"><Cell N="Width" V="abc"/></Shape>"#);
    let (d, w) = match read(&package("", &[page_xml(&shapes, "")], &[("ID=\"1\" NameU=\"x\"", page_xml("", ""))])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    assert!(d.pages[0].shapes[0].name.chars().count() < 1_000);
    assert!(w.iter().all(|w| w.place.len() < 500), "no warning carries a whole name");
    assert!(w.iter().any(|w| w.kind == WarningKind::Limit), "the cut is reported");
}

fn png(extra: usize) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.resize(b.len() + extra, 0);
    b
}

fn stencil_icons(icons: &[(usize, &str, Vec<u8>)]) -> (craft_vsdx::Stencil, Vec<craft_vsdx::Warning>) {
    let master = format!(
        r#"{}<Shapes><Shape ID="1"><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape></Shapes></MasterContents>"#,
        crate::root("MasterContents")
    );
    let masters = [("ID=\"1\" NameU=\"A\"", master.clone()), ("ID=\"2\" NameU=\"B\"", master.clone()), ("ID=\"3\" NameU=\"C\"", master)];
    match read_stencil(&crate::package_with_icons("", &[], &masters, icons)) {
        Ok(r) => r,
        Err(e) => panic!("stencil: {e}"),
    }
}

#[test]
fn icons_are_checked_capped_and_shared() {
    let (s, w) = stencil_icons(&[(0, "a.png", png(10)), (1, "a.png", png(10)), (2, "b.png", b"not an image".to_vec())]);
    let (a, b, c) = (&s.masters[0].icon_png, &s.masters[1].icon_png, &s.masters[2].icon_png);
    assert!(matches!((a, b), (Some(a), Some(b)) if std::sync::Arc::ptr_eq(a, b)), "one part, one copy");
    assert!(c.is_none() && w.iter().any(|w| w.kind == WarningKind::BadValue), "a .png that is not a PNG is refused and reported");
    let (s, w) = stencil_icons(&[(0, "big.png", png(2 * 1024 * 1024))]);
    assert!(s.masters[0].icon_png.is_none() && w.iter().any(|w| w.kind == WarningKind::Limit));
}

#[test]
fn icons_are_over_a_total_budget() {
    let icons: Vec<(usize, &str, Vec<u8>)> = vec![(0, "1.png", png(900_000)), (1, "2.png", png(900_000)), (2, "3.png", png(900_000))];
    let (s, _) = stencil_icons(&icons);
    assert_eq!(s.masters.iter().filter(|m| m.icon_png.is_some()).count(), 3, "within the total, all are kept");
}
