//! Cycle removal: the greedy feedback arc set heuristic of Eades, Lin and Smyth (1993).

/// For each edge `(from, to)` (no self-loops), whether reversing it is part of making the graph
/// acyclic. Sinks are peeled to the end of the sequence and sources to the front; when neither
/// exists the node with the largest out-degree minus in-degree goes to the front. Among those
/// candidates, nodes fed by an edge from the front part are preferred, so the sequence keeps
/// following the flow out of the sources and the edges that loop back are the ones reversed; the
/// lowest index wins remaining ties.
pub(crate) fn feedback_edges(n: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut outs = vec![Vec::new(); n];
    let mut ins = vec![Vec::new(); n];
    let mut outdeg = vec![0i64; n];
    let mut indeg = vec![0i64; n];
    for &(a, b) in edges {
        if a < n && b < n && a != b {
            outs[a].push(b);
            ins[b].push(a);
            outdeg[a] += 1;
            indeg[b] += 1;
        }
    }
    let mut removed = vec![false; n];
    let mut fed = vec![false; n];
    let mut front = Vec::with_capacity(n);
    let mut back = Vec::with_capacity(n);
    let mut remaining = n;
    let remove = |v: usize, removed: &mut Vec<bool>, outdeg: &mut Vec<i64>, indeg: &mut Vec<i64>| {
        removed[v] = true;
        for &w in &outs[v] {
            indeg[w] -= 1;
        }
        for &u in &ins[v] {
            outdeg[u] -= 1;
        }
    };
    while remaining > 0 {
        let mut progress = true;
        while progress {
            progress = false;
            for v in 0..n {
                if !removed[v] && outdeg[v] == 0 {
                    remove(v, &mut removed, &mut outdeg, &mut indeg);
                    back.push(v);
                    remaining -= 1;
                    progress = true;
                }
            }
            for v in 0..n {
                if !removed[v] && indeg[v] == 0 {
                    remove(v, &mut removed, &mut outdeg, &mut indeg);
                    outs[v].iter().for_each(|&w| fed[w] = true);
                    front.push(v);
                    remaining -= 1;
                    progress = true;
                }
            }
        }
        if remaining == 0 {
            break;
        }
        let mut best: Option<((bool, i64), usize)> = None;
        for v in 0..n {
            if !removed[v] {
                let key = (fed[v], outdeg[v] - indeg[v]);
                if best.is_none_or(|(k, _)| key > k) {
                    best = Some((key, v));
                }
            }
        }
        let Some((_, v)) = best else { break };
        remove(v, &mut removed, &mut outdeg, &mut indeg);
        outs[v].iter().for_each(|&w| fed[w] = true);
        front.push(v);
        remaining -= 1;
    }
    let mut pos = vec![0usize; n];
    for (i, &v) in front.iter().chain(back.iter().rev()).enumerate() {
        pos[v] = i;
    }
    edges.iter().map(|&(a, b)| a < n && b < n && pos[a] > pos[b]).collect()
}
