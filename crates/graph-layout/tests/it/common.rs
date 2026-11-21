//! Geometric relations the tests assert, and small graph builders.

use craft_graph_layout::{EdgeSpec, Graph, Layout, NodeSpec, Point, Rect, Size};

pub const EPS: f64 = 1e-6;

pub fn node(w: f64, h: f64) -> NodeSpec {
    NodeSpec { size: Size::new(w, h), group: None }
}

pub fn graph(n: usize, edges: &[(usize, usize)]) -> Graph {
    Graph { nodes: vec![node(60.0, 30.0); n], edges: edges.iter().map(|&(a, b)| EdgeSpec::new(a, b)).collect(), groups: vec![] }
}

/// A deterministic pseudo-random sequence (a linear congruential generator).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n.max(1) as u64) as usize
    }
}

/// A connected random DAG: a random tree plus `extra` forward edges, with varied node sizes.
pub fn random_dag(seed: u64, n: usize, extra: usize) -> Graph {
    let mut r = Lcg(seed);
    let nodes = (0..n).map(|_| node(20.0 + r.below(80) as f64, 16.0 + r.below(40) as f64)).collect();
    let mut edges = Vec::new();
    for i in 1..n {
        edges.push(EdgeSpec::new(r.below(i), i));
    }
    for _ in 0..extra {
        let (a, b) = (r.below(n), r.below(n));
        if a < b {
            edges.push(EdgeSpec::new(a, b));
        }
    }
    Graph { nodes, edges, groups: vec![] }
}

pub fn inner_overlap(a: &Rect, b: &Rect) -> bool {
    a.x + EPS < b.right() && b.x + EPS < a.right() && a.y + EPS < b.bottom() && b.y + EPS < a.bottom()
}

pub fn contains(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x - EPS && inner.y >= outer.y - EPS && inner.right() <= outer.right() + EPS && inner.bottom() <= outer.bottom() + EPS
}

pub fn assert_no_node_overlap(l: &Layout) {
    for i in 0..l.nodes.len() {
        for j in i + 1..l.nodes.len() {
            assert!(!inner_overlap(&l.nodes[i], &l.nodes[j]), "nodes {i} and {j} overlap: {:?} {:?}", l.nodes[i], l.nodes[j]);
        }
    }
}

pub fn on_boundary(r: &Rect, p: Point) -> bool {
    let tol = 1e-4;
    let in_x = p.x >= r.x - tol && p.x <= r.right() + tol;
    let in_y = p.y >= r.y - tol && p.y <= r.bottom() + tol;
    let on_v = (p.x - r.x).abs() <= tol || (p.x - r.right()).abs() <= tol;
    let on_h = (p.y - r.y).abs() <= tol || (p.y - r.bottom()).abs() <= tol;
    in_x && in_y && (on_v || on_h)
}

pub fn axis_aligned(pts: &[Point]) -> bool {
    pts.windows(2).all(|w| (w[0].x - w[1].x).abs() <= 1e-4 || (w[0].y - w[1].y).abs() <= 1e-4)
}

/// Whether the axis-aligned segment `a`-`b` passes through the open interior of `r`.
pub fn segment_hits(a: Point, b: Point, r: &Rect) -> bool {
    let tol = 1e-4;
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let ox = x1.min(r.right()) - x0.max(r.x);
    let oy = y1.min(r.bottom()) - y0.max(r.y);
    if x1 - x0 <= tol { x0 > r.x + tol && x0 < r.right() - tol && oy > tol } else { y0 > r.y + tol && y0 < r.bottom() - tol && ox > tol }
}

pub fn path_hits(pts: &[Point], r: &Rect) -> bool {
    pts.windows(2).any(|w| segment_hits(w[0], w[1], r))
}

/// Every edge is an axis-aligned polyline from its source's boundary to its target's boundary
/// that crosses no other node.
pub fn assert_edges_clean(g: &Graph, l: &Layout) {
    for (i, e) in g.edges.iter().enumerate() {
        let pts = &l.edges[i];
        assert!(pts.len() >= 2, "edge {i} has no path");
        assert!(axis_aligned(pts), "edge {i} is not orthogonal: {pts:?}");
        assert!(on_boundary(&l.nodes[e.from], pts[0]), "edge {i} does not start on its source: {pts:?} {:?}", l.nodes[e.from]);
        assert!(on_boundary(&l.nodes[e.to], pts[pts.len() - 1]), "edge {i} does not end on its target: {pts:?} {:?}", l.nodes[e.to]);
        for (v, r) in l.nodes.iter().enumerate() {
            if v != e.from && v != e.to {
                assert!(!path_hits(pts, r), "edge {i} crosses node {v}: {pts:?} {r:?}");
            }
        }
        if e.from != e.to {
            assert!(!path_hits(pts, &l.nodes[e.from]) && !path_hits(pts, &l.nodes[e.to]), "edge {i} crosses an end node: {pts:?}");
        }
    }
}

/// Proper crossings between segments of different polylines.
pub fn crossings(paths: &[Vec<Point>]) -> usize {
    let mut count = 0;
    for i in 0..paths.len() {
        for j in i + 1..paths.len() {
            for a in paths[i].windows(2) {
                for b in paths[j].windows(2) {
                    if segments_cross(a[0], a[1], b[0], b[1]) {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn segments_cross(a0: Point, a1: Point, b0: Point, b1: Point) -> bool {
    let a_h = (a0.y - a1.y).abs() <= EPS;
    let b_h = (b0.y - b1.y).abs() <= EPS;
    if a_h == b_h {
        return false;
    }
    let (h0, h1, v0, v1) = if a_h { (a0, a1, b0, b1) } else { (b0, b1, a0, a1) };
    let (x, y) = (v0.x, h0.y);
    x > h0.x.min(h1.x) + EPS && x < h0.x.max(h1.x) - EPS && y > v0.y.min(v1.y) + EPS && y < v0.y.max(v1.y) - EPS
}

/// Pairs of segments from different paths that lie on the same line and share a stretch.
pub fn collinear_overlaps(paths: &[Vec<Point>]) -> usize {
    let mut count = 0;
    for i in 0..paths.len() {
        for j in i + 1..paths.len() {
            for a in paths[i].windows(2) {
                for b in paths[j].windows(2) {
                    if collinear_overlap(a[0], a[1], b[0], b[1]) {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn collinear_overlap(a0: Point, a1: Point, b0: Point, b1: Point) -> bool {
    let horiz = |p: Point, q: Point| (p.y - q.y).abs() <= EPS;
    let vert = |p: Point, q: Point| (p.x - q.x).abs() <= EPS;
    if horiz(a0, a1) && horiz(b0, b1) && (a0.y - b0.y).abs() <= EPS {
        a0.x.max(a1.x).min(b0.x.max(b1.x)) - a0.x.min(a1.x).max(b0.x.min(b1.x)) > 1e-3
    } else if vert(a0, a1) && vert(b0, b1) && (a0.x - b0.x).abs() <= EPS {
        a0.y.max(a1.y).min(b0.y.max(b1.y)) - a0.y.min(a1.y).max(b0.y.min(b1.y)) > 1e-3
    } else {
        false
    }
}

pub fn all_finite(l: &Layout) -> bool {
    let r_ok = |r: &Rect| [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite());
    l.nodes.iter().chain(&l.groups).all(r_ok)
        && l.edges.iter().flatten().chain(l.labels.iter().flatten()).all(|p| p.x.is_finite() && p.y.is_finite())
        && l.size.width.is_finite()
        && l.size.height.is_finite()
}
