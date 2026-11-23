use crate::{
    Point, Size, TextMeasurer, font_size,
    text_area::{fitting_prefix, grapheme_boundaries, paragraphs},
};
use std::{borrow::Cow, cell::Cell, ops::Range};
use unicode_bidi::{Direction, InitialInfo, get_base_direction};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{
    LayoutDirection, Node, TextHorizontalAlignment, TextOverflow, TextVerticalAlignment, TextWrap,
    Value,
};

/// Resolves logical alignment using the nearest layout declaration or text direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct TextAlignment {
    mode: TextHorizontalAlignment,
    direction: Option<LayoutDirection>,
}

impl TextAlignment {
    pub(crate) fn from_node(
        node: &Node,
        direction: Option<LayoutDirection>,
        default: TextHorizontalAlignment,
    ) -> Self {
        let mode = match node.properties.get("horizontal-alignment") {
            Some(Value::TextHorizontalAlignment(alignment)) => *alignment,
            _ => default,
        };
        Self { mode, direction }
    }

    pub fn editor(node: &Node, direction: Option<LayoutDirection>) -> Self {
        let default = if direction.is_some() {
            TextHorizontalAlignment::Start
        } else {
            TextHorizontalAlignment::Left
        };
        Self::from_node(node, direction, default)
    }

    pub(crate) fn has_offset(self) -> bool {
        self.mode != TextHorizontalAlignment::Left
    }

    fn factor(self, natural_rtl: bool) -> f32 {
        use TextHorizontalAlignment as Horizontal;
        let rtl = self.direction.map_or(natural_rtl, |direction| {
            direction == LayoutDirection::RightToLeft
        });
        match self.mode {
            Horizontal::Center => 0.5,
            Horizontal::Right => 1.0,
            Horizontal::Start if rtl => 1.0,
            Horizontal::End if !rtl => 1.0,
            _ => 0.0,
        }
    }

    pub fn offset(self, space: f32, natural_rtl: bool) -> f32 {
        let factor = self.factor(natural_rtl);
        if factor == 0.0 { 0.0 } else { space * factor }
    }

    #[doc(hidden)]
    pub fn passive_line_offset(self, width: f32, text: &str, advance: impl FnOnce() -> f32) -> f32 {
        let natural_rtl = self.direction.is_none()
            && matches!(
                self.mode,
                TextHorizontalAlignment::Start | TextHorizontalAlignment::End
            )
            && InitialInfo::new(text, None)
                .paragraphs
                .first()
                .is_some_and(|p| p.level.is_rtl());
        let factor = self.factor(natural_rtl);
        if factor == 0.0 {
            0.0
        } else {
            (width - advance() - 1.0) * factor
        }
    }

    /// Reserve one pixel for the caret and reveal it when a line overflows.
    pub fn single_line_offset(
        self,
        width: f32,
        advance: f32,
        rtl: bool,
        caret: Option<f32>,
    ) -> f32 {
        let free = width - advance - 1.0;
        if free < 0.0
            && let Some(caret) = caret
        {
            -(caret + 1.0 - width).max(0.0)
        } else {
            self.offset(free, rtl)
        }
    }

    #[doc(hidden)]
    pub fn single_line_view_offset(
        self,
        width: f32,
        advance: f32,
        rtl: bool,
        caret: Option<f32>,
        drag_scroll: Option<f32>,
    ) -> f32 {
        let overflow = advance + 1.0 - width;
        if let Some(scroll) = drag_scroll
            && overflow > 0.0
        {
            -scroll.clamp(0.0, overflow)
        } else {
            self.single_line_offset(width, advance, rtl, caret)
        }
    }
}

pub fn uses_label_layout(node: &Node) -> bool {
    matches!(node.kind, "Label" | "StyledText")
        || node.kind == "Text"
            && [
                "horizontal-alignment",
                "vertical-alignment",
                "overflow",
                "max-lines",
                "wrap",
            ]
            .iter()
            .any(|key| node.properties.contains_key(key))
}

