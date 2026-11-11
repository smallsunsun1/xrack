use serde_json::{Value, json};
use xrack_parser::ast::{Diagnostic, Span};

#[derive(Default)]
pub(crate) struct Analysis {
    pub(crate) diagnostics: Vec<Value>,
    pub(crate) symbols: Vec<Value>,
}

pub(crate) fn analyze(uri: &str, text: &str) -> Analysis {
    let source = Source::new(text);
    let document = match xrack_parser::parse(text) {
        Ok(document) => document,
        Err(errors) => {
            return Analysis {
                diagnostics: source.diagnostics(errors, "syntax"),
                ..Analysis::default()
            };
        }
    };

    let symbols = document
        .components
        .iter()
        .map(|component| {
            let declaration = source.offset(component.span);
            let name = source.name_offset(declaration, component.exported);
            let range = if source.text[name..].starts_with(&component.name) {
                source.range(name, name + component.name.len())
            } else {
                source.range(declaration, declaration)
            };
            // A flat SymbolInformation location only needs to cover the declaration name.
            json!({
                "name": component.name,
                "kind": 5,
                "location": { "uri": uri, "range": range },
            })
        })
        .collect();

    let mut diagnostics = Vec::new();
    // The compiler's semantic entry point expects imports to have been resolved first.
    if document.imports.is_empty()
        && let Err(errors) = xrack_ir::semantic::for_each_component(&document, |_, _| {})
    {
        diagnostics = source.diagnostics(errors, "semantic");
    }
    Analysis {
        diagnostics,
        symbols,
    }
}

pub(crate) struct Source<'a> {
    text: &'a str,
    lines: Vec<usize>,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let lines = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(offset, _)| offset + 1))
            .collect();
        Self { text, lines }
    }

    fn offset(&self, span: Span) -> usize {
        let line = span.line.saturating_sub(1).min(self.lines.len() - 1);
        let mut start = self.lines[line];
        let end = self.lines.get(line + 1).copied().unwrap_or(self.text.len());
        if line == 0 && self.text.starts_with('\u{feff}') {
            start += '\u{feff}'.len_utf8(); // The parser strips the initial BOM.
        }
        let text = self.text[start..end].trim_end_matches(['\r', '\n']);
        // ANTLR columns count Unicode scalar values. Convert to bytes first.
        start
            + text
                .char_indices()
                .nth(span.column.saturating_sub(1))
                .map_or(text.len(), |(i, _)| i)
    }

    pub(crate) fn coordinates(&self, offset: usize) -> (usize, usize) {
        let line = self.lines.partition_point(|&start| start <= offset) - 1;
        (
            line,
            self.text[self.lines[line]..offset].encode_utf16().count(),
        )
    }

    fn position(&self, offset: usize) -> Value {
        let (line, character) = self.coordinates(offset);
        json!({ "line": line, "character": character })
    }

    pub(crate) fn offset_at(&self, line: usize, character: usize) -> Option<usize> {
        let &start = self.lines.get(line)?;
        let end = self.lines.get(line + 1).copied().unwrap_or(self.text.len());
        let line = self.text[start..end].trim_end_matches(['\r', '\n']);
        let mut column = 0;
        for (offset, ch) in line.char_indices() {
            if column >= character {
                return (column == character).then_some(start + offset);
            }
            column += ch.len_utf16();
        }
        (column == character).then_some(start + line.len())
    }

    pub(crate) fn range(&self, start: usize, end: usize) -> Value {
        json!({ "start": self.position(start), "end": self.position(end) })
    }

    fn diagnostics(&self, errors: Vec<Diagnostic>, phase: &str) -> Vec<Value> {
        errors
            .into_iter()
            .map(|error| {
                let start = self.offset(error.span);
                let rest = &self.text[start..];
                // The compiler supplies a start point. Highlight the identifier or one character.
                let word = rest
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    .count();
                let width = if word > 0 {
                    word
                } else {
                    rest.chars()
                        .next()
                        .filter(|c| !matches!(c, '\r' | '\n'))
                        .map_or(0, char::len_utf8)
                };
                json!({
                    "range": self.range(start, start + width),
                    "severity": 1,
                    "source": "xrack",
                    "code": phase,
                    "message": error.message,
                })
            })
            .collect()
    }

    fn name_offset(&self, declaration: usize, exported: bool) -> usize {
        let mut header = &self.text[declaration..];
        if exported {
            header = skip_trivia(header.strip_prefix("export").unwrap_or(header));
        }
        let name = skip_trivia(header.strip_prefix("component").unwrap_or(header));
        self.text.len() - name.len()
    }
}

fn skip_trivia(mut text: &str) -> &str {
    loop {
        text = text.trim_start_matches([' ', '\t', '\r', '\n', '\u{feff}']);
        if let Some(comment) = text.strip_prefix("//") {
            text = comment.find(['\r', '\n']).map_or("", |end| &comment[end..]);
        } else if let Some(comment) = text.strip_prefix("/*") {
            text = comment.split_once("*/").map_or("", |(_, rest)| rest);
        } else {
            return text;
        }
    }
}
