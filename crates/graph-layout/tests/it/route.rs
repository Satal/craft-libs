//! Relations a routed connector must hold.

use crate::common::{Lcg, axis_aligned, collinear_overlaps, inner_overlap, on_boundary, path_hits};
use craft_graph_layout::{Connector, LayoutError, MAX_OBSTACLES, Point, Port, Rect, RouteOptions, Side, route_all, route_orthogonal};

fn port(r: Rect) -> Port {
    Port { rect: r, side: None, point: None }
}

fn bends(pts: &[Point]) -> usize {
    pts.len().saturating_sub(2)
}

/// The path starts on `from`, ends on `to`, is orthogonal and crosses no obstacle but its own
/// endpoints.
fn assert_route(pts: &[Point], from: &Rect, to: &Rect, obstacles: &[Rect]) {
    assert!(pts.len() >= 2, "{pts:?}");
    assert!(axis_aligned(pts), "not orthogonal: {pts:?}");
    assert!(on_boundary(from, pts[0]), "does not start on its source: {pts:?} {from:?}");
    assert!(on_boundary(to, pts[pts.len() - 1]), "does not end on its target: {pts:?} {to:?}");
    assert!(!path_hits(pts, from) && !path_hits(pts, to), "passes through an endpoint: {pts:?}");
    for o in obstacles {
        let own = o == from || o == to || o.contains_rect(from) || o.contains_rect(to);
        assert!(own || !path_hits(pts, o), "crosses {o:?}: {pts:?}");
    }
}

#[test]
fn facing_boxes_get_a_straight_line() {
    let (a, b) = (Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(200.0, 0.0, 40.0, 30.0));
    let pts = route_orthogonal(&port(a), &port(b), &[a, b], &RouteOptions::default()).unwrap().unwrap();
    assert_route(&pts, &a, &b, &[]);
    assert_eq!(bends(&pts), 0, "{pts:?}");
}

#[test]
fn a_wall_between_is_walked_round() {
    let (a, b) = (Rect::new(0.0, 100.0, 40.0, 30.0), Rect::new(300.0, 100.0, 40.0, 30.0));
    let wall = Rect::new(140.0, 0.0, 30.0, 260.0);
    let pts = route_orthogonal(&port(a), &port(b), &[a, b, wall], &RouteOptions::default()).unwrap().unwrap();
    assert_route(&pts, &a, &b, &[wall]);
    assert!(pts.iter().any(|p| p.y < wall.y || p.y > wall.bottom()), "must go round the wall: {pts:?}");
}

#[test]
fn random_obstacle_fields_route_cleanly() {
    let mut r = Lcg(7);
    let mut routed = 0;
    for _ in 0..30 {
        let mut obstacles: Vec<Rect> = Vec::new();
        while obstacles.len() < 25 {
            let c = Rect::new(r.below(900) as f64, r.below(700) as f64, 20.0 + r.below(80) as f64, 15.0 + r.below(60) as f64);
            let room = Rect::new(c.x - 8.0, c.y - 8.0, c.width + 16.0, c.height + 16.0);
            if obstacles.iter().all(|o| !inner_overlap(o, &room)) {
                obstacles.push(c);
            }
        }
        let (i, j) = (r.below(25), r.below(25));
        if i == j {
            continue;
        }
        let (a, b) = (obstacles[i], obstacles[j]);
        if let Some(pts) = route_orthogonal(&port(a), &port(b), &obstacles, &RouteOptions::default()).unwrap() {
            assert_route(&pts, &a, &b, &obstacles);
            routed += 1;
        }
    }
    assert!(routed >= 20, "only {routed} of the open fields were routed");
}

#[test]
fn an_enclosed_target_is_unreachable() {
    let a = Rect::new(0.0, 0.0, 30.0, 30.0);
    let b = Rect::new(200.0, 200.0, 30.0, 30.0);
    let ring = [
        Rect::new(150.0, 150.0, 130.0, 10.0),
        Rect::new(150.0, 270.0, 130.0, 10.0),
        Rect::new(150.0, 150.0, 10.0, 130.0),
        Rect::new(270.0, 150.0, 10.0, 130.0),
    ];
    let got = route_orthogonal(&port(a), &port(b), &ring, &RouteOptions::default()).unwrap();
    assert!(got.is_none(), "{got:?}");
}

#[test]
fn a_container_of_the_endpoint_does_not_block() {
    let group = Rect::new(0.0, 0.0, 200.0, 200.0);
    let a = Rect::new(80.0, 80.0, 40.0, 30.0);
    let b = Rect::new(400.0, 80.0, 40.0, 30.0);
    let pts = route_orthogonal(&port(a), &port(b), &[group, a, b], &RouteOptions::default()).unwrap().unwrap();
    assert_route(&pts, &a, &b, &[]);
}

