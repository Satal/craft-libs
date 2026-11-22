//! Relation tests for the flowchart parser: inputs written by us from the public flowchart
//! documentation <https://mermaid.js.org/syntax/flowchart.html> (the first 40 tests, in
//! the order of its sections: header, nodes and shapes, links, subgraphs, styling, interaction),
//! rules the documentation leaves open, and a hostile-input sweep. See `PROVENANCE.md`.

use craft_mermaid::{Diagram, Direction, Edge, Endpoint, Flowchart, Head, Limits, ParseError, Shape, Stroke, parse, parse_with_limits};

fn fc(src: &str) -> Flowchart {
    match parse(src) {
        Ok(Diagram::Flowchart(f)) => f,
        other => panic!("expected a flowchart for {src:?}, got {other:?}"),
    }
}

fn ids(f: &Flowchart) -> Vec<&str> {
    f.nodes.iter().map(|n| n.id.as_str()).collect()
}

fn node<'a>(f: &'a Flowchart, id: &str) -> &'a craft_mermaid::Node {
    f.nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("no node {id}"))
}

fn idx(f: &Flowchart, id: &str) -> usize {
    f.nodes.iter().position(|n| n.id == id).unwrap_or_else(|| panic!("no node {id}"))
}

fn name(f: &Flowchart, e: Endpoint) -> &str {
    match e {
        Endpoint::Node(i) => &f.nodes[i].id,
        Endpoint::Subgraph(i) => &f.subgraphs[i].id,
    }
}

fn pairs(f: &Flowchart) -> Vec<(&str, &str)> {
    f.edges.iter().map(|e| (name(f, e.from), name(f, e.to))).collect()
}

fn edge<'a>(f: &'a Flowchart, a: &str, b: &str) -> &'a Edge {
    f.edges.iter().find(|e| name(f, e.from) == a && name(f, e.to) == b).unwrap_or_else(|| panic!("no edge {a}->{b}"))
}

fn shapes(f: &Flowchart) -> Vec<Shape> {
    f.nodes.iter().map(|n| n.shape).collect()
}

fn sg<'a>(f: &'a Flowchart, id: &str) -> &'a craft_mermaid::Subgraph {
    f.subgraphs.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no subgraph {id}"))
}

fn members<'a>(f: &'a Flowchart, id: &str) -> Vec<&'a str> {
    sg(f, id).members.iter().map(|&i| f.nodes[i].id.as_str()).collect()
}

#[test]
fn header_direction_and_single_edge_parse_with_default_shapes() {
    let f = fc("flowchart LR\nA --> B");
    assert_eq!(f.direction, Direction::LeftToRight);
    assert_eq!(ids(&f), ["A", "B"]);
    assert_eq!(shapes(&f), [Shape::Rect, Shape::Rect]);
    let e = edge(&f, "A", "B");
    assert_eq!((e.stroke, e.start_head, e.end_head), (Stroke::Solid, Head::None, Head::Arrow));
    assert!(f.warnings.is_empty());
}

#[test]
fn graph_keyword_td_and_missing_direction_mean_top_to_bottom() {
    let f = fc("graph TD\nA-->B");
    assert_eq!(f.direction, Direction::TopToBottom);
    assert_eq!(pairs(&f), [("A", "B")]);
    assert_eq!(fc("graph\nA-->B").direction, Direction::TopToBottom);
}

#[test]
fn semicolons_separate_statements_on_one_line() {
    let f = fc("flowchart TB;A;B;A-->B;");
    assert_eq!(ids(&f), ["A", "B"]);
    assert_eq!(pairs(&f), [("A", "B")]);
    assert!(f.warnings.is_empty());
}

#[test]
fn front_matter_block_is_skipped_before_the_header() {
    let f = fc("---\ntitle: Demo\nconfig:\n  theme: dark\n---\nflowchart BT\nX-->Y");
    assert_eq!(f.direction, Direction::BottomToTop);
    assert_eq!(pairs(&f), [("X", "Y")]);
    assert!(f.warnings.is_empty());
}

#[test]
fn comment_lines_are_ignored_and_define_no_nodes() {
    let f = fc("flowchart RL\n%% a comment A --> Z\nA --> B");
    assert_eq!(f.direction, Direction::RightToLeft);
    assert_eq!(pairs(&f), [("A", "B")]);
    assert!(!ids(&f).contains(&"Z"));
}

#[test]
fn init_directive_is_skipped_including_across_lines() {
    let f = fc("%%{init: {\"theme\":\"forest\"}}%%\nflowchart LR\nA-->B");
    assert_eq!(pairs(&f), [("A", "B")]);
    let g = fc("flowchart LR\n%%{\n  init: {\"theme\": \"dark\"}\n}%%\nA-->B");
    assert_eq!(pairs(&g), [("A", "B")]);
    assert!(f.warnings.is_empty() && g.warnings.is_empty());
}

