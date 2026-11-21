//! Hostile and degenerate input: nothing may panic, and every result must be finite.

use crate::common::{Lcg, all_finite, graph, node};
use craft_graph_layout::{
    Connector, Direction, EdgeSpec, Graph, GroupSpec, LayeredOptions, MAX_GROUP_DEPTH, MAX_MIN_LEN, Point, Port, Rect, RouteOptions, Side, Size,
    layered, route_all, route_orthogonal,
};

const BAD: [f64; 6] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 1e300, -0.0];

fn check(g: &Graph, opts: &LayeredOptions) {
    if let Ok(l) = layered(g, opts) {
        assert!(all_finite(&l), "non-finite output for {g:?}");
        assert_eq!(l.nodes.len(), g.nodes.len());
        assert_eq!(l.edges.len(), g.edges.len());
    }
}

#[test]
fn bad_numbers_in_graphs_are_refused_or_handled() {
    for v in BAD {
        let mut g = graph(3, &[(0, 1), (1, 2)]);
        g.nodes[1].size = Size::new(v, 10.0);
        check(&g, &LayeredOptions::default());
        g.nodes[1].size = Size::new(10.0, v);
        check(&g, &LayeredOptions::default());
        let mut g = graph(3, &[(0, 1)]);
        g.edges[0].label = Some(Size::new(v, v));
        check(&g, &LayeredOptions::default());
        g.groups = vec![GroupSpec { parent: None, padding: v, title_height: v }];
        g.nodes[0].group = Some(0);
        check(&g, &LayeredOptions::default());
        let g = graph(3, &[(0, 1)]);
        for opts in [
            LayeredOptions { node_sep: v, ..LayeredOptions::default() },
            LayeredOptions { rank_sep: v, ..LayeredOptions::default() },
            LayeredOptions { edge_sep: v, ..LayeredOptions::default() },
            LayeredOptions { component_sep: v, ..LayeredOptions::default() },
        ] {
            check(&g, &opts);
        }
    }
}

#[test]
fn bad_indices_and_structures_are_refused() {
    let opts = LayeredOptions::default();
    assert!(layered(&graph(2, &[(0, 5)]), &opts).is_err());
    assert!(layered(&graph(2, &[(usize::MAX, 0)]), &opts).is_err());
    let mut g = graph(2, &[(0, 1)]);
    g.nodes[0].group = Some(3);
    assert!(layered(&g, &opts).is_err());
    let mut g = graph(2, &[]);
    g.groups = vec![GroupSpec { parent: Some(1), padding: 0.0, title_height: 0.0 }, GroupSpec { parent: Some(0), padding: 0.0, title_height: 0.0 }];
    assert!(layered(&g, &opts).is_err());
    g.groups = vec![GroupSpec { parent: Some(0), padding: 0.0, title_height: 0.0 }];
    assert!(layered(&g, &opts).is_err());
    let mut g = graph(1, &[]);
    g.groups = (0..=MAX_GROUP_DEPTH).map(|i| GroupSpec { parent: i.checked_sub(1), padding: 1.0, title_height: 0.0 }).collect();
    g.nodes[0].group = Some(MAX_GROUP_DEPTH);
    assert!(layered(&g, &opts).is_err());
    let mut g = graph(2, &[(0, 1)]);
    g.edges[0].min_len = u32::MAX;
    check(&g, &opts);
    g.edges[0].min_len = MAX_MIN_LEN;
    check(&g, &opts);
    g.edges[0].min_len = 0;
    check(&g, &opts);
}

