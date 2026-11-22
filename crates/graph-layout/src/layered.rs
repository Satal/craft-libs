//! The layered layout pipeline: validation, components, ranking, the proper graph, ordering,
//! coordinates, edge ports and tracks, groups, packing and the direction transform.
//!
//! Everything is computed with ranks advancing down the y axis ("internal" coordinates: x is
//! across the ranks, y along them) and mapped to the requested direction at the end.

use crate::channel::{ChannelSeg, assign_tracks};
use crate::proper::{Item, Kind, Proper};
use crate::rank::RankEdge;
use crate::{
    Direction, Graph, LayeredOptions, Layout, LayoutError, MAX_EDGES, MAX_GROUP_DEPTH, MAX_GROUPS, MAX_LAYOUT_ITEMS, MAX_MIN_LEN, MAX_NODES, Point,
    Rect, Size, valid_len,
};

const EPS: f64 = 1e-6;
/// Least share of a node side that edge ports may use.
const PORT_SPAN: f64 = 0.5;
/// Distance kept between edge ports and node corners, where the side is long enough.
const PORT_INSET: f64 = 6.0;
/// Distance between an edge and its label.
const LABEL_GAP: f64 = 4.0;

/// Lays out `graph` in ranks along `opts.direction`. See the crate documentation for the method.
///
/// # Errors
/// [`LayoutError`] when the graph is above a size cap or holds invalid numbers or indices.
pub fn layered(graph: &Graph, opts: &LayeredOptions) -> Result<Layout, LayoutError> {
    validate(graph, opts)?;
    let ctx = Ctx::new(graph, opts);
    let comps = ctx.components();
    let mut outs = Vec::with_capacity(comps.len());
    let mut node_item = vec![usize::MAX; ctx.nodes.len()];
    let mut local = vec![usize::MAX; ctx.nodes.len()];
    let mut total_items = 0usize;
    for (nodes, edges) in &comps {
        let out = ctx.layout_component(nodes, edges, &mut node_item, &mut local, &mut total_items)?;
        outs.push(out);
    }
    Ok(ctx.assemble(outs))
}