fn wrap(node: &Node) -> TextWrap {
    match node.properties.get("wrap") {
        Some(Value::TextWrap(wrap)) => *wrap,
        _ if node.kind == "Label" => TextWrap::NoWrap,
        _ => TextWrap::WordWrap,
    }
}

pub(crate) fn reflows_at_width(node: &Node) -> bool {
    uses_label_layout(node) && wrap(node) != TextWrap::NoWrap
}

pub(crate) fn flexible_width(node: &Node) -> bool {
    reflows_at_width(node)
        || uses_label_layout(node)
            && node.properties.get("overflow") == Some(&Value::TextOverflow(TextOverflow::Elide))
}

pub struct LabelLine<'a> {
    pub text: Cow<'a, str>,
    pub origin: Point,
}

/// The same line plan is used for intrinsic measurement and GPU painting.
pub struct LabelLayout<'a> {
    pub size: Size,
    pub lines: Vec<LabelLine<'a>>,
}

impl<'a> LabelLayout<'a> {
    pub fn new(node: &'a Node, available: Size, advance: impl FnMut(&str) -> f32) -> Self {
        Self::with_font_size(node, available, font_size(node), advance)
    }

    pub fn with_font_size(
        node: &'a Node,
        available: Size,
        size: f32,
        advance: impl FnMut(&str) -> f32,
    ) -> Self {
        Self::in_direction(node, available, size, node.layout_direction(), advance)
    }

    pub fn in_direction(
        node: &'a Node,
        available: Size,
        size: f32,
        direction: Option<LayoutDirection>,
        advance: impl FnMut(&str) -> f32,
    ) -> Self {
        Self::with_style(
            node.text("text").unwrap_or(""),
            available,
            size,
            LabelStyle::for_node(node, direction),
            advance,
        )
    }

    #[doc(hidden)]
    pub fn with_style(
        text: &'a str,
        available: Size,
        size: f32,
        style: LabelStyle,
        mut advance: impl FnMut(&str) -> f32,
    ) -> Self {
        let plan = LabelPlan::plan(
            text,
            available,
            size,
            style,
            |range, ellipsis| advance(&line_text(text, range, ellipsis)),
            None,
            None,
        );
        Self {
            size: plan.size,
            lines: plan
                .lines
                .into_iter()
                .map(|line| LabelLine {
                    text: line_text(text, line.range, line.ellipsis),
                    origin: line.origin,
                })
                .collect(),
        }
    }
}

fn line_text(text: &str, range: Range<usize>, ellipsis: bool) -> Cow<'_, str> {
    if ellipsis {
        let mut shown = text[range].to_owned();
        shown.push('…');
        Cow::Owned(shown)
    } else {
        Cow::Borrowed(&text[range])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedLabelLine {
    pub range: Range<usize>,
    pub ellipsis: bool,
    pub origin: Point,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TitleLayout {
    pub height: f32,
    pub font_size: f32,
    pub lines: Vec<PlannedLabelLine>,
}

impl PlannedLabelLine {
    pub fn text<'a>(&self, source: &'a str) -> Cow<'a, str> {
        line_text(source, self.range.clone(), self.ellipsis)
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LabelStyle {
    horizontal: TextAlignment,
    vertical: TextVerticalAlignment,
    wrap: TextWrap,
    elide: bool,
    max_lines: usize,
}

impl LabelStyle {
    pub fn for_node(node: &Node, direction: Option<LayoutDirection>) -> Self {
        let default = if matches!(node.kind, "Label" | "StyledText") {
            TextHorizontalAlignment::Start
        } else {
            TextHorizontalAlignment::Left
        };
        let vertical = match node.properties.get("vertical-alignment") {
            Some(Value::TextVerticalAlignment(alignment)) => *alignment,
            _ => TextVerticalAlignment::Top,
        };
        Self {
            horizontal: TextAlignment::from_node(node, direction, default),
            vertical,
            wrap: wrap(node),
            elide: node.properties.get("overflow")
                == Some(&Value::TextOverflow(TextOverflow::Elide)),
            max_lines: node
                .int("max-lines")
                .and_then(|count| usize::try_from(count).ok())
                .filter(|&count| count > 0)
                .unwrap_or(usize::MAX),
        }
    }
}

impl std::hash::Hash for LabelStyle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (
            self.horizontal.mode as u8,
            self.horizontal.direction.map(|direction| direction as u8),
            self.vertical as u8,
            self.wrap as u8,
            self.elide,
            self.max_lines,
        )
            .hash(state);
    }
}