#[test]
fn round_family_shapes_map_to_their_variants() {
    let f = fc("flowchart LR\nA[Rect] B(Round) C([Stad]) D[[Sub]] E[(DB)] F((Circ))");
    assert_eq!(shapes(&f), [Shape::Rect, Shape::Rounded, Shape::Stadium, Shape::Subroutine, Shape::Cylinder, Shape::Circle]);
    assert_eq!(node(&f, "C").label, "Stad");
    assert!(f.warnings.is_empty());
}

#[test]
fn flag_decision_hexagon_and_double_circle_shapes_map_to_their_variants() {
    let f = fc("flowchart LR\nA>Flag] B{Dec} C{{Hex}} D(((Dbl)))");
    assert_eq!(shapes(&f), [Shape::Asymmetric, Shape::Diamond, Shape::Hexagon, Shape::DoubleCircle]);
    assert_eq!(node(&f, "D").label, "Dbl");
}

#[test]
fn slanted_shapes_keep_their_orientation() {
    let f = fc("flowchart LR\nA[/P1/] B[\\P2\\] C[/T1\\] D[\\T2/]");
    assert_eq!(shapes(&f), [Shape::LeanRight, Shape::LeanLeft, Shape::TrapezoidBottom, Shape::TrapezoidTop]);
    assert_eq!(node(&f, "B").label, "P2");
}

#[test]
fn quoted_label_may_hold_brackets() {
    let f = fc("flowchart LR\nA[\"Quoted (with) {chars}\"]");
    assert_eq!(node(&f, "A").label, "Quoted (with) {chars}");
    assert!(f.warnings.is_empty());
}

#[test]
fn end_inside_quotes_is_a_label_not_a_subgraph_close() {
    let f = fc("flowchart LR\nA[\"End of line\"] --> B[\"end\"]");
    assert_eq!(node(&f, "A").label, "End of line");
    assert_eq!(node(&f, "B").label, "end");
    assert!(f.subgraphs.is_empty() && f.warnings.is_empty());
}

#[test]
fn br_tags_in_labels_become_newlines() {
    for br in ["<br/>", "<br>", "<BR />"] {
        let f = fc(&format!("flowchart LR\nA[\"a{br}b\"]"));
        assert_eq!(node(&f, "A").label, "a\nb", "{br}");
    }
}

#[test]
fn entity_codes_in_labels_are_decoded() {
    let f = fc("flowchart LR\nA[\"quote #quot;x#quot; and #9829;\"]");
    assert_eq!(node(&f, "A").label, "quote \"x\" and \u{2665}");
    assert!(f.warnings.is_empty());
    assert_eq!(node(&fc("flowchart LR\nA[\"#35; #x41; #nosuch;\"]"), "A").label, "# A #nosuch;");
}

#[test]
fn markdown_label_keeps_real_newlines_and_drops_backticks() {
    let f = fc("flowchart LR\nA[\"`**Bold** line\nsecond`\"] --> B");
    let a = node(&f, "A");
    assert!(a.markdown);
    assert!(a.label.contains("Bold") && a.label.contains("line\nsecond"));
    assert!(!a.label.contains('`'));
    assert!(!node(&f, "B").markdown);
    assert_eq!(pairs(&f), [("A", "B")]);
}

#[test]
fn extended_shape_syntax_sets_shape_and_label() {
    let f = fc("flowchart LR\nA@{ shape: diamond, label: \"Choose\" }");
    assert_eq!((node(&f, "A").shape, node(&f, "A").label.as_str()), (Shape::Diamond, "Choose"));
}

#[test]
fn shape_aliases_fold_into_one_variant() {
    let f = fc("flowchart LR\nA@{ shape: cyl }\nB@{ shape: db, label: \"Store\" }\nC@{ shape: question }");
    assert_eq!(shapes(&f), [Shape::Cylinder, Shape::Cylinder, Shape::Diamond]);
    assert_eq!(node(&f, "A").label, "A");
    assert_eq!(node(&f, "B").label, "Store");
}

#[test]
fn unknown_extended_shape_warns_and_falls_back_to_rect() {
    let f = fc("flowchart LR\nA@{ shape: processes }\nB@{ shape: stop }\nC@{ shape: nosuchshape }");
    assert_eq!(shapes(&f), [Shape::StackedRect, Shape::FramedCircle, Shape::Rect]);
    assert_eq!(f.warnings.len(), 1);
    assert_eq!(f.warnings[0].line, 4);
}

#[test]
fn icon_node_becomes_a_labelled_placeholder() {
    let f = fc("flowchart LR\nA@{ icon: \"fa:user\", form: \"circle\", label: \"Who\", pos: \"t\" }");
    assert_eq!((node(&f, "A").shape, node(&f, "A").label.as_str()), (Shape::Icon, "Who"));
    assert!(f.warnings.is_empty());
}

#[test]
fn link_stroke_kinds_without_heads() {
    let f = fc("flowchart LR\nA --- B -.- C === D ~~~ E");
    let strokes: Vec<_> = f.edges.iter().map(|e| e.stroke).collect();
    assert_eq!(strokes, [Stroke::Solid, Stroke::Dotted, Stroke::Thick, Stroke::Invisible]);
    assert!(f.edges.iter().all(|e| e.end_head == Head::None && e.start_head == Head::None));
}

