//! Orthogonal connector routing after Wybrow, Marriott and Stuckey, "Orthogonal Connector
//! Routing" (Graph Drawing 2009): a grid over the interesting coordinates of the inflated
//! obstacles, A* over (grid point, heading) minimising length plus a penalty per bend, and nudging
//! that orders the segments sharing a channel and spreads them across its free width, so
//! connectors neither hug obstacles nor run bunched together.

use crate::channel::{ChannelSeg, assign_tracks};
use crate::layered::simplify;
use crate::{Connector, LayoutError, MAX_CONNECTORS, MAX_OBSTACLES, Point, Port, Rect, RouteOptions, Side, valid_coord, valid_len};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

const EPS: f64 = 1e-6;
/// Most grid points one search may use.
const MAX_GRID: usize = 1_000_000;

/// Routes one orthogonal connector from `from` to `to` around `obstacles`.
///
/// The path starts on the boundary of `from.rect` and ends on the boundary of `to.rect`, leaving
/// and entering perpendicular to the side, never crosses the open interior of any obstacle (the
/// endpoint rectangles included), and minimises length plus `opts.bend_penalty` per bend. An
/// obstacle that contains an endpoint rectangle is treated as a container, not an obstacle.
/// Returns `Ok(None)` when no such path exists.
///
/// # Errors
/// [`LayoutError`] when there are more than [`MAX_OBSTACLES`](crate::MAX_OBSTACLES) obstacles or
/// a number is invalid.
pub fn route_orthogonal(from: &Port, to: &Port, obstacles: &[Rect], opts: &RouteOptions) -> Result<Option<Vec<Point>>, LayoutError> {
    validate_common(obstacles, opts)?;
    validate_port(from, 0)?;
    validate_port(to, 1)?;
    Ok(route_one(from, to, obstacles, opts).map(|r| r.pts))
}

/// Routes every connector as [`route_orthogonal`] does, then spreads connectors that share a side
/// of a shape along it, and spreads the segments that share a channel between obstacles evenly
/// across it (a lone segment runs down the middle), at least `opts.separation` apart where the
/// free space allows. Each entry is `None` when that connector has no route.
///
/// # Errors
/// [`LayoutError`] when there are more than [`MAX_CONNECTORS`](crate::MAX_CONNECTORS) connectors
/// or [`MAX_OBSTACLES`](crate::MAX_OBSTACLES) obstacles, or a number is invalid.
pub fn route_all(connectors: &[Connector], obstacles: &[Rect], opts: &RouteOptions) -> Result<Vec<Option<Vec<Point>>>, LayoutError> {
    if connectors.len() > MAX_CONNECTORS {
        return Err(LayoutError::TooLarge { what: "connectors", count: connectors.len(), limit: MAX_CONNECTORS });
    }
    validate_common(obstacles, opts)?;
    for (i, c) in connectors.iter().enumerate() {
        validate_port(&c.from, i)?;
        validate_port(&c.to, i)?;
    }
    let mut routes: Vec<Option<Routed>> = connectors.iter().map(|c| route_one(&c.from, &c.to, obstacles, opts)).collect();
    spread_shared_sides(connectors, obstacles, opts, &mut routes);
    let mut paths: Vec<Option<Vec<Point>>> = routes.into_iter().map(|r| r.map(|r| r.pts)).collect();
    let checks: Vec<Vec<Rect>> = connectors.iter().map(|c| blocking(obstacles, &c.from.rect, &c.to.rect)).collect();
    for horizontal in [true, false] {
        let before = paths.clone();
        centre_channels(&mut paths, obstacles, opts, horizontal);
        nudge(&mut paths, obstacles, opts.separation, horizontal);
        for (i, p) in paths.iter_mut().enumerate() {
            if let Some(pts) = p {
                simplify(pts);
                if !valid_path(pts, &checks[i]) {
                    *p = before[i].clone();
                }
            }
        }
    }
    Ok(paths)
}

fn validate_common(obstacles: &[Rect], opts: &RouteOptions) -> Result<(), LayoutError> {
    if obstacles.len() > MAX_OBSTACLES {
        return Err(LayoutError::TooLarge { what: "obstacles", count: obstacles.len(), limit: MAX_OBSTACLES });
    }
    for (i, v) in [opts.clearance, opts.bend_penalty, opts.separation].into_iter().enumerate() {
        if !valid_len(v) {
            return Err(LayoutError::InvalidNumber { what: "route option", index: i });
        }
    }
    if let Some(i) = obstacles.iter().position(|r| !r.is_valid()) {
        return Err(LayoutError::InvalidNumber { what: "obstacle", index: i });
    }
    Ok(())
}

