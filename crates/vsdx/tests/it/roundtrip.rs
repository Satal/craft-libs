//! Writing then reading keeps what the model says.

use craft_vsdx::{Drawing, Shape, read, write};

use crate::flowchart;

const TOL: f64 = 0.01;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn roundtrip(d: &Drawing) -> Drawing {
    let bytes = match write(d) {
        Ok((b, _)) => b,
        Err(e) => panic!("write: {e}"),
    };
    match read(&bytes) {
        Ok((d, _)) => d,
        Err(e) => panic!("read: {e}"),
    }
}

fn by_id(shapes: &[Shape], id: u32) -> &Shape {
    match shapes.iter().find(|s| s.id == id) {
        Some(s) => s,
        None => panic!("shape {id} lost"),
    }
}

#[test]
fn shapes_keep_their_box_text_and_colours() {
    let before = flowchart();
    let after = roundtrip(&before);
    assert_eq!(after.pages.len(), before.pages.len());
    let (b, a) = (&before.pages[0], &after.pages[0]);
    assert!(close(a.width, b.width) && close(a.height, b.height));
    assert_eq!(a.shapes.len(), b.shapes.len());
    for s in &b.shapes {
        let r = by_id(&a.shapes, s.id);
        assert!(close(r.x, s.x) && close(r.y, s.y), "shape {} moved: {:?} -> {:?}", s.id, (s.x, s.y), (r.x, r.y));
        assert!(close(r.width, s.width) && close(r.height, s.height), "shape {} resized", s.id);
        assert!(close(r.angle, s.angle), "shape {} turned: {} -> {}", s.id, s.angle, r.angle);
        assert_eq!(r.text, s.text, "the text, newline and all, is kept");
        assert_eq!((r.fill, r.line, r.text_color), (s.fill, s.line, s.text_color));
        assert_eq!(r.geometry.len(), s.geometry.len());
        assert_eq!(r.connections.len(), s.connections.len());
    }
}

#[test]
fn connectors_keep_glue_ends_and_arrows() {
    let before = flowchart();
    let after = roundtrip(&before);
    let (b, a) = (&before.pages[0], &after.pages[0]);
    assert_eq!(a.connectors.len(), b.connectors.len());
    for c in &b.connectors {
        let r = match a.connectors.iter().find(|r| r.id == c.id) {
            Some(r) => r,
            None => panic!("connector {} lost", c.id),
        };
        assert_eq!((r.from, r.to), (c.from, c.to), "glue of connector {}", c.id);
        assert_eq!((r.begin_arrow, r.end_arrow, r.dashed), (c.begin_arrow, c.end_arrow, c.dashed));
        assert_eq!(r.points.len(), c.points.len(), "the route's bends are kept");
        for (p, q) in r.points.iter().zip(&c.points) {
            assert!(close(p.0, q.0) && close(p.1, q.1), "connector {} point moved: {q:?} -> {p:?}", c.id);
        }
    }
}

#[test]
fn geometry_stays_inside_the_shape_box() {
    let after = roundtrip(&flowchart());
    for s in &after.pages[0].shapes {
        assert!(!s.geometry.is_empty());
        for g in &s.geometry {
            let Some((x0, y0, x1, y1)) = g.bounds() else { panic!("empty path in shape {}", s.id) };
            assert!(
                x0 >= -TOL && y0 >= -TOL && x1 <= s.width + TOL && y1 <= s.height + TOL,
                "shape {} path {:?} outside {}x{}",
                s.id,
                (x0, y0, x1, y1),
                s.width,
                s.height
            );
        }
    }
}

#[test]
fn a_second_trip_changes_nothing() {
    let once = roundtrip(&flowchart());
    let twice = roundtrip(&once);
    assert_eq!(once.pages[0].shapes.len(), twice.pages[0].shapes.len());
    for (a, b) in once.pages[0].shapes.iter().zip(&twice.pages[0].shapes) {
        assert!(close(a.x, b.x) && close(a.y, b.y) && close(a.angle, b.angle));
        assert_eq!(a.geometry.len(), b.geometry.len());
        for (p, q) in a.geometry.iter().zip(&b.geometry) {
            let (Some(pb), Some(qb)) = (p.bounds(), q.bounds()) else { panic!("empty path") };
            assert!(close(pb.0, qb.0) && close(pb.1, qb.1) && close(pb.2, qb.2) && close(pb.3, qb.3), "{pb:?} -> {qb:?}");
        }
    }
}

#[test]
fn ids_that_clash_are_renumbered_and_glue_follows() {
    let mut d = flowchart();
    let page = &mut d.pages[0];
    page.connectors[0].id = 1; // clashes with the start shape
    let after = roundtrip(&d);
    let ids: std::collections::HashSet<u32> =
        after.pages[0].shapes.iter().map(|s| s.id).chain(after.pages[0].connectors.iter().map(|c| c.id)).collect();
    assert_eq!(ids.len(), after.pages[0].shapes.len() + after.pages[0].connectors.len(), "every ID is unique");
    for c in &after.pages[0].connectors {
        for (id, _) in c.from.iter().chain(c.to.iter()) {
            assert!(after.pages[0].shapes.iter().any(|s| s.id == *id), "glue names a shape on the page");
        }
    }
}

#[test]
fn bad_models_are_refused_not_written() {
    let mut d = flowchart();
    d.pages[0].shapes[0].x = f64::NAN;
    assert!(write(&d).is_err());
    let mut d = flowchart();
    d.pages[0].connectors[0].points.truncate(1);
    assert!(write(&d).is_err());
}