fn validate(g: &Graph, opts: &LayeredOptions) -> Result<(), LayoutError> {
    let too = |what, count, limit| if count > limit { Err(LayoutError::TooLarge { what, count, limit }) } else { Ok(()) };
    too("nodes", g.nodes.len(), MAX_NODES)?;
    too("edges", g.edges.len(), MAX_EDGES)?;
    too("groups", g.groups.len(), MAX_GROUPS)?;
    for (i, v) in [opts.node_sep, opts.rank_sep, opts.edge_sep, opts.component_sep].into_iter().enumerate() {
        if !valid_len(v) {
            return Err(LayoutError::InvalidNumber { what: "spacing option", index: i });
        }
    }
    for (i, n) in g.nodes.iter().enumerate() {
        if !valid_len(n.size.width) || !valid_len(n.size.height) {
            return Err(LayoutError::InvalidNumber { what: "node size", index: i });
        }
        if n.group.is_some_and(|gi| gi >= g.groups.len()) {
            return Err(LayoutError::DanglingIndex { what: "node group", index: i });
        }
    }
    for (i, e) in g.edges.iter().enumerate() {
        if e.from >= g.nodes.len() || e.to >= g.nodes.len() {
            return Err(LayoutError::DanglingIndex { what: "edge end", index: i });
        }
        if e.label.is_some_and(|l| !valid_len(l.width) || !valid_len(l.height)) {
            return Err(LayoutError::InvalidNumber { what: "edge label size", index: i });
        }
    }
    for (i, gr) in g.groups.iter().enumerate() {
        if !valid_len(gr.padding) || !valid_len(gr.title_height) {
            return Err(LayoutError::InvalidNumber { what: "group padding", index: i });
        }
        if gr.parent.is_some_and(|p| p >= g.groups.len()) {
            return Err(LayoutError::DanglingIndex { what: "group parent", index: i });
        }
    }
    for i in 0..g.groups.len() {
        let mut depth = 1;
        let mut cur = g.groups[i].parent;
        while let Some(p) = cur {
            depth += 1;
            if depth > g.groups.len() + 1 {
                return Err(LayoutError::GroupCycle { group: i });
            }
            cur = g.groups.get(p).and_then(|x| x.parent);
        }
        too("group nesting levels", depth, MAX_GROUP_DEPTH)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct NodeInfo {
    cross: f64,
    along: f64,
    group: Option<usize>,
}

/// Graph data prepared in internal coordinates.
struct Ctx<'a> {
    g: &'a Graph,
    opts: &'a LayeredOptions,
    /// Input nodes followed by one placeholder per empty group.
    nodes: Vec<NodeInfo>,
    /// Internal padding per group: top, bottom, left, right.
    pads: Vec<[f64; 4]>,
    parent: Vec<Option<usize>>,
    depth: Vec<usize>,
    children: Vec<Vec<usize>>,
    /// Groups ordered deepest first.
    deep_first: Vec<usize>,
    has_labels: bool,
    swap: bool,
}

/// One component's result in internal coordinates, shifted to start at `(0, 0)`.
struct CompOut {
    nodes: Vec<(usize, Rect)>,
    groups: Vec<(usize, Rect)>,
    edges: Vec<(usize, Vec<Point>)>,
    labels: Vec<(usize, Point)>,
    size: Size,
}

/// An edge after cycle removal: its items from the upper end to the lower.
struct Chain {
    edge: usize,
    reversed: bool,
    items: Vec<usize>,
    out_port: f64,
    in_port: f64,
    /// Track y per segment index, for segments that jog.
    jogs: Vec<Option<f64>>,
}

impl<'a> Ctx<'a> {
    fn new(g: &'a Graph, opts: &'a LayeredOptions) -> Self {
        let swap = matches!(opts.direction, Direction::Right | Direction::Left);
        let gl = g.groups.len();
        let parent: Vec<Option<usize>> = g.groups.iter().map(|x| x.parent).collect();
        let mut depth = vec![1usize; gl];
        for (i, d) in depth.iter_mut().enumerate() {
            let mut cur = parent[i];
            while let Some(p) = cur {
                *d += 1;
                cur = parent.get(p).copied().flatten();
                if *d > gl {
                    break;
                }
            }
        }
        let mut children = vec![Vec::new(); gl];
        for (i, p) in parent.iter().enumerate() {
            if let Some(p) = *p {
                children[p].push(i);
            }
        }
        let mut deep_first: Vec<usize> = (0..gl).collect();
        deep_first.sort_by_key(|&i| (std::cmp::Reverse(depth[i]), i));
        let pads = g
            .groups
            .iter()
            .map(|gr| {
                let (p, t) = (gr.padding, gr.title_height);
                // The title band sits on the output top edge, whatever the direction.
                match opts.direction {
                    Direction::Down => [p + t, p, p, p],
                    Direction::Up => [p, p + t, p, p],
                    Direction::Right | Direction::Left => [p, p, p + t, p],
                }
            })
            .collect();
        let mut nodes: Vec<NodeInfo> = g
            .nodes
            .iter()
            .map(|n| {
                let (cross, along) = if swap { (n.size.height, n.size.width) } else { (n.size.width, n.size.height) };
                NodeInfo { cross, along, group: n.group }
            })
            .collect();
        let mut has_member = vec![false; gl];
        let mark = |has: &mut Vec<bool>, mut cur: Option<usize>| {
            let mut steps = 0;
            while let Some(c) = cur {
                if has[c] || steps > gl {
                    break;
                }
                has[c] = true;
                cur = parent[c];
                steps += 1;
            }
        };
        for n in &nodes {
            mark(&mut has_member, n.group);
        }
        for &gi in &deep_first {
            if !has_member[gi] {
                let s = opts.node_sep.max(EPS);
                nodes.push(NodeInfo { cross: s, along: s / 2.0, group: Some(gi) });
                mark(&mut has_member, Some(gi));
            }
        }
        let has_labels = g.edges.iter().any(|e| e.label.is_some() && e.from != e.to);
        Self { g, opts, nodes, pads, parent, depth, children, deep_first, has_labels, swap }
    }

    fn top_group(&self, mut g: usize) -> usize {
        let mut steps = 0;
        while let Some(p) = self.parent[g] {
            g = p;
            steps += 1;
            if steps > self.parent.len() {
                break;
            }
        }
        g
    }

    /// Node lists and edge lists of the layout components: nodes joined by edges or by sharing a
    /// top-level group, in order of their first node.
    fn components(&self) -> Vec<(Vec<usize>, Vec<usize>)> {
        let n = self.nodes.len();
        let mut uf: Vec<usize> = (0..n).collect();
        fn find(uf: &mut [usize], mut v: usize) -> usize {
            while uf[v] != v {
                uf[v] = uf[uf[v]];
                v = uf[v];
            }
            v
        }
        let union = |uf: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (find(uf, a), find(uf, b));
            if ra != rb {
                let (lo, hi) = (ra.min(rb), ra.max(rb));
                uf[hi] = lo;
            }
        };
        for e in &self.g.edges {
            union(&mut uf, e.from, e.to);
        }
        let mut first_in_top = vec![usize::MAX; self.parent.len()];
        for v in 0..n {
            if let Some(gi) = self.nodes[v].group {
                let t = self.top_group(gi);
                if first_in_top[t] == usize::MAX {
                    first_in_top[t] = v;
                } else {
                    union(&mut uf, first_in_top[t], v);
                }
            }
        }
        let mut slot = vec![usize::MAX; n];
        let mut comps: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
        for v in 0..n {
            let r = find(&mut uf, v);
            if slot[r] == usize::MAX {
                slot[r] = comps.len();
                comps.push((Vec::new(), Vec::new()));
            }
            comps[slot[r]].0.push(v);
        }
        for (i, e) in self.g.edges.iter().enumerate() {
            let r = find(&mut uf, e.from);
            if let Some(c) = comps.get_mut(slot[r]) {
                c.1.push(i);
            }
        }
        comps
    }

    fn min_len(&self, e: usize) -> i64 {
        let m = self.g.edges.get(e).map_or(1, |x| x.min_len.clamp(1, MAX_MIN_LEN));
        i64::from(m) * if self.has_labels { 2 } else { 1 }
    }

    fn label_dims(&self, e: usize) -> Option<(f64, f64)> {
        let l = self.g.edges.get(e)?.label?;
        Some(if self.swap { (l.height, l.width) } else { (l.width, l.height) })
    }

    fn layout_component(
        &self,
        nodes: &[usize],
        edges: &[usize],
        node_item: &mut [usize],
        local: &mut [usize],
        total_items: &mut usize,
    ) -> Result<CompOut, LayoutError> {
        for (i, &v) in nodes.iter().enumerate() {
            local[v] = i;
        }
        let mut loops: Vec<(usize, Vec<usize>)> = Vec::new();
        let mut plain: Vec<usize> = Vec::new();
        for &e in edges {
            let ed = &self.g.edges[e];
            if ed.from == ed.to {
                match loops.iter_mut().find(|l| l.0 == ed.from) {
                    Some(l) => l.1.push(e),
                    None => loops.push((ed.from, vec![e])),
                }
            } else {
                plain.push(e);
            }
        }
        let pairs: Vec<(usize, usize)> = plain.iter().map(|&e| (local[self.g.edges[e].from], local[self.g.edges[e].to])).collect();
        let reversed = crate::acyclic::feedback_edges(nodes.len(), &pairs);
        let rank_edges: Vec<RankEdge> = pairs
            .iter()
            .zip(&reversed)
            .zip(&plain)
            .map(|((&(a, b), &rev), &e)| {
                let (from, to) = if rev { (b, a) } else { (a, b) };
                RankEdge { from, to, min_len: self.min_len(e), weight: 1 }
            })
            .collect();
        let local_ranks = crate::rank::rank(nodes.len(), &rank_edges);
        let rank_of = |v: usize| usize::try_from(local_ranks.get(local[v]).copied().unwrap_or(0)).unwrap_or(0);

        let span = self.group_spans(nodes, &rank_of);
        let mut count = nodes.len();
        for (&e, &rev) in plain.iter().zip(&reversed) {
            let ed = &self.g.edges[e];
            let (a, b) = if rev { (ed.to, ed.from) } else { (ed.from, ed.to) };
            count += rank_of(b).saturating_sub(rank_of(a)).saturating_sub(1);
        }
        for s in span.iter().flatten() {
            count += 2 * (s.1 - s.0 + 1);
        }
        *total_items += count;
        if *total_items > MAX_LAYOUT_ITEMS {
            return Err(LayoutError::TooLarge { what: "layout nodes", count: *total_items, limit: MAX_LAYOUT_ITEMS });
        }

        let mut p = Proper { group_parent: self.parent.clone(), ..Proper::default() };
        let loop_extra = |v: usize| self.loop_extra(&loops, v);
        for &v in nodes {
            let ni = self.nodes[v];
            node_item[v] = p.push(Item {
                kind: Kind::Node(v),
                rank: rank_of(v),
                owner: ni.group,
                left_ext: ni.cross / 2.0,
                right_ext: ni.cross / 2.0 + loop_extra(v),
                along: ni.along,
            });
        }
        let mut chains = Vec::with_capacity(plain.len());
        for (&e, &rev) in plain.iter().zip(&reversed) {
            let ed = &self.g.edges[e];
            let (a, b) = if rev { (ed.to, ed.from) } else { (ed.from, ed.to) };
            let (ra, rb) = (rank_of(a), rank_of(b));
            let mut items = vec![node_item[a]];
            for k in ra + 1..rb {
                let owner = self.dummy_owner(a, b, k, &span);
                let label = if k == ra + (rb - ra) / 2 { self.label_dims(e) } else { None };
                let item = match label {
                    Some((c, al)) => Item { kind: Kind::Label(e), rank: k, owner, left_ext: c / 2.0, right_ext: c / 2.0, along: al },
                    None => Item { kind: Kind::Dummy, rank: k, owner, left_ext: 0.0, right_ext: 0.0, along: 0.0 },
                };
                let id = p.push(item);
                if let Some(&prev) = items.last() {
                    p.link(prev, id);
                }
                items.push(id);
            }
            if let Some(&prev) = items.last() {
                p.link(prev, node_item[b]);
            }
            items.push(node_item[b]);
            let segs = items.len() - 1;
            chains.push(Chain { edge: e, reversed: rev, items, out_port: 0.0, in_port: 0.0, jogs: vec![None; segs] });
        }
        let mut borders: Vec<(Vec<usize>, Vec<usize>)> = vec![(Vec::new(), Vec::new()); self.parent.len()];
        for (gi, s) in span.iter().enumerate() {
            let Some((lo, hi)) = *s else { continue };
            for r in lo..=hi {
                let l = p.push(Item { kind: Kind::BorderL(gi), rank: r, owner: Some(gi), left_ext: 0.0, right_ext: 0.0, along: 0.0 });
                let rr = p.push(Item { kind: Kind::BorderR(gi), rank: r, owner: Some(gi), left_ext: 0.0, right_ext: 0.0, along: 0.0 });
                if let (Some(&pl), Some(&pr)) = (borders[gi].0.last(), borders[gi].1.last()) {
                    p.link(pl, l);
                    p.link(pr, rr);
                }
                borders[gi].0.push(l);
                borders[gi].1.push(rr);
            }
        }

        crate::order::order(&mut p);
        let sep = |u: usize, v: usize| self.sep(&p, u, v);
        let x = crate::position::assign(&p, &sep);
        self.ports(&p, &x, &mut chains);
        let tracks = self.tracks(&p, &x, &mut chains);
        let geo = self.rank_geometry(&p, &span, &tracks);
        for c in &mut chains {
            for (k, j) in c.jogs.iter_mut().enumerate() {
                if let Some(t) = j {
                    let r = p.items[c.items[k]].rank;
                    *j = Some(geo.track_y(r, *t));
                }
            }
        }
        let placed = Placed { p, x, geo, chains, loops, borders, span };
        Ok(self.emit_component(&placed))
    }

    fn loop_extra(&self, loops: &[(usize, Vec<usize>)], v: usize) -> f64 {
        let Some((_, l)) = loops.iter().find(|l| l.0 == v) else { return 0.0 };
        let label = l.iter().filter_map(|&e| self.label_dims(e)).map(|d| d.0 + LABEL_GAP).fold(0.0, f64::max);
        self.loop_base() + (l.len() - 1) as f64 * self.opts.edge_sep + label
    }

    fn loop_base(&self) -> f64 {
        (self.opts.edge_sep * 1.5).max(8.0)
    }

    /// First and last rank of each group over the given nodes.
    fn group_spans(&self, nodes: &[usize], rank_of: &dyn Fn(usize) -> usize) -> Vec<Option<(usize, usize)>> {
        let mut span: Vec<Option<(usize, usize)>> = vec![None; self.parent.len()];
        for &v in nodes {
            let r = rank_of(v);
            let mut cur = self.nodes[v].group;
            let mut steps = 0;
            while let Some(gi) = cur {
                span[gi] = Some(span[gi].map_or((r, r), |(a, b)| (a.min(r), b.max(r))));
                cur = self.parent[gi];
                steps += 1;
                if steps > self.parent.len() {
                    break;
                }
            }
        }
        span
    }

    /// The group a dummy of the edge `a -> b` on rank `k` belongs to: the deepest group that
    /// holds `a` and reaches down to `k`, or holds `b` and reaches up to `k`.
    fn dummy_owner(&self, a: usize, b: usize, k: usize, span: &[Option<(usize, usize)>]) -> Option<usize> {
        let pick = |start: Option<usize>, fits: &dyn Fn((usize, usize)) -> bool| {
            let mut cur = start;
            let mut steps = 0;
            while let Some(gi) = cur {
                if span[gi].is_some_and(fits) {
                    return Some(gi);
                }
                cur = self.parent[gi];
                steps += 1;
                if steps > self.parent.len() {
                    break;
                }
            }
            None
        };
        let ga = pick(self.nodes[a].group, &|s| s.1 >= k);
        let gb = pick(self.nodes[b].group, &|s| s.0 <= k);
        match (ga, gb) {
            (Some(x), Some(y)) => Some(if self.depth[y] > self.depth[x] { y } else { x }),
            (x, y) => x.or(y),
        }
    }

    /// Least distance between the centres of `u` and its right neighbour `v`.
    fn sep(&self, p: &Proper, u: usize, v: usize) -> f64 {
        let (iu, iv) = (&p.items[u], &p.items[v]);
        let pad = |g: usize, side: usize| self.pads.get(g).map_or(0.0, |x| x[side]);
        let gap = match (iu.kind, iv.kind) {
            (Kind::BorderL(g), Kind::BorderR(h)) if g == h => pad(g, 2) + pad(g, 3),
            (Kind::BorderL(g), _) => pad(g, 2),
            (_, Kind::BorderR(g)) => pad(g, 3),
            (Kind::Dummy, Kind::Dummy) => self.opts.edge_sep,
            (Kind::Dummy, _) | (_, Kind::Dummy) => self.opts.edge_sep.max(self.opts.node_sep / 2.0),
            _ => self.opts.node_sep,
        };
        iu.right_ext + iv.left_ext + gap
    }

    /// Port positions along the node sides, spread apart and kept straight where possible.
    fn ports(&self, p: &Proper, x: &[f64], chains: &mut [Chain]) {
        let n = p.items.len();
        // (chain index, desired, tiebreak) per item, for the bottom (out) and top (in) sides.
        let mut outs: Vec<Vec<(usize, f64, f64)>> = vec![Vec::new(); n];
        let mut ins: Vec<Vec<(usize, f64, f64)>> = vec![Vec::new(); n];
        let range = |v: usize| {
            let it = &p.items[v];
            let half = it.left_ext.min(it.right_ext);
            let half = (half - PORT_INSET).max(half * PORT_SPAN);
            (x[v] - half, x[v] + half)
        };
        let real = |v: usize| matches!(p.items[v].kind, Kind::Node(_));
        let desired = |v: usize, other: usize| {
            let (lo, hi) = range(v);
            if real(other) {
                let (olo, ohi) = range(other);
                let (a, b) = (lo.max(olo), hi.min(ohi));
                if a <= b {
                    return ((x[v] + x[other]) / 2.0).clamp(a, b);
                }
            }
            x[other].clamp(lo, hi)
        };
        for (ci, c) in chains.iter().enumerate() {
            let (Some(&a), Some(&b)) = (c.items.first(), c.items.last()) else { continue };
            let (Some(&a2), Some(&b2)) = (c.items.get(1), c.items.get(c.items.len().saturating_sub(2))) else { continue };
            outs[a].push((ci, desired(a, a2), x[a2]));
            ins[b].push((ci, desired(b, b2), x[b2]));
        }
        for v in 0..n {
            let (lo, hi) = range(v);
            for (side, list) in [(0, &mut outs[v]), (1, &mut ins[v])] {
                if list.is_empty() {
                    continue;
                }
                list.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)).then(a.0.cmp(&b.0)));
                let want: Vec<f64> = list.iter().map(|r| r.1).collect();
                let got = spread(&want, lo, hi, self.opts.edge_sep);
                for (r, px) in list.iter().zip(got) {
                    if let Some(c) = chains.get_mut(r.0) {
                        if side == 0 { c.out_port = px } else { c.in_port = px }
                    }
                }
            }
        }
    }

    /// Marks every segment that jogs and assigns it a track within its gap; returns the track
    /// count per gap. Track numbers are stored in `jogs` until the gap geometry is known.
    fn tracks(&self, p: &Proper, x: &[f64], chains: &mut [Chain]) -> Vec<usize> {
        let gaps = p.layers.len().saturating_sub(1);
        let mut per_gap: Vec<Vec<(usize, usize, ChannelSeg)>> = vec![Vec::new(); gaps];
        for (ci, c) in chains.iter().enumerate() {
            let last = c.items.len() - 1;
            for k in 0..last {
                let sx = if k == 0 { c.out_port } else { x[c.items[k]] };
                let tx = if k + 1 == last { c.in_port } else { x[c.items[k + 1]] };
                if (sx - tx).abs() > EPS {
                    let r = p.items[c.items[k]].rank;
                    if let Some(gl) = per_gap.get_mut(r) {
                        gl.push((ci, k, ChannelSeg::new((sx, -1), (tx, 1))));
                    }
                }
            }
        }
        let mut counts = vec![0usize; gaps];
        for (r, list) in per_gap.iter().enumerate() {
            let segs: Vec<ChannelSeg> = list.iter().map(|s| s.2).collect();
            let (tr, k) = assign_tracks(&segs, self.opts.edge_sep * 0.5 + EPS);
            counts[r] = k;
            for (s, t) in list.iter().zip(tr) {
                if let Some(c) = chains.get_mut(s.0) {
                    c.jogs[s.1] = Some(t as f64);
                }
            }
        }
        counts
    }

    fn rank_geometry(&self, p: &Proper, span: &[Option<(usize, usize)>], tracks: &[usize]) -> RankGeo {
        let h = p.layers.len();
        let mut lh = vec![0.0f64; h];
        for it in &p.items {
            lh[it.rank] = lh[it.rank].max(it.along);
        }
        let (ext_above, ext_below) = self.band_extents(span);
        let mut above = vec![0.0f64; h];
        let mut below = vec![0.0f64; h];
        for (gi, s) in span.iter().enumerate() {
            if let Some((lo, hi)) = *s {
                above[lo] = above[lo].max(ext_above[gi]);
                below[hi] = below[hi].max(ext_below[gi]);
            }
        }
        let rank_sep = if self.has_labels { self.opts.rank_sep / 2.0 } else { self.opts.rank_sep };
        let mut top = vec![0.0f64; h];
        let mut tracks_k = vec![0usize; h];
        for r in 0..h.saturating_sub(1) {
            let k = tracks.get(r).copied().unwrap_or(0);
            tracks_k[r] = k;
            let bands = below[r] + above[r + 1];
            let region_min = if bands > 0.0 { rank_sep * 0.5 } else { 0.0 };
            let region = region_min.max((k as f64 + 1.0) * self.opts.edge_sep);
            let gap = rank_sep.max(bands + region);
            top[r + 1] = top[r] + lh[r] + gap;
        }
        RankGeo { top, lh, above, below, tracks: tracks_k, ext_above, ext_below }
    }

    /// How far each group reaches above its first rank's top and below its last rank's bottom,
    /// counting nested groups that start or end on the same rank.
    fn band_extents(&self, span: &[Option<(usize, usize)>]) -> (Vec<f64>, Vec<f64>) {
        let gl = self.parent.len();
        let mut up = vec![0.0f64; gl];
        let mut down = vec![0.0f64; gl];
        for &gi in &self.deep_first {
            let Some((lo, hi)) = span[gi] else { continue };
            let mut u = 0.0f64;
            let mut d = 0.0f64;
            for &c in &self.children[gi] {
                if let Some((clo, chi)) = span[c] {
                    if clo == lo {
                        u = u.max(up[c]);
                    }
                    if chi == hi {
                        d = d.max(down[c]);
                    }
                }
            }
            up[gi] = self.pads[gi][0] + u;
            down[gi] = self.pads[gi][1] + d;
        }
        (up, down)
    }

    fn emit_component(&self, pl: &Placed) -> CompOut {
        let mut out = CompOut { nodes: Vec::new(), groups: Vec::new(), edges: Vec::new(), labels: Vec::new(), size: Size::default() };
        for (i, it) in pl.p.items.iter().enumerate() {
            match it.kind {
                Kind::Node(v) if v < self.g.nodes.len() => out.nodes.push((v, self.node_rect(pl, i))),
                Kind::Label(e) => {
                    // The edge runs straight through its label's rank, so the label sits on it.
                    out.labels.push((e, Point::new(pl.x[i], pl.geo.centre(it.rank))));
                }
                _ => {}
            }
        }
        for (gi, s) in pl.span.iter().enumerate() {
            let Some((lo, hi)) = *s else { continue };
            let xs = |v: &Vec<usize>| v.iter().map(|&b| pl.x[b]).collect::<Vec<f64>>();
            let left = xs(&pl.borders[gi].0).into_iter().fold(f64::INFINITY, f64::min);
            let right = xs(&pl.borders[gi].1).into_iter().fold(f64::NEG_INFINITY, f64::max);
            let top = pl.geo.top[lo] - pl.geo.ext_above[gi];
            let bottom = pl.geo.bottom(hi) + pl.geo.ext_below[gi];
            if left.is_finite() && right.is_finite() {
                out.groups.push((gi, Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))));
            }
        }
        for c in &pl.chains {
            out.edges.push((c.edge, self.chain_points(pl, c)));
        }
        for (v, es) in &pl.loops {
            self.self_loops(pl, *v, es, &mut out);
        }
        let sizes: Vec<(usize, Size)> = out.labels.iter().map(|&(e, _)| (e, self.label_size(e))).collect();
        normalise(&mut out, &sizes);
        out
    }

    fn node_rect(&self, pl: &Placed, i: usize) -> Rect {
        let it = &pl.p.items[i];
        let cross = match it.kind {
            Kind::Node(v) => self.nodes[v].cross,
            _ => it.left_ext + it.right_ext,
        };
        Rect::new(pl.x[i] - cross / 2.0, pl.geo.centre(it.rank) - it.along / 2.0, cross, it.along)
    }

    fn chain_points(&self, pl: &Placed, c: &Chain) -> Vec<Point> {
        let (Some(&a), Some(&b)) = (c.items.first(), c.items.last()) else { return Vec::new() };
        let ia = &pl.p.items[a];
        let ib = &pl.p.items[b];
        let mut pts = vec![Point::new(c.out_port, pl.geo.centre(ia.rank) + ia.along / 2.0)];
        let last = c.items.len() - 1;
        for k in 0..last {
            let sx = if k == 0 { c.out_port } else { pl.x[c.items[k]] };
            let tx = if k + 1 == last { c.in_port } else { pl.x[c.items[k + 1]] };
            if let Some(y) = c.jogs[k] {
                pts.push(Point::new(sx, y));
                pts.push(Point::new(tx, y));
            }
        }
        pts.push(Point::new(c.in_port, pl.geo.centre(ib.rank) - ib.along / 2.0));
        simplify(&mut pts);
        if c.reversed {
            pts.reverse();
        }
        pts
    }

    fn self_loops(&self, pl: &Placed, v: usize, es: &[usize], out: &mut CompOut) {
        let Some(i) = pl.p.items.iter().position(|it| it.kind == Kind::Node(v)) else { return };
        let r = self.node_rect(pl, i);
        let (xr, cy, h) = (r.right(), r.y + r.height / 2.0, r.height);
        let n = es.len() as f64;
        for (k, &e) in es.iter().enumerate() {
            let kf = k as f64;
            let d = h / 2.0 * (kf + 1.0) / (n + 1.0);
            let len = self.loop_base() + kf * self.opts.edge_sep;
            out.edges.push((e, vec![Point::new(xr, cy - d), Point::new(xr + len, cy - d), Point::new(xr + len, cy + d), Point::new(xr, cy + d)]));
            if let Some((w, _)) = self.label_dims(e) {
                out.labels.push((e, Point::new(xr + len + LABEL_GAP + w / 2.0, cy)));
            }
        }
    }

    /// Packs the components in rows and maps internal coordinates to the requested direction.
    fn assemble(&self, outs: Vec<CompOut>) -> Layout {
        let mut lay = Layout {
            nodes: vec![Rect::default(); self.g.nodes.len()],
            groups: vec![Rect::default(); self.g.groups.len()],
            edges: vec![Vec::new(); self.g.edges.len()],
            labels: vec![None; self.g.edges.len()],
            size: Size::default(),
        };
        let gap = self.opts.component_sep;
        let area: f64 = outs.iter().map(|o| (o.size.width + gap) * (o.size.height + gap)).sum();
        let widest = outs.iter().map(|o| o.size.width).fold(0.0, f64::max);
        let wrap = widest.max(1.5 * area.sqrt());
        let (mut cx, mut cy, mut row_h) = (0.0f64, 0.0f64, 0.0f64);
        for o in outs {
            if cx > 0.0 && cx + o.size.width > wrap {
                cx = 0.0;
                cy += row_h + gap;
                row_h = 0.0;
            }
            let sh = |p: Point| Point::new(p.x + cx, p.y + cy);
            for (v, r) in o.nodes {
                lay.nodes[v] = Rect::new(r.x + cx, r.y + cy, r.width, r.height);
            }
            for (g, r) in o.groups {
                lay.groups[g] = Rect::new(r.x + cx, r.y + cy, r.width, r.height);
            }
            for (e, pts) in o.edges {
                lay.edges[e] = pts.into_iter().map(sh).collect();
            }
            for (e, p) in o.labels {
                lay.labels[e] = Some(sh(p));
            }
            cx += o.size.width + gap;
            row_h = row_h.max(o.size.height);
        }
        self.transform(&mut lay);
        lay
    }

    fn label_size(&self, e: usize) -> Size {
        let (c, a) = self.label_dims(e).unwrap_or((0.0, 0.0));
        Size::new(c, a)
    }

    fn transform(&self, lay: &mut Layout) {
        let dir = self.opts.direction;
        let f = |p: Point| match dir {
            Direction::Down => p,
            Direction::Up => Point::new(p.x, -p.y),
            Direction::Right => Point::new(p.y, p.x),
            Direction::Left => Point::new(-p.y, p.x),
        };
        let fr = |r: Rect| {
            let (a, b) = (f(Point::new(r.x, r.y)), f(Point::new(r.right(), r.bottom())));
            Rect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
        };
        let label_rects: Vec<Option<Rect>> = lay
            .labels
            .iter()
            .enumerate()
            .map(|(e, l)| {
                l.map(|p| {
                    let s = self.label_size(e);
                    fr(Rect::new(p.x - s.width / 2.0, p.y - s.height / 2.0, s.width, s.height))
                })
            })
            .collect();
        for r in lay.nodes.iter_mut().chain(lay.groups.iter_mut()) {
            *r = fr(*r);
        }
        for pts in &mut lay.edges {
            for p in pts.iter_mut() {
                *p = f(*p);
            }
        }
        let mut b = Bounds::default();
        lay.nodes.iter().chain(&lay.groups).chain(label_rects.iter().flatten()).for_each(|r| b.rect(r));
        lay.edges.iter().flatten().for_each(|p| b.point(*p));
        let (dx, dy) = if b.lo.x.is_finite() { (-b.lo.x, -b.lo.y) } else { (0.0, 0.0) };
        for r in lay.nodes.iter_mut().chain(lay.groups.iter_mut()) {
            r.x += dx;
            r.y += dy;
        }
        for p in lay.edges.iter_mut().flatten() {
            p.x += dx;
            p.y += dy;
        }
        for (l, r) in lay.labels.iter_mut().zip(&label_rects) {
            *l = r.map(|r| Point::new(r.x + r.width / 2.0 + dx, r.y + r.height / 2.0 + dy));
        }
        lay.size = if b.lo.x.is_finite() { Size::new(b.hi.x - b.lo.x, b.hi.y - b.lo.y) } else { Size::default() };
    }
}