#[test]
fn degenerate_graphs_lay_out() {
    let opts = LayeredOptions::default();
    let mut zero = graph(4, &[(0, 1), (1, 2), (2, 0), (3, 3), (3, 3)]);
    for n in &mut zero.nodes {
        n.size = Size::new(0.0, 0.0);
    }
    check(&zero, &opts);
    let complete: Vec<(usize, usize)> = (0..8).flat_map(|a| (0..8).map(move |b| (a, b))).collect();
    check(&graph(8, &complete), &opts);
    let mut huge = graph(2, &[(0, 1)]);
    huge.nodes[0].size = Size::new(1e8, 1e8);
    check(&huge, &opts);
    let zero_sep = LayeredOptions { node_sep: 0.0, rank_sep: 0.0, edge_sep: 0.0, component_sep: 0.0, direction: Direction::Left };
    check(&graph(5, &[(0, 1), (0, 2), (2, 1), (3, 4)]), &zero_sep);
    let mut groups_only = Graph { nodes: vec![node(10.0, 10.0)], ..Graph::default() };
    groups_only.groups = vec![GroupSpec { parent: None, padding: 5.0, title_height: 5.0 }; 3];
    check(&groups_only, &opts);
}

#[test]
fn random_graphs_with_junk_never_panic() {
    let mut r = Lcg(2024);
    for _ in 0..200 {
        let n = r.below(15);
        let mut g = Graph { nodes: (0..n).map(|_| node(r.below(50) as f64, r.below(50) as f64)).collect(), ..Graph::default() };
        let gc = r.below(4);
        g.groups = (0..gc)
            .map(|_| GroupSpec { parent: if r.below(2) == 0 { Some(r.below(gc + 1)) } else { None }, padding: 3.0, title_height: r.below(10) as f64 })
            .collect();
        for v in &mut g.nodes {
            if r.below(2) == 0 {
                v.group = Some(r.below(gc + 1));
            }
        }
        for _ in 0..r.below(25) {
            let mut e = EdgeSpec::new(r.below(n + 1), r.below(n + 1));
            e.min_len = r.below(4) as u32;
            if r.below(3) == 0 {
                e.label = Some(Size::new(r.below(40) as f64, 12.0));
            }
            g.edges.push(e);
        }
        let dir = [Direction::Down, Direction::Up, Direction::Right, Direction::Left][r.below(4)];
        check(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() });
    }
}

#[test]
fn routing_with_junk_never_panics() {
    let opts = RouteOptions::default();
    let ok = Rect::new(0.0, 0.0, 10.0, 10.0);
    let p = |r: Rect| Port { rect: r, side: None, point: None };
    for v in BAD {
        let bad = Rect::new(v, 0.0, 10.0, v);
        let _ = route_orthogonal(&p(bad), &p(ok), &[], &opts);
        let _ = route_orthogonal(&p(ok), &p(ok), &[bad], &opts);
        let pt = Port { point: Some(Point::new(v, v)), side: Some(Side::Left), rect: ok };
        let _ = route_orthogonal(&pt, &p(ok), &[], &opts);
        for o in [RouteOptions { clearance: v, ..opts }, RouteOptions { bend_penalty: v, ..opts }, RouteOptions { separation: v, ..opts }] {
            let _ = route_orthogonal(&p(ok), &p(Rect::new(50.0, 50.0, 10.0, 10.0)), &[], &o);
        }
    }
    // Overlapping, identical, nested and zero-size boxes.
    let a = Rect::new(0.0, 0.0, 50.0, 50.0);
    let cases = [
        (a, a, vec![a]),
        (a, Rect::new(10.0, 10.0, 5.0, 5.0), vec![]),
        (a, Rect::new(25.0, 25.0, 50.0, 50.0), vec![Rect::new(-10.0, -10.0, 200.0, 200.0)]),
        (Rect::new(0.0, 0.0, 0.0, 0.0), Rect::new(100.0, 0.0, 0.0, 0.0), vec![Rect::new(50.0, -50.0, 0.0, 100.0)]),
        (a, Rect::new(1e8, 1e8, 1.0, 1.0), vec![Rect::new(5e7, 5e7, 1e6, 1e6)]),
    ];
    for (from, to, obstacles) in cases {
        if let Ok(Some(pts)) = route_orthogonal(&p(from), &p(to), &obstacles, &opts) {
            assert!(pts.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
        }
        let conns = vec![Connector { from: p(from), to: p(to) }; 3];
        let _ = route_all(&conns, &obstacles, &opts);
    }
    let _ = route_all(&[], &[], &opts);
}
