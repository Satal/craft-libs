//! Rank assignment by network simplex (Gansner, Koutsofios, North and Vo 1993, section 2).
//!
//! Minimises the sum of `weight * (rank(to) - rank(from))` subject to
//! `rank(to) - rank(from) >= min_len` for every edge of an acyclic graph.

#[derive(Clone, Copy, Debug)]
pub(crate) struct RankEdge {
    pub from: usize,
    pub to: usize,
    pub min_len: i64,
    pub weight: i64,
}

/// Ranks for `n` nodes, each weakly connected component normalised to start at rank 0.
/// `edges` must form a DAG with `from != to`; violating edges are ignored.
pub(crate) fn rank(n: usize, edges: &[RankEdge]) -> Vec<i64> {
    let edges: Vec<RankEdge> = edges.iter().copied().filter(|e| e.from < n && e.to < n && e.from != e.to).collect();
    let mut comp = UnionFind::new(n);
    for e in &edges {
        comp.union(e.from, e.to);
    }
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
    for v in 0..n {
        let r = comp.find(v);
        members[r].push(v);
    }
    let mut comp_edges: Vec<Vec<RankEdge>> = vec![Vec::new(); n];
    for e in &edges {
        let r = comp.find(e.from);
        comp_edges[r].push(*e);
    }
    let mut local = vec![0usize; n];
    let mut ranks = vec![0i64; n];
    for r in 0..n {
        let nodes = &members[r];
        if nodes.is_empty() {
            continue;
        }
        for (i, &v) in nodes.iter().enumerate() {
            local[v] = i;
        }
        let es: Vec<RankEdge> =
            comp_edges[r].iter().map(|e| RankEdge { from: local[e.from], to: local[e.to], min_len: e.min_len, weight: e.weight }).collect();
        let rk = Simplex::new(nodes.len(), es).solve();
        for (i, &v) in nodes.iter().enumerate() {
            ranks[v] = rk.get(i).copied().unwrap_or(0);
        }
    }
    ranks
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }
    fn find(&mut self, mut v: usize) -> usize {
        while self.parent[v] != v {
            self.parent[v] = self.parent[self.parent[v]];
            v = self.parent[v];
        }
        v
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        // The smaller root wins so component identity follows input order.
        if ra < rb {
            self.parent[rb] = ra;
        } else if rb < ra {
            self.parent[ra] = rb;
        }
    }
}

struct Simplex {
    n: usize,
    edges: Vec<RankEdge>,
    /// Edge indices incident to each node.
    inc: Vec<Vec<usize>>,
    rank: Vec<i64>,
    in_tree: Vec<bool>,
    tree_edge: Vec<bool>,
    parent_edge: Vec<Option<usize>>,
    low: Vec<usize>,
    lim: Vec<usize>,
    cut: Vec<i64>,
}

impl Simplex {
    fn new(n: usize, edges: Vec<RankEdge>) -> Self {
        let mut inc = vec![Vec::new(); n];
        for (i, e) in edges.iter().enumerate() {
            inc[e.from].push(i);
            inc[e.to].push(i);
        }
        let m = edges.len();
        Self {
            n,
            edges,
            inc,
            rank: vec![0; n],
            in_tree: vec![false; n],
            tree_edge: vec![false; m],
            parent_edge: vec![None; n],
            low: vec![0; n],
            lim: vec![0; n],
            cut: vec![0; m],
        }
    }

    fn slack(&self, e: usize) -> i64 {
        let e = &self.edges[e];
        self.rank[e.to] - self.rank[e.from] - e.min_len
    }

    fn solve(mut self) -> Vec<i64> {
        if self.n <= 1 || self.edges.is_empty() {
            return vec![0; self.n];
        }
        self.init_rank();
        self.feasible_tree();
        self.postorder();
        self.cut_values();
        let max_iter = 20 * self.n + 200;
        let mut start = 0;
        for _ in 0..max_iter {
            let Some(leave) = self.leave_edge(&mut start) else { break };
            let Some(enter) = self.enter_edge(leave) else { break };
            self.tree_edge[leave] = false;
            self.tree_edge[enter] = true;
            self.retree_ranks();
            self.postorder();
            self.cut_values();
        }
        let min = self.rank.iter().copied().min().unwrap_or(0);
        for r in &mut self.rank {
            *r -= min;
        }
        self.balance();
        self.rank
    }