fn validate_port(p: &Port, index: usize) -> Result<(), LayoutError> {
    let point_ok = p.point.is_none_or(|q| valid_coord(q.x) && valid_coord(q.y));
    if p.rect.is_valid() && point_ok { Ok(()) } else { Err(LayoutError::InvalidNumber { what: "port", index }) }
}

/// Headings: 0 = +x, 1 = +y, 2 = -x, 3 = -y.
fn outward(s: Side) -> usize {
    match s {
        Side::Right => 0,
        Side::Bottom => 1,
        Side::Left => 2,
        Side::Top => 3,
    }
}

fn step(d: usize) -> (f64, f64) {
    match d {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    }
}

const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

/// A place a connector may attach: the boundary point, its side and the extra cost of using it.
#[derive(Clone, Copy, Debug)]
struct Term {
    at: Point,
    side: Side,
    cost: f64,
}

impl Term {
    fn stub(&self, c: f64) -> Point {
        let (dx, dy) = step(outward(self.side));
        Point::new(self.at.x + dx * c, self.at.y + dy * c)
    }
}

fn nearest_side(r: &Rect, p: Point) -> Side {
    let d = [(p.y - r.y).abs(), (r.right() - p.x).abs(), (r.bottom() - p.y).abs(), (p.x - r.x).abs()];
    let mut best = 0;
    for i in 1..4 {
        if d[i] < d[best] {
            best = i;
        }
    }
    SIDES[best]
}

fn side_point(r: &Rect, s: Side, t: f64) -> Point {
    match s {
        Side::Top => Point::new(r.x + r.width * t, r.y),
        Side::Bottom => Point::new(r.x + r.width * t, r.bottom()),
        Side::Left => Point::new(r.x, r.y + r.height * t),
        Side::Right => Point::new(r.right(), r.y + r.height * t),
    }
}

fn terminals(port: &Port, bend: f64) -> Vec<Term> {
    let r = &port.rect;
    if let Some(p) = port.point {
        let side = port.side.unwrap_or_else(|| nearest_side(r, p));
        let at = match side {
            Side::Top => Point::new(p.x.clamp(r.x, r.right()), r.y),
            Side::Bottom => Point::new(p.x.clamp(r.x, r.right()), r.bottom()),
            Side::Left => Point::new(r.x, p.y.clamp(r.y, r.bottom())),
            Side::Right => Point::new(r.right(), p.y.clamp(r.y, r.bottom())),
        };
        return vec![Term { at, side, cost: 0.0 }];
    }
    SIDES
        .iter()
        .map(|&s| {
            let cost = if port.side.is_none_or(|p| p == s) { 0.0 } else { 4.0 * bend + r.width + r.height };
            Term { at: side_point(r, s, 0.5), side: s, cost }
        })
        .collect()
}

fn approx_eq(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() <= EPS && (a.y - b.y).abs() <= EPS && (a.width - b.width).abs() <= EPS && (a.height - b.height).abs() <= EPS
}

/// The rectangles a connector between `a` and `b` must avoid: the endpoints themselves and every
/// obstacle except duplicates of the endpoints and containers of either endpoint.
fn blocking(obstacles: &[Rect], a: &Rect, b: &Rect) -> Vec<Rect> {
    let container = |o: &Rect, e: &Rect| o.contains_rect(e) && !approx_eq(o, e);
    let mut out = Vec::with_capacity(obstacles.len() + 2);
    if !container(a, b) {
        out.push(*a);
    }
    if !container(b, a) && !approx_eq(a, b) {
        out.push(*b);
    }
    for o in obstacles {
        if approx_eq(o, a) || approx_eq(o, b) || container(o, a) || container(o, b) {
            continue;
        }
        out.push(*o);
    }
    out
}

fn inflate(r: &Rect, c: f64) -> Rect {
    Rect::new(r.x - c, r.y - c, r.width + 2.0 * c, r.height + 2.0 * c)
}

/// Whether the segment `a`-`b` (axis-aligned) passes through the open interior of `r`.
fn seg_hits(a: Point, b: Point, r: &Rect) -> bool {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    let ox = x1.min(r.right()) - x0.max(r.x);
    let oy = y1.min(r.bottom()) - y0.max(r.y);
    if (x1 - x0) <= EPS { x0 > r.x + EPS && x0 < r.right() - EPS && oy > EPS } else { y0 > r.y + EPS && y0 < r.bottom() - EPS && ox > EPS }
}

