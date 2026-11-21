//! CSS-like style properties of `style`, `classDef` and `linkStyle`.

/// Most raw (`other`) properties one style keeps; later new keys are dropped so one `classDef`
/// shared by thousands of nodes cannot multiply without bound.
const MAX_OTHER: usize = 32;
/// Longest property key accepted, in bytes.
const MAX_KEY: usize = 64;
/// Longest property value accepted, in bytes.
const MAX_VALUE: usize = 256;

/// Style properties. The properties a board needs are typed; every other pair is kept raw (at
/// most 32 pairs per style after merging; keys over 64 bytes and values over 256 bytes are
/// rejected with a warning).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    /// `fill`, as written (`#f9f`, `red`).
    pub fill: Option<String>,
    /// `stroke`, as written.
    pub stroke: Option<String>,
    /// `stroke-width` in pixels (a trailing `px` is accepted).
    pub stroke_width: Option<f64>,
    /// `color` (text colour), as written.
    pub color: Option<String>,
    /// `stroke-dasharray` lengths.
    pub stroke_dasharray: Option<Vec<f64>>,
    /// Every other `key: value` pair, in source order, lower-case keys.
    pub other: Vec<(String, String)>,
}

impl Style {
    /// True when no property is set.
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    /// Overlays `over` on `self`: properties `over` sets win; raw pairs with the same key are replaced.
    pub fn merge(&mut self, over: &Style) {
        if over.fill.is_some() {
            self.fill.clone_from(&over.fill);
        }
        if over.stroke.is_some() {
            self.stroke.clone_from(&over.stroke);
        }
        if over.stroke_width.is_some() {
            self.stroke_width = over.stroke_width;
        }
        if over.color.is_some() {
            self.color.clone_from(&over.color);
        }
        if over.stroke_dasharray.is_some() {
            self.stroke_dasharray.clone_from(&over.stroke_dasharray);
        }
        for (k, v) in &over.other {
            if let Some(slot) = self.other.iter_mut().find(|(ek, _)| ek == k) {
                slot.1.clone_from(v);
            } else if self.other.len() < MAX_OTHER {
                self.other.push((k.clone(), v.clone()));
            }
        }
    }
}

/// Splits on commas that are not escaped with a backslash.
fn split_props(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&',') {
            cur.push(',');
            chars.next();
        } else if c == ',' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    let t = t.strip_suffix("px").unwrap_or(t).trim();
    let n: f64 = t.parse().ok()?;
    n.is_finite().then_some(n)
}

fn parse_dasharray(s: &str) -> Option<Vec<f64>> {
    let parts: Vec<&str> = s.split([' ', ',']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    parts.into_iter().map(parse_number).collect()
}

/// Parses `fill:#f9f,stroke:#333,stroke-width:4px`. Returns the style and the pairs that were
/// not `key:value` (so the caller can warn).
pub(crate) fn parse_style(props: &str) -> (Style, Vec<String>) {
    let mut style = Style::default();
    let mut bad = Vec::new();
    for part in split_props(props.trim().trim_end_matches(';')) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once(':') else {
            bad.push(part.to_owned());
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let value = v.trim().trim_end_matches("!important").trim().to_owned();
        if key.len() > MAX_KEY || value.len() > MAX_VALUE {
            bad.push(part.to_owned());
            continue;
        }
        match key.as_str() {
            "fill" => style.fill = Some(value),
            "stroke" => style.stroke = Some(value),
            "color" => style.color = Some(value),
            "stroke-width" => match parse_number(&value) {
                Some(n) => style.stroke_width = Some(n),
                None => style.other.push((key, value)),
            },
            "stroke-dasharray" => match parse_dasharray(&value) {
                Some(d) => style.stroke_dasharray = Some(d),
                None => style.other.push((key, value)),
            },
            _ => style.other.push((key, value)),
        }
    }
    (style, bad)
}
