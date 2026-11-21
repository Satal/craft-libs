//! The statement parser.

use std::collections::HashMap;

use crate::link::{LinkTok, lex_inline_close, lex_inline_open, lex_link};
use crate::shape::{BRACKETS, shape_by_name};
use crate::split::{Stmt, split};
use crate::style::{Style, parse_style};
use crate::text::{Label, clean_label};
use crate::{ClassDef, Diagram, Direction, Edge, Endpoint, Flowchart, Node, ParseError, Shape, Subgraph, Warning};

/// Cap on stored warnings (and notes); the entry after the last one says more were left out.
const MAX_MESSAGES: usize = 1000;

/// Input limits. Exceeding one is a [`ParseError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest input in bytes.
    pub max_bytes: usize,
    /// Most nodes plus subgraphs.
    pub max_nodes: usize,
    /// Most edges, counted after `&` expansion.
    pub max_edges: usize,
    /// Deepest subgraph nesting.
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_bytes: 1 << 20, max_nodes: 5_000, max_edges: 20_000, max_depth: 64 }
    }
}

/// Parses Mermaid text with the default [`Limits`].
pub fn parse(src: &str) -> Result<Diagram, ParseError> {
    parse_with_limits(src, &Limits::default())
}

/// Parses Mermaid text with explicit limits.
pub fn parse_with_limits(src: &str, limits: &Limits) -> Result<Diagram, ParseError> {
    if src.len() > limits.max_bytes {
        return Err(ParseError::TooLarge { bytes: src.len(), limit: limits.max_bytes });
    }
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let (body, first_line) = strip_front_matter(src)?;
    let stmts = split(body, first_line);
    let mut it = stmts.iter();
    let first = it.next().ok_or(ParseError::MissingHeader)?;
    let (word, rest) = split_word(&first.text);
    if !matches!(word, "flowchart" | "graph" | "flowchart-elk" | "flowchart-v2") {
        return Ok(Diagram::Unsupported { kind: word.chars().take(64).collect() });
    }
    let mut p = Parser::new(limits, &stmts);
    p.prescan()?;
    let mut rest = rest.trim();
    let (dir_word, after) = split_word(rest);
    if let Some(d) = parse_direction(dir_word) {
        p.chart.direction = d;
        rest = after.trim();
    }
    if !rest.is_empty() {
        p.statement(first.line, rest)?;
    }
    for st in it {
        p.statement(st.line, &st.text)?;
    }
    Ok(Diagram::Flowchart(p.finish()))
}

/// Skips a leading `---` front matter block. Returns the remaining text and its first line number.
fn strip_front_matter(src: &str) -> Result<(&str, usize), ParseError> {
    let (mut offset, mut line) = (0, 1);
    for l in src.split_inclusive('\n') {
        if l.trim().is_empty() {
            offset += l.len();
            line += 1;
            continue;
        }
        if l.trim() != "---" {
            break;
        }
        let mut off = offset + l.len();
        let mut ln = line + 1;
        for l2 in src.get(off..).unwrap_or_default().split_inclusive('\n') {
            off += l2.len();
            ln += 1;
            if l2.trim() == "---" {
                return Ok((src.get(off..).unwrap_or_default(), ln));
            }
        }
        return Err(ParseError::UnterminatedFrontMatter);
    }
    Ok((src, 1))
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "TB" | "TD" => Some(Direction::TopToBottom),
        "BT" => Some(Direction::BottomToTop),
        "LR" => Some(Direction::LeftToRight),
        "RL" => Some(Direction::RightToLeft),
        _ => None,
    }
}

/// Splits at the first whitespace: the word and the rest (untrimmed).
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (s.get(..i).unwrap_or_default(), s.get(i..).unwrap_or_default()),
        None => (s, ""),
    }
}

