use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{Color, Component, Node, ScrollAction, StyledRun, StyledText, TextStyle};

pub struct Section {
    pub start: usize,
    pub language: String,
    pub body: StyledText,
}

/// Partitions replies containing top-level fences without changing the
/// canonical rendered document used for search, links and quoting.
pub fn fenced_sections(source: &str, document: &StyledText) -> Vec<Section> {
    if !source.contains("```") && !source.contains("~~~") {
        return Vec::new();
    }
    sections(source, document).unwrap_or_default()
}

fn sections(source: &str, document: &StyledText) -> Option<Vec<Section>> {
    let mut sections = Vec::new();
    let mut source_end = 0;
    let mut text_end = 0;
    let mut depth = 0;
    let mut fence = None;
    for (event, range) in Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH).into_offset_iter()
    {
        match event {
            Event::Start(tag) => {
                if depth == 0
                    && let Tag::CodeBlock(CodeBlockKind::Fenced(info)) = tag
                {
                    let language = info.split_whitespace().next().unwrap_or("代码").to_owned();
                    fence = Some((range.start, language, String::new()));
                }
                depth += 1;
            }
            Event::Text(text) => {
                if let Some((_, _, contents)) = &mut fence {
                    contents.push_str(&text);
                }
            }
            Event::End(tag) => {
                depth -= 1;
                if tag == TagEnd::CodeBlock
                    && let Some((start, language, contents)) = fence.take()
                {
                    let prose = StyledText::from_markdown(source.get(source_end..start)?).ok()?;
                    append_section(
                        document,
                        &mut text_end,
                        &mut sections,
                        prose.text(),
                        String::new(),
                    )?;
                    append_section(document, &mut text_end, &mut sections, &contents, language)?;
                    source_end = range.end;
                }
            }
            _ => {}
        }
    }
    if !sections.iter().any(|section| !section.language.is_empty()) {
        return None;
    }
    let prose = StyledText::from_markdown(source.get(source_end..)?).ok()?;
    append_section(
        document,
        &mut text_end,
        &mut sections,
        prose.text(),
        String::new(),
    )?;
    (text_end == document.text().len()).then_some(sections)
}

fn append_section(
    document: &StyledText,
    cursor: &mut usize,
    sections: &mut Vec<Section>,
    contents: &str,
    language: String,
) -> Option<()> {
    if contents.is_empty() {
        return Some(());
    }
    let before = document.text().get(..*cursor)?;
    let separator = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let start = *cursor + separator.len();
    if !document.text().get(*cursor..)?.starts_with(separator)
        || !document.text().get(start..)?.starts_with(contents)
    {
        return None;
    }
    let end = start + contents.len();
    let body = slice(document, start..end)?;
    sections.push(Section {
        start,
        language,
        body,
    });
    *cursor = end;
    Some(())
}

pub fn slice(document: &StyledText, range: Range<usize>) -> Option<StyledText> {
    let contents = document.text().get(range.clone())?;
    let intersect = |first: usize, last: usize| {
        let first = first.max(range.start);
        let last = last.min(range.end);
        (first < last).then(|| (first - range.start, last - range.start))
    };
    let runs = &document.runs()[document
        .runs()
        .partition_point(|run| run.end <= range.start)..];
    let runs = runs
        .iter()
        .take_while(|run| run.start < range.end)
        .filter_map(|run| {
            let (start, end) = intersect(run.start, run.end)?;
            Some(StyledRun { start, end, ..*run })
        })
        .collect();
    let links = &document.links()[document
        .links()
        .partition_point(|link| link.end <= range.start)..];
    let links = links
        .iter()
        .take_while(|link| link.start < range.end)
        .filter_map(|link| {
            let (start, end) = intersect(link.start, link.end)?;
            Some(xrack_runtime::StyledLink {
                start,
                end,
                href: link.href.clone(),
            })
        })
        .collect();
    StyledText::from_parts(contents, runs, links).ok()
}

/// Matches are in transcript order and align with UTF-8 grapheme boundaries.
pub fn matches(text: &str, query: &str) -> Vec<Range<usize>> {
    let query: String = query.trim().chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut folded = String::with_capacity(text.len());
    let mut boundaries = Vec::new();
    for (offset, grapheme) in text.grapheme_indices(true) {
        boundaries.push((folded.len(), offset));
        folded.extend(grapheme.chars().flat_map(char::to_lowercase));
    }
    boundaries.push((folded.len(), text.len()));
    let source = |offset| {
        boundaries
            .binary_search_by_key(&offset, |&(folded, _)| folded)
            .ok()
            .map(|index| boundaries[index].1)
    };
    folded
        .match_indices(&query)
        .filter_map(|(offset, _)| Some(source(offset)?..source(offset + query.len())?))
        .collect()
}

