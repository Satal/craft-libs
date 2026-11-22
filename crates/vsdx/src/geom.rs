//! Transforms (MS-VSDX §2.2.3.2.1) and Geometry sections (§2.2.3.2.2, §2.4.2).

use crate::WarningKind;
use crate::model::{GeomPath, Point, Segment};
use crate::sheet::{Row, Section, Sheet};
use crate::values::{MAX_LENGTH, Sink, angle, length, scalar};

/// Points per inch.
pub(crate) const PT: f64 = 72.0;

/// `(x, y) -> (a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn apply(&self, (x, y): Point) -> Point {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// `outer` after `self`.
    pub fn then(&self, outer: &Affine) -> Affine {
        let (o, s) = (outer, self);
        Affine {
            a: o.a * s.a + o.c * s.b,
            b: o.b * s.a + o.d * s.b,
            c: o.a * s.c + o.c * s.d,
            d: o.b * s.c + o.d * s.d,
            e: o.a * s.e + o.c * s.f + o.e,
            f: o.b * s.e + o.d * s.f + o.f,
        }
    }

    /// The local-to-parent transform of §2.2.3.2.1: subtract the local pin, mirror for the
    /// flips, rotate counter-clockwise by `angle`, add the pin.
    pub fn local_to_parent(pin: Point, loc: Point, angle: f64, flip_x: bool, flip_y: bool) -> Affine {
        let (sin, cos) = angle.sin_cos();
        let fx = if flip_x { -1.0 } else { 1.0 };
        let fy = if flip_y { -1.0 } else { 1.0 };
        let (a, b, c, d) = (cos * fx, sin * fx, -sin * fy, cos * fy);
        Affine { a, b, c, d, e: pin.0 - (a * loc.0 + c * loc.1), f: pin.1 - (b * loc.0 + d * loc.1) }
    }

    /// The rotation (counter-clockwise radians) and whether x is mirrored, of a linear part
    /// made of rotations and mirrors: `R(angle)` or `R(angle) * diag(-1, 1)`.
    pub fn rotation(&self) -> (f64, bool) {
        if self.a * self.d - self.b * self.c < 0.0 { ((-self.b).atan2(-self.a), true) } else { (self.b.atan2(self.a), false) }
    }
}

/// Where geometry is being read, for warnings and limits.
pub(crate) struct Ctx<'a> {
    pub sink: &'a mut Sink,
    pub place: &'a str,
    pub max_rows: usize,
}

/// One path in the shape's own space (inches, y up); arc rotations are counter-clockwise
/// radians.
struct Src {
    segments: Vec<Segment>,
    no_fill: bool,
    no_line: bool,
}

fn flag(s: &Section, n: &str, ctx: &mut Ctx) -> bool {
    scalar(s.cells.get(n), ctx.sink, ctx.place, n).is_some_and(|v| v != 0.0)
}

fn len(r: &Row, n: &str, ctx: &mut Ctx) -> f64 {
    length(r.cells.get(n), ctx.sink, ctx.place, n).unwrap_or(0.0)
}

fn rel(r: &Row, n: &str, size: f64, ctx: &mut Ctx) -> f64 {
    (scalar(r.cells.get(n), ctx.sink, ctx.place, n).unwrap_or(0.0) * size).clamp(-MAX_LENGTH, MAX_LENGTH)
}

/// The bow of an ArcTo as a point on the arc: `bow` to the left of the chord's middle.
fn bow_point(from: Point, to: Point, bow: f64) -> Point {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let l = dx.hypot(dy);
    let m = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    if l <= 0.0 { m } else { (m.0 - dy / l * bow, m.1 + dx / l * bow) }
}

/// What a row becomes, with the current point.
fn row_segment(r: &Row, t: &str, cur: Point, size: (f64, f64), ctx: &mut Ctx) -> Option<Segment> {
    let (w, h) = size;
    let abs = |ctx: &mut Ctx, x: &str, y: &str| (len(r, x, ctx), len(r, y, ctx));
    let relp = |ctx: &mut Ctx, x: &str, y: &str| (rel(r, x, w, ctx), rel(r, y, h, ctx));
    Some(match t {
        "MoveTo" => Segment::MoveTo(abs(ctx, "X", "Y")),
        "RelMoveTo" => Segment::MoveTo(relp(ctx, "X", "Y")),
        "LineTo" => Segment::LineTo(abs(ctx, "X", "Y")),
        "RelLineTo" => Segment::LineTo(relp(ctx, "X", "Y")),
        "ArcTo" => {
            let to = abs(ctx, "X", "Y");
            let bow = len(r, "A", ctx);
            Segment::ArcTo { to, via: bow_point(cur, to, bow), rotation: 0.0, ratio: 1.0 }
        }
        "EllipticalArcTo" | "RelEllipticalArcTo" => {
            let relative = t.starts_with("Rel");
            let to = if relative { relp(ctx, "X", "Y") } else { abs(ctx, "X", "Y") };
            let via = if relative { relp(ctx, "A", "B") } else { abs(ctx, "A", "B") };
            let rotation = angle(r.cells.get("C"), ctx.sink, ctx.place, "C").unwrap_or(0.0);
            let ratio = scalar(r.cells.get("D"), ctx.sink, ctx.place, "D").unwrap_or(1.0);
            Segment::ArcTo { to, via, rotation, ratio }
        }
        "Ellipse" => Segment::Ellipse { centre: abs(ctx, "X", "Y"), a: abs(ctx, "A", "B"), b: abs(ctx, "C", "D") },
        "RelCubBezTo" => Segment::CubicTo { c1: relp(ctx, "A", "B"), c2: relp(ctx, "C", "D"), to: relp(ctx, "X", "Y") },
        "RelQuadBezTo" => {
            let (q, to) = (relp(ctx, "A", "B"), relp(ctx, "X", "Y"));
            let c1 = (cur.0 + 2.0 / 3.0 * (q.0 - cur.0), cur.1 + 2.0 / 3.0 * (q.1 - cur.1));
            let c2 = (to.0 + 2.0 / 3.0 * (q.0 - to.0), to.1 + 2.0 / 3.0 * (q.1 - to.1));
            Segment::CubicTo { c1, c2, to }
        }
        "NURBSTo" => {
            let to = abs(ctx, "X", "Y");
            ctx.sink.push(WarningKind::Geometry, ctx.place, "NURBSTo drawn as a cubic along its chord: its control points are in a formula");
            let c1 = (cur.0 + (to.0 - cur.0) / 3.0, cur.1 + (to.1 - cur.1) / 3.0);
            let c2 = (cur.0 + 2.0 * (to.0 - cur.0) / 3.0, cur.1 + 2.0 * (to.1 - cur.1) / 3.0);
            Segment::CubicTo { c1, c2, to }
        }
        "PolylineTo" | "SplineStart" | "SplineKnot" => {
            ctx.sink.push(WarningKind::Geometry, ctx.place, format!("{t} drawn as a straight line to its end point"));
            Segment::LineTo(abs(ctx, "X", "Y"))
        }
        other => {
            let shown: String = other.chars().take(32).collect();
            ctx.sink.push(WarningKind::Geometry, ctx.place, format!("geometry row {shown:?} left out"));
            return None;
        }
    })
}

