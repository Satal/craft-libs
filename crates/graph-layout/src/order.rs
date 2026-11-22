//! Crossing reduction: weighted median sweeps and transposition (Gansner et al. 1993, section 3),
//! with each rank sorted hierarchically so every group stays one contiguous block bracketed by its
//! border nodes, and sibling groups in the same left-to-right order on every rank.
//!
//! Back edges take part as reversed edges with dummy nodes, so their chains are ordered like any
//! other edge. As Gansner et al. suggest, alternate iterations flip the order of items with equal
//! medians and let transposition swap pairs with equal crossings, which moves such chains out of
//! local minima (a loop's chain stuck on the far side of a branch). Among orders with the fewest
//! crossings the one that best keeps the input order of the nodes wins: the mirror image of every
//! rank has the same crossings, and adjacent nodes with equal crossings either way are put in
//! input order, so authors see siblings in the order they wrote them.

use crate::proper::{Kind, Proper};

const MAX_ITER: usize = 24;
const MAX_TRANSPOSE_PASSES: usize = 16;

/// Orders every layer of `p` to reduce crossings.
pub(crate) fn order(p: &mut Proper) {
    let groups = p.group_parent.len();
    let mut best_layers = None;
    let mut best_cross = u64::MAX;
    for variant in 0..2 {
        p.layers = initial_order(p, variant == 1);
        let mut group_key = vec![0.0; groups];
        refresh_group_keys(p, &mut group_key);
        let pos = p.positions();
        for r in 0..p.layers.len() {
            let keys: Vec<f64> = (0..p.items.len()).map(|v| pos[v] as f64).collect();
            p.layers[r] = arrange(p, r, &keys, &group_key, false);
        }
        transpose(p);
        let mut cross = crossings(p);
        if cross < best_cross {
            best_cross = cross;
            best_layers = Some(p.layers.clone());
        }
        let mut iter = 0;
        while iter < MAX_ITER && cross > 0 {
            refresh_group_keys(p, &mut group_key);
            let explore = iter % 4 >= 2;
            sweep(p, iter % 2 == 0, &group_key, explore);
            if explore {
                transpose_equal(p);
            }
            transpose(p);
            cross = crossings(p);
            if cross < best_cross {
                best_cross = cross;
                best_layers = Some(p.layers.clone());
            }
            iter += 1;
        }
        if best_cross == 0 {
            break;
        }
    }
    if let Some(layers) = best_layers {
        p.layers = layers;
    }
    prefer_input_order(p);
}

/// Inversions of the input order of real nodes, per rank from the top.
fn disorder(p: &Proper) -> Vec<u64> {
    p.layers
        .iter()
        .map(|layer| {
            let ids: Vec<usize> = layer.iter().filter_map(|&v| if let Kind::Node(i) = p.items[v].kind { Some(i) } else { None }).collect();
            let mut c = 0u64;
            for i in 0..ids.len() {
                for j in i + 1..ids.len() {
                    if ids[i] > ids[j] {
                        c += 1;
                    }
                }
            }
            c
        })
        .collect()
}

