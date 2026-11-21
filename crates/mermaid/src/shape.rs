//! Node shapes: the classic bracket forms and the `@{ shape: ... }` names.

/// A node shape. Aliases of the `@{ shape: ... }` form are folded into one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    /// `[t]`, `rect`, `proc`, `process`, `rectangle`.
    Rect,
    /// `(t)`, `rounded`, `event`.
    Rounded,
    /// `([t])`, `stadium`, `pill`, `terminal`.
    Stadium,
    /// `[[t]]`, `fr-rect`, `subproc`, `subprocess`, `subroutine`.
    Subroutine,
    /// `[(t)]`, `cyl`, `cylinder`, `database`, `db`.
    Cylinder,
    /// `((t))`, `circle`, `circ`.
    Circle,
    /// `sm-circ`, `small-circle`, `start`.
    SmallCircle,
    /// `(((t)))`, `dbl-circ`, `double-circle`.
    DoubleCircle,
    /// `fr-circ`, `framed-circle`, `stop`.
    FramedCircle,
    /// `f-circ`, `filled-circle`, `junction`.
    FilledCircle,
    /// `cross-circ`, `crossed-circle`, `summary`.
    CrossedCircle,
    /// `{t}`, `diam`, `decision`, `diamond`, `question`.
    Diamond,
    /// `{{t}}`, `hex`, `hexagon`, `prepare`.
    Hexagon,
    /// `>t]`, `odd`.
    Asymmetric,
    /// `[/t/]`, `lean-r`, `in-out`, `lean-right`.
    LeanRight,
    /// `[\t\]`, `lean-l`, `lean-left`, `out-in`.
    LeanLeft,
    /// `[/t\]`, wide at the bottom: `trap-b`, `priority`, `trapezoid`, `trapezoid-bottom`.
    TrapezoidBottom,
    /// `[\t/]`, wide at the top: `trap-t`, `inv-trapezoid`, `manual`, `trapezoid-top`.
    TrapezoidTop,
    /// `sl-rect`, `manual-input`, `sloped-rectangle`.
    SlopedRect,
    /// `notch-rect`, `card`, `notched-rectangle`.
    NotchedRect,
    /// `notch-pent`, `loop-limit`, `notched-pentagon`.
    NotchedPentagon,
    /// `div-rect`, `div-proc`, `divided-process`, `divided-rectangle`.
    DividedRect,
    /// `lin-rect`, `lin-proc`, `lined-process`, `lined-rectangle`, `shaded-process`.
    LinedRect,
    /// `st-rect`, `processes`, `procs`, `stacked-rectangle`.
    StackedRect,
    /// `tag-rect`, `tag-proc`, `tagged-process`, `tagged-rectangle`.
    TaggedRect,
    /// `bow-rect`, `bow-tie-rectangle`, `stored-data`.
    BowTieRect,
    /// `win-pane`, `internal-storage`, `window-pane`.
    WindowPane,
    /// `delay`, `half-rounded-rectangle`.
    Delay,
    /// `h-cyl`, `das`, `horizontal-cylinder`.
    HorizontalCylinder,
    /// `lin-cyl`, `disk`, `lined-cylinder`.
    LinedCylinder,
    /// `curv-trap`, `curved-trapezoid`, `display`.
    CurvedTrapezoid,
    /// `doc`, `document`.
    Document,
    /// `lin-doc`, `lined-document`.
    LinedDocument,
    /// `docs`, `documents`, `st-doc`, `stacked-document`.
    StackedDocument,
    /// `tag-doc`, `tagged-document`.
    TaggedDocument,
    /// `tri`, `extract`, `triangle`.
    Triangle,
    /// `flip-tri`, `flipped-triangle`, `manual-file`.
    FlippedTriangle,
    /// `hourglass`, `collate`.
    Hourglass,
    /// `bolt`, `com-link`, `lightning-bolt`.
    LightningBolt,
    /// `brace`, `brace-l`, `comment`.
    BraceLeft,
    /// `brace-r`.
    BraceRight,
    /// `braces`.
    Braces,
    /// `flag`, `paper-tape`.
    Flag,
    /// `fork`, `join`.
    Fork,
    /// `bang`.
    Bang,
    /// `cloud`.
    Cloud,
    /// `datastore`, `data-store`.
    DataStore,
    /// `folder`, `directory`.
    Folder,
    /// `browser`.
    Browser,
    /// `console`.
    Console,
    /// `bucket`.
    Bucket,
    /// `person`.
    Person,
    /// `text`.
    Text,
    /// `@{ icon: ... }` without a shape: a labelled placeholder (the icon name is not kept).
    Icon,
    /// `@{ img: ... }` without a shape: a labelled placeholder (the URL is not kept).
    Image,
}

