# craft-mermaid

A bounded, hand-written parser for Mermaid flowchart text. No `unsafe` and no dependencies.

`parse(text)` reads `flowchart` and `graph` diagrams (a subset of the documented flowchart
syntax: directions, node shapes including the `@{ ... }` form, links with strokes, heads, labels
and chaining, `&` groups, subgraphs, `style`, `classDef`, `class`, `linkStyle`, `click`,
comments, front matter and `%%{init}%%` directives) and returns plain data. Laying out and
drawing the result is the caller's work.

## What it returns

`parse` gives `Result<Diagram, ParseError>`:

- `Diagram::Flowchart(Flowchart)`: direction, nodes, edges, subgraphs, class definitions,
  `notes` for consumed lines (`click`, edge properties) and `warnings`. Edges and subgraph
  members refer to nodes by index; `Flowchart::node_style` gives a node's style after
  classes and `style` lines are applied.
- `Diagram::Unsupported { kind }`: the header names another Mermaid diagram kind, such as
  `sequenceDiagram` or `pie`. The kind is reported, not parsed.
- `ParseError`: the input exceeds a limit, or has no readable header.

## Nothing is dropped silently

Unsupported diagram kinds come back as `Unsupported`. Statements the parser does not
understand, unknown shape names, bad style targets, unterminated labels and misplaced `end`
lines become a `Warning` carrying the line number, and parsing continues. Choices the
documentation leaves open are lenient and listed in the crate docs.

## Limits

`parse_with_limits` takes a `Limits`; `parse` uses the defaults: 1 MiB of input, 5,000 nodes
plus subgraphs, 20,000 edges (counted after `&` expansion) and 64 levels of subgraph nesting.
Going over a limit is a `ParseError`, not a truncation. Warnings are capped at 1,000 and style
values are bounded in size. Nesting is handled without recursion, and a hostile-input sweep in
the tests checks that nothing panics.

## Origin

Written clean room from the public Mermaid documentation. `PROVENANCE.md` lists what was
read and what was not.