/// Takes a comma-separated name list (`a,b` or `a, b`) from the front of `s`.
fn take_names(s: &str) -> (Vec<String>, &str) {
    let mut names = Vec::new();
    let mut rest = s.trim_start();
    loop {
        let (tok, after) = split_word(rest);
        names.extend(tok.split(',').filter(|n| !n.is_empty()).map(str::to_owned));
        rest = after.trim_start();
        if !tok.ends_with(',') || rest.is_empty() {
            break;
        }
    }
    (names, rest)
}

/// A subgraph header: `id [Title]`, `"Title"`, `id["Title"]` or bare words.
fn subgraph_header(rest: &str, anonymous: usize) -> (String, Label) {
    let rest = rest.trim();
    if rest.is_empty() {
        let id = format!("subgraph{anonymous}");
        return (id.clone(), Label { text: id, markdown: false });
    }
    if rest.starts_with('"') {
        let l = clean_label(rest);
        return (l.text.clone(), l);
    }
    let end = rest.find(|c: char| c.is_whitespace() || c == '[').unwrap_or(rest.len());
    let (id, tail) = (rest.get(..end).unwrap_or_default(), rest.get(end..).unwrap_or_default().trim());
    if let Some(inner) = tail.strip_prefix('[') {
        let inner = inner.strip_suffix(']').unwrap_or(inner);
        return (id.to_owned(), clean_label(inner));
    }
    if tail.is_empty() {
        return (id.to_owned(), clean_label(id));
    }
    (rest.to_owned(), clean_label(rest))
}

/// A recoverable statement error (a warning) or a limit error (the whole parse fails).
enum Fail {
    Soft(String),
    Hard(ParseError),
}

impl From<ParseError> for Fail {
    fn from(e: ParseError) -> Self {
        Fail::Hard(e)
    }
}

fn soft<T>(msg: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Soft(msg.into()))
}

/// A cursor over one statement.
struct Cur {
    c: Vec<char>,
    i: usize,
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace() && !c.is_control())
}

/// Reads a node id at `i`: the id and the index after it.
fn read_id_at(c: &[char], mut i: usize) -> (String, usize) {
    let mut s = String::new();
    while let Some(&ch) = c.get(i) {
        let hyphen = ch == '-' && !s.is_empty() && c.get(i + 1).copied().is_some_and(is_id_char);
        if !is_id_char(ch) && !hyphen {
            break;
        }
        s.push(ch);
        i += 1;
    }
    (s, i)
}

impl Cur {
    fn new(s: &str) -> Self {
        Cur { c: s.chars().collect(), i: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.c.get(self.i + n).copied()
    }

    fn eof(&self) -> bool {
        self.i >= self.c.len()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.c.get(self.i + k) == Some(&ch))
    }

    fn read_id(&mut self) -> String {
        let (id, end) = read_id_at(&self.c, self.i);
        self.i = end;
        id
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.c.get(from..to).unwrap_or_default().iter().collect()
    }