fn valid_path(pts: &[Point], rects: &[Rect]) -> bool {
    if pts.len() < 2 || pts.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return false;
    }
    pts.windows(2).all(|w| {
        let straight = (w[0].x - w[1].x).abs() <= EPS || (w[0].y - w[1].y).abs() <= EPS;
        straight && rects.iter().all(|r| !seg_hits(w[0], w[1], r))
    })
}

struct Routed {
    pts: Vec<Point>,
}

fn route_one(from: &Port, to: &Port, obstacles: &[Rect], opts: &RouteOptions) -> Option<Routed> {
    let rects = blocking(obstacles, &from.rect, &to.rect);
    let ts = terminals(from, opts.bend_penalty);
    let tt = terminals(to, opts.bend_penalty);
    let c0 = opts.clearance;
    for c in [c0, c0 / 2.0, 0.0] {
        if let Some(pts) = route_with_clearance(&ts, &tt, &rects, c, opts.bend_penalty)
            && valid_path(&pts, &rects)
        {
            return Some(Routed { pts });
        }
        if c0 <= 0.0 {
            break;
        }
    }
    None
}

fn route_with_clearance(ts: &[Term], tt: &[Term], rects: &[Rect], c: f64, bend: f64) -> Option<Vec<Point>> {
    let inflated: Vec<Rect> = rects.iter().map(|r| inflate(r, c)).collect();
    let mut bb = Bounds::new();
    for t in ts.iter().chain(tt) {
        bb.add(t.stub(c));
    }
    let span = (bb.x1 - bb.x0) + (bb.y1 - bb.y0);
    let base = 2.0 * (c + bend) + 0.25 * span + 1.0;
    let mut all = bb;
    for r in &inflated {
        all.add(Point::new(r.x, r.y));
        all.add(Point::new(r.right(), r.bottom()));
    }
    for region in [bb.grow(base), bb.grow(base * 4.0), all.grow(c + 1.0)] {
        let Some(grid) = Grid::build(&region, &inflated, ts, tt, c) else { continue };
        if let Some(pts) = grid.search(ts, tt, c, bend) {
            return Some(pts);
        }
    }
    None
}

#[derive(Clone, Copy)]
struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Bounds {
    fn new() -> Self {
        Self { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }
    }
    fn add(&mut self, p: Point) {
        self.x0 = self.x0.min(p.x);
        self.y0 = self.y0.min(p.y);
        self.x1 = self.x1.max(p.x);
        self.y1 = self.y1.max(p.y);
    }
    fn grow(self, m: f64) -> Self {
        Self { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m }
    }
}

struct Grid {
    xs: Vec<f64>,
    ys: Vec<f64>,
    node_blocked: Vec<bool>,
    /// Segment from (i, j) to (i + 1, j).
    h_blocked: Vec<bool>,
    /// Segment from (i, j) to (i, j + 1).
    v_blocked: Vec<bool>,
}

fn sorted_coords(mut v: Vec<f64>) -> Vec<f64> {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() <= EPS);
    v
}

/// Indices `i` with `lo < v[i] < hi`.
fn strictly_between(v: &[f64], lo: f64, hi: f64) -> std::ops::Range<usize> {
    let a = v.partition_point(|&x| x <= lo + EPS);
    let b = v.partition_point(|&x| x < hi - EPS);
    a..b.max(a)
}

/// Indices `i` with `lo <= v[i]` and `v[i + 1] <= hi`.
fn cells_within(v: &[f64], lo: f64, hi: f64) -> std::ops::Range<usize> {
    let a = v.partition_point(|&x| x < lo - EPS);
    let b = v.partition_point(|&x| x <= hi + EPS);
    a..b.saturating_sub(1).max(a)
}