#[test]
fn link_end_heads_arrow_circle_and_cross() {
    let f = fc("flowchart LR\nA --> B -.-> C ==> D --o E --x F");
    let got: Vec<_> = f.edges.iter().map(|e| (e.stroke, e.end_head)).collect();
    assert_eq!(
        got,
        [
            (Stroke::Solid, Head::Arrow),
            (Stroke::Dotted, Head::Arrow),
            (Stroke::Thick, Head::Arrow),
            (Stroke::Solid, Head::Circle),
            (Stroke::Solid, Head::Cross)
        ]
    );
}

#[test]
fn links_with_heads_at_both_ends() {
    let f = fc("flowchart LR\nA <--> B o--o C x--x D");
    let got: Vec<_> = f.edges.iter().map(|e| (e.start_head, e.end_head)).collect();
    assert_eq!(got, [(Head::Arrow, Head::Arrow), (Head::Circle, Head::Circle), (Head::Cross, Head::Cross)]);
    assert_eq!(f.nodes.len(), 4);
}

#[test]
fn longer_links_span_more_ranks() {
    let f = fc("flowchart LR\nA ----> B ====> C -...-> D\nE --> F\nG --- H\nI ---- J");
    assert!(edge(&f, "A", "B").length > edge(&f, "E", "F").length);
    assert_eq!(edge(&f, "A", "B").end_head, Head::Arrow);
    assert_eq!(edge(&f, "B", "C").stroke, Stroke::Thick);
    assert!(edge(&f, "B", "C").length > 1);
    assert_eq!(edge(&f, "C", "D").stroke, Stroke::Dotted);
    assert!(edge(&f, "C", "D").length > 1);
    assert!(edge(&f, "I", "J").length > edge(&f, "G", "H").length);
}

#[test]
fn edge_labels_in_pipe_and_inline_syntaxes() {
    let f = fc("flowchart LR\nA -->|yes| B\nB -- no --> C\nC -. maybe .-> D\nD == sure ==> E");
    let got: Vec<_> = f.edges.iter().map(|e| (e.label.as_deref(), e.stroke, e.end_head)).collect();
    assert_eq!(
        got,
        [
            (Some("yes"), Stroke::Solid, Head::Arrow),
            (Some("no"), Stroke::Solid, Head::Arrow),
            (Some("maybe"), Stroke::Dotted, Head::Arrow),
            (Some("sure"), Stroke::Thick, Head::Arrow)
        ]
    );
    assert!(f.warnings.is_empty());
}

#[test]
fn inline_label_with_longer_open_link_lengthens_the_edge() {
    let f = fc("flowchart LR\nA -- long label ---- B\nC --> D");
    let e = edge(&f, "A", "B");
    assert_eq!((e.label.as_deref(), e.stroke, e.end_head), (Some("long label"), Stroke::Solid, Head::None));
    assert!(e.length > edge(&f, "C", "D").length);
}

#[test]
fn chained_links_make_one_edge_per_pair() {
    assert_eq!(pairs(&fc("flowchart LR\nA --> B --> C")), [("A", "B"), ("B", "C")]);
}

#[test]
fn ampersand_groups_expand_to_a_product_in_documented_order() {
    assert_eq!(pairs(&fc("flowchart LR\nA & B --> C & D")), [("A", "C"), ("A", "D"), ("B", "C"), ("B", "D")]);
}

#[test]
fn ampersand_inside_a_chain_expands_each_step() {
    assert_eq!(pairs(&fc("flowchart LR\na --> b & c --> d")), [("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")]);
}

#[test]
fn edge_id_is_kept_and_property_line_is_noted_not_warned() {
    let f = fc("flowchart LR\nA e1@--> B\ne1@{ animate: true }");
    assert_eq!(f.edges.len(), 1);
    assert_eq!(f.edges[0].id.as_deref(), Some("e1"));
    assert_eq!(ids(&f), ["A", "B"]);
    assert!(f.warnings.is_empty());
    assert_eq!(f.notes.len(), 1);
}

#[test]
fn subgraph_title_and_edge_to_a_subgraph() {
    let f = fc("flowchart TB\nsubgraph one [First]\n  A --> B\nend\nC --> one");
    let s = sg(&f, "one");
    assert_eq!(s.title, "First");
    assert_eq!(members(&f, "one"), ["A", "B"]);
    assert_eq!(pairs(&f), [("A", "B"), ("C", "one")]);
    assert_eq!(edge(&f, "C", "one").to, Endpoint::Subgraph(0));
    assert!(!ids(&f).contains(&"one"));
    assert!(f.warnings.is_empty());
}