    /// First index at or after `from` where `pat` occurs.
    fn find(&self, from: usize, pat: &str) -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        let first = *p.first()?;
        (from..self.c.len()).find(|&k| self.c.get(k) == Some(&first) && p.iter().enumerate().all(|(o, ch)| self.c.get(k + o) == Some(ch)))
    }

    /// Reads a quoted run (plain or markdown) starting at `pos`; returns it with its quotes and the end.
    fn quoted_at(&self, pos: usize) -> Result<(String, usize), Fail> {
        let end = if self.c.get(pos + 1) == Some(&'`') { self.find(pos + 2, "`\"").map(|k| k + 2) } else { self.find(pos + 1, "\"").map(|k| k + 1) };
        match end {
            Some(e) => Ok((self.slice(pos, e), e)),
            None => soft("unterminated quote"),
        }
    }

    /// Reads `@{ key: value, ... }` at the cursor into raw pairs (lower-case keys).
    fn read_props(&mut self) -> Result<Vec<(String, String)>, Fail> {
        let start = self.i + 2;
        let mut j = start;
        let mut depth = 1usize;
        let mut in_quote = false;
        while let Some(&ch) = self.c.get(j) {
            match ch {
                '"' => in_quote = !in_quote,
                '{' if !in_quote => depth += 1,
                '}' if !in_quote => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if depth != 0 {
            return soft("unterminated @{ ... } block");
        }
        let body = self.slice(start, j);
        self.i = j + 1;
        Ok(split_pairs(&body))
    }
}

/// Splits `key: value, key: value` outside quotes.
fn split_pairs(body: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                cur.push(ch);
            }
            ',' if !in_quote => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    parts
        .iter()
        .filter_map(|p| {
            let (k, v) = p.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect()
}

/// A node definition found in an operand.
struct Def {
    shape: Option<Shape>,
    label: Option<Label>,
}

/// A link with its label, ready to become edges.
struct LinkSpec {
    tok: LinkTok,
    label: Option<Label>,
    id: Option<String>,
}

struct Parser<'a> {
    limits: &'a Limits,
    stmts: &'a [Stmt],
    chart: Flowchart,
    node_ix: HashMap<String, usize>,
    node_group: Vec<Option<usize>>,
    edge_ix: HashMap<String, usize>,
    /// Subgraph ids in order of first `subgraph` statement (found before parsing).
    sg_ix: HashMap<String, usize>,
    opened: HashMap<String, usize>,
    stack: Vec<usize>,
    line: usize,
    class_assign: Vec<(usize, String, String)>,
    style_assign: Vec<(usize, String, Style)>,
    link_styles: Vec<(usize, Option<usize>, Style)>,
}

impl<'a> Parser<'a> {
    fn new(limits: &'a Limits, stmts: &'a [Stmt]) -> Self {
        Parser {
            limits,
            stmts,
            chart: Flowchart::default(),
            node_ix: HashMap::new(),
            node_group: Vec::new(),
            edge_ix: HashMap::new(),
            sg_ix: HashMap::new(),
            opened: HashMap::new(),
            stack: Vec::new(),
            line: 1,
            class_assign: Vec::new(),
            style_assign: Vec::new(),
            link_styles: Vec::new(),
        }
    }

    /// Finds every subgraph id so an edge may name a subgraph declared later.
    fn prescan(&mut self) -> Result<(), ParseError> {
        for st in self.stmts {
            let (word, rest) = split_word(&st.text);
            if word == "subgraph" {
                let (id, _) = subgraph_header(rest, self.sg_ix.len());
                let n = self.sg_ix.len();
                self.sg_ix.entry(id).or_insert(n);
                if self.sg_ix.len() > self.limits.max_nodes {
                    return Err(ParseError::TooManyNodes { limit: self.limits.max_nodes });
                }
            }
        }
        Ok(())
    }

    fn warn(&mut self, line: usize, msg: impl Into<String>) {
        push_capped(&mut self.chart.warnings, line, msg.into());
    }

    fn note(&mut self, line: usize, msg: impl Into<String>) {
        push_capped(&mut self.chart.notes, line, msg.into());
    }

    fn statement(&mut self, line: usize, text: &str) -> Result<(), ParseError> {
        self.line = line;
        match self.dispatch(text) {
            Ok(()) => Ok(()),
            Err(Fail::Hard(e)) => Err(e),
            Err(Fail::Soft(m)) => {
                self.warn(line, m);
                Ok(())
            }
        }
    }

    fn dispatch(&mut self, text: &str) -> Result<(), Fail> {
        let (word, rest) = split_word(text);
        let rest = rest.trim();
        match word {
            "end" if rest.is_empty() => self.end_subgraph(),
            "subgraph" => self.open_subgraph(rest),
            "direction" if !rest.is_empty() => self.direction(rest),
            "classDef" => self.class_def(rest),
            "class" if !rest.is_empty() => self.class_statement(rest),
            "style" if !rest.is_empty() => self.style_statement(rest),
            "linkStyle" if !rest.is_empty() => self.link_style(rest),
            "click" if !rest.is_empty() => {
                self.note(self.line, "click statement ignored");
                Ok(())
            }
            _ => {
                if self.edge_props_statement(text) {
                    return Ok(());
                }
                self.chain_statement(text)
            }
        }
    }

    fn open_subgraph(&mut self, rest: &str) -> Result<(), Fail> {
        if self.stack.len() >= self.limits.max_depth {
            return Err(ParseError::TooDeep { limit: self.limits.max_depth }.into());
        }
        let (id, title) = subgraph_header(rest, self.chart.subgraphs.len());
        let idx = if let Some(&i) = self.opened.get(&id) {
            i
        } else {
            if self.chart.nodes.len() + self.chart.subgraphs.len() >= self.limits.max_nodes {
                return Err(ParseError::TooManyNodes { limit: self.limits.max_nodes }.into());
            }
            let i = self.chart.subgraphs.len();
            self.chart.subgraphs.push(Subgraph {
                id: id.clone(),
                title: title.text,
                title_markdown: title.markdown,
                direction: None,
                parent: self.stack.last().copied(),
                members: Vec::new(),
                classes: Vec::new(),
                style: Style::default(),
            });
            self.opened.insert(id, i);
            i
        };
        self.stack.push(idx);
        Ok(())
    }

    fn end_subgraph(&mut self) -> Result<(), Fail> {
        if self.stack.pop().is_none() {
            return soft("`end` without an open subgraph");
        }
        Ok(())
    }

    fn direction(&mut self, rest: &str) -> Result<(), Fail> {
        let Some(d) = parse_direction(rest.trim()) else {
            return soft(format!("unknown direction {rest:?}"));
        };
        let Some(&top) = self.stack.last() else {
            return soft("`direction` outside a subgraph");
        };
        if let Some(sg) = self.chart.subgraphs.get_mut(top) {
            sg.direction = Some(d);
        }
        Ok(())
    }

    fn class_def(&mut self, rest: &str) -> Result<(), Fail> {
        let (names, props) = take_names(rest);
        if names.is_empty() || props.is_empty() {
            return soft("classDef needs a class name and properties");
        }
        let (style, bad) = parse_style(props);
        for b in bad {
            self.warn(self.line, format!("classDef: ignored malformed property {b:?}"));
        }
        for name in names {
            if let Some(d) = self.chart.class_defs.iter_mut().find(|d| d.name == name) {
                d.style.merge(&style);
            } else {
                self.chart.class_defs.push(ClassDef { name, style: style.clone() });
            }
        }
        Ok(())
    }

    fn class_statement(&mut self, rest: &str) -> Result<(), Fail> {
        let Some(split) = rest.rfind(char::is_whitespace) else {
            return soft("class needs ids and a class name");
        };
        let ids = rest.get(..split).unwrap_or_default();
        let class = rest.get(split..).unwrap_or_default().trim();
        for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            self.class_assign.push((self.line, id.to_owned(), class.to_owned()));
        }
        Ok(())
    }

    fn style_statement(&mut self, rest: &str) -> Result<(), Fail> {
        let (ids, props) = take_names(rest);
        if ids.is_empty() || props.is_empty() {
            return soft("style needs an id and properties");
        }
        let (style, bad) = parse_style(props);
        for b in bad {
            self.warn(self.line, format!("style: ignored malformed property {b:?}"));
        }
        for id in ids {
            self.style_assign.push((self.line, id, style.clone()));
        }
        Ok(())
    }

    fn link_style(&mut self, rest: &str) -> Result<(), Fail> {
        let (targets, props) = take_names(rest);
        if targets.is_empty() || props.is_empty() {
            return soft("linkStyle needs edge indices and properties");
        }
        let (style, bad) = parse_style(props);
        for b in bad {
            self.warn(self.line, format!("linkStyle: ignored malformed property {b:?}"));
        }
        for t in targets {
            if t == "default" {
                self.link_styles.push((self.line, None, style.clone()));
            } else if let Ok(n) = t.parse::<usize>() {
                self.link_styles.push((self.line, Some(n), style.clone()));
            } else {
                self.warn(self.line, format!("linkStyle: {t:?} is not an edge index"));
            }
        }
        Ok(())
    }

    /// `e1@{ animate: true }` and `sg@{ view: collapsed }`: consumed with a note.
    fn edge_props_statement(&mut self, text: &str) -> bool {
        let mut cur = Cur::new(text);
        let id = cur.read_id();
        if id.is_empty() || cur.peek() != Some('@') || cur.peek_at(1) != Some('{') {
            return false;
        }
        let Ok(props) = cur.read_props() else {
            return false;
        };
        cur.skip_ws();
        if !cur.eof() || props.is_empty() {
            return false;
        }
        let node_keys = props.iter().any(|(k, _)| matches!(k.as_str(), "shape" | "label" | "icon" | "img"));
        let edge_keys = props.iter().all(|(k, _)| matches!(k.as_str(), "animate" | "animation" | "curve" | "step"));
        let view_only = props.iter().all(|(k, _)| k == "view");
        if !node_keys && (edge_keys || self.edge_ix.contains_key(&id)) {
            self.note(self.line, format!("properties of edge {id:?} ignored"));
            return true;
        }
        if !node_keys && view_only && self.sg_ix.contains_key(&id) {
            self.note(self.line, format!("subgraph {id:?} view ignored (drawn expanded)"));
            return true;
        }
        false
    }

    fn chain_statement(&mut self, text: &str) -> Result<(), Fail> {
        let mut cur = Cur::new(text);
        loop {
            cur.skip_ws();
            if cur.eof() {
                return Ok(());
            }
            self.chain(&mut cur)?;
        }
    }

    fn chain(&mut self, cur: &mut Cur) -> Result<(), Fail> {
        let mut prev = self.group(cur)?;
        loop {
            cur.skip_ws();
            let Some(spec) = self.link(cur)? else {
                return Ok(());
            };
            let next = self.group(cur)?;
            self.add_edges(&prev, &next, &spec)?;
            prev = next;
        }
    }

    fn group(&mut self, cur: &mut Cur) -> Result<Vec<Endpoint>, Fail> {
        let mut out = vec![self.operand(cur)?];
        loop {
            let save = cur.i;
            cur.skip_ws();
            if cur.peek() != Some('&') {
                cur.i = save;
                return Ok(out);
            }
            cur.i += 1;
            out.push(self.operand(cur)?);
            if out.len() > self.limits.max_nodes {
                return Err(ParseError::TooManyNodes { limit: self.limits.max_nodes }.into());
            }
        }
    }

    fn operand(&mut self, cur: &mut Cur) -> Result<Endpoint, Fail> {
        cur.skip_ws();
        let id = cur.read_id();
        if id.is_empty() {
            let shown = cur.peek().map_or_else(|| "end of statement".to_owned(), |c| format!("{c:?}"));
            return soft(format!("expected a node id, found {shown}"));
        }
        let def = if cur.peek() == Some('@') && cur.peek_at(1) == Some('{') {
            let props = cur.read_props()?;
            Some(self.def_from_props(&props))
        } else {
            self.bracket(cur)?
        };
        let mut classes = Vec::new();
        while cur.starts_with(":::") {
            cur.i += 3;
            let mut name = String::new();
            while let Some(c) = cur.peek() {
                if is_id_char(c) || c == '-' {
                    name.push(c);
                    cur.i += 1;
                } else {
                    break;
                }
            }
            if name.is_empty() {
                return soft("expected a class name after `:::`");
            }
            classes.push(name);
        }
        if def.is_none()
            && let Some(&sg) = self.sg_ix.get(&id)
        {
            if let Some(s) = self.chart.subgraphs.get_mut(sg) {
                s.classes.extend(classes);
            } else if !classes.is_empty() {
                self.class_assign.extend(classes.into_iter().map(|c| (self.line, id.clone(), c)));
            }
            return Ok(Endpoint::Subgraph(sg));
        }
        let ix = self.node_index(&id)?;
        if let Some(d) = def
            && let Some(n) = self.chart.nodes.get_mut(ix)
        {
            if let Some(s) = d.shape {
                n.shape = s;
            }
            if let Some(l) = d.label {
                n.label = l.text;
                n.markdown = l.markdown;
            }
        }
        if let Some(n) = self.chart.nodes.get_mut(ix) {
            for c in classes {
                if !n.classes.contains(&c) {
                    n.classes.push(c);
                }
            }
        }
        self.touch_member(ix);
        Ok(Endpoint::Node(ix))
    }

    fn def_from_props(&mut self, props: &[(String, String)]) -> Def {
        let (mut shape, mut label, mut icon, mut img) = (None, None, false, false);
        for (k, v) in props {
            match k.as_str() {
                "shape" => {
                    let name = clean_label(v).text;
                    shape = Some(shape_by_name(&name).unwrap_or_else(|| {
                        self.warn(self.line, format!("unknown shape {name:?}, drawn as a rectangle"));
                        Shape::Rect
                    }));
                }
                "label" => label = Some(clean_label(v)),
                "icon" => icon = true,
                "img" => img = true,
                _ => {}
            }
        }
        if shape.is_none() {
            shape = if icon {
                Some(Shape::Icon)
            } else if img {
                Some(Shape::Image)
            } else {
                None
            };
        }
        Def { shape, label }
    }

    /// A classic bracket form after an id, if there is one.
    fn bracket(&mut self, cur: &mut Cur) -> Result<Option<Def>, Fail> {
        let Some(b) = BRACKETS.iter().find(|b| cur.starts_with(b.open)) else {
            return Ok(None);
        };
        let mut pos = cur.i + b.open.chars().count();
        let mut quoted = None;
        let mut probe = pos;
        while cur.c.get(probe).is_some_and(|c| c.is_whitespace()) {
            probe += 1;
        }
        if cur.c.get(probe) == Some(&'"') {
            let (raw, end) = cur.quoted_at(probe)?;
            quoted = Some(raw);
            pos = end;
        }
        let found = if quoted.is_some() {
            let mut at = pos;
            while cur.c.get(at).is_some_and(|c| c.is_whitespace()) {
                at += 1;
            }
            let here = b.closers.iter().find(|(s, _)| starts_at(&cur.c, at, s));
            here.map(|(s, shape)| (at, *s, *shape)).or_else(|| first_closer(cur, pos, b.closers))
        } else {
            first_closer(cur, pos, b.closers)
        };
        let Some((at, closer, shape)) = found else {
            return soft(format!("unterminated `{}` label", b.open));
        };
        let raw = quoted.unwrap_or_else(|| cur.slice(pos, at));
        cur.i = at + closer.chars().count();
        Ok(Some(Def { shape: Some(shape), label: Some(clean_label(&raw)) }))
    }

    /// A link at the cursor with its label, or `None` (cursor unchanged) when there is none.
    fn link(&mut self, cur: &mut Cur) -> Result<Option<LinkSpec>, Fail> {
        let save = cur.i;
        let mut id = None;
        if let Some((name, after)) = edge_id_at(cur) {
            id = Some(name);
            cur.i = after;
        }
        if let Some((tok, end)) = lex_link(&cur.c, cur.i, true) {
            cur.i = end;
            let label = pipe_label(cur)?;
            return Ok(Some(LinkSpec { tok, label, id }));
        }
        if let Some((stroke, after)) = lex_inline_open(&cur.c, cur.i) {
            let mut j = after;
            while cur.c.get(j).is_some_and(|c| c.is_whitespace()) {
                j += 1;
            }
            let (text, mut k) = if cur.c.get(j) == Some(&'"') {
                let (raw, end) = cur.quoted_at(j)?;
                (Some(raw), end)
            } else {
                (None, after)
            };
            let start = k;
            while k < cur.c.len() {
                let run_start = k == start || cur.c.get(k - 1) != cur.c.get(k);
                if run_start && let Some((tok, end)) = lex_inline_close(&cur.c, k, stroke) {
                    let raw = text.unwrap_or_else(|| cur.slice(after, k));
                    cur.i = end;
                    return Ok(Some(LinkSpec { tok, label: Some(clean_label(&raw)), id }));
                }
                k += 1;
            }
            return soft("unterminated edge label");
        }
        cur.i = save;
        Ok(None)
    }

    fn add_edges(&mut self, from: &[Endpoint], to: &[Endpoint], spec: &LinkSpec) -> Result<(), Fail> {
        let count = from.len().saturating_mul(to.len());
        if self.chart.edges.len().saturating_add(count) > self.limits.max_edges {
            return Err(ParseError::TooManyEdges { limit: self.limits.max_edges }.into());
        }
        let mut first = true;
        for &a in from {
            for &b in to {
                let id = if first { spec.id.clone() } else { None };
                first = false;
                if let Some(i) = &id {
                    self.edge_ix.entry(i.clone()).or_insert(self.chart.edges.len());
                }
                self.chart.edges.push(Edge {
                    id,
                    from: a,
                    to: b,
                    label: spec.label.as_ref().map(|l| l.text.clone()),
                    label_markdown: spec.label.as_ref().is_some_and(|l| l.markdown),
                    stroke: spec.tok.stroke,
                    start_head: spec.tok.start,
                    end_head: spec.tok.end,
                    length: spec.tok.length.max(1),
                    classes: Vec::new(),
                    style: Style::default(),
                });
            }
        }
        Ok(())
    }

    fn node_index(&mut self, id: &str) -> Result<usize, Fail> {
        if let Some(&i) = self.node_ix.get(id) {
            return Ok(i);
        }
        if self.chart.nodes.len() + self.chart.subgraphs.len() >= self.limits.max_nodes {
            return Err(ParseError::TooManyNodes { limit: self.limits.max_nodes }.into());
        }
        let i = self.chart.nodes.len();
        self.chart.nodes.push(Node {
            id: id.to_owned(),
            label: id.to_owned(),
            markdown: false,
            shape: Shape::Rect,
            classes: Vec::new(),
            style: Style::default(),
        });
        self.node_group.push(None);
        self.node_ix.insert(id.to_owned(), i);
        Ok(i)
    }

    /// True when `a` encloses `b`.
    fn encloses(&self, a: usize, b: usize) -> bool {
        let mut cur = self.chart.subgraphs.get(b).and_then(|s| s.parent);
        for _ in 0..=self.chart.subgraphs.len() {
            match cur {
                Some(p) if p == a => return true,
                Some(p) => cur = self.chart.subgraphs.get(p).and_then(|s| s.parent),
                None => return false,
            }
        }
        false
    }

    fn touch_member(&mut self, node: usize) {
        let Some(&here) = self.stack.last() else {
            return;
        };
        let Some(&current) = self.node_group.get(node) else {
            return;
        };
        let new = match current {
            None => Some(here),
            Some(g) if g == here || self.encloses(here, g) => return,
            Some(g) if self.encloses(g, here) => Some(here),
            Some(g) => {
                let (a, b) = (self.sg_name(g), self.sg_name(here));
                let id = self.chart.nodes.get(node).map(|n| n.id.clone()).unwrap_or_default();
                self.warn(self.line, format!("node {id:?} is already in subgraph {a:?}; not added to {b:?}"));
                return;
            }
        };
        if let Some(slot) = self.node_group.get_mut(node) {
            *slot = new;
        }
    }

    fn sg_name(&self, i: usize) -> String {
        self.chart.subgraphs.get(i).map(|s| s.id.clone()).unwrap_or_default()
    }

    fn finish(mut self) -> Flowchart {
        for (ni, g) in self.node_group.iter().enumerate() {
            if let Some(sg) = g.and_then(|g| self.chart.subgraphs.get_mut(g)) {
                sg.members.push(ni);
            }
        }
        for (line, id, class) in std::mem::take(&mut self.class_assign) {
            if let Some(&n) = self.node_ix.get(&id) {
                if let Some(node) = self.chart.nodes.get_mut(n)
                    && !node.classes.contains(&class)
                {
                    node.classes.push(class);
                }
            } else if let Some(&e) = self.edge_ix.get(&id) {
                if let Some(edge) = self.chart.edges.get_mut(e) {
                    edge.classes.push(class);
                }
            } else if let Some(s) = self.chart.subgraphs.iter_mut().find(|s| s.id == id) {
                s.classes.push(class);
            } else {
                self.warn(line, format!("class: unknown id {id:?}"));
            }
        }
        for (line, id, style) in std::mem::take(&mut self.style_assign) {
            if let Some(node) = self.node_ix.get(&id).and_then(|&n| self.chart.nodes.get_mut(n)) {
                node.style.merge(&style);
            } else if let Some(s) = self.chart.subgraphs.iter_mut().find(|s| s.id == id) {
                s.style.merge(&style);
            } else {
                self.warn(line, format!("style: unknown id {id:?}"));
            }
        }
        let link_styles = std::mem::take(&mut self.link_styles);
        for (_, _, style) in link_styles.iter().filter(|(_, t, _)| t.is_none()) {
            for e in &mut self.chart.edges {
                e.style.merge(style);
            }
        }
        for (line, target, style) in &link_styles {
            let Some(n) = target else { continue };
            match self.chart.edges.get_mut(*n) {
                Some(e) => e.style.merge(style),
                None => self.warn(*line, format!("linkStyle: edge index {n} out of range")),
            }
        }
        self.chart
    }
}