#[test]
fn clearance_is_kept_where_there_is_room() {
    let (a, b) = (Rect::new(0.0, 100.0, 40.0, 30.0), Rect::new(400.0, 100.0, 40.0, 30.0));
    let wall = Rect::new(180.0, 40.0, 40.0, 160.0);
    let opts = RouteOptions { clearance: 20.0, ..RouteOptions::default() };
    let pts = route_orthogonal(&port(a), &port(b), &[wall], &opts).unwrap().unwrap();
    let grown = Rect::new(wall.x - 19.0, wall.y - 19.0, wall.width + 38.0, wall.height + 38.0);
    assert_route(&pts, &a, &b, &[wall]);
    assert!(!path_hits(&pts, &grown), "passes closer than the clearance: {pts:?}");
}

#[test]
fn the_preferred_side_is_used() {
    let (a, b) = (Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(200.0, 0.0, 40.0, 30.0));
    let from = Port { side: Some(Side::Bottom), ..port(a) };
    let to = Port { side: Some(Side::Top), ..port(b) };
    let pts = route_orthogonal(&from, &to, &[], &RouteOptions::default()).unwrap().unwrap();
    assert_route(&pts, &a, &b, &[]);
    assert!((pts[0].y - a.bottom()).abs() < 1e-6, "should leave from the bottom: {pts:?}");
    assert!((pts[pts.len() - 1].y - b.y).abs() < 1e-6, "should enter from the top: {pts:?}");
}

#[test]
fn a_fixed_point_is_where_the_route_starts() {
    let (a, b) = (Rect::new(0.0, 0.0, 100.0, 30.0), Rect::new(0.0, 200.0, 100.0, 30.0));
    let from = Port { point: Some(Point::new(20.0, 30.0)), ..port(a) };
    let pts = route_orthogonal(&from, &port(b), &[], &RouteOptions::default()).unwrap().unwrap();
    assert_route(&pts, &a, &b, &[]);
    assert!((pts[0].x - 20.0).abs() < 1e-6 && (pts[0].y - 30.0).abs() < 1e-6, "{pts:?}");
}

#[test]
fn route_all_separates_parallel_connectors() {
    let left: Vec<Rect> = (0..4).map(|i| Rect::new(0.0, i as f64 * 60.0, 40.0, 30.0)).collect();
    let right: Vec<Rect> = (0..4).map(|i| Rect::new(500.0, 300.0 + i as f64 * 60.0, 40.0, 30.0)).collect();
    let wall = Rect::new(200.0, 0.0, 80.0, 400.0);
    let mut obstacles = vec![wall];
    obstacles.extend(&left);
    obstacles.extend(&right);
    let conns: Vec<Connector> = (0..4).map(|i| Connector { from: port(left[i]), to: port(right[i]) }).collect();
    let routes = route_all(&conns, &obstacles, &RouteOptions::default()).unwrap();
    let paths: Vec<Vec<Point>> = routes.into_iter().map(Option::unwrap).collect();
    for (i, p) in paths.iter().enumerate() {
        assert_route(p, &left[i], &right[i], &obstacles);
    }
    assert_eq!(collinear_overlaps(&paths), 0, "{paths:?}");
}