#[test]
fn nested_subgraphs_keep_parent_members_and_direction() {
    let f = fc("flowchart TB\nsubgraph \"Outer\"\n  subgraph inner\n    direction LR\n    X --> Y\n  end\n  Z\nend");
    let outer = sg(&f, "Outer");
    let inner = sg(&f, "inner");
    assert_eq!(outer.title, "Outer");
    assert_eq!(members(&f, "Outer"), ["Z"]);
    assert_eq!(members(&f, "inner"), ["X", "Y"]);
    assert_eq!(inner.parent, Some(0));
    assert_eq!(outer.parent, None);
    assert_eq!(inner.direction, Some(Direction::LeftToRight));
    assert_eq!(outer.direction, None);
    assert!(f.warnings.is_empty());
}

#[test]
fn class_shorthand_applies_classdef_style_to_that_node_only() {
    let f = fc("flowchart LR\nA:::hot --> B\nclassDef hot fill:#f96,stroke:#333,color:#fff");
    assert_eq!(node(&f, "A").classes, ["hot"]);
    assert!(node(&f, "B").classes.is_empty());
    let s = f.node_style(idx(&f, "A"));
    assert_eq!((s.fill.as_deref(), s.stroke.as_deref(), s.color.as_deref()), (Some("#f96"), Some("#333"), Some("#fff")));
    assert!(f.node_style(idx(&f, "B")).is_empty());
    assert!(f.warnings.is_empty());
}

#[test]
fn class_statement_before_its_classdef_still_applies() {
    let f = fc("flowchart LR\nA --> B\nclass A,B big\nclassDef big stroke-width:4px");
    assert_eq!(node(&f, "A").classes, ["big"]);
    assert_eq!(node(&f, "B").classes, ["big"]);
    assert_eq!(f.node_style(0).stroke_width, Some(4.0));
    assert!(f.warnings.is_empty());
}

#[test]
fn default_class_applies_to_all_and_style_line_wins() {
    let f = fc("flowchart LR\nclassDef default fill:#eee\nA --> B\nstyle B fill:#0f0,stroke:#000");
    assert_eq!(f.node_style(idx(&f, "A")).fill.as_deref(), Some("#eee"));
    let b = f.node_style(idx(&f, "B"));
    assert_eq!((b.fill.as_deref(), b.stroke.as_deref()), (Some("#0f0"), Some("#000")));
    assert!(f.warnings.is_empty());
}

#[test]
fn link_style_applies_by_index_and_default() {
    let f = fc("flowchart LR\nA --> B\nB --> C\nlinkStyle 1 stroke:#f00,stroke-width:3px\nlinkStyle default color:blue");
    let (e0, e1) = (&f.edges[0], &f.edges[1]);
    assert_eq!(e1.style.stroke.as_deref(), Some("#f00"));
    assert_eq!(e1.style.stroke_width, Some(3.0));
    assert!(e0.style.stroke.is_none());
    assert_eq!(e0.style.color.as_deref(), Some("blue"));
    assert_eq!(e1.style.color.as_deref(), Some("blue"));
    assert!(f.warnings.is_empty());
}

#[test]
fn click_lines_are_consumed_and_noted_not_warned() {
    let f = fc("flowchart LR\nA --> B\nclick A \"https://example.com\" \"tip\" _blank\nclick B call cb()");
    assert_eq!(ids(&f), ["A", "B"]);
    assert_eq!(f.edges.len(), 1);
    assert!(f.warnings.is_empty());
    assert_eq!(f.notes.len(), 2);
}

#[test]
fn o_and_x_ids_after_spaced_or_capital_links_are_node_ids() {
    let f = fc("flowchart LR\ndev --- ops\nA --- oB\nA---Xb");
    assert_eq!(ids(&f), ["dev", "ops", "A", "oB", "Xb"]);
    assert_eq!(f.edges.len(), 3);
    assert!(f.edges.iter().all(|e| e.stroke == Stroke::Solid && e.end_head == Head::None));
}

#[test]
fn glued_o_after_a_link_is_a_circle_end_head() {
    let f = fc("flowchart LR\nA---oB");
    assert_eq!(ids(&f), ["A", "B"]);
    assert_eq!(edge(&f, "A", "B").end_head, Head::Circle);
}

#[test]
fn other_diagram_kinds_are_reported_as_unsupported() {
    assert_eq!(parse("sequenceDiagram\nA->>B: hi"), Ok(Diagram::Unsupported { kind: "sequenceDiagram".to_owned() }));
    for kind in ["classDiagram", "stateDiagram-v2", "erDiagram", "gantt", "pie", "mindmap", "C4Context", "xychart-beta"] {
        assert_eq!(parse(&format!("%% c\n{kind}\n  x")), Ok(Diagram::Unsupported { kind: kind.to_owned() }));
    }
    assert_eq!(parse("---\ntitle: t\n---\npie title Pets\n\"Dogs\" : 3"), Ok(Diagram::Unsupported { kind: "pie".to_owned() }));
}

#[test]
fn last_label_given_for_a_node_wins() {
    let f = fc("flowchart LR\nA[One]\nA[Two] --> B");
    assert_eq!(ids(&f), ["A", "B"]);
    assert_eq!(node(&f, "A").label, "Two");
}