    /// Longest path from the sources, in topological order (lowest index first).
    fn init_rank(&mut self) {
        let mut indeg = vec![0usize; self.n];
        for e in &self.edges {
            indeg[e.to] += 1;
        }
        let mut queue: std::collections::VecDeque<usize> = (0..self.n).filter(|&v| indeg[v] == 0).collect();
        let mut seen = 0;
        while let Some(v) = queue.pop_front() {
            seen += 1;
            for &ei in &self.inc[v] {
                let e = self.edges[ei];
                if e.from == v {
                    self.rank[e.to] = self.rank[e.to].max(self.rank[v] + e.min_len);
                    indeg[e.to] -= 1;
                    if indeg[e.to] == 0 {
                        queue.push_back(e.to);
                    }
                }
            }
        }
        if seen < self.n {
            // Not a DAG (cannot happen after cycle removal): fall back to a feasible-free start and
            // let the tight tree search fix what it can.
            for e in self.edges.clone() {
                if self.rank[e.to] < self.rank[e.from] + e.min_len {
                    self.rank[e.to] = self.rank[e.from] + e.min_len;
                }
            }
        }
    }

    /// Grows a tree of tight edges from node 0, shifting the tree's ranks to make the nearest
    /// non-tree edge tight whenever it stops growing.
    fn feasible_tree(&mut self) {
        self.in_tree[0] = true;
        let mut size = 1 + self.grow(vec![0]);
        while size < self.n {
            let mut best: Option<(i64, usize)> = None;
            for ei in 0..self.edges.len() {
                let e = self.edges[ei];
                if self.in_tree[e.from] != self.in_tree[e.to] {
                    let s = self.slack(ei);
                    if best.is_none_or(|(bs, _)| s < bs) {
                        best = Some((s, ei));
                    }
                }
            }
            let Some((s, ei)) = best else { break };
            let delta = if self.in_tree[self.edges[ei].to] { -s } else { s };
            for v in 0..self.n {
                if self.in_tree[v] {
                    self.rank[v] += delta;
                }
            }
            let tree_nodes: Vec<usize> = (0..self.n).filter(|&v| self.in_tree[v]).collect();
            size += self.grow(tree_nodes);
        }
    }

    fn grow(&mut self, mut stack: Vec<usize>) -> usize {
        let mut added = 0;
        while let Some(v) = stack.pop() {
            for k in 0..self.inc[v].len() {
                let ei = self.inc[v][k];
                let e = self.edges[ei];
                let w = if e.from == v { e.to } else { e.from };
                if !self.in_tree[w] && self.slack(ei) == 0 {
                    self.in_tree[w] = true;
                    self.tree_edge[ei] = true;
                    added += 1;
                    stack.push(w);
                }
            }
        }
        added
    }

    /// Recomputes every rank from node 0 along the tree edges, which are all tight.
    fn retree_ranks(&mut self) {
        let mut seen = vec![false; self.n];
        seen[0] = true;
        let mut stack = vec![0usize];
        while let Some(v) = stack.pop() {
            for k in 0..self.inc[v].len() {
                let ei = self.inc[v][k];
                if !self.tree_edge[ei] {
                    continue;
                }
                let e = self.edges[ei];
                let (w, r) = if e.from == v { (e.to, self.rank[v] + e.min_len) } else { (e.from, self.rank[v] - e.min_len) };
                if !seen[w] {
                    seen[w] = true;
                    self.rank[w] = r;
                    stack.push(w);
                }
            }
        }
    }

    /// Post-order numbers (`lim`) and the lowest number in each subtree (`low`), rooted at 0.
    fn postorder(&mut self) {
        let mut next = 0usize;
        let mut visited = vec![false; self.n];
        // (node, next incident index)
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        visited[0] = true;
        self.parent_edge[0] = None;
        let mut low_acc = vec![usize::MAX; self.n];
        while let Some(top) = stack.last_mut() {
            let v = top.0;
            if top.1 < self.inc[v].len() {
                let ei = self.inc[v][top.1];
                top.1 += 1;
                if !self.tree_edge[ei] {
                    continue;
                }
                let e = self.edges[ei];
                let w = if e.from == v { e.to } else { e.from };
                if !visited[w] {
                    visited[w] = true;
                    self.parent_edge[w] = Some(ei);
                    stack.push((w, 0));
                }
            } else {
                stack.pop();
                self.lim[v] = next;
                self.low[v] = low_acc[v].min(next);
                next += 1;
                if let Some(&(p, _)) = stack.last() {
                    low_acc[p] = low_acc[p].min(self.low[v]);
                }
            }
        }
    }

