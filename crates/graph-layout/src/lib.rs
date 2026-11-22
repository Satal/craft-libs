//! Diagram layout: layered flowchart layout and obstacle-avoiding orthogonal connector routing.
//!
//! Pure geometry in `f64`, y-down, no dependencies. Every result is deterministic: the same input
//! gives bit-identical output, because no step depends on hash order, time or randomness.
//!
//! # Layered layout
//!
//! [`layered`] places the nodes of a directed [`Graph`] in ranks along a [`Direction`] and returns
//! node and group rectangles plus orthogonal edge polylines. It handles cycles, self-loops,
//! multi-edges, longer edges ([`EdgeSpec::min_len`]), variable node sizes, edge labels,
//! disconnected components (packed side by side, wrapping into rows) and nested groups
//! (subgraphs), which stay contiguous rectangles with padding and an optional title band.
//!
//! The pipeline is the one described by Sugiyama, Tagawa and Toda (1981) and refined by Gansner,
//! Koutsofios, North and Vo (1993):
//!
//! 1. **Cycle removal:** the greedy heuristic of Eades, Lin and Smyth, "A fast and effective
//!    heuristic for the feedback arc set problem" (Information Processing Letters, 1993). Ties are
//!    broken by input order, so the edges reversed are the ones that loop back to earlier nodes.
//! 2. **Ranking:** network simplex, from Gansner et al., "A Technique for Drawing Directed Graphs"
//!    (IEEE TSE 19(3), 1993), section 2: a feasible tight tree, cut values, edge exchange,
//!    normalisation and balancing, honouring each edge's minimum length.
//! 3. **Crossing reduction:** long edges are split into dummy nodes, then layer-by-layer sweeps
//!    order each rank by the weighted median of its neighbours followed by transposition of
//!    adjacent nodes (Gansner et al. 1993, section 3). Groups are kept contiguous by sorting each
//!    rank hierarchically and bracketing each group with border nodes on every rank it spans, in the
//!    manner of compound-graph layout (Sander 1996, "Layout of compound directed graphs").
//! 4. **Coordinates:** Brandes and Köpf, "Fast and Simple Horizontal Coordinate Assignment"
//!    (Graph Drawing 2001): type 1 conflict marking, four vertical alignments, block compaction
//!    (as a longest path over the block constraint graph) and balancing by the average median.
//!    Group border chains are always aligned, so each group is a rectangle.
//! 5. **Edges:** every edge leaves its source on the side facing the flow and enters its target on
//!    the opposite side; ports on one side are spread apart, keeping straight edges straight where
//!    the spacing allows. Bends between ranks are assigned to separate horizontal tracks, ordered to
//!    avoid crossings, as in the nudging step below.
//!
//! # Connector routing
//!
//! [`route_orthogonal`] finds one orthogonal connector between two [`Port`]s that avoids a set of
//! obstacle rectangles, and [`route_all`] routes many and separates their shared segments. They
//! follow Wybrow, Marriott and Stuckey, "Orthogonal Connector Routing" (Graph Drawing 2009): a
//! orthogonal visibility graph over the obstacle boundaries (here built as the grid of all
//! interesting coordinates, a superset of the visibility graph), A* search minimising length plus a
//! penalty per bend, and nudging that orders overlapping segments in a shared channel to avoid
//! crossings and spreads them apart within the free space.
//!
//! # Limits
//!
//! Inputs above [`MAX_NODES`], [`MAX_EDGES`], [`MAX_GROUPS`], [`MAX_GROUP_DEPTH`],
//! [`MAX_LAYOUT_ITEMS`] (nodes plus dummy and border nodes), [`MAX_OBSTACLES`] or
//! [`MAX_CONNECTORS`] are refused with [`LayoutError::TooLarge`]; sizes and coordinates must be
//! finite, not negative and at most [`MAX_COORD`]. Edge minimum lengths are clamped to
//! `1..=`[`MAX_MIN_LEN`]. Nothing panics on any input.
//!
//! ```
//! use craft_graph_layout::{EdgeSpec, Graph, LayeredOptions, NodeSpec, Size, layered};
//! let node = NodeSpec { size: Size::new(80.0, 40.0), group: None };
//! let graph = Graph {
//!     nodes: vec![node; 3],
//!     edges: vec![EdgeSpec::new(0, 1), EdgeSpec::new(1, 2), EdgeSpec::new(0, 2)],
//!     groups: vec![],
//! };
//! let layout = layered(&graph, &LayeredOptions::default())?;
//! assert!(layout.nodes[0].bottom() < layout.nodes[1].y);
//! # Ok::<(), craft_graph_layout::LayoutError>(())
//! ```
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod acyclic;
mod channel;
mod layered;
mod order;
mod position;
mod proper;
mod rank;
mod route;

pub use layered::layered;
pub use route::{route_all, route_orthogonal};

/// Most nodes [`layered`] accepts.
pub const MAX_NODES: usize = 2_000;
/// Most edges [`layered`] accepts.
pub const MAX_EDGES: usize = 8_000;
/// Most groups [`layered`] accepts.
pub const MAX_GROUPS: usize = 500;
/// Deepest group nesting [`layered`] accepts (a top-level group has depth 1).
pub const MAX_GROUP_DEPTH: usize = 32;
/// Largest edge minimum length; longer requests are clamped to this.
pub const MAX_MIN_LEN: u32 = 16;
/// Most nodes [`layered`] lays out after adding dummy nodes for long edges and group borders.
pub const MAX_LAYOUT_ITEMS: usize = 100_000;
/// Most obstacles [`route_orthogonal`] and [`route_all`] accept.
pub const MAX_OBSTACLES: usize = 5_000;
/// Most connectors [`route_all`] accepts.
pub const MAX_CONNECTORS: usize = 2_000;
/// Largest size, spacing or absolute coordinate accepted.
pub const MAX_COORD: f64 = 1e9;

