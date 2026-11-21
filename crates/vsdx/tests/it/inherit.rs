//! Hand-built packages: master inheritance, style sheets, flips, units and glue formulas.

use craft_vsdx::{ConnectionPoint, WarningKind, read, read_stencil};

use crate::{package, page_xml, root};

/// A master: a 2 x 1 inch box with a red rectangle, text and one connection point.
fn box_master() -> String {
    format!(
        r##"{}<Shapes><Shape ID="5" Type="Shape">
            <Cell N="PinX" V="1"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>
            <Cell N="FillForegnd" V="#FF0000"/><Cell N="FillPattern" V="1"/>
            <Section N="Geometry" IX="0">
              <Row T="RelMoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
              <Row T="RelLineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="0"/></Row>
              <Row T="RelLineTo" IX="3"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row>
              <Row T="RelLineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row>
            </Section>
            <Section N="Connection"><Row IX="0"><Cell N="X" V="2"/><Cell N="Y" V="0.5"/></Row></Section>
            <Text>From master</Text>
        </Shape></Shapes></MasterContents>"##,
        root("MasterContents")
    )
}

/// A master of two shapes side by side, inherited as a group.
fn pair_master() -> String {
    let one = |id: u32, x: f64| {
        format!(
            r#"<Shape ID="{id}"><Cell N="PinX" V="{x}"/><Cell N="PinY" V="0.5"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
            <Section N="Geometry" IX="0"><Row T="RelMoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="RelLineTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row></Section></Shape>"#
        )
    };
    format!("{}<Shapes>{}{}</Shapes></MasterContents>", root("MasterContents"), one(1, 0.5), one(2, 2.5))
}

