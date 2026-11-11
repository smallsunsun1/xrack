use std::ops::Range;

pub(crate) struct Token<'a> {
    pub(crate) text: &'a str,
    pub(crate) range: Range<usize>,
    // A cursor immediately after an unterminated string or comment is still inside it.
    opaque: Option<bool>,
}

impl Token<'_> {
    pub(crate) fn blocks_completion(&self, cursor: usize) -> bool {
        self.opaque.is_some_and(|closed| {
            self.range.start < cursor
                && (cursor < self.range.end || !closed && cursor == self.range.end)
        })
    }

    pub(crate) fn code(&self) -> bool {
        self.opaque.is_none()
    }

    pub(crate) fn quoted(&self) -> bool {
        self.opaque == Some(true) && self.text.starts_with('"')
    }
}

pub(crate) fn name(text: &str) -> bool {
    text.as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
}

/// A tolerant token scan for locating the cursor in incomplete source. Parsing and
/// diagnostics still go through ANTLR.
pub(crate) fn scan(mut text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = 0;
    while let Some(c) = text.chars().next() {
        let mut opaque = None;
        let len = if let Some(rest) = text.strip_prefix("//") {
            opaque = Some(false);
            2 + rest.find(['\r', '\n']).unwrap_or(rest.len())
        } else if let Some(rest) = text.strip_prefix("/*") {
            let end = rest.find("*/");
            opaque = Some(end.is_some());
            end.map_or(text.len(), |end| end + 4)
        } else if c == '"' {
            let (len, closed) = quoted(text);
            opaque = Some(closed);
            len
        } else if c.is_ascii_digit() {
            number(text)
        } else if c == '#' {
            1 + text
                .bytes()
                .skip(1)
                .take_while(u8::is_ascii_hexdigit)
                .count()
        } else if name(text) || c == '@' {
            text.bytes()
                .skip(1)
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                .count()
                + 1
        } else {
            [
                "<=>", ":=", "=>", "->", "+=", "-=", "==", "!=", "<=", ">=", "&&", "||",
            ]
            .into_iter()
            .find(|operator| text.starts_with(operator))
            .map_or(c.len_utf8(), str::len)
        };
        if !c.is_whitespace() && c != '\u{feff}' {
            tokens.push(Token {
                text: &text[..len],
                range: start..start + len,
                opaque,
            });
        }
        start += len;
        text = &text[len..];
    }
    tokens
}

fn quoted(text: &str) -> (usize, bool) {
    let mut escaped = false;
    for (offset, c) in text.char_indices().skip(1) {
        match c {
            '\r' | '\n' => return (offset, false),
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return (offset + 1, true),
            _ => {}
        }
    }
    (text.len(), false)
}

fn number(text: &str) -> usize {
    let digits = |text: &str| text.bytes().take_while(u8::is_ascii_digit).count();
    let mut end = digits(text);
    if let Some(fraction) = text[end..].strip_prefix('.') {
        let count = digits(fraction);
        if count != 0 {
            end += 1 + count;
        }
    }
    if let Some(exponent) = text[end..].strip_prefix(['e', 'E']) {
        let sign = usize::from(exponent.starts_with(['+', '-']));
        let count = digits(&exponent[sign..]);
        if count != 0 {
            end += 1 + sign + count;
        }
    }
    if let Some(unit) = ["px", "ms", "deg", "%"].into_iter().find(|unit| {
        text[end..]
            .strip_prefix(unit)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
    }) {
        end += unit.len();
    }
    end
}
