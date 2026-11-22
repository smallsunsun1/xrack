use crate::Color;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{borrow::Cow, sync::Arc};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

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

    /// Rewrite link destinations and add links to runs outside explicit links.
    /// Ranges come from the document's validated annotations, in source order.
    pub fn rewrite_links(
        &self,
        mut destination: impl FnMut(&StyledLink) -> Option<Cow<'static, str>>,
        mut run_destination: impl FnMut(&StyledRun) -> Option<Cow<'static, str>>,
    ) -> Self {
        let mut remap = |link: &StyledLink| {
            destination(link).map(|href| StyledLink {
                start: link.start,
                end: link.end,
                href,
            })
        };
        let mut explicit = self.links().iter().peekable();
        let mut links = Vec::new();
        for run in self.runs() {
            while let Some(link) = explicit.next_if(|link| link.end <= run.start) {
                links.extend(remap(link));
            }
            if explicit.peek().is_none_or(|link| link.start >= run.end)
                && let Some(href) = run_destination(run)
            {
                links.push(StyledLink {
                    start: run.start,
                    end: run.end,
                    href,
                });
            }
        }
        links.extend(explicit.filter_map(remap));
        if links == self.links() {
            return self.clone();
        }
        let (text, runs) = match &self.0 {
            Storage::Compiled(text, runs, _) => (Arc::from(*text), Arc::from(*runs)),
            Storage::Owned(text, runs, _) => (text.clone(), runs.clone()),
        };
        Self(Storage::Owned(text, runs, links.into()))
    }

    pub fn from_markdown(source: &str) -> Result<Self, StyledTextError> {
        let mut builder = Markdown::default();
        for event in Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH) {
            builder.push_event(event)?;
        }
        Ok(builder.finish())
    }
}

/// Keeps a verified prefix when a Markdown document is updated repeatedly.
#[derive(Default)]
pub struct MarkdownStream {
    sealed_source: String,
    document: StyledText,
    checkpoint: Option<MarkdownCheckpoint>,
}

#[derive(Clone, Copy)]
struct MarkdownCheckpoint {
    source_end: usize,
    text_end: usize,
}

impl MarkdownStream {
    pub fn update(&mut self, source: &str) -> Result<StyledText, StyledTextError> {
        let mut checkpoint = self.checkpoint.filter(|checkpoint| {
            // A BOM at the suffix start would be interpreted as a document BOM.
            source.starts_with(&self.sealed_source)
                && !source[checkpoint.source_end..].starts_with('\u{feff}')
        });
        let mut offset = checkpoint.map_or(0, |prefix| prefix.source_end);
        let mut parser = Parser::new_ext(&source[offset..], Options::ENABLE_STRIKETHROUGH);
        // Reference definitions can change earlier links, even across blocks.
        if offset != 0 && parser.reference_definitions().iter().next().is_some() {
            checkpoint = None;
            offset = 0;
            parser = Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH);
        }
        let reference_free = parser.reference_definitions().iter().next().is_none();
        let mut builder = checkpoint.map_or_else(Markdown::default, |prefix| {
            Markdown::from_prefix(&self.document, prefix.text_end)
        });
        for (event, range) in parser.into_offset_iter() {
            let paragraph = matches!(event, Event::End(TagEnd::Paragraph));
            builder.push_event(event)?;
            if reference_free
                && paragraph
                && builder.stack.is_empty()
                && !builder.text.is_empty()
                && !builder.text.ends_with(['\r', '\n'])
                && let Some(source_end) = sealed_paragraph_end(source, offset + range.end)
            {
                checkpoint = Some(MarkdownCheckpoint {
                    source_end,
                    text_end: builder.text.len(),
                });
            }
        }
        let document = builder.finish();
        self.sealed_source.truncate(offset);
        if let Some(checkpoint) = checkpoint {
            self.sealed_source
                .push_str(&source[offset..checkpoint.source_end]);
            self.document = document.clone();
        } else {
            self.document = StyledText::default();
        }
        self.checkpoint = checkpoint;
        Ok(document)
    }
}

// OffsetIter puts a paragraph's end at the following line. A complete blank
// line proves that later input cannot extend the paragraph or turn it into a heading.
fn sealed_paragraph_end(source: &str, paragraph_end: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(paragraph_end.checked_sub(1)?) != Some(&b'\n') {
        return None;
    }
    let spaces = bytes[paragraph_end..]
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .count();
    let blank = paragraph_end + spaces;
    match bytes.get(blank..) {
        Some([b'\n', ..]) => Some(blank + 1),
        Some([b'\r', b'\n', ..]) => Some(blank + 2),
        _ => None,
    }
}

