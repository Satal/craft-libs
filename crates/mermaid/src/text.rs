//! Label text: quoting, markdown strings, entity codes and `<br>` line breaks.

/// Looks up a named entity code (`#quot;`).
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "quot" => '"',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "hellip" => '\u{2026}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hearts" => '\u{2665}',
        _ => return None,
    })
}

/// Decodes one entity body (the text between `#` and `;`).
fn decode_one(body: &str) -> Option<char> {
    if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
        let n = u32::from_str_radix(hex, 16).ok()?;
        return printable(n);
    }
    if body.bytes().all(|b| b.is_ascii_digit()) && !body.is_empty() {
        let n: u32 = body.parse().ok()?;
        return printable(n);
    }
    named_entity(body)
}

fn printable(n: u32) -> Option<char> {
    let c = char::from_u32(n)?;
    (!c.is_control() || c == '\t').then_some(c)
}

/// Replaces `#name;`, `#123;` and `#x1F;` entity codes. Unknown codes stay literal.
pub(crate) fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '#' {
            let mut j = i + 1;
            while chars.get(j).is_some_and(|c| c.is_ascii_alphanumeric()) && j - i <= 10 {
                j += 1;
            }
            if j > i + 1 && chars.get(j) == Some(&';') {
                let body: String = chars.get(i + 1..j).unwrap_or_default().iter().collect();
                if let Some(d) = decode_one(&body) {
                    out.push(d);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// End (exclusive) of a `<br>`, `<br/>` or `<br />` tag starting at `i`.
fn br_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if !b.get(j)?.eq_ignore_ascii_case(&b'b') {
        return None;
    }
    j += 1;
    if !b.get(j)?.eq_ignore_ascii_case(&b'r') {
        return None;
    }
    j += 1;
    while b.get(j) == Some(&b' ') {
        j += 1;
    }
    if b.get(j) == Some(&b'/') {
        j += 1;
    }
    while b.get(j) == Some(&b' ') {
        j += 1;
    }
    (b.get(j) == Some(&b'>')).then_some(j + 1)
}

/// Replaces `<br>` variants with a newline.
fn replace_br(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut last) = (0, 0);
    while i < b.len() {
        if b.get(i) == Some(&b'<')
            && let Some(end) = br_end(b, i)
        {
            out.push_str(s.get(last..i).unwrap_or_default());
            out.push('\n');
            i = end;
            last = end;
            continue;
        }
        i += 1;
    }
    out.push_str(s.get(last..).unwrap_or_default());
    out
}

/// A cleaned label and whether it was a markdown string.
pub(crate) struct Label {
    pub text: String,
    pub markdown: bool,
}

/// Cleans label text as written in the source: strips one pair of quotes and, inside them, the
/// markdown backticks; then turns `<br>` into newlines and decodes entity codes.
pub(crate) fn clean_label(raw: &str) -> Label {
    let t = raw.trim();
    let mut markdown = false;
    let mut inner = t;
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        inner = t.get(1..t.len() - 1).unwrap_or_default();
    }
    let trimmed = inner.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        markdown = true;
        inner = trimmed.get(1..trimmed.len() - 1).unwrap_or_default();
    }
    let text = decode_entities(&replace_br(inner));
    Label { text: text.trim().to_owned(), markdown }
}
