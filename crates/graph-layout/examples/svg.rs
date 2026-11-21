//! Writes sample layouts as SVG files for visual checks: `cargo run --example svg -- <out dir>`.

use craft_graph_layout::{
    Connector, Direction, EdgeSpec, Graph, GroupSpec, LayeredOptions, Layout, NodeSpec, Point, Port, Rect, RouteOptions, Side, Size, layered,
    route_all,
};
use std::fmt::Write as _;

fn text_size(s: &str) -> Size {
    Size::new(s.chars().count() as f64 * 7.0 + 8.0, 16.0)
}

fn node(label: &str) -> NodeSpec {
    let s = text_size(label);
    NodeSpec { size: Size::new(s.width.max(60.0) + 16.0, 36.0), group: None }
}

fn edge(from: usize, to: usize, label: Option<&str>) -> EdgeSpec {
    EdgeSpec { label: label.map(text_size), ..EdgeSpec::new(from, to) }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn polyline(out: &mut String, pts: &[Point], colour: &str) {
    let d: Vec<String> = pts.iter().map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect();
    let _ = writeln!(out, r#"<polyline points="{}" fill="none" stroke="{colour}" stroke-width="1.4" marker-end="url(#a)"/>"#, d.join(" "));
}

fn svg(layout: &Layout, names: &[String], edge_labels: &[Option<&str>], group_names: &[&str]) -> String {
    let mut s = String::new();
    let (w, h) = (layout.size.width + 40.0, layout.size.height + 40.0);
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" font-family="sans-serif" font-size="12">"#);
    s.push_str(r##"<defs><marker id="a" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" fill="#333"/></marker></defs>"##);
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="white"/><g transform="translate(20,20)">"#);
    for (i, g) in layout.groups.iter().enumerate() {
        let _ = writeln!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#eef3fb" fill-opacity="0.6" stroke="#7a9cc6" rx="4"/>"##,
            g.x, g.y, g.width, g.height
        );
        let name = group_names.get(i).copied().unwrap_or("");
        let _ = writeln!(s, r##"<text x="{:.1}" y="{:.1}" fill="#345">{}</text>"##, g.x + 6.0, g.y + 15.0, esc(name));
    }
    for pts in &layout.edges {
        polyline(&mut s, pts, "#333");
    }
    for (i, l) in layout.labels.iter().enumerate() {
        let (Some(p), Some(Some(t))) = (l, edge_labels.get(i)) else { continue };
        let sz = text_size(t);
        let _ = writeln!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#fff8d0"/><text x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"##,
            p.x - sz.width / 2.0,
            p.y - sz.height / 2.0,
            sz.width,
            sz.height,
            p.x,
            p.y + 4.0,
            esc(t)
        );
    }
    for (i, r) in layout.nodes.iter().enumerate() {
        let _ = writeln!(
            s,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#fff" stroke="#222" rx="5"/><text x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"##,
            r.x,
            r.y,
            r.width,
            r.height,
            r.x + r.width / 2.0,
            r.y + r.height / 2.0 + 4.0,
            esc(names.get(i).map_or("", String::as_str))
        );
    }
    s.push_str("</g></svg>\n");
    s
}

fn flowchart() -> (Graph, Vec<String>, Vec<Option<&'static str>>) {
    let names = ["Start", "Read input", "Valid?", "Show error", "Parse", "Transform", "Retry?", "Wait", "Save", "Notify", "Log", "End"];
    let spec: [(usize, usize, Option<&'static str>); 14] = [
        (0, 1, None),
        (1, 2, None),
        (2, 3, Some("no")),
        (3, 1, None),
        (2, 4, Some("yes")),
        (4, 5, None),
        (5, 6, None),
        (6, 7, Some("yes")),
        (7, 5, None),
        (6, 8, Some("no")),
        (8, 9, None),
        (8, 10, None),
        (9, 11, None),
        (10, 11, None),
    ];
    let edges = spec.iter().map(|&(a, b, l)| edge(a, b, l)).collect();
    let labels = spec.iter().map(|s| s.2).collect();
    let graph = Graph { nodes: names.iter().map(|n| node(n)).collect(), edges, groups: vec![] };
    (graph, names.iter().map(|s| (*s).to_owned()).collect(), labels)
}

fn nested() -> (Graph, Vec<String>, Vec<&'static str>) {
    let groups = vec![
        GroupSpec { parent: None, padding: 12.0, title_height: 18.0 },
        GroupSpec { parent: Some(0), padding: 12.0, title_height: 18.0 },
        GroupSpec { parent: Some(1), padding: 12.0, title_height: 18.0 },
        GroupSpec { parent: None, padding: 12.0, title_height: 18.0 },
    ];
    let spec = [
        ("client", None),
        ("gateway", Some(0)),
        ("auth", Some(1)),
        ("tokens", Some(2)),
        ("keys", Some(2)),
        ("users", Some(1)),
        ("cache", Some(0)),
        ("db primary", Some(3)),
        ("db replica", Some(3)),
        ("metrics", None),
    ];
    let nodes = spec.iter().map(|(n, g)| NodeSpec { group: *g, ..node(n) }).collect();
    let edges = vec![
        edge(0, 1, None),
        edge(1, 2, None),
        edge(2, 3, None),
        edge(3, 4, None),
        edge(2, 5, None),
        edge(1, 6, None),
        edge(5, 7, None),
        edge(6, 7, None),
        edge(7, 8, None),
        edge(4, 9, None),
        edge(8, 9, None),
    ];
    let names = spec.iter().map(|(n, _)| (*n).to_owned()).collect();
    (Graph { nodes, edges, groups }, names, vec!["edge", "auth service", "secrets", "storage"])
}

fn dense() -> (Graph, Vec<String>) {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut rnd = move |n: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % n
    };
    let n = 40;
    let names: Vec<String> = (0..n).map(|i| format!("n{i}")).collect();
    let nodes = (0..n).map(|_| NodeSpec { size: Size::new(40.0 + rnd(40) as f64, 28.0 + rnd(16) as f64), group: None }).collect::<Vec<_>>();
    let mut edges = Vec::new();
    for i in 1..n {
        let parent = rnd(i as u64) as usize;
        edges.push(EdgeSpec::new(parent, i));
    }
    for _ in 0..30 {
        let a = rnd(n as u64) as usize;
        let b = rnd(n as u64) as usize;
        if a < b {
            edges.push(EdgeSpec::new(a, b));
        }
    }
    (Graph { nodes, edges, groups: vec![] }, names)
}

fn routing() -> String {
    let mut obstacles = Vec::new();
    for row in 0..4 {
        for col in 0..5 {
            obstacles.push(Rect::new(40.0 + col as f64 * 140.0, 40.0 + row as f64 * 110.0, 80.0 + (col * 7 % 3) as f64 * 10.0, 50.0));
        }
    }
    let port = |i: usize, side: Option<Side>| Port { rect: obstacles[i], side, point: None };
    let pairs = [(0, 19), (4, 15), (2, 17), (5, 9), (10, 14), (0, 4), (1, 18), (6, 13), (3, 16), (0, 12), (0, 7)];
    let conns: Vec<Connector> = pairs.iter().map(|&(a, b)| Connector { from: port(a, None), to: port(b, None) }).collect();
    let routes = route_all(&conns, &obstacles, &RouteOptions::default()).unwrap_or_default();
    let mut s = String::new();
    s.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width="780" height="520">"#);
    s.push_str(r##"<defs><marker id="a" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0L10,5L0,10z" fill="#333"/></marker></defs><rect width="100%" height="100%" fill="white"/>"##);
    for r in &obstacles {
        let _ = write!(s, r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#f4f4f4" stroke="#222"/>"##, r.x, r.y, r.width, r.height);
    }
    let colours = ["#c0392b", "#2980b9", "#27ae60", "#8e44ad", "#d35400", "#16a085", "#2c3e50", "#7f8c8d", "#e67e22", "#1abc9c", "#9b59b6"];
    for (i, r) in routes.iter().enumerate() {
        if let Some(pts) = r {
            polyline(&mut s, pts, colours[i % colours.len()]);
        }
    }
    s.push_str("</svg>\n");
    s
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    let write = |name: &str, body: String| {
        let path = std::path::Path::new(&dir).join(name);
        if let Err(e) = std::fs::write(&path, body) {
            eprintln!("{}: {e}", path.display());
        }
    };
    let (g, names, labels) = flowchart();
    for (dir_name, d) in [("down", Direction::Down), ("right", Direction::Right)] {
        match layered(&g, &LayeredOptions { direction: d, ..LayeredOptions::default() }) {
            Ok(l) => write(&format!("flowchart-{dir_name}.svg"), svg(&l, &names, &labels, &[])),
            Err(e) => eprintln!("flowchart: {e}"),
        }
    }
    let (g, names, gnames) = nested();
    match layered(&g, &LayeredOptions::default()) {
        Ok(l) => write("nested.svg", svg(&l, &names, &[], &gnames)),
        Err(e) => eprintln!("nested: {e}"),
    }
    let (g, names) = dense();
    match layered(&g, &LayeredOptions { node_sep: 24.0, ..LayeredOptions::default() }) {
        Ok(l) => write("dense.svg", svg(&l, &names, &[], &[])),
        Err(e) => eprintln!("dense: {e}"),
    }
    write("routing.svg", routing());
}