impl Default for StyledText {
    fn default() -> Self {
        Self::from_compiled("", &[])
    }
}

impl PartialEq for StyledText {
    fn eq(&self, other: &Self) -> bool {
        // Scene snapshots and link targets often clone the immutable storage.
        if std::ptr::eq(self.text(), other.text())
            && std::ptr::eq(self.runs(), other.runs())
            && std::ptr::eq(self.links(), other.links())
        {
            return true;
        }
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
    style: TextStyle,
    stack: Vec<TextStyle>,
    trusted_runs: usize,
    trusted_links: usize,
    paragraph_break: bool,
    lists: Vec<Option<u64>>,
}

impl Markdown {
    fn from_prefix(document: &StyledText, end: usize) -> Self {
        // Appending the next block inserts a paragraph break. The trusted
        // prefix's graphemes and annotations cannot extend into that block.
        let trusted_runs = document.runs().partition_point(|run| run.end <= end);
        let trusted_links = document.links().partition_point(|link| link.end <= end);
        Self {
            text: document.text()[..end].to_owned(),
            runs: document.runs()[..trusted_runs].to_vec(),
            links: document.links()[..trusted_links].to_vec(),
            trusted_runs,
            trusted_links,
            paragraph_break: true,
            ..Self::default()
        }
    }

    fn push_event(&mut self, event: Event<'_>) -> Result<(), StyledTextError> {
        match event {
            Event::Start(tag) => {
                self.stack.push(self.style);
                match tag {
                    Tag::Paragraph => {}
                    Tag::Strong | Tag::Heading { .. } => self.style.bold = true,
                    Tag::Emphasis => self.style.italic = true,
                    Tag::CodeBlock(_) => self.style.monospace = true,
                    Tag::Strikethrough => self.style.strikethrough = true,
                    Tag::List(start) => self.begin_list(start),
                    Tag::Item => self.begin_item(self.style),
                    Tag::HtmlBlock => return Err(StyledTextError::Unsupported("HTML")),
                    Tag::Link { dest_url, .. } => self.begin_link(dest_url.into_string()),
                    Tag::Image { .. } => return Err(StyledTextError::Unsupported("images")),
                    _ => return Err(StyledTextError::Unsupported("block or inline element")),
                }
            }
            Event::End(tag) => {
                self.style = self.stack.pop().unwrap_or_default();
                if tag == TagEnd::Link {
                    self.end_link();
                }
                if matches!(tag, TagEnd::List(_)) {
                    self.lists.pop();
                    self.paragraph_break = true;
                }
                if matches!(
                    tag,
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock
                ) {
                    self.paragraph_break = true;
                }
            }
            Event::Text(text) => self.append(&text, self.style),
            Event::Code(text) => self.append(
                &text,
                TextStyle {
                    monospace: true,
                    ..self.style
                },
            ),
            Event::SoftBreak => self.append(" ", self.style),
            Event::HardBreak => self.append("\n", self.style),
            Event::Html(_) | Event::InlineHtml(_) => {
                return Err(StyledTextError::Unsupported("HTML"));
            }
            _ => return Err(StyledTextError::Unsupported("inline element")),
        }
        Ok(())
    }

    fn begin_list(&mut self, start: Option<u64>) {
        if self.lists.is_empty() {
            self.paragraph_break = true;
        }
        self.lists.push(start);
    }

    fn begin_item(&mut self, style: TextStyle) {
        let indent = "  ".repeat(self.lists.len().saturating_sub(1));
        let marker = match self.lists.last_mut() {
            Some(Some(next)) => {
                let marker = format!("{next}. ");
                *next = next.saturating_add(1);
                marker
            }
            Some(None) => "• ".into(),
            None => return,
        };
        if !self.paragraph_break && !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.append(&format!("{indent}{marker}"), style);
    }

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
        // The builder emits ordered, nonoverlapping ranges. Keep those ranges
        // if every endpoint is already a grapheme boundary. A single UTF-8
        // scalar on each side is sufficient for common boundaries. If the
        // cursor needs more context, segment the document once below.
        let boundary = |offset: usize| {
            if offset == 0 || offset == self.text.len() {
                return true;
            }
            let bytes = self.text.as_bytes();
            let (before, after) = (bytes[offset - 1], bytes[offset]);
            // The only joined ASCII pair in the grapheme rules is CRLF.
            if before.is_ascii() && after.is_ascii() {
                return (before, after) != (b'\r', b'\n');
            }
            let context = char::MAX.len_utf8();
            let start = self
                .text
                .floor_char_boundary(offset.saturating_sub(context));
            let end = self
                .text
                .ceil_char_boundary((offset + context).min(self.text.len()));
            GraphemeCursor::new(offset, self.text.len(), true)
                .is_boundary(&self.text[start..end], start)
                == Ok(true)
        };
        let (runs, links) = if self.runs[self.trusted_runs..]
            .iter()
            .all(|run| boundary(run.start) && boundary(run.end))
            && self.links[self.trusted_links..]
                .iter()
                .all(|link| boundary(link.start) && boundary(link.end))
        {
            (self.runs, self.links)
        } else {
            self.normalize_ranges()
        };
        StyledText(Storage::Owned(self.text.into(), runs.into(), links.into()))
    }