#[test]
fn connectors_sharing_a_side_attach_at_different_points() {
    let hub = Rect::new(200.0, 0.0, 120.0, 40.0);
    let leaves: Vec<Rect> = (0..3).map(|i| Rect::new(i as f64 * 200.0, 200.0, 60.0, 30.0)).collect();
    let mut obstacles = vec![hub];
    obstacles.extend(&leaves);
    let conns: Vec<Connector> = leaves.iter().map(|&l| Connector { from: Port { side: Some(Side::Bottom), ..port(hub) }, to: port(l) }).collect();
    let paths: Vec<Vec<Point>> = route_all(&conns, &obstacles, &RouteOptions::default()).unwrap().into_iter().map(Option::unwrap).collect();
    let mut xs: Vec<f64> = paths.iter().map(|p| p[0].x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(xs.windows(2).all(|w| w[1] - w[0] > 1.0), "{xs:?}");
    for (i, p) in paths.iter().enumerate() {
        assert_route(p, &hub, &leaves[i], &obstacles);
    }
}

#[test]
fn routing_is_deterministic() {
    let obstacles: Vec<Rect> = (0..12).map(|i| Rect::new((i % 4) as f64 * 120.0, (i / 4) as f64 * 100.0, 60.0, 40.0)).collect();
    let conns: Vec<Connector> =
        [(0, 11), (3, 8), (1, 10), (0, 3)].iter().map(|&(a, b)| Connector { from: port(obstacles[a]), to: port(obstacles[b]) }).collect();
    let a = route_all(&conns, &obstacles, &RouteOptions::default()).unwrap();
    let b = route_all(&conns, &obstacles, &RouteOptions::default()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn bad_input_is_an_error() {
    let ok = Rect::new(0.0, 0.0, 10.0, 10.0);
    let opts = RouteOptions::default();
    let nan = Rect::new(f64::NAN, 0.0, 10.0, 10.0);
    assert!(matches!(route_orthogonal(&port(nan), &port(ok), &[], &opts), Err(LayoutError::InvalidNumber { .. })));
    assert!(matches!(route_orthogonal(&port(ok), &port(ok), &[nan], &opts), Err(LayoutError::InvalidNumber { .. })));
    let bad = RouteOptions { clearance: -1.0, ..opts };
    assert!(route_orthogonal(&port(ok), &port(ok), &[], &bad).is_err());
    let many = vec![ok; MAX_OBSTACLES + 1];
    assert!(matches!(route_orthogonal(&port(ok), &port(ok), &many, &opts), Err(LayoutError::TooLarge { .. })));
}

/// Horizontal segments of `p` that run through the corridor `x0..x1`, as their y.
fn corridor_runs(p: &[Point], x0: f64, x1: f64) -> Vec<f64> {
    p.windows(2).filter(|w| (w[0].y - w[1].y).abs() < 1e-6 && w[0].x.min(w[1].x) < x0 && w[0].x.max(w[1].x) > x1).map(|w| w[0].y).collect()
}

/// Two tall walls with a corridor between them, y 40 to 140, too tall to go round.
fn corridor() -> (Rect, Rect) {
    (Rect::new(0.0, -1000.0, 400.0, 1040.0), Rect::new(0.0, 140.0, 400.0, 1000.0))
}

#[test]
fn route_all_runs_a_lone_connector_down_the_middle_of_its_channel() {
    let (top, bottom) = corridor();
    let (a, b) = (Rect::new(-100.0, -20.0, 40.0, 30.0), Rect::new(460.0, 200.0, 40.0, 30.0));
    let obstacles = [top, bottom, a, b];
    let paths = route_all(&[Connector { from: port(a), to: port(b) }], &obstacles, &RouteOptions::default()).unwrap();
    let p = paths[0].as_ref().unwrap();
    assert_route(p, &a, &b, &obstacles);
    let runs = corridor_runs(p, 0.0, 400.0);
    assert!(!runs.is_empty(), "should go through the corridor: {p:?}");
    let third = (bottom.y - top.bottom()) / 3.0;
    assert!(runs.iter().all(|&y| y > top.bottom() + third && y < bottom.y - third), "hugs a wall: {p:?}");
}

#[test]
fn route_all_spreads_connectors_sharing_a_channel_across_it() {
    let (top, bottom) = corridor();
    let from: Vec<Rect> = (0..3).map(|i| Rect::new(-100.0, -20.0 - 50.0 * i as f64, 40.0, 30.0)).collect();
    let to: Vec<Rect> = (0..3).map(|i| Rect::new(460.0 + 60.0 * i as f64, 200.0, 40.0, 30.0)).collect();
    let mut obstacles = vec![top, bottom];
    obstacles.extend(&from);
    obstacles.extend(&to);
    let opts = RouteOptions::default();
    let conns: Vec<Connector> = (0..3).map(|i| Connector { from: port(from[i]), to: port(to[i]) }).collect();
    let paths: Vec<Vec<Point>> = route_all(&conns, &obstacles, &opts).unwrap().into_iter().map(Option::unwrap).collect();
    let mut ys = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        assert_route(p, &from[i], &to[i], &obstacles);
        ys.extend(corridor_runs(p, 0.0, 400.0));
    }
    assert_eq!(ys.len(), 3, "each should go through the corridor once: {paths:?}");
    ys.sort_by(f64::total_cmp);
    assert!(ys[0] >= top.bottom() + opts.clearance && ys[2] <= bottom.y - opts.clearance, "hugs a wall: {ys:?}");
    assert!(ys.windows(2).all(|w| w[1] - w[0] > 2.0 * opts.separation), "bunched together: {ys:?}");
    assert_eq!(crate::common::crossings(&paths), 0, "{paths:?}");
}