/// A point, y-down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle: top-left corner and size, y-down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
    /// Whether the open interiors of the two rectangles overlap (touching edges do not count).
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right() && other.x < self.right() && self.y < other.bottom() && other.y < self.bottom()
    }
    /// Whether `other` lies inside this rectangle, edges included.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x && other.y >= self.y && other.right() <= self.right() && other.bottom() <= self.bottom()
    }
    fn is_valid(&self) -> bool {
        valid_coord(self.x) && valid_coord(self.y) && valid_len(self.width) && valid_len(self.height)
    }
}

/// A side of a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// The direction ranks advance in: `Down` is top to bottom (Mermaid `TD`/`TB`), `Up` is `BT`,
/// `Right` is `LR`, `Left` is `RL`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

/// A node to lay out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeSpec {
    pub size: Size,
    /// The innermost group the node belongs to, an index into [`Graph::groups`].
    pub group: Option<usize>,
}

/// A directed edge between two nodes, by index into [`Graph::nodes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeSpec {
    pub from: usize,
    pub to: usize,
    /// The least number of ranks between the two ends, 1 for a normal edge; clamped to
    /// `1..=`[`MAX_MIN_LEN`].
    pub min_len: u32,
    /// The size of the edge's label, if it has one; the layout reserves room for it.
    pub label: Option<Size>,
}

impl EdgeSpec {
    /// An unlabelled edge of minimum length 1.
    pub const fn new(from: usize, to: usize) -> Self {
        Self { from, to, min_len: 1, label: None }
    }
}

/// A group (subgraph): a rectangle drawn around its member nodes and child groups.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupSpec {
    /// The enclosing group, an index into [`Graph::groups`].
    pub parent: Option<usize>,
    /// Space between the group's edge and its contents.
    pub padding: f64,
    /// Extra space for a title band along the group's top edge (in output coordinates, whatever
    /// the direction), 0 for none.
    pub title_height: f64,
}

/// A directed graph to lay out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    pub groups: Vec<GroupSpec>,
}

/// Spacing and direction for [`layered`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayeredOptions {
    pub direction: Direction,
    /// Space between neighbouring nodes in one rank.
    pub node_sep: f64,
    /// Space between ranks.
    pub rank_sep: f64,
    /// Space between parallel edge segments and between edge ports on one side.
    pub edge_sep: f64,
    /// Space between disconnected components.
    pub component_sep: f64,
}

impl Default for LayeredOptions {
    fn default() -> Self {
        Self { direction: Direction::Down, node_sep: 40.0, rank_sep: 50.0, edge_sep: 12.0, component_sep: 60.0 }
    }
}

/// The result of [`layered`], in the coordinates of the requested direction, with the top-left
/// of everything at `(0, 0)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// One rectangle per input node.
    pub nodes: Vec<Rect>,
    /// One rectangle per input group.
    pub groups: Vec<Rect>,
    /// One orthogonal polyline per input edge, from a point on the source rectangle's boundary
    /// to a point on the target rectangle's boundary.
    pub edges: Vec<Vec<Point>>,
    /// The centre of each edge's label, for edges that have one. The label sits on a straight run
    /// of its edge, clear of every node; a self-loop's label sits beside its loop.
    pub labels: Vec<Option<Point>>,
    /// The size of the bounding box of everything above.
    pub size: Size,
}

/// One end of a connector for [`route_orthogonal`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Port {
    /// The shape the connector attaches to.
    pub rect: Rect,
    /// The preferred side. With a `point`, the side the point is on (the nearest side if `None`);
    /// without one, the router uses this side's midpoint when it can, and any side's otherwise.
    pub side: Option<Side>,
    /// A fixed attachment point, snapped onto the side.
    pub point: Option<Point>,
}

/// A connector for [`route_all`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Connector {
    pub from: Port,
    pub to: Port,
}

/// Costs and spacing for connector routing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteOptions {
    /// Preferred distance between a connector and any obstacle; the router falls back to a
    /// smaller one when shapes are too close to keep it.
    pub clearance: f64,
    /// Cost of one bend, in the same units as length.
    pub bend_penalty: f64,
    /// Spacing [`route_all`] puts between parallel segments that would otherwise overlap.
    pub separation: f64,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self { clearance: 12.0, bend_penalty: 40.0, separation: 6.0 }
    }
}

/// Why a layout could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The input has more of something than the documented cap.
    TooLarge { what: &'static str, count: usize, limit: usize },
    /// A size, spacing or coordinate is not finite, is negative or is above [`MAX_COORD`].
    InvalidNumber { what: &'static str, index: usize },
    /// An index points past the end of its list.
    DanglingIndex { what: &'static str, index: usize },
    /// A group is its own ancestor.
    GroupCycle { group: usize },
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { what, count, limit } => write!(f, "too many {what}: {count}, the limit is {limit}"),
            Self::InvalidNumber { what, index } => write!(f, "invalid {what} at index {index}"),
            Self::DanglingIndex { what, index } => write!(f, "{what} {index} points past the end of its list"),
            Self::GroupCycle { group } => write!(f, "group {group} is its own ancestor"),
        }
    }
}

impl std::error::Error for LayoutError {}

pub(crate) fn valid_coord(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

pub(crate) fn valid_len(v: f64) -> bool {
    v.is_finite() && (0.0..=MAX_COORD).contains(&v)
}
