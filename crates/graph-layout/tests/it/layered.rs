//! Relations a layered layout must hold.

use crate::common::{
    EPS, Lcg, all_finite, assert_edges_clean, assert_no_node_overlap, contains, crossings, graph, inner_overlap, node, path_hits, random_dag,
};
use craft_graph_layout::{Direction, EdgeSpec, Graph, GroupSpec, LayeredOptions, Layout, LayoutError, MAX_NODES, Point, Rect, Size, layered};

fn lay(g: &Graph) -> Layout {
    layered(g, &LayeredOptions::default()).unwrap()
}

fn in_group(g: &Graph, mut grp: Option<usize>, target: usize) -> bool {
    while let Some(x) = grp {
        if x == target {
            return true;
        }
        grp = g.groups[x].parent;
    }
    false
}

/// Members inside their groups and every ancestor, non-members outside, child groups inside
/// their parents and unrelated groups apart.
pub fn assert_groups_hold(g: &Graph, l: &Layout) {
    for gi in 0..g.groups.len() {
        let gr = &l.groups[gi];
        for (v, n) in g.nodes.iter().enumerate() {
            if in_group(g, n.group, gi) {
                assert!(contains(gr, &l.nodes[v]), "node {v} outside its group {gi}: {:?} {gr:?}", l.nodes[v]);
            } else {
                assert!(!inner_overlap(gr, &l.nodes[v]), "node {v} inside group {gi} it is not in: {:?} {gr:?}", l.nodes[v]);
            }
        }
        for gj in 0..g.groups.len() {
            if gj == gi {
                continue;
            }
            if in_group(g, Some(gj), gi) {
                assert!(contains(gr, &l.groups[gj]), "group {gj} outside its ancestor {gi}");
            } else if !in_group(g, Some(gi), gj) {
                assert!(!inner_overlap(gr, &l.groups[gj]), "groups {gi} and {gj} overlap: {gr:?} {:?}", l.groups[gj]);
            }
        }
    }
}

#[test]
fn edges_point_forward_and_stay_clean_on_a_dag() {
    for seed in 1..6 {
        let g = random_dag(seed, 30, 20);
        let l = lay(&g);
        assert_no_node_overlap(&l);
        assert_edges_clean(&g, &l);
        for e in &g.edges {
            assert!(l.nodes[e.from].bottom() <= l.nodes[e.to].y + EPS, "seed {seed}: edge {e:?} does not point down");
        }
    }
}

#[test]
fn each_direction_puts_targets_after_sources_and_ports_on_facing_sides() {
    let g = graph(5, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)]);
    for dir in [Direction::Down, Direction::Up, Direction::Right, Direction::Left] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        assert_no_node_overlap(&l);
        assert_edges_clean(&g, &l);
        for (i, e) in g.edges.iter().enumerate() {
            let (a, b) = (l.nodes[e.from], l.nodes[e.to]);
            let pts = &l.edges[i];
            let (s, t) = (pts[0], pts[pts.len() - 1]);
            let (ok, leaves, enters) = match dir {
                Direction::Down => (a.bottom() <= b.y + EPS, (s.y - a.bottom()).abs() < EPS, (t.y - b.y).abs() < EPS),
                Direction::Up => (b.bottom() <= a.y + EPS, (s.y - a.y).abs() < EPS, (t.y - b.bottom()).abs() < EPS),
                Direction::Right => (a.right() <= b.x + EPS, (s.x - a.right()).abs() < EPS, (t.x - b.x).abs() < EPS),
                Direction::Left => (b.right() <= a.x + EPS, (s.x - a.x).abs() < EPS, (t.x - b.right()).abs() < EPS),
            };
            assert!(ok && leaves && enters, "{dir:?} edge {i}: {a:?} -> {b:?} via {pts:?}");
        }
    }
}

#[test]
fn a_cycle_keeps_one_edge_back_and_the_rest_forward() {
    let g = graph(4, &[(0, 1), (1, 2), (2, 3), (3, 1)]);
    let l = lay(&g);
    assert_no_node_overlap(&l);
    assert_edges_clean(&g, &l);
    let forward = g.edges.iter().filter(|e| l.nodes[e.from].bottom() <= l.nodes[e.to].y + EPS).count();
    assert_eq!(forward, 3, "only the edge closing the loop should point back");
    assert!(l.nodes[3].y > l.nodes[1].bottom(), "the loop's body should follow its entry");
}