#[test]
fn blank_lines_and_indentation_are_ignored() {
    let f = fc("flowchart LR\nA[\"x\"] --> B\n\n\n   C --> D   \n");
    assert_eq!(ids(&f), ["A", "B", "C", "D"]);
    assert_eq!(pairs(&f), [("A", "B"), ("C", "D")]);
    assert!(f.warnings.is_empty());
}

// ---- rules the documentation leaves open ----

#[test]
fn node_order_is_first_appearance_and_redefinition_keeps_the_node() {
    let f = fc("flowchart LR\nC --> A\nB\nA[Late]");
    assert_eq!(ids(&f), ["C", "A", "B"]);
    assert_eq!(node(&f, "A").label, "Late");
}

#[test]
fn bare_mention_keeps_an_earlier_shape() {
    let f = fc("flowchart LR\nA{Q}\nA --> B");
    assert_eq!(node(&f, "A").shape, Shape::Diamond);
    assert_eq!(node(&f, "A").label, "Q");
}

#[test]
fn style_and_class_may_name_nodes_defined_later() {
    let f = fc("flowchart LR\nstyle Z fill:#123\nclass Z k\nA --> Z");
    assert_eq!(node(&f, "Z").style.fill.as_deref(), Some("#123"));
    assert_eq!(node(&f, "Z").classes, ["k"]);
}

#[test]
fn style_properties_are_typed_and_others_kept_raw() {
    let f = fc("flowchart LR\nA\nstyle A fill:#f9f,stroke:#333,stroke-width:4px,color:#fff,stroke-dasharray:5 5,font-size:12px");
    let s = &node(&f, "A").style;
    assert_eq!(s.stroke_width, Some(4.0));
    assert_eq!(s.stroke_dasharray.as_deref(), Some(&[5.0, 5.0][..]));
    assert_eq!(s.other, [("font-size".to_owned(), "12px".to_owned())]);
    assert_eq!(s.fill.as_deref(), Some("#f9f"));
}

#[test]
fn classdef_with_several_names_and_trailing_semicolon() {
    let f = fc("flowchart LR\nclassDef a,b fill:#111,stroke:#222;\nA:::a --> B:::b");
    assert_eq!(f.class_defs.len(), 2);
    assert_eq!(f.node_style(idx(&f, "B")).stroke.as_deref(), Some("#222"));
    assert!(f.warnings.is_empty());
}

#[test]
fn subgraph_declared_after_the_edge_still_resolves() {
    let f = fc("flowchart LR\nA --> grp\nsubgraph grp\n  B\nend");
    assert_eq!(edge(&f, "A", "grp").to, Endpoint::Subgraph(0));
    assert_eq!(members(&f, "grp"), ["B"]);
}

#[test]
fn edge_may_leave_a_subgraph() {
    let f = fc("flowchart LR\nsubgraph s\n  A\nend\ns --> B");
    assert_eq!(edge(&f, "s", "B").from, Endpoint::Subgraph(0));
}

#[test]
fn node_in_two_unrelated_subgraphs_belongs_to_the_first_with_a_warning() {
    let f = fc("flowchart LR\nsubgraph a\n  X\nend\nsubgraph b\n  X\nend");
    assert_eq!(members(&f, "a"), ["X"]);
    assert!(members(&f, "b").is_empty());
    assert_eq!(f.warnings.len(), 1);
}

#[test]
fn node_mentioned_in_an_inner_subgraph_moves_inward() {
    let f = fc("flowchart LR\nsubgraph out\n  X\n  subgraph in\n    X\n  end\nend");
    assert_eq!(members(&f, "in"), ["X"]);
    assert!(members(&f, "out").is_empty());
    assert!(f.warnings.is_empty());
}

#[test]
fn subgraph_titles_in_all_forms() {
    let f = fc("flowchart LR\nsubgraph a[\"Quoted title\"]\nend\nsubgraph Bare Words\nend\nsubgraph \"Only title\"\nend");
    assert_eq!(sg(&f, "a").title, "Quoted title");
    assert_eq!(sg(&f, "Bare Words").title, "Bare Words");
    assert_eq!(sg(&f, "Only title").title, "Only title");
}

#[test]
fn edge_labels_support_quotes_markdown_and_entities() {
    let f = fc("flowchart LR\nA -->|\"a | b\"| B\nB -- \"#quot;q#quot;\" --> C\nC -->|`**md**`| D");
    assert_eq!(edge(&f, "A", "B").label.as_deref(), Some("a | b"));
    assert_eq!(edge(&f, "B", "C").label.as_deref(), Some("\"q\""));
    let e = edge(&f, "C", "D");
    assert!(e.label_markdown && e.label.as_deref() == Some("**md**"));
}

#[test]
fn labels_with_semicolons_do_not_split_statements() {
    let f = fc("flowchart LR\nA[\"x; y\"] --> B[a #35; b]");
    assert_eq!(node(&f, "A").label, "x; y");
    assert_eq!(node(&f, "B").label, "a # b");
    assert!(f.warnings.is_empty());
}

#[test]
fn colour_with_semicolon_in_a_style_line_is_not_an_entity() {
    let f = fc("flowchart LR\nA\nstyle A fill:#f9f;\nB");
    assert_eq!(node(&f, "A").style.fill.as_deref(), Some("#f9f"));
    assert_eq!(ids(&f), ["A", "B"]);
}

