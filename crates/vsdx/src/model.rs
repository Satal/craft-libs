//! The format-neutral drawing model: points (1/72 inch), y down, origin at the top-left of the
//! page.

/// A colour with straight alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }
}

/// A point in points.
pub type Point = (f64, f64);

/// A drawing: its pages in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub pages: Vec<Page>,
}

/// One page. `width` and `height` are in points; shapes and connectors are in page
/// coordinates (points, y down, origin top-left).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub name: String,
    pub width: f64,
    pub height: f64,
    /// In display order: a later shape is drawn over an earlier one. Groups are flattened.
    pub shapes: Vec<Shape>,
    pub connectors: Vec<Connector>,
}

/// A 2-D shape.
///
/// `x`, `y`, `width` and `height` are the shape's box before rotation; `angle` turns the box
/// clockwise about its centre, in degrees. Geometry is in the box's own space: points from its
/// top-left corner, y down, so an unflipped rectangle runs from `(0, 0)` to `(width, height)`.
/// A mirrored source shape is mirrored in its geometry; the box itself is never flipped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// The shape's ID: unique on its page. Sub-shapes that come only from a master have 0.
    pub id: u32,
    pub name: String,
    /// The master's universal name (`NameU`, or `Name`), for a shape made from a master.
    pub master: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Clockwise, in degrees.
    pub angle: f64,
    pub geometry: Vec<GeomPath>,
    /// Plain text; a newline is a line break.
    pub text: String,
    /// None: no fill.
    pub fill: Option<Rgba>,
    /// None: no line.
    pub line: Option<Rgba>,
    /// In points.
    pub line_weight: f64,
    /// A dashed line pattern (any pattern other than solid).
    pub dashed: bool,
    pub text_color: Option<Rgba>,
    /// Connection points in the box's space (points, y down). [`ConnectionPoint::Point`]`(n)`
    /// names entry `n - 1`.
    pub connections: Vec<Point>,
}

/// One path of a shape's geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeomPath {
    pub segments: Vec<Segment>,
    /// The path ends where it starts.
    pub closed: bool,
    /// The shape's fill is not applied to this path.
    pub no_fill: bool,
    /// The shape's line is not applied to this path.
    pub no_line: bool,
}

/// A step of a path, in the shape box's space (points, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Starts the path, or a new piece of it.
    MoveTo(Point),
    LineTo(Point),
    /// A cubic Bezier from the current point.
    CubicTo {
        c1: Point,
        c2: Point,
        to: Point,
    },
    /// An elliptical arc from the current point through `via` to `to`. `rotation` is the
    /// clockwise angle of the major axis in degrees and `ratio` the major to minor axis ratio:
    /// a circular arc has ratio 1.
    ArcTo {
        to: Point,
        via: Point,
        rotation: f64,
        ratio: f64,
    },
    /// A whole ellipse: its centre and two points on it, the ends of two conjugate
    /// semi-diameters (for an upright ellipse, the right end and the bottom or top end).
    Ellipse {
        centre: Point,
        a: Point,
        b: Point,
    },
}

/// Where a connector end is glued.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionPoint {
    /// To the shape as a whole (dynamic glue, `PinX`).
    Shape,
    /// To connection point `n` (one-based, `Connections.X<n>`): the target's
    /// [`Shape::connections`]`[n - 1]`.
    Point(u32),
}

/// A 1-D connector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Connector {
    pub id: u32,
    /// The shape ID and point the begin end is glued to.
    pub from: Option<(u32, ConnectionPoint)>,
    /// The shape ID and point the end is glued to.
    pub to: Option<(u32, ConnectionPoint)>,
    /// The route in page coordinates, begin first and end last.
    pub points: Vec<Point>,
    /// The arrowhead index at the begin end: 0 none. See [`crate::ARROW`].
    pub begin_arrow: u8,
    /// The arrowhead index at the end: 0 none.
    pub end_arrow: u8,
    /// None: no line.
    pub line: Option<Rgba>,
    /// In points.
    pub line_weight: f64,
    pub dashed: bool,
    pub label: String,
}

/// A stencil: its masters in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stencil {
    pub masters: Vec<Master>,
}

/// A master of a stencil (or drawing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Master {
    /// The universal name (`NameU`), or the local one.
    pub name: String,
    /// The local, language-dependent name.
    pub local_name: String,
    /// The `UniqueID` GUID, a stable key across documents.
    pub unique_id: Option<String>,
    /// The bounding box of the shapes.
    pub width: f64,
    pub height: f64,
    /// The master's shapes, flattened, in a space whose top-left is the bounding box's.
    pub shapes: Vec<Shape>,
    /// A PNG image the master part relates to, when there is one: at most 1 MB, starting
    /// like a PNG, and shared by the masters that relate to the same part.
    pub icon_png: Option<std::sync::Arc<[u8]>>,
}

impl GeomPath {
    /// A closed rectangle path from `(0, 0)` to `(width, height)`.
    pub fn rect(width: f64, height: f64) -> GeomPath {
        GeomPath::polygon(&[(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)])
    }

    /// A closed diamond touching the middle of each side of the box.
    pub fn diamond(width: f64, height: f64) -> GeomPath {
        GeomPath::polygon(&[(width / 2.0, 0.0), (width, height / 2.0), (width / 2.0, height), (0.0, height / 2.0)])
    }

    /// The ellipse inscribed in the box.
    pub fn ellipse(width: f64, height: f64) -> GeomPath {
        let centre = (width / 2.0, height / 2.0);
        GeomPath { segments: vec![Segment::Ellipse { centre, a: (width, height / 2.0), b: (width / 2.0, 0.0) }], closed: true, ..GeomPath::default() }
    }

    /// A closed polygon through `points`.
    pub fn polygon(points: &[Point]) -> GeomPath {
        let mut segments = Vec::with_capacity(points.len() + 1);
        if let Some(&first) = points.first() {
            segments.push(Segment::MoveTo(first));
            segments.extend(points.iter().skip(1).map(|&p| Segment::LineTo(p)));
            segments.push(Segment::LineTo(first));
        }
        GeomPath { segments, closed: true, ..GeomPath::default() }
    }

    /// The box around every point the path names: ends, arc points, Bezier control points and
    /// ellipse extremes, as `(min_x, min_y, max_x, max_y)`. Arcs bulging past their three points
    /// can reach a little outside it. None for an empty path.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut add = |(x, y): Point| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for s in &self.segments {
            match *s {
                Segment::MoveTo(p) | Segment::LineTo(p) => add(p),
                Segment::CubicTo { c1, c2, to } => [c1, c2, to].into_iter().for_each(&mut add),
                Segment::ArcTo { to, via, .. } => [via, to].into_iter().for_each(&mut add),
                Segment::Ellipse { centre, a, b } => {
                    let (u, v) = ((a.0 - centre.0, a.1 - centre.1), (b.0 - centre.0, b.1 - centre.1));
                    let (hx, hy) = (u.0.hypot(v.0), u.1.hypot(v.1));
                    add((centre.0 - hx, centre.1 - hy));
                    add((centre.0 + hx, centre.1 + hy));
                }
            }
        }
        b
    }
}
