//! Hostile input: every read ends in a drawing or an error, never a panic or a hang.

use craft_opc::PackageOut;
use craft_testkit::hostile::{HammerConfig, hammer};
use craft_vsdx::{Limits, WarningKind, read, read_stencil, read_with, write};

use crate::{flowchart, package, page_xml};

fn written() -> Vec<u8> {
    match write(&flowchart()) {
        Ok(b) => b,
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
