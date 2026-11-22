//! Writes a small flowchart to the path given, reads it back and prints what was read.
//!
//! `cargo run -p craft-vsdx --example roundtrip -- flow.vsdx`

use craft_vsdx::{ARROW, ConnectionPoint, Connector, Drawing, GeomPath, Page, Rgba, Shape, read, write};

fn node(id: u32, x: f64, y: f64, geometry: GeomPath, text: &str) -> Shape {
    let (w, h) = geometry.bounds().map_or((0.0, 0.0), |b| (b.2, b.3));
    Shape {
        id,
        x,
        y,
        width: w,
        height: h,
        geometry: vec![geometry],
        text: text.into(),
        fill: Some(Rgba::rgb(0xDD, 0xEB, 0xF7)),
        line: Some(Rgba::rgb(0x1F, 0x4E, 0x79)),
        line_weight: 1.0,
        text_color: Some(Rgba::BLACK),
        connections: vec![(w / 2.0, 0.0), (w / 2.0, h)],
        ..Shape::default()
    }
}

fn link(id: u32, from: u32, to: u32, points: Vec<(f64, f64)>) -> Connector {
    Connector {
        id,
        from: Some((from, ConnectionPoint::Point(2))),
        to: Some((to, ConnectionPoint::Point(1))),
        points,
        end_arrow: ARROW,
        line: Some(Rgba::rgb(0x1F, 0x4E, 0x79)),
        line_weight: 1.0,
        ..Connector::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: roundtrip <out.vsdx>");
        std::process::exit(2);
    };
    let shapes = vec![
        node(1, 190.0, 30.0, GeomPath::ellipse(120.0, 50.0), "Start"),
        node(2, 180.0, 130.0, GeomPath::rect(140.0, 60.0), "Gather input"),
        node(3, 180.0, 240.0, GeomPath::diamond(140.0, 90.0), "Valid?"),
        node(4, 190.0, 380.0, GeomPath::ellipse(120.0, 50.0), "Done\nThank you"),
    ];
    let connectors = vec![
        link(5, 1, 2, vec![(250.0, 80.0), (250.0, 130.0)]),
        link(6, 2, 3, vec![(250.0, 190.0), (250.0, 240.0)]),
        link(7, 3, 4, vec![(250.0, 330.0), (250.0, 380.0)]),
    ];
    let drawing = Drawing { pages: vec![Page { name: "Flowchart".into(), width: 500.0, height: 480.0, shapes, connectors }] };
    let (bytes, written) = write(&drawing)?;
    std::fs::write(&path, &bytes)?;
    let (back, warnings) = read(&bytes)?;
    let page = back.pages.first().ok_or("no page read back")?;
    println!(
        "{path}: {} bytes, {} shapes, {} connectors, {} warnings",
        bytes.len(),
        page.shapes.len(),
        page.connectors.len(),
        warnings.len() + written.len()
    );
    Ok(())
}