#[test]
fn self_loops_and_multi_edges_get_distinct_paths() {
    let g = graph(2, &[(0, 0), (0, 0), (0, 1), (0, 1), (0, 1)]);
    let l = lay(&g);
    assert_edges_clean(&g, &l);
    for i in 0..g.edges.len() {
        for j in i + 1..g.edges.len() {
            assert_ne!(l.edges[i], l.edges[j], "edges {i} and {j} share a path");
        }
    }
    let starts: Vec<f64> = l.edges[2..].iter().map(|p| p[0].x).collect();
    assert!(starts.windows(2).all(|w| (w[0] - w[1]).abs() > 1.0), "parallel edges should leave from spread ports: {starts:?}");
}

#[test]
fn ports_spread_along_a_shared_side() {
    let mut g = graph(4, &[(0, 1), (0, 2), (0, 3)]);
    g.nodes[0].size = Size::new(200.0, 30.0);
    let l = lay(&g);
    let mut xs: Vec<f64> = l.edges.iter().map(|p| p[0].x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(xs.windows(2).all(|w| w[1] - w[0] > 1.0), "ports should not coincide: {xs:?}");
}

#[test]
fn min_len_pushes_the_target_further() {
    let mut g = graph(3, &[(0, 1), (0, 2)]);
    g.edges[1].min_len = 3;
    let l = lay(&g);
    assert!(l.nodes[2].y > l.nodes[1].bottom(), "a longer edge should land below a normal one");
    assert_edges_clean(&g, &l);
}

#[test]
fn variable_sizes_do_not_overlap() {
    let mut g = random_dag(7, 25, 10);
    for (i, n) in g.nodes.iter_mut().enumerate() {
        n.size = Size::new(10.0 + (i * 37 % 150) as f64, 10.0 + (i * 53 % 90) as f64);
    }
    let l = lay(&g);
    assert_no_node_overlap(&l);
    assert_edges_clean(&g, &l);
}

#[test]
fn crossing_reduction_beats_the_input_order() {
    // Two ranks whose input order crosses every pair of edges.
    let k = 6;
    let edges: Vec<(usize, usize)> = (0..k).map(|i| (i, 2 * k - 1 - i)).collect();
    let naive = k * (k - 1) / 2;
    let l = lay(&graph(2 * k, &edges));
    assert!(crossings(&l.edges) < naive, "{} crossings, input order has {naive}", crossings(&l.edges));
    // A denser graph: compare with the crossings of the same edges laid out by index order.
    let g = random_dag(11, 40, 30);
    let l = lay(&g);
    let ordered = by_index_order(&g, &l);
    assert!(crossings(&l.edges) < crossings(&ordered), "{} vs naive {}", crossings(&l.edges), crossings(&ordered));
}

/// The same ranks as `l`, each rank ordered by node index, edges drawn as one jog each.
fn by_index_order(g: &Graph, l: &Layout) -> Vec<Vec<craft_graph_layout::Point>> {
    use craft_graph_layout::Point;
    let mut xs = vec![0.0; g.nodes.len()];
    let mut ranks: Vec<(f64, Vec<usize>)> = Vec::new();
    for v in 0..g.nodes.len() {
        let y = l.nodes[v].center().y;
        match ranks.iter_mut().find(|r| (r.0 - y).abs() < 1.0) {
            Some(r) => r.1.push(v),
            None => ranks.push((y, vec![v])),
        }
    }
    for (_, members) in &ranks {
        for (i, &v) in members.iter().enumerate() {
            xs[v] = i as f64 * 100.0;
        }
    }
    g.edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (a, b) = (l.nodes[e.from], l.nodes[e.to]);
            let mid = (a.bottom() + b.y) / 2.0 + i as f64 * 0.01;
            vec![Point::new(xs[e.from], a.bottom()), Point::new(xs[e.from], mid), Point::new(xs[e.to], mid), Point::new(xs[e.to], b.y)]
        })
        .collect()
}

#[test]
fn disconnected_components_sit_side_by_side() {
    let g = graph(6, &[(0, 1), (2, 3), (4, 5)]);
    let l = lay(&g);
    assert_no_node_overlap(&l);
    assert_edges_clean(&g, &l);
    assert!(l.size.width > l.nodes[0].width * 2.0, "components should be packed across, not stacked into one column");
}

fn nested_graph() -> Graph {
    let mut g = graph(9, &[(0, 1), (1, 2), (2, 3), (1, 4), (4, 5), (3, 6), (5, 6), (6, 7), (0, 8)]);
    g.groups = vec![
        GroupSpec { parent: None, padding: 10.0, title_height: 20.0 },
        GroupSpec { parent: Some(0), padding: 10.0, title_height: 20.0 },
        GroupSpec { parent: Some(1), padding: 10.0, title_height: 0.0 },
        GroupSpec { parent: None, padding: 10.0, title_height: 20.0 },
    ];
    for (v, grp) in [(1, 0), (2, 1), (3, 2), (4, 0), (6, 3), (7, 3)] {
        g.nodes[v].group = Some(grp);
    }
    g
}

