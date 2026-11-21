//! Splits source text into statements: newline and `;` separated, aware of quotes, markdown
//! strings, `@{ ... }` blocks, entity codes, `%%` comments and `%%{ ... }%%` directives.
//!
//! Every search that can run to the end of the input is guarded by a precomputed "last possible
//! closer" position, so junk input stays linear.

/// One statement and the 1-based line it starts on.
pub(crate) struct Stmt {
    pub line: usize,
    pub text: String,
}

struct Splitter<'a> {
    src: &'a str,
    b: &'a [u8],
    out: Vec<Stmt>,
    buf: Vec<u8>,
    line: usize,
    started: Option<usize>,
    style_kw: Option<bool>,
    last_md_close: Option<usize>,
    last_brace: Option<usize>,
    last_directive_close: Option<usize>,
    groups_possible: bool,
}

fn rfind_bytes(hay: &str, needle: &str) -> Option<usize> {
    hay.rfind(needle)
}

impl<'a> Splitter<'a> {
    fn new(src: &'a str, first_line: usize) -> Self {
        Splitter {
            src,
            b: src.as_bytes(),
            out: Vec::new(),
            buf: Vec::new(),
            line: first_line,
            started: None,
            style_kw: None,
            last_md_close: rfind_bytes(src, "`\""),
            last_brace: src.rfind('}'),
            last_directive_close: rfind_bytes(src, "}%%"),
            groups_possible: true,
        }
    }

    fn push_byte(&mut self, c: u8) {
        if self.started.is_none() && !c.is_ascii_whitespace() {
            self.started = Some(self.line);
        }
        self.buf.push(c);
    }

    /// Copies a range into the statement. Newlines become spaces unless `keep_newlines` is set
    /// (markdown strings keep them as real line breaks); carriage returns always become spaces.
    fn push_range(&mut self, from: usize, to: usize, keep_newlines: bool) {
        for k in from..to.min(self.b.len()) {
            if let Some(&c) = self.b.get(k) {
                let out = match c {
                    b'\n' if keep_newlines => b'\n',
                    b'\n' | b'\r' => b' ',
                    other => other,
                };
                self.push_byte(out);
            }
        }
    }

    fn count_lines(&self, from: usize, to: usize) -> usize {
        self.b.get(from..to.min(self.b.len())).map_or(0, |s| s.iter().filter(|&&c| c == b'\n').count())
    }

    fn flush(&mut self) {
        let text = String::from_utf8_lossy(&self.buf).trim().to_owned();
        if !text.is_empty() {
            let line = self.started.unwrap_or(self.line);
            self.out.push(Stmt { line, text });
        }
        self.buf.clear();
        self.started = None;
        self.style_kw = None;
    }

    /// True when the statement so far starts with a styling keyword (its `#fff;` is a colour).
    fn in_style_statement(&mut self) -> bool {
        if let Some(v) = self.style_kw {
            return v;
        }
        let head = String::from_utf8_lossy(&self.buf);
        let word = head.split_whitespace().next().unwrap_or("");
        let v = matches!(word, "style" | "classDef" | "class" | "linkStyle");
        self.style_kw = Some(v);
        v
    }

    /// Entity code `#name;` / `#123;` at `i`: end (exclusive) if there is one.
    fn entity_end(&self, i: usize) -> Option<usize> {
        let mut j = i + 1;
        while self.b.get(j).is_some_and(u8::is_ascii_alphanumeric) && j - i <= 10 {
            j += 1;
        }
        (j > i + 1 && self.b.get(j) == Some(&b';')).then_some(j + 1)
    }

    /// A quoted run starting at the `"` at `i`. Returns the index after it.
    fn quote(&mut self, i: usize) -> usize {
        if self.b.get(i + 1) == Some(&b'`')
            && self.last_md_close.is_some_and(|l| l >= i + 2)
            && let Some(rel) = self.src.get(i + 2..).and_then(|s| s.find("`\""))
        {
            let end = i + 2 + rel + 2;
            self.push_range(i, end, true);
            self.line += self.count_lines(i, end);
            return end;
        }
        // Plain quote: closes at the next quote or at the end of the line.
        let mut j = i + 1;
        while let Some(&c) = self.b.get(j) {
            if c == b'\n' {
                break;
            }
            j += 1;
            if c == b'"' {
                break;
            }
        }
        self.push_range(i, j, false);
        j
    }

    /// An `@{ ... }` block at `i`: kept whole, even across lines. Returns the index after it.
    fn group(&mut self, i: usize) -> Option<usize> {
        if !self.groups_possible || self.last_brace.is_none_or(|l| l <= i) {
            return None;
        }
        let mut depth = 1usize;
        let mut j = i + 2;
        while let Some(&c) = self.b.get(j) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = j + 1;
                        self.push_range(i, end, false);
                        self.line += self.count_lines(i, end);
                        return Some(end);
                    }
                }
                b'"' => {
                    j += 1;
                    while let Some(&q) = self.b.get(j) {
                        if q == b'"' || q == b'\n' {
                            break;
                        }
                        j += 1;
                    }
                    if self.b.get(j) == Some(&b'\n') {
                        continue;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        self.groups_possible = false;
        None
    }

    /// A `%%` comment or `%%{ ... }%%` directive at `i`. Returns the index to continue from.
    fn comment(&mut self, i: usize) -> usize {
        self.flush();
        if self.b.get(i + 2) == Some(&b'{')
            && self.last_directive_close.is_some_and(|l| l >= i)
            && let Some(rel) = self.src.get(i..).and_then(|s| s.find("}%%"))
        {
            let end = i + rel + 3;
            self.line += self.count_lines(i, end);
            return end;
        }
        let mut j = i;
        while self.b.get(j).is_some_and(|&c| c != b'\n') {
            j += 1;
        }
        j
    }

    fn run(mut self) -> Vec<Stmt> {
        let mut i = 0;
        while let Some(&c) = self.b.get(i) {
            match c {
                b'\n' => {
                    self.flush();
                    self.line += 1;
                    i += 1;
                }
                b';' => {
                    self.flush();
                    i += 1;
                }
                b'\r' => {
                    self.push_byte(b' ');
                    i += 1;
                }
                b'"' => i = self.quote(i),
                b'%' if self.b.get(i + 1) == Some(&b'%') => i = self.comment(i),
                b'@' if self.b.get(i + 1) == Some(&b'{') => match self.group(i) {
                    Some(end) => i = end,
                    None => {
                        self.push_byte(c);
                        i += 1;
                    }
                },
                b'#' if !self.in_style_statement() => match self.entity_end(i) {
                    Some(end) => {
                        self.push_range(i, end, false);
                        i = end;
                    }
                    None => {
                        self.push_byte(c);
                        i += 1;
                    }
                },
                _ => {
                    self.push_byte(c);
                    i += 1;
                }
            }
        }
        self.flush();
        self.out
    }
}

/// Splits `src` into statements; `first_line` is the 1-based number of its first line.
pub(crate) fn split(src: &str, first_line: usize) -> Vec<Stmt> {
    Splitter::new(src, first_line).run()
}