    fn in_subtree(&self, root: usize, v: usize) -> bool {
        self.low[root] <= self.lim[v] && self.lim[v] <= self.lim[root]
    }

    /// The cut value of the tree edge above node `x` is the net weight leaving `x`'s subtree,
    /// signed by the edge's direction: edges inside the subtree cancel out.
    fn cut_values(&mut self) {
        let mut net = vec![0i64; self.n];
        for e in &self.edges {
            net[e.from] += e.weight;
            net[e.to] -= e.weight;
        }
        let mut order: Vec<usize> = (0..self.n).collect();
        order.sort_by_key(|&v| self.lim[v]);
        let mut sub = net;
        for &x in &order {
            if let Some(pe) = self.parent_edge[x] {
                let e = self.edges[pe];
                let p = if e.from == x { e.to } else { e.from };
                self.cut[pe] = if e.from == x { sub[x] } else { -sub[x] };
                sub[p] += sub[x];
            }
        }
    }

    fn leave_edge(&self, start: &mut usize) -> Option<usize> {
        let m = self.edges.len();
        for k in 0..m {
            let ei = (*start + k) % m;
            if self.tree_edge[ei] && self.cut[ei] < 0 {
                *start = (ei + 1) % m;
                return Some(ei);
            }
        }
        None
    }

    fn enter_edge(&self, leave: usize) -> Option<usize> {
        let e = self.edges[leave];
        // The child end of the tree edge roots the subtree it cuts off.
        let x = if self.parent_edge[e.from] == Some(leave) { e.from } else { e.to };
        let tail_in_subtree = e.from == x;
        let mut best: Option<(i64, usize)> = None;
        for (fi, f) in self.edges.iter().enumerate() {
            if self.tree_edge[fi] {
                continue;
            }
            let from_in = self.in_subtree(x, f.from);
            let to_in = self.in_subtree(x, f.to);
            // The entering edge goes from the head component to the tail component.
            let ok = if tail_in_subtree { !from_in && to_in } else { from_in && !to_in };
            if ok {
                let s = self.slack(fi);
                if best.is_none_or(|(bs, _)| s < bs) {
                    best = Some((s, fi));
                }
            }
        }
        best.map(|(_, fi)| fi)
    }

    /// Moves nodes with equal in and out weight to the feasible rank holding the fewest nodes.
    fn balance(&mut self) {
        let max_rank = self.rank.iter().copied().max().unwrap_or(0);
        let Ok(len) = usize::try_from(max_rank + 1) else { return };
        let mut count = vec![0usize; len];
        for &r in &self.rank {
            if let Ok(i) = usize::try_from(r) {
                count[i] += 1;
            }
        }
        for v in 0..self.n {
            let (mut win, mut wout) = (0, 0);
            let (mut lo, mut hi) = (0i64, max_rank);
            for &ei in &self.inc[v] {
                let e = self.edges[ei];
                if e.to == v {
                    win += e.weight;
                    lo = lo.max(self.rank[e.from] + e.min_len);
                } else {
                    wout += e.weight;
                    hi = hi.min(self.rank[e.to] - e.min_len);
                }
            }
            if win != wout || lo >= hi {
                continue;
            }
            let cur = self.rank[v];
            let Ok(ci) = usize::try_from(cur) else { continue };
            let mut best = cur;
            let mut best_count = count[ci];
            for r in lo..=hi {
                let Ok(ri) = usize::try_from(r) else { continue };
                if r != cur && count[ri] + 1 < best_count {
                    best = r;
                    best_count = count[ri] + 1;
                }
            }
            if best != cur {
                if let (Ok(ci), Ok(bi)) = (usize::try_from(cur), usize::try_from(best)) {
                    count[ci] -= 1;
                    count[bi] += 1;
                }
                self.rank[v] = best;
            }
        }
    }
}