pub fn highlight(
    document: &StyledText,
    matches: &[Range<usize>],
    selected: Option<&Range<usize>>,
    dark: bool,
) -> StyledText {
    if matches.is_empty() {
        return document.clone();
    }
    let mut boundaries: Vec<_> = document
        .runs()
        .iter()
        .flat_map(|run| [run.start, run.end])
        .chain(matches.iter().flat_map(|range| [range.start, range.end]))
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::new();
    let mut original = document.runs().iter().peekable();
    let mut hits = matches.iter().peekable();
    for pair in boundaries.windows(2) {
        let [start, end] = [pair[0], pair[1]];
        while original.peek().is_some_and(|run| run.end <= start) {
            original.next();
        }
        while hits.peek().is_some_and(|hit| hit.end <= start) {
            hits.next();
        }
        let mut style = original
            .peek()
            .filter(|run| run.start <= start)
            .map_or_else(TextStyle::default, |run| run.style);
        if let Some(hit) = hits.peek().filter(|hit| hit.start <= start) {
            style.underline = true;
            let active = selected == Some(*hit);
            style.color = Some(Color(match (dark, active) {
                (false, false) => 0x745219ff,
                (false, true) => 0xb13e11ff,
                (true, false) => 0xe2bf76ff,
                (true, true) => 0xffa873ff,
            }));
        }
        if style != TextStyle::default() {
            runs.push(StyledRun { start, end, style });
        }
    }
    StyledText::from_parts(document.text(), runs, document.links().to_vec())
        .unwrap_or_else(|_| document.clone())
}

/// Targets the current materialization of the selected message.
pub fn reveal(component: &impl Component, row: usize, offset: usize) -> bool {
    reveal_section(component, row, 0, offset)
}

pub fn reveal_section(
    component: &impl Component,
    row: usize,
    section: usize,
    offset: usize,
) -> bool {
    let scene = component.render_shared();
    let Some(viewport) = scene
        .find("transcript")
        .and_then(|node| node.viewport_handle.as_ref())
    else {
        return false;
    };
    let Some(rows) = scene.find("transcript-rows") else {
        return false;
    };
    fn visible_paragraphs(node: &Node) -> usize {
        if !node.is_visible() {
            return 0;
        }
        if node.kind == "StyledText" {
            return 1;
        }
        node.materialized_children()
            .map(|(_, child)| visible_paragraphs(child))
            .sum()
    }
    let Some(messages) = rows
        .children
        .iter()
        .find(|node| node.kind == "VerticalLayout")
    else {
        return false;
    };
    let mut candidates = messages
        .materialized_children()
        .map(|(_, child)| visible_paragraphs(child));
    let paragraph = candidates.by_ref().take(row).sum::<usize>();
    if candidates.next().is_none_or(|count| section >= count) {
        return false;
    }
    viewport.request(ScrollAction::ToStyledText {
        paragraph: paragraph + section,
        offset,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_sections_keep_whitespace_and_distinguish_inline_and_repeated_code() {
        let source = "参见 [file](workspace-file:src/lib.rs:7)，并运行 `echo hi`。\r\n\r\n~~~sh callback\r\necho hi\r\n\r\n\t# π\r\n~~~\r\n\r\n```\r\necho hi\r\n```\r\n\r\n**完成**";
        let document = StyledText::from_markdown(source).unwrap();
        let sections = fenced_sections(source, &document);
        let pieces: Vec<_> = sections
            .iter()
            .map(|section| (section.language.as_str(), section.body.text()))
            .collect();
        assert_eq!(
            pieces,
            [
                ("", "参见 file，并运行 echo hi。"),
                ("sh", "echo hi\n\n\t# π\n"),
                ("代码", "echo hi\n"),
                ("", "完成"),
            ]
        );
        assert_eq!(
            sections[0].body.links()[0].href,
            "workspace-file:src/lib.rs:7"
        );
        for section in &sections {
            assert_eq!(
                &document.text()[section.start..section.start + section.body.text().len()],
                section.body.text(),
            );
        }
        assert!(sections[3].body.runs()[0].style.bold);
    }

    #[test]
    fn a_partition_that_would_change_the_canonical_text_keeps_the_original_document() {
        let source = "[linked][name]\n\n```rust\nx\n```\n\n[name]: src/lib.rs:7";
        let document = StyledText::from_markdown(source).unwrap();
        assert_eq!(document.links()[0].href, "src/lib.rs:7");
        assert!(fenced_sections(source, &document).is_empty());
    }

    #[test]
    fn matches_unicode_without_splitting_a_grapheme() {
        let text = "结果 İstandan CAFÉ 工具 舞👩🏽‍💻舞";
        for (query, expected) in [
            ("i\u{307}STANDAN", "İstandan"),
            ("café", "CAFÉ"),
            ("工具", "工具"),
            ("👩🏽‍💻", "👩🏽‍💻"),
        ] {
            let result = matches(text, query);
            assert_eq!(
                result
                    .iter()
                    .map(|range| &text[range.clone()])
                    .collect::<Vec<_>>(),
                [expected]
            );
        }
        assert!(matches(text, "👩").is_empty());
        assert!(matches(text, "  ").is_empty());
    }

    #[test]
    fn decorations_preserve_links_and_existing_styles() {
        let text = StyledText::from_markdown("**check** [checked](source.xr:7) `checked`").unwrap();
        let spans = matches(text.text(), "check");
        let styled = highlight(&text, &spans, spans.last(), false);
        assert_eq!(styled.links(), text.links());
        assert_eq!(styled.text(), text.text());
        assert!(styled.runs()[0].style.bold);
        assert!(
            styled
                .runs()
                .iter()
                .any(|run| run.style.monospace && run.style.underline)
        );
        assert_eq!(
            styled
                .runs()
                .iter()
                .filter(|run| run.style.underline)
                .count(),
            3
        );
    }
}
