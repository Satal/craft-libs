//! VSDX drawings and VSSX stencils: the XML drawing format specified by Microsoft in
//! [MS-VSDX] *Visio Graphics Service VSDX File Format* v20250819, read and written on
//! [`craft_opc`] and [`craft_xml`].
//!
//! [`read`] turns a `.vsdx` package into a [`Drawing`]: pages, flattened shapes with their
//! geometry, text, fill and line, and 1-D connectors with the shapes their ends are glued to.
//! [`read_stencil`] lists the masters of a `.vssx` package. [`write`] makes a minimal drawing
//! package from a [`Drawing`]. The model is format-neutral: points, y down, origin at the
//! top-left of the page; the crate converts to inches, y up, origin bottom-left, as the format
//! uses (MS-VSDX §2.2.2.2).
//!
//! # What is read
//!
//! - Parts are found through relationships, never by name (§2.3.1, §2.3.3): package →
//!   Document → Pages → Page, and Document → Masters → Master. Unknown parts are ignored.
//! - Cell values are the cached `V` (§2.3.4.2.5, Cell_Type); formulas are not evaluated. A cell
//!   without `V` takes its default: 0 inches or 0 radians (§2.2.5.3.3.1). Units in `U` are
//!   honoured for lengths and angles; no unit means inches or radians (§2.5.8).
//! - Master inheritance (§2.2.5.4): a shape inherits every cell, section, row and sub-shape it
//!   does not specify from its master shape; local rows replace inherited ones by `IX` and
//!   `Del="1"` removes them. Sub-shapes match by `MasterShape`. A master with several
//!   top-level shapes is inherited as a group of them. Line, fill and text cells that neither
//!   the shape nor its master set come from its `LineStyle`, `FillStyle` and `TextStyle`
//!   style sheets and their parents (§2.3.4.2.94, StyleSheet_Type).
//! - The local-to-parent transform of §2.2.3.2.1 (subtract LocPin, mirror for FlipX/FlipY,
//!   rotate counter-clockwise by Angle, add Pin), composed through groups. Groups are
//!   flattened; a group's own geometry or text becomes a shape of its own.
//! - Geometry rows (§2.4.2): MoveTo, LineTo, ArcTo, EllipticalArcTo, Ellipse, RelMoveTo,
//!   RelLineTo, RelCubBezTo, RelQuadBezTo (as a cubic), RelEllipticalArcTo; `NoFill`, `NoLine`
//!   and `NoShow` per path (§2.2.3.2.2).
//! - Text (§2.3.4.2.96, Text_Type): the plain text with `cp`/`pp`/`tp` markers dropped, field
//!   text kept and newlines kept. The text colour is the Character section's row 0 `Color`.
//! - Colours: `#RRGGBB`, indices into the document's `Colors` table, `RGB(r,g,b)`, and the
//!   `…Trans` cells (0 opaque to 1 clear, §2.4.4.120).
//! - Connectors are shapes with `BeginX`, `BeginY`, `EndX` and `EndY` (§2.2.3.1.1). Their ends
//!   are glued through `Connects` (§2.3.4.2.12), or else through a `Sheet.<id>!` reference in
//!   the endpoint formula.
//!
//! Everything else that changes the picture is reported as a [`Warning`]: NURBS, polyline and
//! spline rows (drawn as their chord), infinite lines, fill patterns other than solid, theme
//! colours (`V="themed"`, drawn black or white), unknown units, unreadable numbers, foreign
//! (image or embedded) shapes, guides, missing masters, and groups nested deeper than
//! [`Limits::max_group_depth`].
//!
//! # What is written
//!
//! [`write`] follows the minimal package of the spec's samples (§3.1): `[Content_Types].xml`,
//! the package relationship to the Document, a Document with a `FaceNames` entry, style sheet
//! 0 "No Style" as in §3.1.1, and a `DocumentSheet`; the Pages part with one `Page` per page
//! (IDs from 0, as in §3.1.2) and one Page part each. Shapes keep their model IDs when those
//! are unique and non-zero, and are otherwise numbered from 4 (§2.3.4.2.88),
//! their transform, fill, line and text cells, one Geometry section per path with rows from
//! `IX="1"` (§3.1.3), a Connection section for their connection points and their `Text`.
//! Connectors are 1-D shapes with `BeginX`…`EndY`, a polyline Geometry section, arrows and
//! line cells; glue is written as `Connects` and in the endpoint cells.
//!
//! # Choices where the spec is silent
//!
//! The format notes this crate was built from leave these open; each is a choice, not a fact:
//!
//! - **O1 arrowheads.** `BeginArrow`/`EndArrow` take 0 (none) to 45, but which index is a
//!   filled triangle is not stated. Reading keeps the index. [`ARROW`] (13) is what the writer
//!   suggests for an arrowhead, unverified.
//! - **O2 required parts.** Whether a package without style sheets, theme or document
//!   properties opens everywhere is not stated. The writer emits style sheet 0 as in §3.1.1
//!   and no theme.
//! - **O3 stencils.** The content types of `.vssx` parts are not given; [`read_stencil`]
//!   follows relationships only and checks no content type.
//! - **O4 line breaks in text.** How a paragraph break is stored is not stated. The writer
//!   writes a literal newline (the roots carry `xml:space="preserve"`, as in §3.1); the reader
//!   keeps newlines as they are.
//! - **O5 document properties.** Their relationship types are outside MS-VSDX; the writer
//!   omits them.
//! - **O6 glue formulas.** The formula an application writes for a glued end is not shown.
//!   The writer stores real coordinates in `V`; for glue to a connection point it adds
//!   `F="PAR(PNT(Sheet.<id>!Connections.X<n>,Sheet.<id>!Connections.Y<n>))"` (the form the
//!   notes record), and for glue to a whole shape no formula. `Connects` names both.
//!   `Connections.X<n>` is taken to name connection row `IX = n - 1`.
//! - **O7 1-D shape box.** A connector's Width is the distance between its ends and Angle
//!   their direction (`ATAN2`); its Height is not stated, and the writer uses 0.
//! - **O8 master icons.** The content of a master's `Icon` element is not stated; only a PNG
//!   image related to the master part becomes [`Master::icon_png`].
//! - **Conflicts.** `Connects` is "unused" in MS-VSDX but meaningful elsewhere: both are
//!   written and it is read as a hint (c1). Renumbered shape IDs start at 4, though the §3.1.3 sample uses 1 (c2). Geometry rows are
//!   written from `IX="1"` like the samples, and read by the `IX` given or counted from 0 (c3).
//!   Elements are read by local name, so the 2011/1/core and 2012/main namespaces both work;
//!   the writer uses 2011/1/core (c4).
//! - **ArcTo.** `A` is the distance from the arc's middle to its chord's middle; its sign is
//!   not stated. A positive bow is taken to lie to the left of the travel from the current
//!   point to the end, in the shape's y-up space.
//!
//! # Limits
//!
//! [`Limits`] caps the package (entries, bytes per part, bytes in all), the XML (depth and
//! nodes), the pages, masters and shapes, and how deep groups nest. A package over a limit
//! is refused with an error that says so ([`Error::is_limit`]); groups deeper than the limit
//! are cut with a warning.
//!
//! [MS-VSDX]: https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod geom;
mod model;
mod read;
mod sheet;
mod values;
mod write;

