//! Coordinates across the ranks: Brandes and Köpf, "Fast and Simple Horizontal Coordinate
//! Assignment" (2001). Four vertical alignments (towards the upper or lower median, scanning left
//! or right) are compacted and then balanced; type 1 conflicts keep long edges straight, and every
//! segment crossing a group border chain is marked so the chains align and groups stay rectangles.

use crate::proper::Proper;
use std::collections::BTreeSet;

/// Cross-axis centre of every item. `sep(u, v)` is the least distance between the centres of `u`
/// and its right neighbour `v`.
pub(crate) fn assign(p: &Proper, sep: &dyn Fn(usize, usize) -> f64) -> Vec<f64> {
    let n = p.items.len();
    if n == 0 {
        return Vec::new();
    }
    let pos = p.positions();
    let marked = mark_conflicts(p, &pos);
    let mut layouts: Vec<(Vec<f64>, bool)> = Vec::with_capacity(4);
    for vdown in [true, false] {
        for left in [true, false] {
            match one_direction(p, &pos, &marked, vdown, left, sep) {
                Some(x) => layouts.push((x, left)),
                None => return fallback(p, sep),
            }
        }
    }
    balance(&layouts, n)
}

fn mark_conflicts(p: &Proper, pos: &[usize]) -> BTreeSet<(usize, usize)> {
    let mut marked = BTreeSet::new();
    let h = p.layers.len();
    for i in 0..h.saturating_sub(1) {
        let upper_len = p.layers[i].len();
        let lower = &p.layers[i + 1];
        let mut k0 = 0usize;
        let mut l = 0usize;
        for (l1, &v) in lower.iter().enumerate() {
            let inner_upper = p.up[v].iter().copied().find(|&u| p.inner(u, v));
            if l1 + 1 == lower.len() || inner_upper.is_some() {
                let k1 = inner_upper.map_or(upper_len.saturating_sub(1), |u| pos[u]);
                while l <= l1 {
                    let w = lower[l];
                    for &u in &p.up[w] {
                        if (pos[u] < k0 || pos[u] > k1) && !p.inner(u, w) {
                            marked.insert((u, w));
                        }
                    }
                    l += 1;
                }
                k0 = k1;
            }
        }
        // Nothing may cross a group border chain, inner segments included.
        for &b in lower {
            if !p.items[b].is_border() {
                continue;
            }
            for &bu in &p.up[b] {
                if !p.inner(bu, b) {
                    continue;
                }
                for &w in lower {
                    for &u in &p.up[w] {
                        if (u, w) == (bu, b) {
                            continue;
                        }
                        let crosses = (pos[u] < pos[bu] && pos[w] > pos[b]) || (pos[u] > pos[bu] && pos[w] < pos[b]);
                        if crosses {
                            marked.insert((u, w));
                        }
                    }
                }
            }
        }
    }
    marked
}

/// One of the four alignments plus compaction. Returns `None` if the block graph is cyclic,
/// which the alignment rules exclude.
fn one_direction(
    p: &Proper,
    pos: &[usize],
    marked: &BTreeSet<(usize, usize)>,
    vdown: bool,
    left: bool,
    sep: &dyn Fn(usize, usize) -> f64,
) -> Option<Vec<f64>> {
    let n = p.items.len();
    let width = |layer: usize| p.layers[layer].len();
    // Position in the scan direction.
    let tpos = |v: usize| if left { pos[v] } else { width(p.items[v].rank) - 1 - pos[v] };
    let layer_ids: Vec<usize> = if vdown { (0..p.layers.len()).collect() } else { (0..p.layers.len()).rev().collect() };
    let mut root: Vec<usize> = (0..n).collect();
    let mut align: Vec<usize> = (0..n).collect();
    for &li in layer_ids.iter().skip(1) {
        let mut layer = p.layers[li].clone();
        if !left {
            layer.reverse();
        }
        let mut r: isize = -1;
        for &v in &layer {
            let mut nbrs: Vec<usize> = if vdown { p.up[v].clone() } else { p.down[v].clone() };
            if nbrs.is_empty() {
                continue;
            }
            nbrs.sort_by_key(|&u| tpos(u));
            let d = nbrs.len();
            let ms = if d % 2 == 1 { vec![d / 2] } else { vec![d / 2 - 1, d / 2] };
            for m in ms {
                if align[v] != v {
                    break;
                }
                let u = nbrs[m];
                let seg = if vdown { (u, v) } else { (v, u) };
                let tu = tpos(u) as isize;
                if !marked.contains(&seg) && r < tu {
                    align[u] = v;
                    root[v] = root[u];
                    align[v] = root[v];
                    r = tu;
                }
            }
        }
    }
    // Block constraint graph: each item's scan-order predecessor in its layer.
    let mut succ: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for layer in &p.layers {
        for k in 1..layer.len() {
            let (a, b) = (layer[k - 1], layer[k]);
            let s = sep(a, b);
            // In scan order the predecessor of b is a (left) or b is the predecessor of a (right).
            let (from, to) = if left { (root[a], root[b]) } else { (root[b], root[a]) };
            succ[from].push((to, s));
            indeg[to] += 1;
        }
    }
    let mut xb = vec![0.0f64; n];
    let mut ready: BTreeSet<usize> = (0..n).filter(|&v| root[v] == v && indeg[v] == 0).collect();
    let mut done = 0usize;
    let blocks = (0..n).filter(|&v| root[v] == v).count();
    while let Some(b) = ready.pop_first() {
        done += 1;
        for &(c, s) in &succ[b] {
            xb[c] = xb[c].max(xb[b] + s);
            indeg[c] -= 1;
            if indeg[c] == 0 {
                ready.insert(c);
            }
        }
    }
    if done < blocks {
        return None;
    }
    let sign = if left { 1.0 } else { -1.0 };
    Some((0..n).map(|v| sign * xb[root[v]]).collect())
}

/// Aligns the four layouts to the narrowest and takes the mean of the two median candidates.
fn balance(layouts: &[(Vec<f64>, bool)], n: usize) -> Vec<f64> {
    let extent = |x: &Vec<f64>| {
        let lo = x.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };
    let mut best = 0;
    let mut best_w = f64::INFINITY;
    for (i, (x, _)) in layouts.iter().enumerate() {
        let (lo, hi) = extent(x);
        if hi - lo < best_w {
            best_w = hi - lo;
            best = i;
        }
    }
    let (blo, bhi) = extent(&layouts[best].0);
    let shifted: Vec<Vec<f64>> = layouts
        .iter()
        .map(|(x, left)| {
            let (lo, hi) = extent(x);
            let d = if *left { blo - lo } else { bhi - hi };
            x.iter().map(|v| v + d).collect()
        })
        .collect();
    (0..n)
        .map(|v| {
            let mut c: Vec<f64> = shifted.iter().map(|x| x[v]).collect();
            c.sort_by(f64::total_cmp);
            (c[1] + c[2]) / 2.0
        })
        .collect()
}

/// Packs each layer left to right; used only if alignment went wrong.
fn fallback(p: &Proper, sep: &dyn Fn(usize, usize) -> f64) -> Vec<f64> {
    let mut x = vec![0.0; p.items.len()];
    for layer in &p.layers {
        for k in 1..layer.len() {
            x[layer[k]] = x[layer[k - 1]] + sep(layer[k - 1], layer[k]);
        }
    }
    x
}