fn masters() -> Vec<(&'static str, String)> {
    vec![(r#"ID="1" NameU="Box" Name="Boîte" UniqueID="{AA}""#, box_master()), (r#"ID="2" NameU="Pair""#, pair_master())]
}

#[test]
fn an_instance_inherits_what_it_does_not_set() {
    // Shape 7 sets only its place and a third row; shape 8 deletes the triangle's row 3 and its
    // text; shape 9 is a group of two inherited from a two-shape master.
    let shapes = r#"
        <Shape ID="7" Master="1"><Cell N="PinX" V="3"/><Cell N="PinY" V="4"/>
          <Section N="Geometry" IX="0"><Row IX="4" T="RelLineTo"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row IX="5" T="RelLineTo"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>
        </Shape>
        <Shape ID="8" Master="1"><Cell N="PinX" V="6"/><Cell N="PinY" V="4"/><Cell N="Width" V="3"/>
          <Section N="Geometry" IX="0"><Row IX="3" Del="1"/></Section><Text>Own</Text>
        </Shape>
        <Shape ID="9" Master="2"><Cell N="PinX" V="5"/><Cell N="PinY" V="1"/><Cell N="Width" V="3"/><Cell N="Height" V="1"/></Shape>"#;
    let bytes = package("", &[page_xml(shapes, "")], &masters());
    let (d, warnings) = match read(&bytes) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    let page = &d.pages[0];
    let get = |id: u32| match page.shapes.iter().find(|s| s.id == id) {
        Some(s) => s,
        None => panic!("shape {id} missing; warnings {warnings:?}"),
    };
    let (a, b) = (get(7), get(8));
    assert_eq!(a.text, "From master");
    assert_eq!(b.text, "Own");
    assert_eq!(a.master.as_deref(), Some("Box"));
    assert_eq!(a.fill, b.fill);
    assert!(a.fill.is_some_and(|c| c.r > c.g), "the master's red fill is inherited");
    assert!((a.width - 144.0).abs() < 1e-6 && b.width > a.width, "the local Width overrides the master's");
    let rows = |s: &craft_vsdx::Shape| s.geometry[0].segments.len();
    assert!(rows(a) > rows(b), "a local row adds to and Del removes from the inherited rows");
    assert!(a.geometry[0].closed, "the added row brings the path back to its start");
    assert_eq!(a.connections.len(), 1);
    // PinY 4 of an 8 inch page: the box's middle is 4 inches from the top.
    assert!((a.y + a.height / 2.0 - 4.0 * 72.0).abs() < 1e-6 && (a.x + a.width / 2.0 - 3.0 * 72.0).abs() < 1e-6);
    let pair: Vec<_> = page.shapes.iter().filter(|s| s.id == 0).collect();
    assert_eq!(pair.len(), 2, "both shapes of the two-shape master are drawn");
    assert!(pair[0].x < pair[1].x);
    assert!(pair.iter().all(|s| s.x >= 3.5 * 72.0 - 1e-6 && s.x + s.width <= 6.5 * 72.0 + 1e-6), "inside the group's box");
}

#[test]
fn a_stencil_lists_its_masters() {
    let bytes = package("", &[], &masters());
    let (stencil, _) = match read_stencil(&bytes) {
        Ok(r) => r,
        Err(e) => panic!("read_stencil: {e}"),
    };
    assert_eq!(stencil.masters.len(), 2);
    let m = &stencil.masters[0];
    assert_eq!((m.name.as_str(), m.local_name.as_str(), m.unique_id.as_deref()), ("Box", "Boîte", Some("{AA}")));
    assert!((m.width - 144.0).abs() < 1e-6 && (m.height - 72.0).abs() < 1e-6);
    assert!(m.shapes.iter().all(|s| s.x >= -1e-6 && s.y >= -1e-6));
    assert!(stencil.masters[1].shapes.len() == 2 && stencil.masters[1].width > m.width);
}

#[test]
fn style_sheets_supply_unset_cells() {
    let doc = r##"<Colors><ColorEntry IX="7" RGB="#00FF00"/></Colors><StyleSheets>
        <StyleSheet ID="3"><Cell N="LineColor" V="7"/><Cell N="LinePattern" V="3"/><Cell N="FillPattern" V="0"/></StyleSheet>
        <StyleSheet ID="4" LineStyle="3" FillStyle="3"><Cell N="LineWeight" V="2" U="PT"/></StyleSheet></StyleSheets>"##;
    let shapes = r#"<Shape ID="1" LineStyle="4" FillStyle="4"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
        <Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="1" U="IN"/><Cell N="Y" V="25.4" U="MM"/></Row></Section></Shape>"#;
    let (d, _) = match read(&package(doc, &[page_xml(shapes, "")], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    let s = &d.pages[0].shapes[0];
    assert!(s.line.is_some_and(|c| c.g > c.r && c.g > c.b), "the line colour comes from the parent style's colour table entry");
    assert!(s.dashed && s.fill.is_none());
    assert!((s.line_weight - 2.0).abs() < 1e-9, "a weight in points stays two points");
    let Some((_, y0, x1, _)) = s.geometry[0].bounds() else { panic!("no geometry") };
    assert!((x1 - 72.0).abs() < 1e-6 && y0.abs() < 1e-6, "millimetres and inches meet at the box's corner");
}

#[test]
fn flips_and_rotation_keep_geometry_in_the_box() {
    // A right triangle, flipped in x and rotated 90 degrees, in a rotated group.
    let tri = r#"<Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="2"><Cell N="X" V="2"/><Cell N="Y" V="0"/></Row><Row T="LineTo" IX="3"><Cell N="X" V="0"/><Cell N="Y" V="1"/></Row><Row T="LineTo" IX="4"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row></Section>"#;
    let shapes = format!(
        r#"<Shape ID="1" Type="Group"><Cell N="PinX" V="5"/><Cell N="PinY" V="4"/><Cell N="Width" V="4"/><Cell N="Height" V="4"/><Cell N="Angle" V="30" U="DEG"/><Shapes>
          <Shape ID="2"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/><Cell N="FlipX" V="1"/><Cell N="Angle" V="1.5707963267948966"/>{tri}</Shape>
          <Shape ID="3"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="2"/><Cell N="Height" V="1"/>{tri}</Shape>
        </Shapes></Shape>"#
    );
    let (d, _) = match read(&package("", &[page_xml(&shapes, "")], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    let shapes = &d.pages[0].shapes;
    assert_eq!(shapes.len(), 2, "an empty group draws nothing of its own");
    let (flipped, plain) = (&shapes[0], &shapes[1]);
    for s in shapes {
        let Some((x0, y0, x1, y1)) = s.geometry[0].bounds() else { panic!("no geometry") };
        assert!(x0 >= -1e-6 && y0 >= -1e-6 && x1 <= s.width + 1e-6 && y1 <= s.height + 1e-6);
    }
    assert!((plain.angle - -30.0).abs() < 1e-6, "counter-clockwise 30 degrees is -30 in the model");
    assert!((flipped.angle - plain.angle).abs() > 1.0, "the child's own turn adds to the group's");
    let first = |s: &craft_vsdx::Shape| match s.geometry[0].segments.get(1) {
        Some(craft_vsdx::Segment::LineTo(p)) => *p,
        other => panic!("{other:?}"),
    };
    assert!(first(flipped).0 < first(plain).0, "the flip mirrors the geometry in the box");
}

#[test]
fn glue_comes_from_connects_or_from_endpoint_formulas() {
    let target = r#"<Shape ID="1"><Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
        <Section N="Connection"><Row IX="0"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row IX="1"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row></Section></Shape>"#;
    let line = |id: u32, f: &str| {
        format!(
            r#"<Shape ID="{id}"><Cell N="BeginX" V="5"/><Cell N="BeginY" V="5"/><Cell N="EndX" V="2.5" F="{f}"/><Cell N="EndY" V="2.5"/><Cell N="EndArrow" V="13"/><Cell N="LinePattern" V="1"/></Shape>"#
        )
    };
    let shapes = format!("{target}{}{}", line(2, ""), line(3, "PAR(PNT(Sheet.1!Connections.X2,Sheet.1!Connections.Y2))"));
    let connects = r#"<Connects><Connect FromSheet="2" FromCell="EndX" ToSheet="1" ToCell="PinX"/></Connects>"#;
    let (d, _) = match read(&package("", &[page_xml(&shapes, connects)], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    let page = &d.pages[0];
    assert_eq!(page.shapes.len(), 1);
    assert_eq!(page.connectors.len(), 2);
    assert_eq!(page.connectors[0].to, Some((1, ConnectionPoint::Shape)));
    assert_eq!(page.connectors[1].to, Some((1, ConnectionPoint::Point(2))));
    assert!(page.connectors.iter().all(|c| c.from.is_none() && c.end_arrow > 0 && c.points.len() == 2));
    let c = &page.connectors[0];
    assert!(c.points[0].1 < c.points[1].1, "the higher end (y up) is nearer the top (y down)");
}

#[test]
fn what_is_not_drawn_is_warned_about() {
    let shapes = r#"<Shape ID="1" Master="42"><Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/><Cell N="FillForegnd" V="Themed"/>
        <Section N="Geometry" IX="0"><Row T="MoveTo" IX="1"><Cell N="X" V="0"/><Cell N="Y" V="0"/></Row><Row T="NURBSTo" IX="2"><Cell N="X" V="1"/><Cell N="Y" V="1"/></Row><Row T="InfiniteLine" IX="3"/></Section></Shape>
        <Shape ID="2" Type="Foreign"><Cell N="PinX" V="3"/><Cell N="PinY" V="3"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/></Shape>"#;
    let (d, warnings) = match read(&package("", &[page_xml(shapes, "")], &[])) {
        Ok(r) => r,
        Err(e) => panic!("read: {e}"),
    };
    assert_eq!(d.pages[0].shapes.len(), 2);
    for kind in [WarningKind::Missing, WarningKind::Themed, WarningKind::Geometry, WarningKind::Unsupported] {
        assert!(warnings.iter().any(|w| w.kind == kind), "no {kind:?} warning in {warnings:?}");
    }
}