#[test]
fn unknown_statements_warn_with_their_line_and_parsing_continues() {
    let f = fc("flowchart LR\nA --> B\n)))\nC --> D\n&& junk");
    assert_eq!(pairs(&f), [("A", "B"), ("C", "D")]);
    let lines: Vec<usize> = f.warnings.iter().map(|w| w.line).collect();
    assert_eq!(lines, [3, 5]);
    assert!(f.warnings.iter().all(|w| !w.message.is_empty()));
}

#[test]
fn unterminated_label_warns_and_keeps_the_node() {
    let f = fc("flowchart LR\nA[open\nB --> C");
    assert_eq!(f.warnings.len(), 1);
    assert_eq!(pairs(&f), [("B", "C")]);
}

#[test]
fn bad_link_style_index_and_unknown_style_target_warn() {
    let f = fc("flowchart LR\nA --> B\nlinkStyle 5 stroke:red\nstyle Nope fill:red\nclass Nope k\nlinkStyle x stroke:red");
    assert_eq!(f.warnings.len(), 4);
}

#[test]
fn stray_end_and_direction_outside_a_subgraph_warn() {
    let f = fc("flowchart LR\nend\ndirection TB\nA");
    assert_eq!(f.warnings.len(), 2);
    assert_eq!(ids(&f), ["A"]);
}

#[test]
fn unclosed_subgraph_still_yields_its_members() {
    let f = fc("flowchart LR\nsubgraph s\nA --> B");
    assert_eq!(members(&f, "s"), ["A", "B"]);
}

#[test]
fn keywords_are_whole_words_only() {
    let f = fc("flowchart LR\nendpoint --> classy\nstyles --> subgraphs");
    assert_eq!(f.edges.len(), 2);
    assert!(f.subgraphs.is_empty() && f.warnings.is_empty());
}

#[test]
fn hyphen_and_unicode_ids() {
    let f = fc("flowchart LR\nmy-node --> \u{4e2d}\u{6587} --> caf\u{e9}[\"\u{2764} ok\"]");
    assert_eq!(ids(&f), ["my-node", "\u{4e2d}\u{6587}", "caf\u{e9}"]);
    assert_eq!(f.edges.len(), 2);
}

#[test]
fn crlf_and_bom_are_tolerated() {
    let f = fc("\u{feff}flowchart LR\r\nA --> B\r\nB[\"x\"] --> C\r\n");
    assert_eq!(pairs(&f), [("A", "B"), ("B", "C")]);
    assert_eq!(node(&f, "B").label, "x");
    assert!(f.warnings.is_empty());
}

#[test]
fn header_and_first_statement_may_share_a_semicolon_line() {
    let f = fc("graph LR; A-->B; B-->C");
    assert_eq!(f.direction, Direction::LeftToRight);
    assert_eq!(f.edges.len(), 2);
}

// ---- limits ----

#[test]
fn limits_turn_into_errors() {
    let tiny = Limits { max_bytes: 20, ..Limits::default() };
    assert!(matches!(parse_with_limits("flowchart LR\nA --> B --> C", &tiny), Err(ParseError::TooLarge { .. })));
    let nodes = Limits { max_nodes: 3, ..Limits::default() };
    assert_eq!(parse_with_limits("flowchart LR\nA --> B --> C --> D", &nodes), Err(ParseError::TooManyNodes { limit: 3 }));
    let edges = Limits { max_edges: 3, ..Limits::default() };
    assert_eq!(parse_with_limits("flowchart LR\nA & B --> C & D", &edges), Err(ParseError::TooManyEdges { limit: 3 }));
    let depth = Limits { max_depth: 2, ..Limits::default() };
    assert_eq!(parse_with_limits("flowchart LR\nsubgraph a\nsubgraph b\nsubgraph c\nend\nend\nend", &depth), Err(ParseError::TooDeep { limit: 2 }));
    assert!(parse_with_limits("flowchart LR\nsubgraph a\nsubgraph b\nend\nend", &depth).is_ok());
}

#[test]
fn empty_and_headerless_inputs_are_errors_or_unsupported() {
    assert_eq!(parse(""), Err(ParseError::MissingHeader));
    assert_eq!(parse("  \n%% only a comment\n"), Err(ParseError::MissingHeader));
    assert_eq!(parse("---\ntitle: x\n"), Err(ParseError::UnterminatedFrontMatter));
    assert!(matches!(parse("A --> B"), Ok(Diagram::Unsupported { .. })));
}

// ---- hostile input ----