impl Grid {
    fn build(region: &Bounds, inflated: &[Rect], ts: &[Term], tt: &[Term], c: f64) -> Option<Self> {
        let inside = |r: &Rect| r.x < region.x1 && r.right() > region.x0 && r.y < region.y1 && r.bottom() > region.y0;
        let used: Vec<&Rect> = inflated.iter().filter(|r| inside(r)).collect();
        let mut xs = vec![region.x0, region.x1];
        let mut ys = vec![region.y0, region.y1];
        for r in &used {
            xs.extend([r.x, r.right()]);
            ys.extend([r.y, r.bottom()]);
        }
        for t in ts.iter().chain(tt) {
            let s = t.stub(c);
            xs.push(s.x);
            ys.push(s.y);
        }
        xs.retain(|&x| x >= region.x0 && x <= region.x1);
        ys.retain(|&y| y >= region.y0 && y <= region.y1);
        let (xs, ys) = (sorted_coords(xs), sorted_coords(ys));
        let (nx, ny) = (xs.len(), ys.len());
        if nx.checked_mul(ny).is_none_or(|n| n > MAX_GRID) {
            return None;
        }
        let mut g = Self { node_blocked: vec![false; nx * ny], h_blocked: vec![false; nx * ny], v_blocked: vec![false; nx * ny], xs, ys };
        for r in used {
            g.block(r);
        }
        Some(g)
    }

    fn idx(&self, i: usize, j: usize) -> usize {
        j * self.xs.len() + i
    }

    fn block(&mut self, r: &Rect) {
        let (xi, yi) = (strictly_between(&self.xs, r.x, r.right()), strictly_between(&self.ys, r.y, r.bottom()));
        let (xc, yc) = (cells_within(&self.xs, r.x, r.right()), cells_within(&self.ys, r.y, r.bottom()));
        for j in yi.clone() {
            for i in xi.clone() {
                let k = self.idx(i, j);
                self.node_blocked[k] = true;
            }
            for i in xc.clone() {
                let k = self.idx(i, j);
                self.h_blocked[k] = true;
            }
        }
        for j in yc {
            for i in xi.clone() {
                let k = self.idx(i, j);
                self.v_blocked[k] = true;
            }
        }
    }

    fn find(&self, p: Point) -> Option<usize> {
        let i = self.xs.iter().position(|&x| (x - p.x).abs() <= EPS)?;
        let j = self.ys.iter().position(|&y| (y - p.y).abs() <= EPS)?;
        let k = self.idx(i, j);
        (!self.node_blocked[k]).then_some(k)
    }

    fn point(&self, k: usize) -> Point {
        let nx = self.xs.len();
        Point::new(self.xs[k % nx], self.ys[k / nx])
    }

    /// The grid point one step from `k` in heading `d`, if the step is free.
    fn next(&self, k: usize, d: usize) -> Option<usize> {
        let nx = self.xs.len();
        let (i, j) = (k % nx, k / nx);
        let to = match d {
            0 if i + 1 < nx && !self.h_blocked[k] => k + 1,
            1 if j + 1 < self.ys.len() && !self.v_blocked[k] => k + nx,
            2 if i > 0 && !self.h_blocked[k - 1] => k - 1,
            3 if j > 0 && !self.v_blocked[k - nx] => k - nx,
            _ => return None,
        };
        (!self.node_blocked[to]).then_some(to)
    }

    fn search(&self, ts: &[Term], tt: &[Term], c: f64, bend: f64) -> Option<Vec<Point>> {
        let goals: Vec<(usize, usize, &Term)> = tt.iter().filter_map(|t| Some((self.find(t.stub(c))?, (outward(t.side) + 2) % 4, t))).collect();
        if goals.is_empty() {
            return None;
        }
        let mut s = Search::new(self.xs.len() * self.ys.len() * 4, goals.len());
        for t in ts {
            if let Some(k) = self.find(t.stub(c)) {
                s.push(k * 4 + outward(t.side), t.cost, self.h(k, &goals, bend));
            }
        }
        let n_states = self.xs.len() * self.ys.len() * 4;
        while let Some(e) = s.heap.pop() {
            if e.state >= n_states {
                let gi = e.state - n_states;
                return Some(self.trace(&s, s.goal_prev[gi], ts, goals[gi].2, c));
            }
            if e.g > s.dist[e.state] {
                continue;
            }
            self.expand(&mut s, e, &goals, bend, n_states);
        }
        None
    }