/// Text ranges shared by plain and styled labels, independent of the shaping backend.
pub struct LabelPlan {
    pub size: Size,
    pub lines: Vec<PlannedLabelLine>,
}

pub(crate) struct PreparedLabel {
    paragraphs: Vec<PreparedParagraph>,
    graphemes: Vec<usize>,
    words: Vec<usize>,
    first_line_probes: Vec<Cell<FirstLineProbes>>,
    font_generation: Cell<u32>,
}

struct PreparedParagraph {
    source: Range<usize>,
    trimmed_end: usize,
    rtl: bool,
    graphemes: Range<usize>,
    words: Range<usize>,
    one_line: Cell<Option<OneLineFit>>,
}

#[derive(Clone, Copy)]
struct OneLineFit {
    font_generation: u32,
    wrap: TextWrap,
    minimum_width: f32,
}

const FIRST_LINE_PROBE_COUNT: usize = 8;

#[derive(Clone, Copy)]
struct FirstLineProbes {
    font_generation: u32,
    widths: [f32; FIRST_LINE_PROBE_COUNT],
}

impl FirstLineProbes {
    fn new(font_generation: u32) -> Self {
        Self {
            font_generation,
            widths: [f32::NAN; FIRST_LINE_PROBE_COUNT],
        }
    }

    fn measure(
        &mut self,
        index: usize,
        last: usize,
        font_generation: u32,
        advance: impl FnOnce() -> f32,
    ) -> f32 {
        if self.font_generation != font_generation {
            *self = Self::new(font_generation);
        }
        // The first line always probes the same powers of two, capped at
        // the last grapheme. Binary search can then explore other boundaries.
        if (index.is_power_of_two() || index == last)
            && let Some(width) = self
                .widths
                .get_mut(index.next_power_of_two().trailing_zeros() as usize)
        {
            if width.is_nan() {
                *width = advance();
            }
            *width
        } else {
            advance()
        }
    }
}

impl PreparedLabel {
    pub(crate) fn new(text: &str) -> Option<Self> {
        const MAX_PARAGRAPHS: usize = 65_536;
        const MAX_BOUNDARIES: usize = 512 * 1024;
        let mut result = Self {
            paragraphs: Vec::new(),
            graphemes: Vec::new(),
            words: Vec::new(),
            first_line_probes: Vec::new(),
            font_generation: Cell::new(0),
        };
        for source in paragraphs(text) {
            let content = &text[source.clone()];
            if result.paragraphs.len() == MAX_PARAGRAPHS || content.len() > MAX_BOUNDARIES {
                return None;
            }
            let grapheme_start = result.graphemes.len();
            result
                .graphemes
                .extend(content.grapheme_indices(true).map(|(byte, _)| byte));
            result.graphemes.push(content.len());
            let word_start = result.words.len();
            result.words.extend(
                content
                    .split_word_bound_indices()
                    .map(|(byte, word)| byte + word.len()),
            );
            if result.graphemes.len() + result.words.len() > MAX_BOUNDARIES {
                return None;
            }
            result.paragraphs.push(PreparedParagraph {
                trimmed_end: source.start + content.trim_end().len(),
                source,
                rtl: get_base_direction(content) == Direction::Rtl,
                graphemes: grapheme_start..result.graphemes.len(),
                words: word_start..result.words.len(),
                one_line: Cell::new(None),
            });
        }
        // Width changes that keep a paragraph on one line only read the
        // paragraph metadata. Keep the search probes out of that traversal.
        result
            .first_line_probes
            .resize_with(result.paragraphs.len(), || {
                Cell::new(FirstLineProbes::new(0))
            });
        Some(result)
    }

