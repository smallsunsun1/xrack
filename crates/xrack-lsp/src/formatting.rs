use crate::{analysis::Source, editor_tokens};
use serde_json::{Value, json};
use xrack_parser::FormatToken;

pub(crate) fn edits(text: &str, options: &Value) -> Value {
    let Some(formatted) = format(text, options).filter(|formatted| formatted != text) else {
        return json!([]);
    };
    json!([{"range": Source::new(text).range(0, text.len()), "newText": formatted}])
}

struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    hint: FormatToken,
}

impl Token<'_> {
    fn comment(&self) -> bool {
        self.text.starts_with("//") || self.text.starts_with("/*")
    }

    fn opens_block(&self) -> bool {
        self.hint.block && matches!(self.text, "{" | "[")
    }

    fn closes_block(&self) -> bool {
        self.hint.block && matches!(self.text, "}" | "]")
    }

    fn opens_inline(&self) -> bool {
        !self.hint.block && matches!(self.text, "(" | "[" | "{")
    }

    fn closes_inline(&self) -> bool {
        !self.hint.block && matches!(self.text, ")" | "]" | "}")
    }
}

fn tokens(text: &str) -> Option<Vec<Token<'_>>> {
    let hints = xrack_parser::format_tokens(text).ok()?;
    let editor_tokens = editor_tokens::scan(text);
    let mut editor = editor_tokens.iter().peekable();
    let mut result = Vec::with_capacity(hints.len());
    for hint in hints {
        while let Some(token) = editor.next_if(|token| token.range.start < hint.offset) {
            if token.range.end <= hint.offset
                && (token.text.starts_with("//") || token.text.starts_with("/*"))
            {
                result.push(Token {
                    text: token.text,
                    start: token.range.start,
                    end: token.range.end,
                    hint: FormatToken::default(),
                });
            }
        }
        let end = hint.offset + hint.byte_len;
        result.push(Token {
            text: text.get(hint.offset..end)?,
            start: hint.offset,
            end,
            hint,
        });
    }
    result.extend(
        editor
            .filter(|token| token.text.starts_with("//") || token.text.starts_with("/*"))
            .map(|token| Token {
                text: token.text,
                start: token.range.start,
                end: token.range.end,
                hint: FormatToken::default(),
            }),
    );
    Some(result)
}

fn format(text: &str, options: &Value) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let tokens = tokens(text)?;
    let size = options["tabSize"].as_u64().unwrap_or(4).clamp(1, 32) as usize;
    let indent = if options["insertSpaces"] == false {
        "\t".into()
    } else {
        " ".repeat(size)
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(text.len());
    if text.starts_with('\u{feff}') {
        out.push('\u{feff}');
    }

    let mut delimiters = Vec::new();
    let mut multiline = vec![false; tokens.len()];
    for (index, token) in tokens.iter().enumerate() {
        if token.opens_inline() || token.opens_block() {
            delimiters.push(index);
        } else if (token.closes_inline() || token.closes_block())
            && let Some(open) = delimiters.pop()
        {
            multiline[open] = text[tokens[open].end..token.start].contains('\n');
        }
    }

    let mut level = 0usize;
    let mut inline = Vec::new();
    let mut previous: Option<&Token<'_>> = None;
    for (index, token) in tokens.iter().enumerate() {
        let multiline_close = token.closes_inline() && inline.pop() == Some(true);
        if token.closes_block() || multiline_close {
            level = level.saturating_sub(1);
        }
        if let Some(prev) = previous {
            let original = &text[prev.end..token.start];
            let breaks = original.bytes().filter(|&byte| byte == b'\n').count();
            let empty = prev.opens_block() && token.closes_block();
            let trailing_comment = token.comment() && breaks == 0;
            let split = !empty
                && !trailing_comment
                && (prev.opens_block()
                    || token.closes_block()
                    || prev.text == ";"
                    || prev.closes_block() && !matches!(token.text, ";" | "," | ")" | "]")
                    || prev.text == "," && inline.is_empty()
                    || prev.text.starts_with("//")
                    || multiline_close
                    || breaks != 0);
            if split {
                out.push_str(newline);
                if breaks > 1 {
                    out.push_str(newline);
                }
                for _ in 0..level {
                    out.push_str(&indent);
                }
            } else if !empty && space(prev, token) {
                out.push(' ');
            }
        }
        out.push_str(token.text);
        if token.opens_block() {
            level += 1;
        } else if token.opens_inline() {
            // Keep multiline literals and argument lists, indenting their
            // contents without wrapping short lists.
            let multiline = multiline[index];
            inline.push(multiline);
            if multiline {
                level += 1;
            }
        }
        previous = Some(token);
    }

    let trailing = tokens.last().map_or(text, |token| &text[token.end..]);
    let existing = trailing.bytes().filter(|&byte| byte == b'\n').count();
    let final_lines = if options["trimFinalNewlines"] == false {
        existing.max(usize::from(options["insertFinalNewline"] != false))
    } else {
        usize::from(existing > 0 || options["insertFinalNewline"] != false)
    };
    for _ in 0..final_lines {
        out.push_str(newline);
    }
    // The editor scan retains comments and the parser supplies exact tokens.
    // Recheck the result before emitting an edit in case token spacing changes.
    let input = tokens
        .iter()
        .filter(|token| !token.comment())
        .map(|token| token.text);
    let same_code = xrack_parser::same_tokens(&out, input);
    let same_comments = tokens
        .iter()
        .filter(|token| token.comment())
        .map(|token| token.text)
        .eq(editor_tokens::scan(&out)
            .into_iter()
            .filter(|token| token.text.starts_with("//") || token.text.starts_with("/*"))
            .map(|token| token.text));
    (same_code && same_comments).then_some(out)
}

fn space(previous: &Token<'_>, token: &Token<'_>) -> bool {
    if previous.comment() || token.comment() {
        return true;
    }
    if previous.hint.attach_after || token.hint.attach_before {
        return false;
    }
    if matches!(token.text, "," | ";" | "." | ")" | "]") || matches!(previous.text, "." | "(" | "[")
    {
        return false;
    }
    if token.text == ":" && !token.hint.space_before {
        return false;
    }
    true
}