#[test]
fn nested_groups_contain_members_and_exclude_the_rest() {
    let g = nested_graph();
    for dir in [Direction::Down, Direction::Up, Direction::Right, Direction::Left] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        assert_no_node_overlap(&l);
        assert_groups_hold(&g, &l);
        assert_edges_clean(&g, &l);
    }
}

#[test]
fn edges_go_round_title_bands_when_ranks_run_vertically() {
    let g = nested_graph();
    for dir in [Direction::Down, Direction::Up] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        assert_edges_clean(&g, &l);
        for (gi, spec) in g.groups.iter().enumerate().filter(|(_, s)| s.title_height > 0.0) {
            let band = Rect::new(l.groups[gi].x, l.groups[gi].y, l.groups[gi].width, spec.title_height);
            for (i, pts) in l.edges.iter().enumerate() {
                assert!(!path_hits(pts, &band), "{dir:?}: edge {i} crosses group {gi}'s title: {pts:?} {band:?}");
            }
        }
    }
}

#[test]
fn the_title_band_stays_clear_of_members_in_every_direction() {
    let g = nested_graph();
    for dir in [Direction::Down, Direction::Up, Direction::Right, Direction::Left] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        for (gi, spec) in g.groups.iter().enumerate() {
            let band = Rect::new(l.groups[gi].x, l.groups[gi].y, l.groups[gi].width, spec.title_height + spec.padding);
            for (v, r) in l.nodes.iter().enumerate() {
                assert!(!inner_overlap(&band, r), "{dir:?}: node {v} is under group {gi}'s title");
            }
            for (gj, r) in l.groups.iter().enumerate() {
                if gj != gi && contains(&l.groups[gi], r) {
                    assert!(!inner_overlap(&band, r), "{dir:?}: group {gj} is under group {gi}'s title");
                }
            }
        }
    }
}

#[test]
fn an_empty_group_still_gets_a_box_of_its_own() {
    let mut g = graph(2, &[(0, 1)]);
    g.groups = vec![GroupSpec { parent: None, padding: 8.0, title_height: 0.0 }];
    let l = lay(&g);
    assert!(l.groups[0].width > 0.0 && l.groups[0].height > 0.0);
    assert!(l.nodes.iter().all(|r| !inner_overlap(r, &l.groups[0])));
}

#[test]
fn labels_sit_on_their_edges_clear_of_nodes_and_other_edges() {
    let mut g = graph(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
    g.edges[0].label = Some(Size::new(60.0, 16.0));
    g.edges[3].label = Some(Size::new(40.0, 16.0));
    for dir in [Direction::Down, Direction::Right] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        assert_edges_clean(&g, &l);
        for (i, e) in g.edges.iter().enumerate() {
            let Some(s) = e.label else {
                assert!(l.labels[i].is_none());
                continue;
            };
            let c = l.labels[i].unwrap();
            let r = Rect::new(c.x - s.width / 2.0, c.y - s.height / 2.0, s.width, s.height);
            for (v, n) in l.nodes.iter().enumerate() {
                assert!(!inner_overlap(&r, n), "{dir:?}: label {i} covers node {v}");
            }
            assert!(contains(&Rect::new(0.0, 0.0, l.size.width, l.size.height), &r), "{dir:?}: label {i} is outside the layout");
            let on = |a: &Point, b: &Point| {
                let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
                c.x >= x0 - EPS && c.x <= x1 + EPS && c.y >= y0 - EPS && c.y <= y1 + EPS
            };
            assert!(l.edges[i].windows(2).any(|w| on(&w[0], &w[1])), "{dir:?}: label {i} is off its edge: {c:?} {:?}", l.edges[i]);
            for (j, p) in l.edges.iter().enumerate() {
                assert!(j == i || !path_hits(p, &r), "{dir:?}: edge {j} runs through label {i}");
            }
        }
    }
}

#[test]
fn layout_is_deterministic() {
    let mut g = random_dag(3, 60, 40);
    g.groups = vec![GroupSpec { parent: None, padding: 8.0, title_height: 12.0 }];
    for v in 10..20 {
        g.nodes[v].group = Some(0);
    }
    let a = lay(&g);
    let b = lay(&g.clone());
    assert_eq!(a, b);
}

#[test]
fn every_node_lies_within_the_reported_size() {
    let g = nested_graph();
    let l = lay(&g);
    let all = Rect::new(0.0, 0.0, l.size.width, l.size.height);
    assert!(l.nodes.iter().chain(&l.groups).all(|r| contains(&all, r)));
    assert!(l.edges.iter().flatten().all(|p| p.x >= -EPS && p.y >= -EPS && p.x <= all.right() + EPS && p.y <= all.bottom() + EPS));
}