    pub(crate) fn invalidate_widths(&self) {
        let next = self.font_generation.get().wrapping_add(1);
        if next == 0 {
            for paragraph in &self.paragraphs {
                paragraph.one_line.set(None);
            }
            for probes in &self.first_line_probes {
                probes.set(FirstLineProbes::new(0));
            }
        }
        self.font_generation.set(next);
    }
}

impl LabelPlan {
    pub fn with_font_size(
        node: &Node,
        available: Size,
        size: f32,
        advance: impl FnMut(Range<usize>, bool) -> f32,
    ) -> Self {
        Self::in_direction(node, available, size, node.layout_direction(), advance)
    }

    fn in_direction(
        node: &Node,
        available: Size,
        size: f32,
        direction: Option<LayoutDirection>,
        advance: impl FnMut(Range<usize>, bool) -> f32,
    ) -> Self {
        Self::with_widths(node, available, size, direction, advance, None, None)
    }

    pub(crate) fn with_widths(
        node: &Node,
        available: Size,
        size: f32,
        direction: Option<LayoutDirection>,
        advance: impl FnMut(Range<usize>, bool) -> f32,
        widths: Option<&mut Vec<f32>>,
        prepared: Option<&PreparedLabel>,
    ) -> Self {
        Self::plan(
            node.text("text").unwrap_or(""),
            available,
            size,
            LabelStyle::for_node(node, direction),
            advance,
            widths,
            prepared,
        )
    }

    pub(crate) fn leading_title(
        node: &Node,
        title: &str,
        width: f32,
        size: f32,
        direction: LayoutDirection,
        max_lines: usize,
        text: &mut impl TextMeasurer,
    ) -> Self {
        let preferred_size = text.font_size(node);
        Self::plan(
            title,
            Size::new(width, f32::INFINITY),
            size,
            LabelStyle {
                horizontal: TextAlignment {
                    mode: TextHorizontalAlignment::Start,
                    direction: Some(direction),
                },
                vertical: TextVerticalAlignment::Top,
                wrap: TextWrap::WordWrap,
                elide: true,
                max_lines,
            },
            |range, ellipsis| {
                let value = line_text(title, range, ellipsis);
                if size == preferred_size {
                    text.measure_node(node, &value, f32::MAX).width
                } else {
                    text.measure_node_at_size(node, &value, size, f32::MAX)
                        .width
                }
            },
            None,
            None,
        )
    }