/// Looks up a `@{ shape: NAME }` name (ASCII case-insensitive).
pub(crate) fn shape_by_name(name: &str) -> Option<Shape> {
    use Shape as S;
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "rect" | "proc" | "process" | "rectangle" => S::Rect,
        "rounded" | "event" => S::Rounded,
        "stadium" | "pill" | "terminal" => S::Stadium,
        "fr-rect" | "framed-rectangle" | "subproc" | "subprocess" | "subroutine" => S::Subroutine,
        "cyl" | "cylinder" | "database" | "db" => S::Cylinder,
        "circle" | "circ" => S::Circle,
        "sm-circ" | "small-circle" | "start" => S::SmallCircle,
        "dbl-circ" | "double-circle" => S::DoubleCircle,
        "fr-circ" | "framed-circle" | "stop" => S::FramedCircle,
        "f-circ" | "filled-circle" | "junction" => S::FilledCircle,
        "cross-circ" | "crossed-circle" | "summary" => S::CrossedCircle,
        "diam" | "decision" | "diamond" | "question" => S::Diamond,
        "hex" | "hexagon" | "prepare" => S::Hexagon,
        "odd" => S::Asymmetric,
        "lean-r" | "in-out" | "lean-right" => S::LeanRight,
        "lean-l" | "lean-left" | "out-in" => S::LeanLeft,
        "trap-b" | "priority" | "trapezoid" | "trapezoid-bottom" => S::TrapezoidBottom,
        "trap-t" | "inv-trapezoid" | "manual" | "trapezoid-top" => S::TrapezoidTop,
        "sl-rect" | "manual-input" | "sloped-rectangle" => S::SlopedRect,
        "notch-rect" | "card" | "notched-rectangle" => S::NotchedRect,
        "notch-pent" | "loop-limit" | "notched-pentagon" => S::NotchedPentagon,
        "div-rect" | "div-proc" | "divided-process" | "divided-rectangle" => S::DividedRect,
        "lin-rect" | "lin-proc" | "lined-process" | "lined-rectangle" | "shaded-process" => S::LinedRect,
        "st-rect" | "processes" | "procs" | "stacked-rectangle" => S::StackedRect,
        "tag-rect" | "tag-proc" | "tagged-process" | "tagged-rectangle" => S::TaggedRect,
        "bow-rect" | "bow-tie-rectangle" | "stored-data" => S::BowTieRect,
        "win-pane" | "internal-storage" | "window-pane" => S::WindowPane,
        "delay" | "half-rounded-rectangle" => S::Delay,
        "h-cyl" | "das" | "horizontal-cylinder" => S::HorizontalCylinder,
        "lin-cyl" | "disk" | "lined-cylinder" => S::LinedCylinder,
        "curv-trap" | "curved-trapezoid" | "display" => S::CurvedTrapezoid,
        "doc" | "document" => S::Document,
        "lin-doc" | "lined-document" => S::LinedDocument,
        "docs" | "documents" | "st-doc" | "stacked-document" => S::StackedDocument,
        "tag-doc" | "tagged-document" => S::TaggedDocument,
        "tri" | "extract" | "triangle" => S::Triangle,
        "flip-tri" | "flipped-triangle" | "manual-file" => S::FlippedTriangle,
        "hourglass" | "collate" => S::Hourglass,
        "bolt" | "com-link" | "lightning-bolt" => S::LightningBolt,
        "brace" | "brace-l" | "comment" => S::BraceLeft,
        "brace-r" => S::BraceRight,
        "braces" => S::Braces,
        "flag" | "paper-tape" => S::Flag,
        "fork" | "join" => S::Fork,
        "bang" => S::Bang,
        "cloud" => S::Cloud,
        "datastore" | "data-store" => S::DataStore,
        "folder" | "directory" => S::Folder,
        "browser" => S::Browser,
        "console" => S::Console,
        "bucket" => S::Bucket,
        "person" => S::Person,
        "text" => S::Text,
        _ => return None,
    })
}

/// One classic bracket form: its opener and the closers that may end it.
pub(crate) struct Bracket {
    pub open: &'static str,
    pub closers: &'static [(&'static str, Shape)],
}

/// Classic bracket forms, longest opener first.
pub(crate) const BRACKETS: &[Bracket] = &[
    Bracket { open: "(((", closers: &[(")))", Shape::DoubleCircle)] },
    Bracket { open: "((", closers: &[("))", Shape::Circle)] },
    Bracket { open: "([", closers: &[("])", Shape::Stadium)] },
    Bracket { open: "(", closers: &[(")", Shape::Rounded)] },
    Bracket { open: "[[", closers: &[("]]", Shape::Subroutine)] },
    Bracket { open: "[(", closers: &[(")]", Shape::Cylinder)] },
    Bracket { open: "[/", closers: &[("/]", Shape::LeanRight), ("\\]", Shape::TrapezoidBottom)] },
    Bracket { open: "[\\", closers: &[("\\]", Shape::LeanLeft), ("/]", Shape::TrapezoidTop)] },
    Bracket { open: "[", closers: &[("]", Shape::Rect)] },
    Bracket { open: "{{", closers: &[("}}", Shape::Hexagon)] },
    Bracket { open: "{", closers: &[("}", Shape::Diamond)] },
    Bracket { open: ">", closers: &[("]", Shape::Asymmetric)] },
];
