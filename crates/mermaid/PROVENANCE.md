# Where the syntax comes from

The parser reads the flowchart syntax as the public Mermaid documentation describes it.

## What was read

Documentation pages only, on 2026-11-21, at the version the site showed then (Mermaid 12.3.0):

- https://mermaid.js.org/syntax/flowchart.html: orientation, node shapes, expanded node shapes
  (11.3.0), special characters and Markdown strings, links between nodes (types, text on links,
  chaining, edge ids and animation), subgraphs, styling and classes (`style`, `classDef`, `class`,
  `linkStyle`), interaction (`click`) and comments
- https://mermaid.js.org/intro/syntax-reference.html: comments and the diagram header
- https://mermaid.js.org/config/directives.html: the `%%{init}%%` directive
- https://mermaid.js.org/intro/ and the per-diagram pages (`stateDiagram`, `pie`, `mindmap`,
  `entityRelationshipDiagram`, `timeline`, `sankey`, `block`, `c4`, `requirementDiagram`,
  `architecture`, `kanban`, `packet`) for the header keywords that report `Diagram::Unsupported`
- https://github.com/mermaid-js/mermaid/blob/develop/LICENSE: the licence text only (MIT)

The pages were read through a summarising fetch tool, so the exact wording was not always seen.
Where the documentation is silent or unclear, the parser's choice is listed in the crate docs
under "Choices where the documentation is silent" and is not claimed as Mermaid's behaviour.

## What was not read

No source, grammar, test or lexer of mermaid-js, and no other Mermaid parser or renderer in any
language. No output of Mermaid was observed to settle a question. No documentation text or
example is copied.

## Test inputs

Every test input in `tests/it/main.rs` was written by us from the documented syntax. The first 40 tests
follow the order of the sections of the flowchart page; the later tests cover
choices the documentation leaves open, limits and hostile input. No fixture file is used.