/// Every index in the result must point at something that exists.
fn check_invariants(f: &Flowchart) {
    for e in &f.edges {
        for end in [e.from, e.to] {
            match end {
                Endpoint::Node(i) => assert!(i < f.nodes.len()),
                Endpoint::Subgraph(i) => assert!(i < f.subgraphs.len()),
            }
        }
        assert!(e.length >= 1);
    }
    let mut seen = std::collections::HashSet::new();
    for n in &f.nodes {
        assert!(seen.insert(n.id.as_str()), "duplicate node {}", n.id);
    }
    let mut owned = vec![0usize; f.nodes.len()];
    for (si, s) in f.subgraphs.iter().enumerate() {
        assert!(s.parent.is_none_or(|p| p < si), "parent must come earlier");
        for &m in &s.members {
            owned[m] += 1;
        }
    }
    assert!(owned.iter().all(|&c| c <= 1));
}

fn sweep(src: &str) -> Option<Flowchart> {
    match parse(src) {
        Ok(Diagram::Flowchart(f)) => {
            check_invariants(&f);
            Some(f)
        }
        Ok(Diagram::Unsupported { kind }) => {
            assert!(kind.chars().count() <= 64);
            None
        }
        Err(_) => None,
    }
}

#[test]
fn hostile_small_inputs_never_panic() {
    let cases: &[&str] = &[
        "",
        " ",
        "\n\n\n",
        "flowchart",
        "flowchart LR",
        "graph",
        "graph TD;",
        "flowchart LR\nA[",
        "flowchart LR\nA[\"",
        "flowchart LR\nA[\"`",
        "flowchart LR\nA[\"`x\"]",
        "flowchart LR\nA((",
        "flowchart LR\nA(((x))",
        "flowchart LR\nA[/x",
        "flowchart LR\nA@{",
        "flowchart LR\nA@{ shape:",
        "flowchart LR\nA@{ shape: \"",
        "flowchart LR\nA@{ shape: diamond, label: \"x }",
        "flowchart LR\nA --",
        "flowchart LR\nA -- text",
        "flowchart LR\nA -. text",
        "flowchart LR\nA -->|",
        "flowchart LR\nA -->|\"x",
        "flowchart LR\nA --> ",
        "flowchart LR\nA & ",
        "flowchart LR\n& A",
        "flowchart LR\nA:::",
        "flowchart LR\ne@--> ",
        "flowchart LR\n@@@@",
        "flowchart LR\nsubgraph",
        "flowchart LR\nsubgraph [",
        "flowchart LR\nsubgraph \"",
        "flowchart LR\nend\nend\nend",
        "flowchart LR\nclassDef",
        "flowchart LR\nclassDef ,,, ,,",
        "flowchart LR\nclass",
        "flowchart LR\nclass a",
        "flowchart LR\nstyle",
        "flowchart LR\nstyle A",
        "flowchart LR\nlinkStyle",
        "flowchart LR\nlinkStyle 99999999999999999999 a:b",
        "flowchart LR\nclick",
        "flowchart LR\n%%{",
        "flowchart LR\n%%",
        "flowchart LR\nA[\"#",
        "flowchart LR\nA[\"#99999999999;\"]",
        "flowchart LR\nA[\"#x110000;\"]",
        "flowchart LR\nA[\"<br\"]",
        "flowchart LR\nA --> B\u{0}\nC\u{0}-->D",
        "flowchart LR\u{0}",
        "\u{0}\u{0}\u{0}",
        "flowchart LR\r\nA-->B\rC-->D\r\n",
        "flowchart LR\n\u{1f600} --> \u{202e}x\u{202c} --> \u{200b}",
        "flowchart LR\nA[\u{1f600}\u{1f600}] --> B{\u{4e2d}\u{6587}}",
        "---\n---\nflowchart LR\nA",
        "---\n",
        "\u{feff}",
        "flowchart LR\nA --- ---- ---- B",
        "flowchart LR\nA ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ B",
        "flowchart LR\nA -----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------> B",
        "flowchart LR\nA <-- B",
        "flowchart LR\nA o-- B",
    ];
    for c in cases {
        sweep(c);
    }
}

#[test]
fn deeply_nested_subgraphs_hit_the_depth_limit_without_recursing() {
    let mut s = String::from("flowchart LR\n");
    for i in 0..10_000 {
        s.push_str(&format!("subgraph s{i}\n"));
    }
    s.push_str("A\n");
    for _ in 0..10_000 {
        s.push_str("end\n");
    }
    // Past the node/subgraph budget the parse is refused, again without recursion.
    assert!(matches!(parse(&s), Err(ParseError::TooDeep { .. } | ParseError::TooManyNodes { .. })));
    let mut hundred = String::from("flowchart LR\n");
    for i in 0..100 {
        hundred.push_str(&format!("subgraph s{i}\n"));
    }
    assert_eq!(parse(&hundred), Err(ParseError::TooDeep { limit: Limits::default().max_depth }));
    let ok = {
        let mut s = String::from("flowchart LR\n");
        for i in 0..64 {
            s.push_str(&format!("subgraph s{i}\n"));
        }
        s.push('A');
        s
    };
    let f = sweep(&ok).expect("64 levels fit");
    assert_eq!(f.subgraphs.len(), 64);
    assert_eq!(f.subgraphs[63].members.len(), 1);
}