/// Mirrors every rank when that keeps the input order better (the crossings are the same), then
/// swaps adjacent nodes into input order where that leaves the crossings unchanged.
fn prefer_input_order(p: &mut Proper) {
    let before = disorder(p);
    let original = p.layers.clone();
    for layer in &mut p.layers {
        layer.reverse();
    }
    mirror_borders(p);
    if disorder(p) >= before {
        p.layers = original;
    }
    let mut pos = p.positions();
    for _ in 0..MAX_TRANSPOSE_PASSES {
        let mut changed = false;
        for r in 0..p.layers.len() {
            for i in 0..p.layers[r].len().saturating_sub(1) {
                let (v, w) = (p.layers[r][i], p.layers[r][i + 1]);
                let (iv, iw) = (&p.items[v], &p.items[w]);
                let (Kind::Node(a), Kind::Node(b)) = (iv.kind, iw.kind) else { continue };
                if a < b || iv.owner != iw.owner {
                    continue;
                }
                let (before, after) = swap_costs(p, v, w, &pos);
                if after <= before {
                    p.layers[r].swap(i, i + 1);
                    pos[v] = i + 1;
                    pos[w] = i;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// After reversing every rank each group's right border comes first: swap the two back.
fn mirror_borders(p: &mut Proper) {
    for layer in &mut p.layers {
        let mut at: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();
        for (i, &v) in layer.iter().enumerate() {
            match p.items[v].kind {
                Kind::BorderL(g) => match at.iter_mut().find(|a| a.0 == g) {
                    Some(a) => a.1 = Some(i),
                    None => at.push((g, Some(i), None)),
                },
                Kind::BorderR(g) => match at.iter_mut().find(|a| a.0 == g) {
                    Some(a) => a.2 = Some(i),
                    None => at.push((g, None, Some(i))),
                },
                _ => {}
            }
        }
        for (_, l, r) in at {
            if let (Some(l), Some(r)) = (l, r) {
                layer.swap(l, r);
            }
        }
    }
}

/// Breadth-first order from the sources (or, `from_bottom`, from the sinks upwards), visiting
/// items in input order.
fn initial_order(p: &Proper, from_bottom: bool) -> Vec<Vec<usize>> {
    let n = p.items.len();
    let mut layers = vec![Vec::new(); p.layers.len()];
    let mut seen = vec![false; n];
    let (back, fwd) = if from_bottom { (&p.down, &p.up) } else { (&p.up, &p.down) };
    let mut queue = std::collections::VecDeque::new();
    for start in 0..n {
        if seen[start] || p.items[start].is_border() || !back[start].is_empty() {
            continue;
        }
        seen[start] = true;
        queue.push_back(start);
        while let Some(v) = queue.pop_front() {
            layers[p.items[v].rank].push(v);
            for &w in &fwd[v] {
                if !seen[w] {
                    seen[w] = true;
                    queue.push_back(w);
                }
            }
        }
    }
    for v in 0..n {
        if !seen[v] {
            layers[p.items[v].rank].push(v);
        }
    }
    layers
}

/// Each group's key is the mean relative position of its left border over the ranks it spans.
fn refresh_group_keys(p: &Proper, key: &mut [f64]) {
    let mut sum = vec![0.0; key.len()];
    let mut cnt = vec![0usize; key.len()];
    for layer in &p.layers {
        let len = layer.len().max(1) as f64;
        for (i, &v) in layer.iter().enumerate() {
            if let Kind::BorderL(g) = p.items[v].kind {
                sum[g] += i as f64 / len;
                cnt[g] += 1;
            }
        }
    }
    for g in 0..key.len() {
        if cnt[g] > 0 {
            key[g] = sum[g] / cnt[g] as f64;
        }
    }
}

fn sweep(p: &mut Proper, down: bool, group_key: &[f64], flip_ties: bool) {
    let h = p.layers.len();
    if h == 0 {
        return;
    }
    let ranks: Vec<usize> = if down { (0..h).collect() } else { (0..h).rev().collect() };
    let mut pos = p.positions();
    for (step, &r) in ranks.iter().enumerate() {
        let mut keys: Vec<f64> = pos.iter().map(|&i| i as f64).collect();
        if step > 0 {
            for &v in &p.layers[r] {
                let nbrs = if down { &p.up[v] } else { &p.down[v] };
                let mut ps: Vec<f64> = nbrs.iter().map(|&u| pos[u] as f64).collect();
                ps.sort_by(f64::total_cmp);
                if let Some(m) = median_value(&ps) {
                    keys[v] = m;
                }
            }
        }
        p.layers[r] = arrange(p, r, &keys, group_key, flip_ties);
        for (i, &v) in p.layers[r].iter().enumerate() {
            pos[v] = i;
        }
    }
}

/// The weighted median of Gansner et al.: the median, or for an even count the two middle
/// values weighted towards the side where the neighbours are packed more tightly.
fn median_value(ps: &[f64]) -> Option<f64> {
    let len = ps.len();
    let m = len / 2;
    match len {
        0 => None,
        _ if len % 2 == 1 => ps.get(m).copied(),
        2 => Some((ps[0] + ps[1]) / 2.0),
        _ => {
            let left = ps[m - 1] - ps[0];
            let right = ps[len - 1] - ps[m];
            if left + right <= 0.0 { Some((ps[m - 1] + ps[m]) / 2.0) } else { Some((ps[m - 1] * right + ps[m] * left) / (left + right)) }
        }
    }
}

#[derive(Clone, Copy)]
enum Child {
    Item(usize),
    Group(usize),
}

/// Sorts layer `r` by `keys`, keeping groups contiguous, bracketed by their borders and in the
/// sibling order given by `group_key`. Equal keys keep their current order, or reverse it with
/// `flip_ties`.
fn arrange(p: &Proper, r: usize, keys: &[f64], group_key: &[f64], flip_ties: bool) -> Vec<usize> {
    let groups = p.group_parent.len();
    // Slot 0 is the root, slot g + 1 is group g.
    let mut children: Vec<Vec<Child>> = vec![Vec::new(); groups + 1];
    let mut borders: Vec<(Option<usize>, Option<usize>)> = vec![(None, None); groups];
    for &v in &p.layers[r] {
        let it = &p.items[v];
        match it.kind {
            Kind::BorderL(g) => borders[g].0 = Some(v),
            Kind::BorderR(g) => borders[g].1 = Some(v),
            _ => children[it.owner.map_or(0, |g| g + 1)].push(Child::Item(v)),
        }
    }
    for g in 0..groups {
        if borders[g].0.is_some() {
            children[p.group_parent[g].map_or(0, |q| q + 1)].push(Child::Group(g));
        }
    }
    let mut out = Vec::with_capacity(p.layers[r].len());
    Tree { children: &children, borders: &borders, keys, group_key, flip_ties }.emit(0, &mut out, 0);
    // Anything not reached (a group whose parent has no borders here) keeps its place at the end.
    if out.len() < p.layers[r].len() {
        let mut placed = vec![false; p.items.len()];
        for &v in &out {
            placed[v] = true;
        }
        for &v in &p.layers[r] {
            if !placed[v] {
                out.push(v);
            }
        }
    }
    out
}

/// One rank as a tree of groups, with the keys to sort it by.
struct Tree<'a> {
    children: &'a [Vec<Child>],
    borders: &'a [(Option<usize>, Option<usize>)],
    keys: &'a [f64],
    group_key: &'a [f64],
    flip_ties: bool,
}

impl Tree<'_> {
    /// Appends the ordered contents of `slot` to `out`; returns the sum and count of item keys.
    fn emit(&self, slot: usize, out: &mut Vec<usize>, depth: usize) -> (f64, usize) {
        let (children, borders, keys, group_key, flip_ties) = (self.children, self.borders, self.keys, self.group_key, self.flip_ties);
        let kids = &children[slot];
        let mut sorted: Vec<(f64, usize, Child, Vec<usize>)> = Vec::with_capacity(kids.len());
        let (mut sum, mut cnt) = (0.0, 0usize);
        for (i, &c) in kids.iter().enumerate() {
            match c {
                Child::Item(v) => {
                    sum += keys[v];
                    cnt += 1;
                    sorted.push((keys[v], i, c, vec![v]));
                }
                Child::Group(g) => {
                    let mut seq = Vec::new();
                    let (s, k) = if depth < crate::MAX_GROUP_DEPTH + 1 { self.emit(g + 1, &mut seq, depth + 1) } else { (0.0, 0) };
                    sum += s;
                    cnt += k;
                    let fallback = borders[g].0.map_or(0.0, |b| keys[b]);
                    let key = if k > 0 { s / k as f64 } else { fallback };
                    let mut block = Vec::with_capacity(seq.len() + 2);
                    block.extend(borders[g].0);
                    block.extend(seq);
                    block.extend(borders[g].1);
                    sorted.push((key, i, c, block));
                }
            }
        }
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(if flip_ties { b.1.cmp(&a.1) } else { a.1.cmp(&b.1) }));
        // Groups keep their sibling order: refill the slots groups landed in, in group-key order.
        let slots: Vec<usize> = sorted.iter().enumerate().filter(|(_, s)| matches!(s.2, Child::Group(_))).map(|(i, _)| i).collect();
        let mut gs: Vec<(f64, usize, Vec<usize>)> = slots
            .iter()
            .filter_map(|&i| match sorted[i].2 {
                Child::Group(g) => Some((group_key[g], g, std::mem::take(&mut sorted[i].3))),
                Child::Item(_) => None,
            })
            .collect();
        gs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (&slot_i, g) in slots.iter().zip(gs) {
            sorted[slot_i].3 = g.2;
        }
        for s in sorted {
            out.extend(s.3);
        }
        (sum, cnt)
    }
}

/// Crossings between the segments of `a` and `b` (a left of b) and one neighbouring layer.
fn pair_crossings(na: &[usize], nb: &[usize], pos: &[usize]) -> u64 {
    let mut c = 0;
    for &x in na {
        for &y in nb {
            if pos[x] > pos[y] {
                c += 1;
            }
        }
    }
    c
}

/// Swaps neighbours with the same owner while that removes crossings.
fn transpose(p: &mut Proper) {
    let mut pos = p.positions();
    for _ in 0..MAX_TRANSPOSE_PASSES {
        let mut improved = false;
        for r in 0..p.layers.len() {
            for i in 0..p.layers[r].len().saturating_sub(1) {
                let (v, w) = (p.layers[r][i], p.layers[r][i + 1]);
                let (iv, iw) = (&p.items[v], &p.items[w]);
                if iv.is_border() || iw.is_border() || iv.owner != iw.owner {
                    continue;
                }
                let (before, after) = swap_costs(p, v, w, &pos);
                if after < before {
                    p.layers[r].swap(i, i + 1);
                    pos[v] = i + 1;
                    pos[w] = i;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Crossings at `v` and its right neighbour `w` as they are, and swapped.
fn swap_costs(p: &Proper, v: usize, w: usize, pos: &[usize]) -> (u64, u64) {
    let before = pair_crossings(&p.up[v], &p.up[w], pos) + pair_crossings(&p.down[v], &p.down[w], pos);
    let after = pair_crossings(&p.up[w], &p.up[v], pos) + pair_crossings(&p.down[w], &p.down[v], pos);
    (before, after)
}

/// One pass that swaps neighbours with the same owner whose crossings are equal either way (and
/// not zero), to leave a local minimum.
fn transpose_equal(p: &mut Proper) {
    let mut pos = p.positions();
    for r in 0..p.layers.len() {
        let mut i = 0;
        while i + 1 < p.layers[r].len() {
            let (v, w) = (p.layers[r][i], p.layers[r][i + 1]);
            let (iv, iw) = (&p.items[v], &p.items[w]);
            if !iv.is_border() && !iw.is_border() && iv.owner == iw.owner {
                let (before, after) = swap_costs(p, v, w, &pos);
                if before > 0 && after == before {
                    p.layers[r].swap(i, i + 1);
                    pos[v] = i + 1;
                    pos[w] = i;
                    i += 1;
                }
            }
            i += 1;
        }
    }
}

/// Total crossings between all neighbouring layers, by counting inversions.
pub(crate) fn crossings(p: &Proper) -> u64 {
    let pos = p.positions();
    let mut total = 0;
    for r in 0..p.layers.len().saturating_sub(1) {
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for &v in &p.layers[r] {
            for &w in &p.down[v] {
                segs.push((pos[v], pos[w]));
            }
        }
        segs.sort_unstable();
        let lower: Vec<usize> = segs.iter().map(|s| s.1).collect();
        total += inversions(&lower, p.layers.get(r + 1).map_or(0, Vec::len));
    }
    total
}

/// Pairs `i < j` with `a[i] > a[j]`, with a Fenwick tree over values `0..size`.
fn inversions(a: &[usize], size: usize) -> u64 {
    let mut tree = vec![0u64; size + 1];
    let mut total = 0u64;
    for (seen, &x) in a.iter().enumerate() {
        // Count earlier values <= x.
        let mut i = x + 1;
        let mut le = 0;
        while i > 0 {
            le += tree.get(i).copied().unwrap_or(0);
            i &= i - 1;
        }
        total += seen as u64 - le;
        let mut i = x + 1;
        while i <= size {
            if let Some(t) = tree.get_mut(i) {
                *t += 1;
            }
            i += i & i.wrapping_neg();
        }
    }
    total
}