pub use model::{ConnectionPoint, Connector, Drawing, GeomPath, Master, Page, Point, Rgba, Segment, Shape, Stencil};
pub use read::{read, read_stencil, read_stencil_with, read_with};
pub use write::write;

/// The arrowhead index the writer suggests for an arrow: unverified (see O1 above).
pub const ARROW: u8 = 13;

/// What a reader refuses to exceed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The package: entries and inflated bytes.
    pub package: craft_opc::Limits,
    /// The XML of one part: nesting depth.
    pub max_xml_depth: usize,
    /// The XML of one part: nodes.
    pub max_xml_nodes: usize,
    pub max_pages: usize,
    pub max_masters: usize,
    /// Shapes and connectors across the drawing, counted after master inheritance.
    pub max_shapes: usize,
    /// Groups inside groups; deeper sub-shapes are left out with a warning.
    pub max_group_depth: usize,
    /// Rows in one Geometry section; more are left out with a warning.
    pub max_rows: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        package: craft_opc::Limits::new(10_000, 64 * 1024 * 1024, 256 * 1024 * 1024),
        max_xml_depth: 256,
        max_xml_nodes: 4_000_000,
        max_pages: 1_000,
        max_masters: 10_000,
        max_shapes: 100_000,
        max_group_depth: 32,
        max_rows: 100_000,
    };
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::DEFAULT
    }
}

/// Why a package could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Package(#[from] craft_opc::Error),
    #[error("not a VSDX package: {0}")]
    NotVsdx(String),
    #[error("more than {limit} {what}")]
    TooMany { what: &'static str, limit: usize },
    #[error("cannot write: {0}")]
    Invalid(String),
}

impl Error {
    /// The input went over a [`Limits`] value.
    pub fn is_limit(&self) -> bool {
        match self {
            Error::Package(craft_opc::Error::Xml { error, .. }) => error.is_limit(),
            Error::Package(e) => e.is_limit(),
            Error::TooMany { .. } => true,
            Error::NotVsdx(_) | Error::Invalid(_) => false,
        }
    }
}

/// What a warning is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WarningKind {
    /// A geometry row drawn approximately or not at all.
    Geometry,
    /// A feature that is not drawn as the source shows it.
    Unsupported,
    /// A value that could not be read; its default was used.
    BadValue,
    /// A theme value; a neutral colour was used.
    Themed,
    /// A missing or unreadable part or master.
    Missing,
    /// Content left out at a limit.
    Limit,
}

/// Something read differently from the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: WarningKind,
    /// Where: the page or master name, and the shape ID when there is one.
    pub place: String,
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.place, self.message)
    }
}