fn end_of(s: &Segment) -> Option<Point> {
    match *s {
        Segment::MoveTo(p) | Segment::LineTo(p) => Some(p),
        Segment::CubicTo { to, .. } | Segment::ArcTo { to, .. } => Some(to),
        Segment::Ellipse { .. } => None,
    }
}

fn source_path(sec: &Section, size: (f64, f64), ctx: &mut Ctx) -> Option<Src> {
    if flag(sec, "NoShow", ctx) {
        return None;
    }
    let mut out = Src { segments: Vec::new(), no_fill: flag(sec, "NoFill", ctx), no_line: flag(sec, "NoLine", ctx) };
    if sec.rows.len() > ctx.max_rows {
        ctx.sink.push(WarningKind::Limit, ctx.place, format!("geometry rows past {} left out", ctx.max_rows));
    }
    let mut cur = (0.0, 0.0);
    for r in sec.rows.values().take(ctx.max_rows) {
        let Some(t) = r.t.as_deref() else { continue };
        if t == "InfiniteLine" {
            ctx.sink.push(WarningKind::Geometry, ctx.place, "InfiniteLine left out");
            continue;
        }
        if let Some(seg) = row_segment(r, t, cur, size, ctx) {
            cur = end_of(&seg).unwrap_or(cur);
            out.segments.push(seg);
        }
    }
    Some(out)
}

/// The geometry of a sheet in the model's box space: points, y down from the top of a box
/// `size` inches, mirrored in x when `mirror`.
pub(crate) fn paths(sheet: &Sheet, size: (f64, f64), mirror: bool, ctx: &mut Ctx) -> Vec<GeomPath> {
    let (w, h) = size;
    let map = |(x, y): Point| ((if mirror { w - x } else { x }) * PT, (h - y) * PT);
    let rot = |r: f64| if mirror { r.to_degrees() } else { -r.to_degrees() };
    let mut out = Vec::new();
    for sec in sheet.geometry() {
        let Some(src) = source_path(sec, size, ctx) else { continue };
        let segments: Vec<Segment> = src
            .segments
            .iter()
            .map(|s| match *s {
                Segment::MoveTo(p) => Segment::MoveTo(map(p)),
                Segment::LineTo(p) => Segment::LineTo(map(p)),
                Segment::CubicTo { c1, c2, to } => Segment::CubicTo { c1: map(c1), c2: map(c2), to: map(to) },
                Segment::ArcTo { to, via, rotation, ratio } => Segment::ArcTo { to: map(to), via: map(via), rotation: rot(rotation), ratio },
                Segment::Ellipse { centre, a, b } => Segment::Ellipse { centre: map(centre), a: map(a), b: map(b) },
            })
            .collect();
        if segments.is_empty() {
            continue;
        }
        let closed = is_closed(&segments);
        out.push(GeomPath { segments, closed, no_fill: src.no_fill, no_line: src.no_line });
    }
    out
}

fn is_closed(segments: &[Segment]) -> bool {
    if segments.iter().any(|s| matches!(s, Segment::Ellipse { .. })) {
        return true;
    }
    let first = segments.iter().find_map(|s| if let Segment::MoveTo(p) = s { Some(*p) } else { None });
    match (first, segments.last().and_then(end_of)) {
        (Some(a), Some(b)) if segments.len() > 2 => (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
        _ => false,
    }
}

/// The vertices of the first shown Geometry section, in the sheet's own space (inches, y up):
/// a connector's route.
pub(crate) fn vertices(sheet: &Sheet, size: (f64, f64), ctx: &mut Ctx) -> Vec<Point> {
    for sec in sheet.geometry() {
        if let Some(src) = source_path(sec, size, ctx) {
            return src.segments.iter().filter_map(end_of).collect();
        }
    }
    Vec::new()
}