    fn expand(&self, s: &mut Search, e: Entry, goals: &[(usize, usize, &Term)], bend: f64, n_states: usize) {
        let (k, d) = (e.state / 4, e.state % 4);
        for (gi, &(gk, gd, t)) in goals.iter().enumerate() {
            if gk == k && (s.moved[e.state] || gd == d) {
                let turns = if gd == d {
                    0.0
                } else if gd == (d + 2) % 4 {
                    2.0
                } else {
                    1.0
                };
                let total = e.g + turns * bend + t.cost;
                if total < s.goal_best[gi] {
                    s.goal_best[gi] = total;
                    s.goal_prev[gi] = e.state;
                    s.push_raw(n_states + gi, total, total);
                }
            }
        }
        let here = self.point(k);
        for nd in 0..4 {
            if nd == (d + 2) % 4 {
                continue;
            }
            let Some(to) = self.next(k, nd) else { continue };
            let p = self.point(to);
            let len = (p.x - here.x).abs() + (p.y - here.y).abs();
            let g = e.g + len + if nd == d { 0.0 } else { bend };
            let st = to * 4 + nd;
            if g < s.dist[st] {
                s.prev[st] = e.state;
                s.moved[st] = true;
                s.push(st, g, self.h(to, goals, bend));
            }
        }
    }