/// Everything placed for one component, in internal coordinates.
struct Placed {
    p: Proper,
    x: Vec<f64>,
    geo: RankGeo,
    chains: Vec<Chain>,
    loops: Vec<(usize, Vec<usize>)>,
    borders: Vec<(Vec<usize>, Vec<usize>)>,
    span: Vec<Option<(usize, usize)>>,
}

struct Bounds {
    lo: Point,
    hi: Point,
}

impl Default for Bounds {
    fn default() -> Self {
        Self { lo: Point::new(f64::INFINITY, f64::INFINITY), hi: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }
}

impl Bounds {
    fn point(&mut self, p: Point) {
        self.lo = Point::new(self.lo.x.min(p.x), self.lo.y.min(p.y));
        self.hi = Point::new(self.hi.x.max(p.x), self.hi.y.max(p.y));
    }
    fn rect(&mut self, r: &Rect) {
        self.point(Point::new(r.x, r.y));
        self.point(Point::new(r.right(), r.bottom()));
    }
}

/// Shifts a component so its bounding box starts at the origin and records its size.
fn normalise(out: &mut CompOut, label_sizes: &[(usize, Size)]) {
    let mut b = Bounds::default();
    out.nodes.iter().chain(&out.groups).for_each(|(_, r)| b.rect(r));
    out.edges.iter().flat_map(|(_, p)| p).for_each(|p| b.point(*p));
    for ((_, p), (_, s)) in out.labels.iter().zip(label_sizes) {
        b.rect(&Rect::new(p.x - s.width / 2.0, p.y - s.height / 2.0, s.width, s.height));
    }
    if !b.lo.x.is_finite() {
        return;
    }
    let (dx, dy) = (-b.lo.x, -b.lo.y);
    for (_, r) in out.nodes.iter_mut().chain(out.groups.iter_mut()) {
        r.x += dx;
        r.y += dy;
    }
    for p in out.edges.iter_mut().flat_map(|(_, p)| p.iter_mut()).chain(out.labels.iter_mut().map(|(_, p)| p)) {
        p.x += dx;
        p.y += dy;
    }
    out.size = Size::new(b.hi.x - b.lo.x, b.hi.y - b.lo.y);
}

/// Vertical geometry of the ranks of one component.
struct RankGeo {
    top: Vec<f64>,
    lh: Vec<f64>,
    above: Vec<f64>,
    below: Vec<f64>,
    tracks: Vec<usize>,
    ext_above: Vec<f64>,
    ext_below: Vec<f64>,
}

impl RankGeo {
    fn bottom(&self, r: usize) -> f64 {
        self.top[r] + self.lh[r]
    }
    fn centre(&self, r: usize) -> f64 {
        self.top[r] + self.lh[r] / 2.0
    }
    /// The y of track `t` in the gap below rank `r`, between the group bands.
    fn track_y(&self, r: usize, t: f64) -> f64 {
        let a = self.bottom(r) + self.below[r];
        let b = self.top.get(r + 1).map_or(a, |t1| t1 - self.above.get(r + 1).copied().unwrap_or(0.0));
        let k = self.tracks.get(r).copied().unwrap_or(1).max(1) as f64;
        a + (b - a) * (t + 1.0) / (k + 1.0)
    }
}

/// Positions for sorted `want`ed values inside `lo..=hi`, at least `sep` apart (less if they do
/// not fit), as close as possible to the wanted values in the least-squares sense.
fn spread(want: &[f64], lo: f64, hi: f64, sep: f64) -> Vec<f64> {
    let k = want.len();
    if k == 0 {
        return Vec::new();
    }
    let s = if k > 1 { sep.min((hi - lo).max(0.0) / (k - 1) as f64).max(0.0) } else { 0.0 };
    // Pool adjacent violators on want[i] - i*s, which must be non-decreasing.
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(k);
    for (i, &w) in want.iter().enumerate() {
        blocks.push((w - i as f64 * s, 1));
        while blocks.len() > 1 {
            let (Some(&(sb, cb)), Some(&(sa, ca))) = (blocks.last(), blocks.get(blocks.len() - 2)) else { break };
            if sa / ca as f64 <= sb / cb as f64 {
                break;
            }
            blocks.pop();
            blocks.pop();
            blocks.push((sa + sb, ca + cb));
        }
    }
    let mut out = Vec::with_capacity(k);
    for (sum, cnt) in blocks {
        for _ in 0..cnt {
            let i = out.len();
            out.push(sum / cnt as f64 + i as f64 * s);
        }
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = v.max(lo + i as f64 * s);
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = v.min(hi - (k - 1 - i) as f64 * s);
    }
    out
}

/// Removes repeated points and the middle of three collinear points.
pub(crate) fn simplify(pts: &mut Vec<Point>) {
    let mut out: Vec<Point> = Vec::with_capacity(pts.len());
    for &q in pts.iter() {
        if out.last().is_some_and(|l| (l.x - q.x).abs() <= EPS && (l.y - q.y).abs() <= EPS) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let col_x = (a.x - b.x).abs() <= EPS && (b.x - q.x).abs() <= EPS;
            let col_y = (a.y - b.y).abs() <= EPS && (b.y - q.y).abs() <= EPS;
            if col_x || col_y {
                out.pop();
            }
        }
        out.push(q);
    }
    *pts = out;
}
