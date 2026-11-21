#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

//! A bounded, hand-written parser for Mermaid flowchart text.
//!
//! [`parse`] turns `flowchart` / `graph` text into plain data ([`Flowchart`]); any other diagram
//! header yields [`Diagram::Unsupported`]. Statements the parser cannot read become
//! [`Warning`]s and parsing continues; nothing is dropped silently and nothing panics. Input
//! limits ([`Limits`]) turn oversized input into a [`ParseError`].
//!
//! # Clean room
//!
//! The syntax is taken from the user-facing Mermaid documentation only (a docs-derived
//! specification kept in the FigCraft research notes); no Mermaid source, grammar or third-party
//! parser was read. Documentation sources (Mermaid 12.3 docs, read 2026-11-21):
//!
//! - <https://mermaid.js.org/syntax/flowchart.html> (nodes, shapes, links, subgraphs, styling)
//! - <https://mermaid.js.org/intro/syntax-reference.html> (comments, header)
//! - <https://mermaid.js.org/config/directives.html> (`%%{init}%%`)
//! - <https://mermaid.js.org/intro/> and the per-diagram pages (header keywords)
//!
//! # Choices where the documentation is silent (all lenient)
//!
//! - A missing direction defaults to top-to-bottom; `TD` is the same as `TB`; `flowchart-elk` and
//!   `flowchart-v2` are read as `flowchart`.
//! - Several statements may share a line separated by `;`, and several nodes or chains may share
//!   a statement separated only by whitespace (`A[x] B[y]`).
//! - A node may be defined inside an edge statement, including the `@{ ... }` form.
//! - Node ids are ASCII letters, digits and `_`, any non-ASCII non-space character, and `-` when
//!   followed by a letter, digit or `_`.
//! - A node belongs to at most one subgraph: the one it is first mentioned in. A later mention
//!   inside a subgraph nested in that one moves it inward; a mention in an unrelated subgraph is
//!   kept out and reported as a warning.
//! - A bare subgraph name used as an edge endpoint is [`Endpoint::Subgraph`] whether the
//!   subgraph is declared before or after the edge.
//! - `style` beats `classDef` classes, which beat `classDef default`; `linkStyle default` is
//!   applied before indexed `linkStyle` lines.
//! - An unknown `@{ shape: ... }` name becomes [`Shape::Rect`] with a warning. `icon` and `img`
//!   nodes without a shape become [`Shape::Icon`] / [`Shape::Image`] placeholders.
//! - An edge id (`A e1@--> B`) is kept on the first edge only when `&` expands one link to many.
//! - `click` lines, edge property lines (`e1@{ animate: true }`) and subgraph `view` lines are
//!   consumed and recorded in [`Flowchart::notes`].
//! - Link length is the rank span: `-->` and `---` are 1, each extra dash adds 1 (the
//!   documentation lists `---`, `----`, `-----` as 1, 2, 3).
//! - Markdown strings keep their emphasis marks as written; only the backticks are removed.

mod link;
mod parse;
mod shape;
mod split;
mod style;
mod text;

pub use parse::{Limits, parse, parse_with_limits};
pub use shape::Shape;
pub use style::Style;

use std::fmt;

/// A parsed document.
#[derive(Debug, Clone, PartialEq)]
pub enum Diagram {
    /// A `flowchart` / `graph` diagram.
    Flowchart(Flowchart),
    /// Any other diagram type, named by its header keyword (for example `sequenceDiagram`).
    Unsupported {
        /// The first keyword of the diagram, at most 64 characters.
        kind: String,
    },
}

/// Layout direction of a flowchart or subgraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// `TB` and `TD`: top to bottom (the default).
    #[default]
    TopToBottom,
    /// `BT`.
    BottomToTop,
    /// `LR`.
    LeftToRight,
    /// `RL`.
    RightToLeft,
}

/// The line style of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// `---`, `-->`.
    Solid,
    /// `-.-`, `-.->`.
    Dotted,
    /// `===`, `==>`.
    Thick,
    /// `~~~`: a layout-only link that draws nothing.
    Invisible,
}

/// The end decoration of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// A plain line end.
    None,
    /// An arrow head.
    Arrow,
    /// A circle end (`--o`).
    Circle,
    /// A cross end (`--x`).
    Cross,
}

/// One end of an edge: a node or a subgraph, by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// An index into [`Flowchart::nodes`].
    Node(usize),
    /// An index into [`Flowchart::subgraphs`].
    Subgraph(usize),
}

/// A node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The id as written.
    pub id: String,
    /// The label (the id when none was given); `<br>` is a newline and entity codes are decoded.
    pub label: String,
    /// True when the label was a markdown string.
    pub markdown: bool,
    /// The shape.
    pub shape: Shape,
    /// Class names from `:::name` and `class` statements, in order of assignment.
    pub classes: Vec<String>,
    /// Properties from `style` statements only; see [`Flowchart::node_style`] for the cascade.
    pub style: Style,
}