    fn normalize_ranges(&self) -> (Vec<StyledRun>, Vec<StyledLink>) {
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
                if last_link_start == Some(link.start)
                    && let Some(previous) = normalized_links.last_mut()
                {
                    previous.end = end;
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
        (runs, normalized_links)
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
    fn streaming_markdown_keeps_a_verified_prefix() {
        let mut stream = MarkdownStream::default();
        let first = "**a**&#769; [link](#ref)\n\n";
        assert_eq!(stream.update(first), StyledText::from_markdown(first));
        assert_eq!(stream.sealed_source, first);
        let source = format!("{first}**تحديث** [خيار](#next)\n\nnew tail");
        assert_eq!(stream.update(&source), StyledText::from_markdown(&source));
        assert!(stream.sealed_source.ends_with("](#next)\n\n"));
        let edited = source.replace("link", "updated link");
        assert_eq!(stream.update(&edited), StyledText::from_markdown(&edited));
    }

    #[test]
    fn streamed_prefixes_equal_complete_markdown_with_context_changes() {
        for source in [
            "前文 **你好** `code`\n\n尾部 [港](file.rs:8)\n\n🇩🇪🇨[🇦](#flag) [🧑](#dev)&#8205;💻",
            "[forward][key]\n\ncontinue\n\n[key]: /later\n\n[key]",
            "> quoted \n> a block\n\nlead\n\n```rs\nlet a = [1; 4];\n\n```\n\ntail",
            "one\r\n  \r\n**two**\r\n\r\nlast",
            "begin\n\n\u{feff}following file `t.rs`\n\nend",
            "before\n\nx&#10;&#10;\n\n**a**&#769;\n\nplain\n---",
            "safe\n\n<p>unsupported</p>\n\nafter",
            "- list item\n\n- more\n\nparagraph\n\n    indented code",
        ] {
            let mut stream = MarkdownStream::default();
            for end in source
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([source.len()])
            {
                let prefix = &source[..end];
                assert_eq!(
                    stream.update(prefix),
                    StyledText::from_markdown(prefix),
                    "{prefix:?}"
                );
            }
            for end in source.char_indices().map(|(offset, _)| offset).rev() {
                let prefix = &source[..end];
                assert_eq!(
                    stream.update(prefix),
                    StyledText::from_markdown(prefix),
                    "{prefix:?}"
                );
            }
        }
    }

    #[test]
    fn markdown_lists_keep_order_indentation_and_inline_styles() {
        let text = StyledText::from_markdown(
            "计划\n\n3. **构建**示例\n   - 运行测试\n4. 检查输出\n\n完成后记录结果",
        )
        .unwrap();
        assert!(text.text().contains("3. 构建示例\n  • 运行测试"));
        assert!(text.text().contains("4. 检查输出\n\n完成后记录结果"));
        assert!(
            text.runs()
                .iter()
                .any(|run| run.style.bold && &text.text()[run.start..run.end] == "构建")
        );
    }

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

    #[test]
    fn markdown_link_boundaries_preserve_flags_and_joined_emoji() {
        let document = StyledText::from_markdown("🇩🇪🇨[🇦](#flag) [🧑](#dev)&#8205;💻").unwrap();
        assert_eq!(document.text(), "🇩🇪🇨🇦 🧑‍💻");
        let [link] = document.links() else {
            panic!("link inside a flag must be dropped and the emoji link kept");
        };
        assert_eq!(&document.text()[link.start..link.end], "🧑‍💻");
        assert_eq!(link.href, "#dev");
    }

    #[test]
    fn rewriting_links_shares_document_data_and_preserves_graphemes() {
        let original = StyledText::from_markdown("**a**&#769; [b](#old)").unwrap();
        let linked = original.rewrite_links(|_| None, |_| Some("#new".into()));
        assert!(std::ptr::eq(original.text(), linked.text()));
        assert!(std::ptr::eq(original.runs(), linked.runs()));
        assert_eq!(linked.links()[0].end, "a\u{301}".len());
        assert_eq!(linked.links()[0].href, "#new");
        assert_eq!(original.links()[0].href, "#old");
    }
}
