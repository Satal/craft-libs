# Where the format comes from

The reader and writer follow one public specification:

- [MS-VSDX], *VSDX File Format*, version 20250819, published by Microsoft at
  https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/ . The code cites its
  sections (for example §2.2.3.2.1 transforms, §2.2.3.2.2 geometry, §3.1 the minimal package).
  The drawing package structure rests on the Open Packaging Conventions through `craft-opc`.

Points the specification leaves open (arrowhead indices, required parts, stencil content types,
line breaks in text, glue formulas, 1-D shape boxes, master icons) are listed as choices O1 to
O8 in the crate docs. They are choices, not claims about any other application.

## What was not read

No other implementation of the format was read: not libvisio, not LibreOffice's import or export
code, not MisterDiagram, not any other reader, writer or converter, in any language. No other
description of the format was used. The writer's output was once opened in LibreOffice Draw as a
black box to see that an arrowhead draws; no source or file internals of it were read.

## Test inputs

Every test package is built in the tests by the crate's own writer helpers or by hand-written
XML from the specification's element names. No drawing or stencil file made by another
application is used.

[MS-VSDX]: https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/
