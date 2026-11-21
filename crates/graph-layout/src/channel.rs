//! Ordering parallel segments that share a channel, as in the nudging step of Wybrow, Marriott
//! and Stuckey (2009): each pair of overlapping segments is put in the order that gives fewer
//! crossings with the segments' perpendicular continuations, and overlapping segments get
//! distinct tracks.

const EPS: f64 = 1e-6;

/// A segment running along a channel from `lo` to `hi`. Each end has a position along the channel
/// and a turn: -1 when the path continues towards lower channel coordinates (for a horizontal
/// segment: up), +1 towards higher, 0 when the path ends there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChannelSeg {
    pub lo: f64,
    pub hi: f64,
    pub ends: [(f64, i8); 2],
}

impl ChannelSeg {
    pub fn new(a: (f64, i8), b: (f64, i8)) -> Self {
        Self { lo: a.0.min(b.0), hi: a.0.max(b.0), ends: [a, b] }
    }
}

fn overlaps(a: &ChannelSeg, b: &ChannelSeg, margin: f64) -> bool {
    a.lo < b.hi + margin && b.lo < a.hi + margin
}

/// Crossings and overlaps caused by putting `i` at a lower channel coordinate than `j`.
fn cost(i: &ChannelSeg, j: &ChannelSeg) -> u32 {
    let mut c = 0;
    for &(x, t) in &j.ends {
        if x > i.lo + EPS && x < i.hi - EPS && t < 0 {
            c += 1;
        }
    }
    for &(x, t) in &i.ends {
        if x > j.lo + EPS && x < j.hi - EPS && t > 0 {
            c += 1;
        }
    }
    for &(xi, ti) in &i.ends {
        for &(xj, tj) in &j.ends {
            if (xi - xj).abs() <= EPS && (ti > 0 || tj < 0) {
                c += 4;
            }
        }
    }
    c
}

/// A track for each segment (0 at the lowest channel coordinate) and the number of tracks.
/// Segments that overlap (allowing `margin`) never share a track.
pub(crate) fn assign_tracks(segs: &[ChannelSeg], margin: f64) -> (Vec<usize>, usize) {
    let n = segs.len();
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    let mut conflicts: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in i + 1..n {
            if !overlaps(&segs[i], &segs[j], margin) {
                continue;
            }
            conflicts[i].push(j);
            conflicts[j].push(i);
            let (cij, cji) = (cost(&segs[i], &segs[j]), cost(&segs[j], &segs[i]));
            if cij < cji {
                succ[i].push(j);
                indeg[j] += 1;
            } else if cji < cij {
                succ[j].push(i);
                indeg[i] += 1;
            }
        }
    }
    // Kahn's algorithm, lowest index first; a cycle is broken at the node with fewest
    // remaining predecessors.
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let pick = (0..n).filter(|&v| !done[v]).min_by_key(|&v| (indeg[v], v));
        let Some(v) = pick else { break };
        done[v] = true;
        order.push(v);
        for &w in &succ[v] {
            indeg[w] = indeg[w].saturating_sub(1);
        }
    }
    let mut track = vec![0usize; n];
    let mut placed = vec![false; n];
    let mut count = 0;
    for &v in &order {
        let t = conflicts[v].iter().filter(|&&w| placed[w]).map(|&w| track[w] + 1).max().unwrap_or(0);
        track[v] = t;
        placed[v] = true;
        count = count.max(t + 1);
    }
    (track, count)
}