/// An edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The edge id of `A e1@--> B`.
    pub id: Option<String>,
    /// The source.
    pub from: Endpoint,
    /// The target.
    pub to: Endpoint,
    /// The label, if any.
    pub label: Option<String>,
    /// True when the label was a markdown string.
    pub label_markdown: bool,
    /// The line style.
    pub stroke: Stroke,
    /// The decoration at the source end.
    pub start_head: Head,
    /// The decoration at the target end.
    pub end_head: Head,
    /// The rank span hint, at least 1.
    pub length: u8,
    /// Classes from `class` statements naming the edge id.
    pub classes: Vec<String>,
    /// Properties from `linkStyle` (default first, then by index).
    pub style: Style,
}

/// A subgraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Subgraph {
    /// The id (the title when only a title was given).
    pub id: String,
    /// The title shown for the subgraph.
    pub title: String,
    /// True when the title was a markdown string.
    pub title_markdown: bool,
    /// The `direction` line inside the subgraph, if any.
    pub direction: Option<Direction>,
    /// Index of the enclosing subgraph.
    pub parent: Option<usize>,
    /// Indices of the nodes that belong directly to this subgraph (not to a nested one).
    pub members: Vec<usize>,
    /// Classes assigned to the subgraph.
    pub classes: Vec<String>,
    /// Properties from `style` statements.
    pub style: Style,
}

/// A named `classDef`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDef {
    /// The class name; `default` applies to nodes without a class.
    pub name: String,
    /// The properties.
    pub style: Style,
}

/// A statement the parser could not read, or a choice it made that the reader should know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// 1-based source line.
    pub line: usize,
    /// What was wrong.
    pub message: String,
}

/// A flowchart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Flowchart {
    /// The diagram direction.
    pub direction: Direction,
    /// Nodes in order of first appearance.
    pub nodes: Vec<Node>,
    /// Edges in order of definition (the order `linkStyle` indexes).
    pub edges: Vec<Edge>,
    /// Subgraphs in order of first appearance.
    pub subgraphs: Vec<Subgraph>,
    /// Every `classDef`.
    pub class_defs: Vec<ClassDef>,
    /// Statements that could not be read (capped at 1000; the last entry says when more were left out).
    pub warnings: Vec<Warning>,
    /// Statements consumed on purpose without effect (`click`, edge properties).
    pub notes: Vec<Warning>,
}

impl Flowchart {
    /// The style of node `index` after the cascade: `classDef default`, then its classes in
    /// order, then its own `style`. Returns an empty style for an index out of range.
    pub fn node_style(&self, index: usize) -> Style {
        let Some(node) = self.nodes.get(index) else {
            return Style::default();
        };
        let mut out = Style::default();
        if let Some(d) = self.class_defs.iter().find(|c| c.name == "default") {
            out.merge(&d.style);
        }
        for class in &node.classes {
            for def in self.class_defs.iter().filter(|c| &c.name == class) {
                out.merge(&def.style);
            }
        }
        out.merge(&node.style);
        out
    }

    /// The style of edge `index` after the cascade: the classes named by its id, in order, then
    /// its own `linkStyle` properties. Returns an empty style for an index out of range.
    pub fn edge_style(&self, index: usize) -> Style {
        let Some(edge) = self.edges.get(index) else {
            return Style::default();
        };
        let mut out = Style::default();
        for class in &edge.classes {
            for def in self.class_defs.iter().filter(|c| &c.name == class) {
                out.merge(&def.style);
            }
        }
        out.merge(&edge.style);
        out
    }
}

/// Why a document was rejected outright.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The input is larger than [`Limits::max_bytes`].
    TooLarge {
        /// Input size.
        bytes: usize,
        /// The limit.
        limit: usize,
    },
    /// More than [`Limits::max_nodes`] nodes and subgraphs.
    TooManyNodes {
        /// The limit.
        limit: usize,
    },
    /// More than [`Limits::max_edges`] edges.
    TooManyEdges {
        /// The limit.
        limit: usize,
    },
    /// Subgraphs nested deeper than [`Limits::max_depth`].
    TooDeep {
        /// The limit.
        limit: usize,
    },
    /// A `---` front matter block that never closes.
    UnterminatedFrontMatter,
    /// No diagram header: the input is empty or holds only comments.
    MissingHeader,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::TooLarge { bytes, limit } => write!(f, "input is {bytes} bytes, limit is {limit}"),
            ParseError::TooManyNodes { limit } => write!(f, "more than {limit} nodes and subgraphs"),
            ParseError::TooManyEdges { limit } => write!(f, "more than {limit} edges"),
            ParseError::TooDeep { limit } => write!(f, "subgraphs nested deeper than {limit}"),
            ParseError::UnterminatedFrontMatter => f.write_str("front matter block is not closed"),
            ParseError::MissingHeader => f.write_str("no diagram header"),
        }
    }
}

impl std::error::Error for ParseError {}
