//! Glue says what the model has: dangling glue is dropped and reported, and connection rows
//! are matched by `IX`.

use craft_vsdx::{ConnectionPoint, Connector, Drawing, Page, Shape, WarningKind, read, write};

use crate::{package, page_xml};

fn read_ok(shapes: &str, connects: &str) -> (Drawing, Vec<craft_vsdx::Warning>) {
    match read(&package("", &[page_xml(shapes, connects)], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    }
}

const LINK: &str = r#"<Shape ID="5"><Cell N="BeginX" V="1"/><Cell N="BeginY" V="1"/><Cell N="EndX" V="3"/><Cell N="EndY" V="3"/></Shape>"#;

#[test]
fn glue_to_a_group_with_no_shape_is_dropped_and_the_result_still_writes() {
    // A group with no geometry and no text becomes no shape: nothing for the glue to name.
    let group = r#"<Shape ID="7" Type="Group"><Cell N="Width" V="1"/><Shapes><Shape ID="8"><Cell N="Width" V="1"/></Shape></Shapes></Shape>"#;
    let (d, w) = read_ok(&format!("{group}{LINK}"), r#"<Connects><Connect FromSheet="5" FromCell="BeginX" ToSheet="7" ToCell="PinX"/></Connects>"#);
    let page = &d.pages[0];
    assert!(page.shapes.iter().all(|s| s.id != 7));
    assert_eq!(page.connectors[0].from, None, "the end is left free");
    assert!(w.iter().any(|w| w.kind == WarningKind::Missing && w.message.contains("not drawn")), "{w:?}");
    assert!(write(&d).is_ok(), "what was read can be written");
}

#[test]
fn the_writer_frees_glue_it_cannot_resolve_and_says_so() {
    let shape = Shape { id: 1, width: 10.0, height: 10.0, connections: vec![(5.0, 0.0)], ..Shape::default() };
    let link = |from, to| Connector { id: 2, from, to, points: vec![(0.0, 0.0), (10.0, 10.0)], ..Connector::default() };
    let d = Drawing {
        pages: vec![Page {
            name: "P".into(),
            width: 100.0,
            height: 100.0,
            shapes: vec![shape],
            connectors: vec![link(Some((99, ConnectionPoint::Shape)), Some((1, ConnectionPoint::Point(2))))],
        }],
    };
    let (bytes, w) = match write(&d) {
        Ok(r) => r,
        Err(e) => panic!("write: {e}"),
    };
    assert_eq!(w.iter().filter(|w| w.kind == WarningKind::Missing).count(), 2, "{w:?}");
    let back = match read(&bytes) {
        Ok((d, _)) => d,
        Err(e) => panic!("read: {e}"),
    };
    assert_eq!((back.pages[0].connectors[0].from, back.pages[0].connectors[0].to), (None, None));
    // Glue that resolves is kept, with no warning.
    let mut ok = d.clone();
    ok.pages[0].connectors[0] = link(Some((1, ConnectionPoint::Point(1))), Some((1, ConnectionPoint::Shape)));
    assert!(write(&ok).is_ok_and(|(_, w)| w.is_empty()));
}

#[test]
fn connection_rows_are_matched_by_ix_not_by_position() {
    let target = r#"<Shape ID="4"><Cell N="Width" V="2"/><Cell N="Height" V="2"/>
        <Section N="Connection"><Row IX="0"><Cell N="X" V="0"/><Cell N="Y" V="2"/></Row><Row IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row></Section></Shape>"#;
    let glue = |cell: &str| format!(r#"<Connects><Connect FromSheet="5" FromCell="BeginX" ToSheet="4" ToCell="{cell}"/></Connects>"#);
    let (d, w) = read_ok(&format!("{target}{LINK}"), &glue("Connections.X3"));
    let s = &d.pages[0].shapes[0];
    assert_eq!(s.connections.len(), 2, "both rows are kept");
    let at = match d.pages[0].connectors[0].from {
        Some((4, ConnectionPoint::Point(n))) => n,
        other => panic!("glue {other:?}"),
    };
    let p = s.connections[(at - 1) as usize];
    assert!(p.0 > 100.0 && p.1 > 100.0, "IX 2 is the row at the bottom right (X 2, Y 0), not row 0: {p:?}");
    assert!(w.is_empty(), "{w:?}");
    // IX 1 does not exist: the end is glued to the shape, with a warning.
    let (d, w) = read_ok(&format!("{target}{LINK}"), &glue("Connections.X2"));
    assert_eq!(d.pages[0].connectors[0].from, Some((4, ConnectionPoint::Shape)));
    assert!(w.iter().any(|w| w.kind == WarningKind::Missing));
    // Row 0 is point 1.
    let (d, _) = read_ok(&format!("{target}{LINK}"), &glue("Connections.X1"));
    assert_eq!(d.pages[0].connectors[0].from, Some((4, ConnectionPoint::Point(1))));
}
