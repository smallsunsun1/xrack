use crate::completion::Workspace;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use xrack_ir::modules::{ModuleError, normalize_source_path, resolve_document};
use xrack_parser::ast::{Diagnostic, Span};

#[derive(Default)]
pub(crate) struct Analysis {
    pub(crate) diagnostics: BTreeMap<String, Vec<Value>>,
    pub(crate) symbols: Vec<Value>,
    pub(crate) inputs: BTreeMap<PathBuf, Span>,
}

pub(crate) fn analyze(uri: &str, text: &str, workspace: Option<&Workspace<'_>>) -> Analysis {
    let source = Source::new(text);
    let document = match xrack_parser::parse(text) {
        Ok(document) => document,
        Err(errors) => {
            return Analysis {
                diagnostics: BTreeMap::from([(uri.into(), source.diagnostics(errors, "syntax"))]),
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

    let mut analysis = Analysis {
        symbols,
        ..Analysis::default()
    };
    let mut files = Vec::new();
    let first_import = document.imports.first().map(|import| import.span);
    let document = if let Some(workspace) = workspace {
        match resolve_document(
            document,
            &workspace.entry,
            None,
            &workspace.buffers(),
            &mut analysis.inputs,
            Some(&mut files),
        ) {
            Ok(document) => document,
            Err(ModuleError::Diagnostics {
                filename,
                diagnostics,
                syntax,
            }) => {
                let file = normalize_source_path(Path::new(&filename));
                let import = analysis.inputs.get(&file).copied();
                for error in diagnostics {
                    analysis.report(
                        uri,
                        &source,
                        workspace,
                        &file,
                        error,
                        if syntax { "syntax" } else { "semantic" },
                        import,
                    );
                }
                return analysis;
            }
            Err(ModuleError::Io {
                path,
                source: error,
            }) => {
                analysis.report(
                    uri,
                    &source,
                    workspace,
                    &path,
                    Diagnostic {
                        span: first_import.unwrap_or_default(),
                        message: error.to_string(),
                    },
                    "semantic",
                    first_import,
                );
                return analysis;
            }
        }
    } else if first_import.is_none() {
        document
    } else {
        analysis.diagnostics.insert(
            uri.into(),
            source.diagnostics(
                vec![Diagnostic {
                    span: first_import.unwrap_or_default(),
                    message: "save the document to resolve relative imports".into(),
                }],
                "semantic",
            ),
        );
        return analysis;
    };
    if let Err(errors) = xrack_ir::semantic::for_each_component(&document, |_, _| {}) {
        for error in errors {
            if let Some((workspace, file)) =
                workspace.zip(error.span.source.checked_sub(1).and_then(|i| files.get(i)))
            {
                let import = analysis.inputs.get(file).copied();
                analysis.report(uri, &source, workspace, file, error, "semantic", import);
            } else {
                analysis
                    .diagnostics
                    .entry(uri.into())
                    .or_default()
                    .push(source.diagnostic(error, "semantic"));
            }
        }
    }
    analysis
}

impl Analysis {
    #[allow(clippy::too_many_arguments)]
    fn report(
        &mut self,
        uri: &str,
        source: &Source<'_>,
        workspace: &Workspace<'_>,
        file: &Path,
        error: Diagnostic,
        phase: &str,
        import: Option<Span>,
    ) {
        let file = normalize_source_path(file);
        if file == normalize_source_path(&workspace.entry) {
            self.diagnostics
                .entry(uri.into())
                .or_default()
                .push(source.diagnostic(error, phase));
        } else if let Some((target, text)) = workspace.uri(&file).zip(workspace.load(&file)) {
            let message = error.message.clone();
            let diagnostic = Source::new(&text).diagnostic(error, phase);
            if let Some(span) = import {
                let mut dependent = source.diagnostic(
                    Diagnostic {
                        span,
                        message: format!(
                            "{}: {message}",
                            file.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    },
                    "import",
                );
                dependent["relatedInformation"] = json!([{
                    "location": { "uri": target, "range": diagnostic["range"] }, "message": message,
                }]);
                self.diagnostics
                    .entry(uri.into())
                    .or_default()
                    .push(dependent);
            }
            self.diagnostics.entry(target).or_default().push(diagnostic);
        }
    }
}

pub(crate) struct Source<'a> {
    text: &'a str,
    lines: Cow<'a, [usize]>,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            lines: Cow::Owned(Self::line_starts(text)),
        }
    }

    pub(crate) fn line_starts(text: &str) -> Vec<usize> {
        std::iter::once(0)
            .chain(text.match_indices('\n').map(|(offset, _)| offset + 1))
            .collect()
    }

    pub(crate) fn with_lines(text: &'a str, lines: &'a [usize]) -> Self {
        Self {
            text,
            lines: Cow::Borrowed(lines),
        }
    }

    pub(crate) fn offset(&self, span: Span) -> usize {
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
            self.text[self.lines[line]..offset]
                .trim_end_matches('\r')
                .encode_utf16()
                .count(),
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
            .map(|error| self.diagnostic(error, phase))
            .collect()
    }

    fn diagnostic(&self, error: Diagnostic, phase: &str) -> Value {
        let start = self.offset(error.span);
        let rest = &self.text[start..];
        // The compiler supplies a start point. Highlight the identifier or one character.
        let intrinsic_prefix = usize::from(rest.starts_with('@'));
        let word = rest[intrinsic_prefix..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            .count();
        let mut width = if word > 0 {
            intrinsic_prefix + word
        } else {
            rest.chars()
                .next()
                .filter(|c| !matches!(c, '\r' | '\n'))
                .map_or(0, char::len_utf8)
        };
        if phase == "semantic"
            && xrack_parser::builtins::NAMESPACES.contains(&&rest[..width])
            && let Some(member) = rest[width..].strip_prefix('.')
        {
            width += 1 + member
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                .count();
        }
        let mut diagnostic = json!({
            "range": self.range(start, start + width),
            "severity": 1,
            "source": "xrack",
            "code": phase,
            "message": error.message,
        });
        if phase == "semantic"
            && let Some(name) = missing_component(&error.message)
            && self.text.get(start..start + width) == Some(name)
        {
            diagnostic["data"] = json!({"missingComponent": name});
        }
        diagnostic
    }

    fn name_offset(&self, declaration: usize, exported: bool) -> usize {
        let mut header = &self.text[declaration..];
        if exported {
            header = skip_trivia(header.strip_prefix("export").unwrap_or(header));
        }
        let name = skip_trivia(
            header
                .strip_prefix("component")
                .or_else(|| header.strip_prefix("global"))
                .unwrap_or(header),
        );
        self.text.len() - name.len()
    }
}

fn missing_component(message: &str) -> Option<&str> {
    message
        .strip_prefix("unknown element or component `")
        .and_then(|name| name.strip_suffix('`'))
        .or_else(|| {
            message
                .strip_prefix("unknown base element or component `")
                .and_then(|name| name.strip_suffix('`'))
        })
        .or_else(|| {
            message
                .strip_prefix("component or element `")
                .and_then(|name| name.strip_suffix("` is not in scope"))
        })
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