    fn plan(
        text: &str,
        available: Size,
        size: f32,
        style: LabelStyle,
        mut advance: impl FnMut(Range<usize>, bool) -> f32,
        mut widths: Option<&mut Vec<f32>>,
        prepared: Option<&PreparedLabel>,
    ) -> Self {
        use TextHorizontalAlignment as Horizontal;
        use TextVerticalAlignment as Vertical;

        let LabelStyle {
            horizontal,
            vertical,
            wrap,
            elide,
            max_lines,
        } = style;
        let line_height = size * 1.4;
        let by_height = available.height / line_height;
        let by_height = if elide {
            // The measured height is a rounded product of the line count and height.
            by_height.next_up().floor()
        } else {
            by_height.ceil()
        } as usize;
        let limit = max_lines.min(by_height);
        if limit == 0 || available.width <= 0.0 {
            return Self {
                size: Size::default(),
                lines: Vec::new(),
            };
        }

        let mut raw = Vec::new();
        let mut generated = paragraphs(text);
        let mut retained = prepared.map(|prepared| prepared.paragraphs.iter());
        let sources = std::iter::from_fn(|| match &mut retained {
            Some(retained) => retained
                .next()
                .map(|paragraph| (paragraph.source.clone(), Some(paragraph))),
            None => generated.next().map(|range| (range, None)),
        });
        'paragraphs: for (paragraph_index, (range, paragraph)) in sources.enumerate() {
            let content = &text[range.clone()];
            let rtl = horizontal.direction.is_none()
                && matches!(horizontal.mode, Horizontal::Start | Horizontal::End)
                && paragraph.map_or_else(
                    || get_base_direction(content) == Direction::Rtl,
                    |paragraph| paragraph.rtl,
                );
            if wrap == TextWrap::NoWrap || content.is_empty() {
                raw.push((range, rtl));
                if raw.len() > limit {
                    break;
                }
                continue;
            }
            let retained = prepared.zip(paragraph);
            let font_generation = prepared.map_or(0, |prepared| prepared.font_generation.get());
            if let Some(paragraph) = paragraph
                && paragraph.one_line.get().is_some_and(|fit| {
                    fit.font_generation == font_generation
                        && fit.wrap == wrap
                        && available.width >= fit.minimum_width
                })
            {
                raw.push((range.start..paragraph.trimmed_end, rtl));
                if raw.len() > limit {
                    break;
                }
                continue;
            }
            let generated_graphemes;
            let graphemes = if let Some((prepared, paragraph)) = retained {
                &prepared.graphemes[paragraph.graphemes.clone()]
            } else {
                generated_graphemes = grapheme_boundaries(content);
                &generated_graphemes
            };
            let generated_words;
            let words: &[usize] = if wrap != TextWrap::WordWrap {
                &[]
            } else if let Some((prepared, paragraph)) = retained {
                &prepared.words[paragraph.words.clone()]
            } else {
                generated_words = content
                    .split_word_bound_indices()
                    .map(|(start, word)| start + word.len())
                    .collect::<Vec<_>>();
                &generated_words
            };
            let last = graphemes.len() - 1;
            let mut start = 0;
            while start < last {
                let mut minimum_width = f32::NEG_INFINITY;
                let (fit, whole_paragraph_fits) = if let Some((prepared, _)) = retained
                    && start == 0
                {
                    let cache = &prepared.first_line_probes[paragraph_index];
                    let mut probes = cache.get();
                    let fit = fitting_prefix(0, last, |index| {
                        let width =
                            probes.measure(index, last, prepared.font_generation.get(), || {
                                advance(range.start..range.start + graphemes[index], false)
                            });
                        minimum_width = minimum_width.max(width);
                        width <= available.width
                    });
                    if probes.font_generation == prepared.font_generation.get() {
                        cache.set(probes);
                    }
                    (fit, fit == last)
                } else {
                    let fit = fitting_prefix(start, last, |index| {
                        let from = range.start + graphemes[start];
                        let to = range.start + graphemes[index];
                        advance(from..to, false) <= available.width
                    });
                    (fit, false)
                };
                let fit = fit.max(start + 1);
                let word_end = words.partition_point(|&byte| byte <= graphemes[fit]);
                let next = word_end
                    .checked_sub(1)
                    .and_then(|index| graphemes.binary_search(&words[index]).ok())
                    .filter(|&index| index > start)
                    .unwrap_or(fit);
                if whole_paragraph_fits
                    && next == last
                    && let Some((prepared, paragraph)) = retained
                    && font_generation == prepared.font_generation.get()
                {
                    // Reaching the last grapheme required every exponential
                    // probe to fit. A wider line repeats those same decisions.
                    paragraph.one_line.set(Some(OneLineFit {
                        font_generation,
                        wrap,
                        minimum_width,
                    }));
                }
                let from = range.start + graphemes[start];
                let to = range.start + graphemes[next];
                raw.push((trim_trailing_space(text, from..to), rtl));
                if raw.len() > limit {
                    break 'paragraphs;
                }
                start = next;
                while start < last
                    && content[graphemes[start]..graphemes[start + 1]]
                        .chars()
                        .all(char::is_whitespace)
                {
                    start += 1;
                }
            }
        }