    fn h(&self, k: usize, goals: &[(usize, usize, &Term)], bend: f64) -> f64 {
        let p = self.point(k);
        goals
            .iter()
            .map(|&(gk, _, t)| {
                let q = self.point(gk);
                let (dx, dy) = ((p.x - q.x).abs(), (p.y - q.y).abs());
                let turns = if dx > EPS && dy > EPS { bend } else { 0.0 };
                dx + dy + turns + t.cost
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn trace(&self, s: &Search, last: usize, ts: &[Term], goal: &Term, c: f64) -> Vec<Point> {
        let mut states = vec![last];
        let mut cur = last;
        while s.prev[cur] != usize::MAX && states.len() <= s.prev.len() {
            cur = s.prev[cur];
            states.push(cur);
        }
        states.reverse();
        let start = self.point(cur / 4);
        let mut pts = Vec::with_capacity(states.len() + 4);
        if let Some(t) = ts.iter().find(|t| {
            let st = t.stub(c);
            (st.x - start.x).abs() <= EPS && (st.y - start.y).abs() <= EPS && outward(t.side) == cur % 4
        }) {
            pts.push(t.at);
        }
        pts.extend(states.iter().map(|&st| self.point(st / 4)));
        pts.push(goal.at);
        simplify(&mut pts);
        pts
    }
}

#[derive(Clone, Copy)]
struct Entry {
    f: f64,
    g: f64,
    seq: u64,
    state: usize,
}

impl PartialEq for Entry {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry {
    /// Reversed, so the max-heap pops the lowest `f` first, then the earliest pushed.
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(o.seq.cmp(&self.seq))
    }
}

struct Search {
    dist: Vec<f64>,
    prev: Vec<usize>,
    moved: Vec<bool>,
    heap: BinaryHeap<Entry>,
    seq: u64,
    goal_best: Vec<f64>,
    goal_prev: Vec<usize>,
}

impl Search {
    fn new(states: usize, goals: usize) -> Self {
        Self {
            dist: vec![f64::INFINITY; states],
            prev: vec![usize::MAX; states],
            moved: vec![false; states],
            heap: BinaryHeap::new(),
            seq: 0,
            goal_best: vec![f64::INFINITY; goals],
            goal_prev: vec![usize::MAX; goals],
        }
    }
    fn push(&mut self, st: usize, g: f64, h: f64) {
        if g < self.dist[st] {
            self.dist[st] = g;
            self.push_raw(st, g, g + h);
        }
    }
    fn push_raw(&mut self, st: usize, g: f64, f: f64) {
        self.seq += 1;
        self.heap.push(Entry { f, g, seq: self.seq, state: st });
    }
}

/// Ends of connectors attached to the same side of the same shape without a fixed point are
/// spread evenly along that side, in the order their routes head off, and routed again.
fn spread_shared_sides(connectors: &[Connector], obstacles: &[Rect], opts: &RouteOptions, routes: &mut [Option<Routed>]) {
    type Key = ([u64; 4], Side);
    let mut groups: BTreeMap<Key, Vec<(usize, usize, f64)>> = BTreeMap::new();
    for (i, (c, r)) in connectors.iter().zip(routes.iter()).enumerate() {
        let Some(r) = r else { continue };
        for (end, port) in [(0, &c.from), (1, &c.to)] {
            if port.point.is_some() {
                continue;
            }
            let Some((side, along)) = end_side(&r.pts, end) else { continue };
            let k = port.rect;
            groups.entry(([k.x.to_bits(), k.y.to_bits(), k.width.to_bits(), k.height.to_bits()], side)).or_default().push((i, end, along));
        }
    }
    let mut fixed: Vec<[Option<Port>; 2]> = vec![[None, None]; connectors.len()];
    for ((_, side), mut list) in groups {
        if list.len() < 2 {
            continue;
        }
        list.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        let k = list.len() as f64;
        for (n, &(i, end, _)) in list.iter().enumerate() {
            let base = if end == 0 { connectors[i].from } else { connectors[i].to };
            let at = side_point(&base.rect, side, (n as f64 + 1.0) / (k + 1.0));
            fixed[i][end] = Some(Port { rect: base.rect, side: Some(side), point: Some(at) });
        }
    }
    for (i, f) in fixed.iter().enumerate() {
        if f[0].is_none() && f[1].is_none() {
            continue;
        }
        let from = f[0].unwrap_or(connectors[i].from);
        let to = f[1].unwrap_or(connectors[i].to);
        if let Some(r) = route_one(&from, &to, obstacles, opts) {
            routes[i] = Some(r);
        }
    }
}

/// The side an end of `pts` is attached to and where along that side's axis the route heads.
fn end_side(pts: &[Point], end: usize) -> Option<(Side, f64)> {
    let (a, b, c) = if end == 0 {
        (*pts.first()?, *pts.get(1)?, pts.get(2).copied())
    } else {
        let n = pts.len();
        (*pts.last()?, *pts.get(n.checked_sub(2)?)?, n.checked_sub(3).and_then(|i| pts.get(i).copied()))
    };
    let side = if (a.x - b.x).abs() <= EPS {
        if b.y < a.y { Side::Top } else { Side::Bottom }
    } else if b.x < a.x {
        Side::Left
    } else {
        Side::Right
    };
    let towards = c.unwrap_or(b);
    let along = match side {
        Side::Top | Side::Bottom => towards.x,
        Side::Left | Side::Right => towards.y,
    };
    Some((side, along))
}

/// One movable segment of a path, for nudging.
struct NSeg {
    path: usize,
    seg: usize,
    at: f64,
    lo: f64,
    hi: f64,
    fixed: bool,
}

fn collect_segs(paths: &[Option<Vec<Point>>], horizontal: bool) -> Vec<NSeg> {
    let mut out = Vec::new();
    for (pi, p) in paths.iter().enumerate() {
        let Some(pts) = p else { continue };
        let last = pts.len().saturating_sub(2);
        for (k, w) in pts.windows(2).enumerate() {
            let is_h = (w[0].y - w[1].y).abs() <= EPS && (w[0].x - w[1].x).abs() > EPS;
            let is_v = (w[0].x - w[1].x).abs() <= EPS && (w[0].y - w[1].y).abs() > EPS;
            if (horizontal && is_h) || (!horizontal && is_v) {
                let (at, a, b) = if horizontal { (w[0].y, w[0].x, w[1].x) } else { (w[0].x, w[0].y, w[1].y) };
                out.push(NSeg { path: pi, seg: k, at, lo: a.min(b), hi: a.max(b), fixed: k == 0 || k == last });
            }
        }
    }
    out
}

/// Groups of segments on the same line whose spans overlap, transitively.
fn clusters(segs: &[NSeg]) -> Vec<Vec<usize>> {
    let mut idx: Vec<usize> = (0..segs.len()).collect();
    idx.sort_by(|&a, &b| segs[a].at.total_cmp(&segs[b].at).then(segs[a].lo.total_cmp(&segs[b].lo)).then(a.cmp(&b)));
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for i in idx {
        let s = &segs[i];
        let same = cur.first().is_some_and(|&f| (segs[f].at - s.at).abs() <= EPS) && s.lo < reach - EPS;
        if !same {
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            reach = f64::NEG_INFINITY;
        }
        reach = reach.max(s.hi);
        cur.push(i);
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Turn sign of the path at the end of segment `seg` that is point `end_pt`, seen from a channel
/// along `horizontal`.
fn turn(pts: &[Point], end_pt: usize, other: Option<usize>, horizontal: bool) -> i8 {
    let Some(o) = other.and_then(|i| pts.get(i)) else { return 0 };
    let Some(p) = pts.get(end_pt) else { return 0 };
    let d = if horizontal { o.y - p.y } else { o.x - p.x };
    if d > EPS {
        1
    } else if d < -EPS {
        -1
    } else {
        0
    }
}

/// Free room on either side of a channel line over `lo..hi`.
fn room(obstacles: &[Rect], at: f64, lo: f64, hi: f64, horizontal: bool) -> (f64, f64) {
    let (mut below, mut above) = (f64::INFINITY, f64::INFINITY);
    for r in obstacles {
        let (a0, a1, c0, c1) = if horizontal { (r.x, r.right(), r.y, r.bottom()) } else { (r.y, r.bottom(), r.x, r.right()) };
        if a1 <= lo + EPS || a0 >= hi - EPS {
            continue;
        }
        if c1 <= at + EPS {
            below = below.min(at - c1);
        } else if c0 >= at - EPS {
            above = above.min(c0 - at);
        }
    }
    (below, above)
}

fn nudge(paths: &mut [Option<Vec<Point>>], obstacles: &[Rect], sep: f64, horizontal: bool) {
    let segs = collect_segs(paths, horizontal);
    for cl in clusters(&segs) {
        let fixed: Vec<usize> = cl.iter().copied().filter(|&i| segs[i].fixed).collect();
        if fixed.len() > 1 {
            continue;
        }
        let chans: Vec<ChannelSeg> = cl
            .iter()
            .map(|&i| {
                let s = &segs[i];
                let pts = paths[s.path].as_deref().unwrap_or(&[]);
                let (a, b) = (s.seg, s.seg + 1);
                let coord = |p: Option<&Point>| p.map_or(0.0, |p| if horizontal { p.x } else { p.y });
                let ta = turn(pts, a, a.checked_sub(1), horizontal);
                let tb = turn(pts, b, Some(b + 1), horizontal);
                ChannelSeg::new((coord(pts.get(a)), ta), (coord(pts.get(b)), tb))
            })
            .collect();
        let (tracks, k) = assign_tracks(&chans, EPS);
        if k < 2 {
            continue;
        }
        let centre = fixed.first().and_then(|f| cl.iter().position(|i| i == f)).map_or((k as f64 - 1.0) / 2.0, |p| tracks[p] as f64);
        let at = segs[cl[0]].at;
        let lo = cl.iter().map(|&i| segs[i].lo).fold(f64::INFINITY, f64::min);
        let hi = cl.iter().map(|&i| segs[i].hi).fold(f64::NEG_INFINITY, f64::max);
        let (below, above) = room(obstacles, at, lo, hi, horizontal);
        let s = sep.min(below / (centre + 1.0)).min(above / (k as f64 - centre));
        if s <= EPS {
            continue;
        }
        for (&i, &t) in cl.iter().zip(&tracks) {
            let sg = &segs[i];
            if sg.fixed {
                continue;
            }
            let v = at + (t as f64 - centre) * s;
            if let Some(pts) = paths[sg.path].as_mut() {
                for q in [sg.seg, sg.seg + 1] {
                    if let Some(p) = pts.get_mut(q) {
                        if horizontal { p.y = v } else { p.x = v }
                    }
                }
            }
        }
    }
}

/// Spreads the movable segments of each channel across it (Wybrow, Marriott and Stuckey 2009,
/// section 5): segments that run side by side between the same obstacles are ordered so they do
/// not cross needlessly, then spaced evenly over the free width, or centred `ideal` apart when the
/// channel is wide; a channel open on one side is filled outwards from its obstacle at
/// `clearance`. Segments that would reverse a neighbouring segment are left alone.
fn centre_channels(paths: &mut [Option<Vec<Point>>], obstacles: &[Rect], opts: &RouteOptions, horizontal: bool) {
    let segs: Vec<NSeg> = collect_segs(paths, horizontal).into_iter().filter(|s| !s.fixed).collect();
    let n = segs.len();
    if n == 0 {
        return;
    }
    let bounds: Vec<(f64, f64)> = segs
        .iter()
        .map(|s| {
            let (below, above) = room(obstacles, s.at, s.lo, s.hi, horizontal);
            (s.at - below, s.at + above)
        })
        .collect();
    let ideal = (3.0 * opts.separation).max(opts.clearance);
    for g in channel_groups(&segs, &bounds) {
        let b0 = g.iter().map(|&i| bounds[i].0).fold(f64::NEG_INFINITY, f64::max);
        let b1 = g.iter().map(|&i| bounds[i].1).fold(f64::INFINITY, f64::min);
        if g.iter().any(|&i| segs[i].at < b0 - EPS || segs[i].at > b1 + EPS) {
            continue;
        }
        let chans: Vec<ChannelSeg> = g.iter().map(|&i| channel_seg(paths, &segs[i], horizontal)).collect();
        let (tracks, k) = assign_tracks(&chans, EPS);
        let mean = g.iter().map(|&i| segs[i].at).sum::<f64>() / g.len() as f64;
        let Some(targets) = track_places(&tracks, k, (b0, b1), mean, ideal, opts) else { continue };
        if g.iter().zip(&targets).any(|(&i, &v)| !keeps_neighbours(paths, &segs[i], v, opts, horizontal)) {
            continue;
        }
        for (&i, &v) in g.iter().zip(&targets) {
            move_seg(paths, &segs[i], v, horizontal);
        }
    }
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Groups of segments whose spans overlap and that can reach each other's line without passing an
/// obstacle: the segments sharing one channel.
fn channel_groups(segs: &[NSeg], bounds: &[(f64, f64)]) -> Vec<Vec<usize>> {
    let n = segs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| segs[a].lo.total_cmp(&segs[b].lo).then(a.cmp(&b)));
    for (x, &i) in order.iter().enumerate() {
        for &j in &order[x + 1..] {
            if segs[j].lo >= segs[i].hi - EPS {
                break;
            }
            let (b0, b1) = (bounds[i].0.max(bounds[j].0), bounds[i].1.min(bounds[j].1));
            if b0 <= segs[i].at.min(segs[j].at) + EPS && b1 >= segs[i].at.max(segs[j].at) - EPS {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri.max(rj)] = ri.min(rj);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().collect()
}

fn channel_seg(paths: &[Option<Vec<Point>>], s: &NSeg, horizontal: bool) -> ChannelSeg {
    let pts = paths[s.path].as_deref().unwrap_or(&[]);
    let (a, b) = (s.seg, s.seg + 1);
    let coord = |p: Option<&Point>| p.map_or(0.0, |p| if horizontal { p.x } else { p.y });
    ChannelSeg::new((coord(pts.get(a)), turn(pts, a, a.checked_sub(1), horizontal)), (coord(pts.get(b)), turn(pts, b, Some(b + 1), horizontal)))
}

/// Where each of `k` tracks goes in the free interval `b0..b1` (either end may be unbounded).
fn track_places(tracks: &[usize], k: usize, (b0, b1): (f64, f64), mean: f64, ideal: f64, opts: &RouteOptions) -> Option<Vec<f64>> {
    let kf = k as f64;
    let centred = |mid: f64, t: usize| mid + (t as f64 - (kf - 1.0) / 2.0) * ideal;
    let place = |t: usize| -> Option<f64> {
        match (b0.is_finite(), b1.is_finite()) {
            (true, true) if b1 - b0 >= 2.0 * opts.clearance + (kf - 1.0) * ideal => Some(centred((b0 + b1) / 2.0, t)),
            (true, true) => {
                let s = (b1 - b0) / (kf + 1.0);
                (s >= opts.separation.max(EPS)).then_some(b0 + (t as f64 + 1.0) * s)
            }
            (true, false) => Some(b0 + opts.clearance + t as f64 * ideal),
            (false, true) => Some(b1 - opts.clearance - (kf - 1.0 - t as f64) * ideal),
            (false, false) => Some(centred(mean, t)),
        }
    };
    tracks.iter().map(|&t| place(t)).collect()
}

fn move_seg(paths: &mut [Option<Vec<Point>>], s: &NSeg, v: f64, horizontal: bool) {
    if let Some(pts) = paths[s.path].as_mut() {
        for q in [s.seg, s.seg + 1] {
            if let Some(p) = pts.get_mut(q) {
                if horizontal { p.y = v } else { p.x = v }
            }
        }
    }
}

/// Whether moving segment `s` to `v` keeps both perpendicular neighbours pointing the way they
/// did, and the end stubs at least `clearance` long (or as long as they were).
fn keeps_neighbours(paths: &[Option<Vec<Point>>], s: &NSeg, v: f64, opts: &RouteOptions, horizontal: bool) -> bool {
    let Some(pts) = paths[s.path].as_deref() else { return false };
    let c = |p: &Point| if horizontal { p.y } else { p.x };
    let last = pts.len().saturating_sub(1);
    [(s.seg.checked_sub(1), s.seg), (Some(s.seg + 2), s.seg + 1)].into_iter().all(|(far, near)| {
        let (Some(far), Some(near)) = (far.and_then(|f| pts.get(f).map(|p| (f, p))), pts.get(near)) else { return true };
        let (fi, fp) = far;
        let was = c(near) - c(fp);
        let stub = fi == 0 || fi == last;
        let need = if stub { was.abs().min(opts.clearance) } else { was.abs().min(opts.separation) };
        (v - c(fp)) * was.signum() >= need - EPS
    })
}
