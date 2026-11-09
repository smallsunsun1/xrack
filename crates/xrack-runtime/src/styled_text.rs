use crate::Color;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{borrow::Cow, sync::Arc};
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

/// A link's visible text range and its application-defined destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledLink {
    pub start: usize,
    pub end: usize,
    pub href: Cow<'static, str>,
}

#[derive(Clone, Debug)]
enum Storage {
    Compiled(&'static str, &'static [StyledRun], &'static [StyledLink]),
    Owned(Arc<str>, Arc<[StyledRun]>, Arc<[StyledLink]>),
}

/// Immutable text with inline styles. Cloning shares the underlying data.
#[derive(Clone, Debug)]
pub struct StyledText(Storage);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StyledTextError {
    #[error("style ranges must be ordered, nonempty and aligned to grapheme boundaries")]
    InvalidRange,
    #[error("link ranges must be ordered, nonempty and aligned to grapheme boundaries")]
    InvalidLinkRange,
    #[error("unsupported Markdown syntax: {0}")]
    Unsupported(&'static str),
}

impl StyledText {
    #[doc(hidden)]
    pub const fn from_compiled(text: &'static str, runs: &'static [StyledRun]) -> Self {
        Self::from_compiled_parts(text, runs, &[])
    }

    #[doc(hidden)]
    pub const fn from_compiled_parts(
        text: &'static str,
        runs: &'static [StyledRun],
        links: &'static [StyledLink],
    ) -> Self {
        Self(Storage::Compiled(text, runs, links))
    }

    pub fn from_plain_text(text: impl Into<String>) -> Self {
        Self(Storage::Owned(
            text.into().into(),
            Arc::default(),
            Arc::default(),
        ))
    }

    pub fn from_runs(
        text: impl Into<String>,
        runs: Vec<StyledRun>,
    ) -> Result<Self, StyledTextError> {
        Self::from_parts(text, runs, Vec::new())
    }

    pub fn from_parts(
        text: impl Into<String>,
        runs: Vec<StyledRun>,
        links: Vec<StyledLink>,
    ) -> Result<Self, StyledTextError> {
        let text = text.into();
        if !valid_ranges(&text, runs.iter().map(|run| (run.start, run.end))) {
            return Err(StyledTextError::InvalidRange);
        }
        if !valid_ranges(&text, links.iter().map(|link| (link.start, link.end))) {
            return Err(StyledTextError::InvalidLinkRange);
        }
        Ok(Self(Storage::Owned(text.into(), runs.into(), links.into())))
    }

    pub fn text(&self) -> &str {
        match &self.0 {
            Storage::Compiled(text, ..) => text,
            Storage::Owned(text, ..) => text,
        }
    }

    pub fn runs(&self) -> &[StyledRun] {
        match &self.0 {
            Storage::Compiled(_, runs, _) => runs,
            Storage::Owned(_, runs, _) => runs,
        }
    }

    pub fn links(&self) -> &[StyledLink] {
        match &self.0 {
            Storage::Compiled(_, _, links) => links,
            Storage::Owned(_, _, links) => links,
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
                        Tag::Link { dest_url, .. } => builder.begin_link(dest_url.into_string()),
                        Tag::Image { .. } => return Err(StyledTextError::Unsupported("images")),
                        _ => return Err(StyledTextError::Unsupported("block or inline element")),
                    }
                }
                Event::End(tag) => {
                    style = stack.pop().expect("balanced Markdown events");
                    if tag == TagEnd::Link {
                        builder.end_link();
                    }
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
        self.text() == other.text() && self.runs() == other.runs() && self.links() == other.links()
    }
}

impl Eq for StyledText {}

fn valid_ranges(text: &str, ranges: impl Iterator<Item = (usize, usize)>) -> bool {
    let mut boundaries = text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .peekable();
    let mut previous_end = 0;
    for (start, end) in ranges {
        if start < previous_end || start >= end {
            return false;
        }
        for offset in [start, end] {
            while boundaries.peek().is_some_and(|&index| index < offset) {
                boundaries.next();
            }
            if boundaries.peek().copied() != Some(offset) {
                return false;
            }
        }
        previous_end = end;
    }
    true
}

#[derive(Default)]
struct Markdown {
    text: String,
    runs: Vec<StyledRun>,
    links: Vec<StyledLink>,
    active_link: Option<StyledLink>,
    paragraph_break: bool,
}

impl Markdown {
    fn begin_link(&mut self, href: String) {
        self.active_link = Some(StyledLink {
            start: 0,
            end: 0,
            href: Cow::Owned(href),
        });
    }

    fn end_link(&mut self) {
        if let Some(link) = self.active_link.take().filter(|link| link.start < link.end) {
            self.links.push(link);
        }
    }

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
        if let Some(link) = &mut self.active_link {
            if link.start == link.end {
                link.start = start;
            }
            link.end = self.text.len();
        }
        push_run(&mut self.runs, start, self.text.len(), style);
    }

    fn finish(self) -> StyledText {
        // Markdown delimiters can fall between the code points of one grapheme.
        // The base code point's style applies to the entire grapheme in that case.
        let mut runs = Vec::new();
        let mut links = self.links.iter().peekable();
        let mut normalized_links: Vec<StyledLink> = Vec::new();
        let mut last_link_start = None;
        let mut index = 0;
        for (start, grapheme) in self.text.grapheme_indices(true) {
            while self.runs.get(index).is_some_and(|run| run.end <= start) {
                index += 1;
            }
            while links.peek().is_some_and(|link| link.end <= start) {
                links.next();
            }
            if let Some(link) = links.peek().filter(|link| link.start <= start) {
                let end = start + grapheme.len();
                if last_link_start == Some(link.start) {
                    normalized_links.last_mut().unwrap().end = end;
                } else {
                    normalized_links.push(StyledLink {
                        start,
                        end,
                        href: link.href.clone(),
                    });
                    last_link_start = Some(link.start);
                }
            }
            if let Some(run) = self.runs.get(index).filter(|run| run.start <= start) {
                push_run(&mut runs, start, start + grapheme.len(), run.style);
            }
        }
        StyledText(Storage::Owned(
            self.text.into(),
            runs.into(),
            normalized_links.into(),
        ))
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
    fn markdown_link_ranges_keep_graphemes_and_distinguish_adjacent_anchors() {
        let text = StyledText::from_markdown("[a](#first)&#769;[**b**](#first)").unwrap();
        assert_eq!(text.text(), "a\u{301}b");
        assert_eq!(
            text.links(),
            &[
                StyledLink {
                    start: 0,
                    end: 3,
                    href: "#first".into()
                },
                StyledLink {
                    start: 3,
                    end: 4,
                    href: "#first".into()
                },
            ]
        );
        assert!(text.runs()[0].style.bold);
        assert_eq!(
            StyledText::from_parts(
                text.text(),
                vec![],
                vec![StyledLink {
                    start: 0,
                    end: 1,
                    href: "#invalid".into()
                },]
            )
            .unwrap_err(),
            StyledTextError::InvalidLinkRange,
        );
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
