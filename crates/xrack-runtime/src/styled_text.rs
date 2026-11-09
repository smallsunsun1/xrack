use crate::Color;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub color: Option<Color>,
}

/// A half-open UTF-8 byte range. Gaps use the element's base style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyledRun {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

#[derive(Clone, Debug)]
enum Storage {
    Compiled(&'static str, &'static [StyledRun]),
    Owned(Arc<str>, Arc<[StyledRun]>),
}

/// Immutable text with inline styles. Cloning shares the underlying data.
#[derive(Clone, Debug)]
pub struct StyledText(Storage);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StyledTextError {
    #[error("style ranges must be ordered, nonempty and aligned to grapheme boundaries")]
    InvalidRange,
    #[error("unsupported Markdown syntax: {0}")]
    Unsupported(&'static str),
}

impl StyledText {
    #[doc(hidden)]
    pub const fn from_compiled(text: &'static str, runs: &'static [StyledRun]) -> Self {
        Self(Storage::Compiled(text, runs))
    }

    pub fn from_plain_text(text: impl Into<String>) -> Self {
        Self(Storage::Owned(text.into().into(), Arc::default()))
    }

    pub fn from_runs(
        text: impl Into<String>,
        runs: Vec<StyledRun>,
    ) -> Result<Self, StyledTextError> {
        let text = text.into();
        let mut boundaries = text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .peekable();
        let mut end = 0;
        for run in &runs {
            if run.start < end || run.end <= run.start {
                return Err(StyledTextError::InvalidRange);
            }
            for offset in [run.start, run.end] {
                while boundaries.peek().is_some_and(|&index| index < offset) {
                    boundaries.next();
                }
                if boundaries.peek().copied() != Some(offset) {
                    return Err(StyledTextError::InvalidRange);
                }
            }
            end = run.end;
        }
        Ok(Self(Storage::Owned(text.into(), runs.into())))
    }

    pub fn text(&self) -> &str {
        match &self.0 {
            Storage::Compiled(text, _) => text,
            Storage::Owned(text, _) => text,
        }
    }

    pub fn runs(&self) -> &[StyledRun] {
        match &self.0 {
            Storage::Compiled(_, runs) => runs,
            Storage::Owned(_, runs) => runs,
        }
    }

    pub fn from_markdown(source: &str) -> Result<Self, StyledTextError> {
        let mut builder = Markdown::default();
        let mut style = TextStyle::default();
        let mut stack = Vec::new();
        for event in Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH) {
            match event {
                Event::Start(tag) => {
                    stack.push(style);
                    match tag {
                        Tag::Paragraph => {}
                        Tag::Strong | Tag::Heading { .. } => style.bold = true,
                        Tag::Emphasis => style.italic = true,
                        Tag::CodeBlock(_) => style.monospace = true,
                        Tag::Strikethrough => style.strikethrough = true,
                        Tag::HtmlBlock => return Err(StyledTextError::Unsupported("HTML")),
                        Tag::Link { .. } => return Err(StyledTextError::Unsupported("links")),
                        Tag::Image { .. } => return Err(StyledTextError::Unsupported("images")),
                        _ => return Err(StyledTextError::Unsupported("block or inline element")),
                    }
                }
                Event::End(tag) => {
                    style = stack.pop().expect("balanced Markdown events");
                    if matches!(
                        tag,
                        TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock
                    ) {
                        builder.paragraph_break = true;
                    }
                }
                Event::Text(text) => builder.append(&text, style),
                Event::Code(text) => builder.append(
                    &text,
                    TextStyle {
                        monospace: true,
                        ..style
                    },
                ),
                Event::SoftBreak => builder.append(" ", style),
                Event::HardBreak => builder.append("\n", style),
                Event::Html(_) | Event::InlineHtml(_) => {
                    return Err(StyledTextError::Unsupported("HTML"));
                }
                _ => return Err(StyledTextError::Unsupported("inline element")),
            }
        }
        Ok(builder.finish())
    }
}

impl Default for StyledText {
    fn default() -> Self {
        Self::from_compiled("", &[])
    }
}

impl PartialEq for StyledText {
    fn eq(&self, other: &Self) -> bool {
        self.text() == other.text() && self.runs() == other.runs()
    }
}

#[derive(Default)]
struct Markdown {
    text: String,
    runs: Vec<StyledRun>,
    paragraph_break: bool,
}

impl Markdown {
    fn append(&mut self, text: &str, style: TextStyle) {
        if text.is_empty() {
            return;
        }
        if std::mem::take(&mut self.paragraph_break) && !self.text.is_empty() {
            if !self.text.ends_with('\n') {
                self.text.push('\n');
            }
            if !self.text.ends_with("\n\n") {
                self.text.push('\n');
            }
        }
        let start = self.text.len();
        self.text.push_str(text);
        push_run(&mut self.runs, start, self.text.len(), style);
    }

    fn finish(self) -> StyledText {
        // Markdown delimiters can fall between the code points of one grapheme.
        // The base code point's style applies to the entire grapheme in that case.
        let mut runs = Vec::new();
        let mut index = 0;
        for (start, grapheme) in self.text.grapheme_indices(true) {
            while self.runs.get(index).is_some_and(|run| run.end <= start) {
                index += 1;
            }
            if let Some(run) = self.runs.get(index).filter(|run| run.start <= start) {
                push_run(&mut runs, start, start + grapheme.len(), run.style);
            }
        }
        StyledText(Storage::Owned(self.text.into(), runs.into()))
    }
}

fn push_run(runs: &mut Vec<StyledRun>, start: usize, end: usize, style: TextStyle) {
    if style == TextStyle::default() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.end == start
        && last.style == style
    {
        last.end = end;
    } else {
        runs.push(StyledRun { start, end, style });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_merges_nested_styles_and_preserves_paragraphs() {
        let text = StyledText::from_markdown("**粗体 *italic***\n\n`code` ~~old~~").unwrap();
        assert_eq!(text.text(), "粗体 italic\n\ncode old");
        let runs = text.runs();
        assert_eq!(runs.len(), 4);
        let value = |run: &StyledRun| &text.text()[run.start..run.end];
        assert_eq!(value(&runs[0]), "粗体 ");
        assert!(runs[0].style.bold && !runs[0].style.italic);
        assert_eq!(value(&runs[1]), "italic");
        assert!(runs[1].style.bold && runs[1].style.italic);
        assert_eq!(value(&runs[2]), "code");
        assert!(runs[2].style.monospace);
        assert_eq!(value(&runs[3]), "old");
        assert!(runs[3].style.strikethrough);
    }

    #[test]
    fn inline_styles_cannot_split_a_grapheme() {
        let plain = "a\u{301}🧑‍💻";
        let style = TextStyle {
            bold: true,
            ..Default::default()
        };
        let invalid = StyledRun {
            start: 0,
            end: 1,
            style,
        };
        assert_eq!(
            StyledText::from_runs(plain, vec![invalid]).unwrap_err(),
            StyledTextError::InvalidRange
        );
        let text = StyledText::from_markdown("**a**&#769;🧑‍💻").unwrap();
        assert_eq!(text.text(), plain);
        assert_eq!(
            text.runs(),
            &[StyledRun {
                end: "a\u{301}".len(),
                ..invalid
            }]
        );
    }
}