#[test]
fn random_grouped_graphs_hold_every_relation() {
    let mut r = Lcg(99);
    for round in 0..12 {
        let n = 8 + r.below(25);
        let mut g = random_dag(round + 100, n, r.below(15));
        let groups = r.below(5);
        g.groups = (0..groups)
            .map(|i| GroupSpec {
                parent: if i > 0 && r.below(2) == 0 { Some(r.below(i)) } else { None },
                padding: 6.0,
                title_height: r.below(2) as f64 * 14.0,
            })
            .collect();
        for v in 0..n {
            if groups > 0 && r.below(3) > 0 {
                g.nodes[v].group = Some(r.below(groups));
            }
        }
        // A few back edges and self-loops.
        g.edges.push(EdgeSpec::new(n - 1, 0));
        g.edges.push(EdgeSpec::new(n / 2, n / 2));
        for dir in [Direction::Down, Direction::Right] {
            let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
            assert!(all_finite(&l));
            assert_no_node_overlap(&l);
            assert_groups_hold(&g, &l);
            assert_edges_clean(&g, &l);
        }
    }
}

#[test]
fn five_hundred_nodes_lay_out() {
    let g = random_dag(5, 500, 250);
    let l = lay(&g);
    assert_no_node_overlap(&l);
}

#[test]
fn inputs_above_the_caps_are_refused() {
    let g = Graph { nodes: vec![node(1.0, 1.0); MAX_NODES + 1], ..Graph::default() };
    assert!(matches!(layered(&g, &LayeredOptions::default()), Err(LayoutError::TooLarge { .. })));
}

#[test]
fn the_empty_graph_is_empty() {
    let l = lay(&Graph::default());
    assert!(l.nodes.is_empty() && l.edges.is_empty());
    assert_eq!(l.size, Size::default());
}

/// The example flowchart: a validation loop (Show error back to Read input) and a retry loop
/// (Wait back to Transform), with labelled branches. It has a drawing without crossings: each
/// loop's back edge runs up beside the nodes it spans, on the side away from the other branch.
fn loops_flowchart() -> Graph {
    let mut g = graph(12, &[(0, 1), (1, 2), (2, 3), (3, 1), (2, 4), (4, 5), (5, 6), (6, 7), (7, 5), (6, 8), (8, 9), (8, 10), (9, 11), (10, 11)]);
    for (e, w) in [(2, 30.0), (4, 37.0), (7, 37.0), (9, 30.0)] {
        g.edges[e].label = Some(Size::new(w, 16.0));
    }
    g
}

#[test]
fn back_edges_wrap_their_loops_without_crossing_other_edges() {
    let g = loops_flowchart();
    for dir in [Direction::Down, Direction::Right] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        assert_edges_clean(&g, &l);
        assert_eq!(crossings(&l.edges), 0, "{dir:?}: a crossing-free drawing exists: {:?}", l.edges);
    }
}

#[test]
fn siblings_keep_their_input_order_when_crossings_tie() {
    // User -> LB -> {Web 1, Web 2} -> DB -> Replica, Web 1 -> Cache; Web 1 and Web 2 share a group.
    let mut g = graph(7, &[(0, 1), (1, 2), (1, 3), (2, 4), (3, 4), (4, 5), (2, 6), (4, 6)]);
    g.groups = vec![
        GroupSpec { parent: None, padding: 10.0, title_height: 20.0 },
        GroupSpec { parent: Some(0), padding: 10.0, title_height: 20.0 },
        GroupSpec { parent: Some(0), padding: 10.0, title_height: 20.0 },
    ];
    for (v, grp) in [(1, 0), (2, 1), (3, 1), (4, 2), (5, 2)] {
        g.nodes[v].group = Some(grp);
    }
    for dir in [Direction::Down, Direction::Right] {
        let l = layered(&g, &LayeredOptions { direction: dir, ..LayeredOptions::default() }).unwrap();
        let (a, b) = (l.nodes[2].center(), l.nodes[3].center());
        let first = if dir == Direction::Down { a.x < b.x } else { a.y < b.y };
        assert!(first, "{dir:?}: node 2 should come before node 3: {a:?} {b:?}");
    }
    // Plain fan-outs too.
    let l = lay(&graph(4, &[(0, 1), (0, 2), (0, 3)]));
    assert!(l.nodes[1].x < l.nodes[2].x && l.nodes[2].x < l.nodes[3].x, "{:?}", l.nodes);
}