fn push_capped(v: &mut Vec<Warning>, line: usize, message: String) {
    match v.len() {
        n if n < MAX_MESSAGES => v.push(Warning { line, message }),
        MAX_MESSAGES => v.push(Warning { line, message: "further messages omitted".to_owned() }),
        _ => {}
    }
}

fn starts_at(c: &[char], at: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, ch)| c.get(at + k) == Some(&ch))
}

/// The earliest closer at or after `from`: position, text and shape.
fn first_closer(cur: &Cur, from: usize, closers: &'static [(&'static str, Shape)]) -> Option<(usize, &'static str, Shape)> {
    closers.iter().filter_map(|(s, shape)| cur.find(from, s).map(|at| (at, *s, *shape))).min_by_key(|(at, _, _)| *at)
}

/// `ident@` directly before a link (an edge id), returning the id and the index after the `@`.
fn edge_id_at(cur: &Cur) -> Option<(String, usize)> {
    let (id, j) = read_id_at(&cur.c, cur.i);
    if id.is_empty() || cur.c.get(j) != Some(&'@') {
        return None;
    }
    let next = *cur.c.get(j + 1)?;
    (next != '{' && !next.is_whitespace()).then_some((id, j + 1))
}

/// `|label|` after a link, if present.
fn pipe_label(cur: &mut Cur) -> Result<Option<Label>, Fail> {
    let mut j = cur.i;
    while cur.c.get(j).is_some_and(|c| c.is_whitespace()) {
        j += 1;
    }
    if cur.c.get(j) != Some(&'|') {
        return Ok(None);
    }
    let start = j + 1;
    let mut q = start;
    while cur.c.get(q).is_some_and(|c| c.is_whitespace()) {
        q += 1;
    }
    let (raw, end) = if cur.c.get(q) == Some(&'"') {
        let (raw, e) = cur.quoted_at(q)?;
        let Some(bar) = cur.find(e, "|") else {
            return soft("unterminated edge label");
        };
        (raw, bar)
    } else {
        let Some(bar) = cur.find(start, "|") else {
            return soft("unterminated edge label");
        };
        (cur.slice(start, bar), bar)
    };
    cur.i = end + 1;
    Ok(Some(clean_label(&raw)))
}