        let truncated = raw.len() > limit;
        raw.truncate(limit);
        let last = raw.len().saturating_sub(1);
        let height = raw.len() as f32 * line_height;
        let free = if available.height.is_finite() {
            (available.height - height).max(0.0)
        } else {
            0.0
        };
        let top = match vertical {
            Vertical::Top => 0.0,
            Vertical::Center => free * 0.5,
            Vertical::Bottom => free,
        };
        let mut width = 0.0_f32;
        let lines = raw
            .into_iter()
            .enumerate()
            .map(|(index, (mut range, rtl))| {
                let mut span = advance(range.clone(), false);
                let mut ellipsis = false;
                if elide && (span > available.width || truncated && index == last) {
                    (range, ellipsis) = elided(text, range, available.width, &mut advance);
                    span = advance(range.clone(), ellipsis);
                }
                width = width.max(span);
                if let Some(widths) = widths.as_deref_mut() {
                    widths.push(width.min(available.width));
                }
                let free = available.width - span;
                let x = horizontal.offset(free, rtl);
                PlannedLabelLine {
                    range,
                    ellipsis,
                    origin: Point {
                        x,
                        y: top + index as f32 * line_height,
                    },
                }
            })
            .collect();
        Self {
            size: Size::new(width.min(available.width), height),
            lines,
        }
    }
}

fn trim_trailing_space(text: &str, range: Range<usize>) -> Range<usize> {
    range.start..range.start + text[range.clone()].trim_end().len()
}

/// Fit a single line to the available width without splitting a grapheme.
#[doc(hidden)]
pub fn elide_to_width(
    text: &str,
    width: f32,
    mut advance: impl FnMut(&str) -> f32,
) -> Cow<'_, str> {
    if advance(text) <= width {
        return Cow::Borrowed(text);
    }
    let (range, ellipsis) = elided(text, 0..text.len(), width, &mut |range, ellipsis| {
        advance(&line_text(text, range, ellipsis))
    });
    line_text(text, range, ellipsis)
}

fn elided(
    text: &str,
    range: Range<usize>,
    width: f32,
    advance: &mut impl FnMut(Range<usize>, bool) -> f32,
) -> (Range<usize>, bool) {
    let start = range.start;
    if advance(start..start, true) > width {
        return (start..start, false);
    }
    let boundaries = grapheme_boundaries(&text[range]);
    let end = fitting_prefix(0, boundaries.len() - 1, |index| {
        advance(
            trim_trailing_space(text, start..start + boundaries[index]),
            true,
        ) <= width
    });
    (
        trim_trailing_space(text, start..start + boundaries[end]),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_probes_are_reused_at_new_widths_and_reset_for_new_fonts() {
        let text = "abcdefghi";
        let prepared = PreparedLabel::new(text).unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert(
            "text",
            xrack_runtime::StyledText::from_plain_text(text).into(),
        );
        node.properties.insert("wrap", TextWrap::CharWrap.into());
        let layout = |width, scale, probes: &mut Vec<Range<usize>>| {
            LabelPlan::with_widths(
                &node,
                Size::new(width, f32::INFINITY),
                12.0,
                None,
                |range, _| {
                    let width = range.len() as f32 * scale;
                    probes.push(range);
                    width
                },
                None,
                Some(&prepared),
            )
        };

        let mut probes = Vec::new();
        let initial = layout(16.0, 1.0, &mut probes);
        assert_eq!(initial.lines[0].range, 0..9);
        assert!(probes.contains(&(0..1)));

        probes.clear();
        let narrower = layout(5.5, 1.0, &mut probes);
        assert_eq!(narrower.lines[0].range, 0..5);
        assert!(!probes.contains(&(0..1)));
        assert!(!probes.contains(&(0..4)));
        assert!(probes.contains(&(0..6)));

        prepared.invalidate_widths();
        probes.clear();
        let new_font = layout(5.5, 2.0, &mut probes);
        assert_eq!(new_font.lines[0].range, 0..2);
        assert!(probes.contains(&(0..1)));
        assert!(probes.contains(&(0..4)));
    }
}
