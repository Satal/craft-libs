# craft-vsdx

VSDX drawings and VSSX stencils, read and written on `craft-opc` and `craft-xml`, from the
public specification [MS-VSDX] v20250819 only (clean room: no other implementation was read).

| Item | What |
|---|---|
| `read(bytes) -> Result<(Drawing, Vec<Warning>), Error>` | pages, flattened shapes and 1-D connectors; `read_with` takes `Limits` |
| `read_stencil(bytes) -> Result<(Stencil, Vec<Warning>), Error>` | the masters of a stencil: names, shapes, PNG icon; `read_stencil_with` takes `Limits` |
| `write(&Drawing) -> Result<Vec<u8>, Error>` | the minimal package of §3.1 |

The model is in points, y down, origin at the page's top-left. Shapes carry their unrotated
box, a clockwise angle in degrees, geometry paths in box space (move, line, cubic, elliptical
arc, ellipse), text with newlines, fill, line and text colours, and connection points.
Connectors carry their route, their glue (`ConnectionPoint::Shape` or `Point(n)`), arrowhead
indices and line.

Reading resolves parts through relationships, applies master inheritance (§2.2.5.4) and style
sheets, uses cached `V` values only, composes the local-to-parent transform (§2.2.3.2.1)
through groups, and reports what it cannot draw as a `Warning`: NURBS, polyline and spline rows
(drawn as their chord), infinite lines, pattern fills, theme colours, unknown units, bad
numbers, foreign shapes, guides, missing masters and cut-off groups.

Not supported: formula evaluation, themes, text formatting beyond the first run's colour,
layers, hyperlinks, data, images inside foreign shapes, and the binary `.vsd` format.

The choices made where the spec is silent (O1–O8 and the conflicts c1–c4) are listed in the
crate documentation (`src/lib.rs`).

`cargo run -p craft-vsdx --example roundtrip -- out.vsdx` writes a sample flowchart.

[MS-VSDX]: https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/
