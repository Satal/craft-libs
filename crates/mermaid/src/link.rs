//! Link (edge) tokens: `-->`, `---`, `-.->`, `==>`, `~~~`, with heads on either end.

use crate::{Head, Stroke};

/// A lexed link without its label.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkTok {
    pub stroke: Stroke,
    pub start: Head,
    pub end: Head,
    pub length: u8,
}

fn clamp(n: usize) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

fn tail_head(c: Option<&char>) -> Option<Head> {
    match c {
        Some('>') => Some(Head::Arrow),
        Some('o') => Some(Head::Circle),
        Some('x') => Some(Head::Cross),
        _ => None,
    }
}

fn run(c: &[char], mut j: usize, ch: char) -> (usize, usize) {
    let from = j;
    while c.get(j) == Some(&ch) {
        j += 1;
    }
    (j - from, j)
}

/// Lexes a complete link at `i`. With `allow_start`, a head (`<`, `o`, `x`) may precede the body.
pub(crate) fn lex_link(c: &[char], i: usize, allow_start: bool) -> Option<(LinkTok, usize)> {
    let mut j = i;
    let mut start = Head::None;
    if allow_start {
        let head = match c.get(j) {
            Some('<') => Head::Arrow,
            Some('o') => Head::Circle,
            Some('x') => Head::Cross,
            _ => Head::None,
        };
        if head != Head::None && matches!(c.get(j + 1), Some('-' | '=')) {
            start = head;
            j += 1;
        }
    }
    let has_start = start != Head::None;
    match c.get(j)? {
        '-' if c.get(j + 1) == Some(&'.') => {
            let (dots, k) = run(c, j + 1, '.');
            if c.get(k) != Some(&'-') {
                return None;
            }
            let k = k + 1;
            let tail = tail_head(c.get(k));
            let end = tail.unwrap_or(Head::None);
            let after = if tail.is_some() { k + 1 } else { k };
            Some((LinkTok { stroke: Stroke::Dotted, start, end, length: clamp(dots) }, after))
        }
        ch @ ('-' | '=') => {
            let (n, k) = run(c, j, *ch);
            let tail = tail_head(c.get(k));
            let stroke = if *ch == '-' { Stroke::Solid } else { Stroke::Thick };
            match tail {
                Some(end) if n >= 2 => Some((LinkTok { stroke, start, end, length: clamp(n - 1) }, k + 1)),
                None if n >= 3 || (has_start && n >= 2) => Some((LinkTok { stroke, start, end: Head::None, length: clamp(n - 2).max(1) }, k)),
                _ => None,
            }
        }
        '~' if !has_start => {
            let (n, k) = run(c, j, '~');
            (n >= 3).then(|| (LinkTok { stroke: Stroke::Invisible, start, end: Head::None, length: clamp(n - 2) }, k))
        }
        _ => None,
    }
}

/// The opener of an inline label (`-- text -->`, `-. text .->`, `== text ==>`): the stroke and
/// the index after the opener. The opener must be followed by whitespace or a quote.
pub(crate) fn lex_inline_open(c: &[char], i: usize) -> Option<(Stroke, usize)> {
    let (stroke, len) = match (c.get(i)?, c.get(i + 1)?) {
        ('-', '-') => (Stroke::Solid, 2),
        ('-', '.') => (Stroke::Dotted, 2),
        ('=', '=') => (Stroke::Thick, 2),
        _ => return None,
    };
    let after = c.get(i + len)?;
    (after.is_whitespace() || *after == '"').then_some((stroke, i + len))
}

/// The closer of an inline label of `stroke`, at `i`: token and index after it.
pub(crate) fn lex_inline_close(c: &[char], i: usize, stroke: Stroke) -> Option<(LinkTok, usize)> {
    if stroke == Stroke::Dotted {
        let (dots, k) = run(c, i, '.');
        if dots == 0 || c.get(k) != Some(&'-') {
            return None;
        }
        let k = k + 1;
        let tail = tail_head(c.get(k));
        let after = if tail.is_some() { k + 1 } else { k };
        let tok = LinkTok { stroke, start: Head::None, end: tail.unwrap_or(Head::None), length: clamp(dots) };
        return Some((tok, after));
    }
    let (tok, after) = lex_link(c, i, false)?;
    (tok.stroke == stroke).then_some((tok, after))
}