#[test]
fn one_megabyte_of_junk_returns_promptly_and_bounded() {
    let unit_list: &[&str] = &[
        "!@#$%^&*()_+ ",
        "[[[[[ ",
        "\"` ",
        "@{ ",
        "-- ",
        "-. ",
        ".......",
        "((( ",
        "A --> B\n",
        "%%{ ",
        "|\"| ",
        "&&& ",
        "\u{1f600} ",
        "\0",
        "#a; #",
        "subgraph x\n",
        "end ",
        "A[\"`",
    ];
    for unit in unit_list {
        let mut s = String::from("flowchart LR\n");
        while s.len() < 1_000_000 {
            s.push_str(unit);
        }
        if let Some(f) = sweep(&s) {
            assert!(f.warnings.len() <= 1001 && f.notes.len() <= 1001);
        }
    }
}

#[test]
fn single_pathological_statements_are_bounded() {
    let big = 900_000;
    let cases = [
        format!("flowchart LR\nA -. {}", ".".repeat(big)),
        format!("flowchart LR\nA -- {}", "-".repeat(big)),
        format!("flowchart LR\nA[{}", "[".repeat(big)),
        format!("flowchart LR\nA@{}", "{".repeat(big)),
        format!("flowchart LR\n{}", "\"`".repeat(big / 2)),
        format!("flowchart LR\n{}", "A@{\"".repeat(big / 4)),
        format!("flowchart LR\nA{}", " & A".repeat(big / 4)),
        format!("flowchart LR\nA {}B", "--> B ".repeat(big / 6)),
        format!("flowchart LR\n{}", "A ".repeat(big / 2)),
        // An unmatched second closer of `[/` must not rescan the statement for every node.
        format!("flowchart LR\n{}", "A[/x/] ".repeat(big / 7)),
        format!("flowchart LR\n{}{}", "A-->B\n".repeat(10_000), "linkStyle default stroke:red\n".repeat(30_000)),
        format!("flowchart LR\nA\nstyle {} fill:red", vec!["a"; big / 4].join(",")),
        format!("flowchart LR\nclassDef {} fill:red", (0..big / 8).map(|i| format!("c{i}")).collect::<Vec<_>>().join(",")),
    ];
    for c in &cases {
        sweep(c);
    }
}

#[test]
fn ampersand_blow_up_is_refused_not_expanded() {
    let side = (0..300).map(|i| format!("a{i}")).collect::<Vec<_>>().join(" & ");
    let other = (0..300).map(|i| format!("b{i}")).collect::<Vec<_>>().join(" & ");
    let src = format!("flowchart LR\n{side} --> {other}");
    assert_eq!(parse(&src), Err(ParseError::TooManyEdges { limit: Limits::default().max_edges }));
}

#[test]
fn warnings_are_capped() {
    let src = format!("flowchart LR\n{}", ")\n".repeat(5_000));
    let f = fc(&src);
    assert!(f.warnings.len() <= 1001);
    assert!(f.warnings.len() > 100);
}

#[test]
fn input_over_the_byte_limit_is_an_error() {
    let src = format!("flowchart LR\n{}", "A\n".repeat(600_000));
    assert!(matches!(parse(&src), Err(ParseError::TooLarge { .. })));
}

#[test]
fn edge_style_applies_edge_classes_then_link_style() {
    let f = fc("flowchart LR\nA e1@--> B\nclassDef c stroke:red,stroke-width:1px\nclass e1 c\nlinkStyle 0 stroke-width:3px");
    let st = f.edge_style(0);
    assert_eq!(st.stroke.as_deref(), Some("red"));
    assert!(st.stroke_width > Some(1.0), "linkStyle beats the class");
    assert!(f.edge_style(9).is_empty());
}

#[test]
fn oversized_style_properties_are_rejected_with_a_warning() {
    let long = "x".repeat(10_000);
    let f = fc(&format!("flowchart LR\nA\nstyle A fill:red,custom:{long}"));
    assert_eq!(node(&f, "A").style.fill.as_deref(), Some("red"));
    assert!(node(&f, "A").style.other.is_empty());
    assert_eq!(f.warnings.len(), 1);
    assert!(f.warnings[0].message.len() < 1000, "warnings do not echo unbounded source text");
}

#[test]
fn raw_style_pairs_per_node_stay_bounded() {
    let props = (0..500).map(|i| format!("k{i}:v")).collect::<Vec<_>>().join(",");
    let f = fc(&format!("flowchart LR\nA\nstyle A {props}"));
    assert!(node(&f, "A").style.other.len() < 500);
    let f = fc(&format!("flowchart LR\nA\nclassDef big {props}\nclass A big"));
    assert!(f.node_style(0).other.len() < 500);
}

#[test]
fn class_def_names_are_limited_by_the_node_budget() {
    let limits = Limits { max_nodes: 3, ..Limits::default() };
    let Ok(Diagram::Flowchart(f)) = parse_with_limits("flowchart LR\nclassDef a,b,c,d,e fill:red", &limits) else {
        panic!("expected a flowchart");
    };
    assert_eq!(f.class_defs.len(), 3);
    assert!(!f.warnings.is_empty());
}
