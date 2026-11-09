use crate::{
    Point, Size, font_size,
    text_area::{fitting_prefix, grapheme_boundaries, paragraphs},
};
use std::{borrow::Cow, ops::Range};
use unicode_bidi::{Direction, get_base_direction};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{
    Node, TextHorizontalAlignment, TextOverflow, TextVerticalAlignment, TextWrap, Value,
};

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
        mut advance: impl FnMut(&str) -> f32,
    ) -> Self {
        let text = node.text("text").unwrap_or("");
        let plan = LabelPlan::with_font_size(node, available, size, |range, ellipsis| {
            advance(&line_text(text, range, ellipsis))
        });
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

pub struct PlannedLabelLine {
    pub range: Range<usize>,
    pub ellipsis: bool,
    pub origin: Point,
}

/// Text ranges shared by plain and styled labels, independent of the shaping backend.
pub struct LabelPlan {
    pub size: Size,
    pub lines: Vec<PlannedLabelLine>,
}

impl LabelPlan {
    pub fn with_font_size(
        node: &Node,
        available: Size,
        size: f32,
        mut advance: impl FnMut(Range<usize>, bool) -> f32,
    ) -> Self {
        use TextHorizontalAlignment as Horizontal;
        use TextVerticalAlignment as Vertical;

        let text = node.text("text").unwrap_or("");
        let line_height = size * 1.4;
        let elide =
            node.properties.get("overflow") == Some(&Value::TextOverflow(TextOverflow::Elide));
        let horizontal = match node.properties.get("horizontal-alignment") {
            Some(Value::TextHorizontalAlignment(alignment)) => *alignment,
            _ if matches!(node.kind, "Label" | "StyledText") => Horizontal::Start,
            _ => Horizontal::Left,
        };
        let vertical = match node.properties.get("vertical-alignment") {
            Some(Value::TextVerticalAlignment(alignment)) => *alignment,
            _ => Vertical::Top,
        };
        let max_lines = node
            .int("max-lines")
            .and_then(|count| usize::try_from(count).ok())
            .filter(|&count| count > 0)
            .unwrap_or(usize::MAX);
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
        let wrap = wrap(node);
        'paragraphs: for range in paragraphs(text) {
            let content = &text[range.clone()];
            let rtl = matches!(horizontal, Horizontal::Start | Horizontal::End)
                && get_base_direction(content) == Direction::Rtl;
            if wrap == TextWrap::NoWrap || content.is_empty() {
                raw.push((range, rtl));
                if raw.len() > limit {
                    break;
                }
                continue;
            }
            let graphemes = grapheme_boundaries(content);
            let words = if wrap == TextWrap::WordWrap {
                content
                    .split_word_bound_indices()
                    .map(|(start, word)| start + word.len())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let last = graphemes.len() - 1;
            let mut start = 0;
            while start < last {
                let fit = fitting_prefix(start, last, |index| {
                    let from = range.start + graphemes[start];
                    let to = range.start + graphemes[index];
                    advance(from..to, false) <= available.width
                })
                .max(start + 1);
                let word_end = words.partition_point(|&byte| byte <= graphemes[fit]);
                let next = word_end
                    .checked_sub(1)
                    .and_then(|index| graphemes.binary_search(&words[index]).ok())
                    .filter(|&index| index > start)
                    .unwrap_or(fit);
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
                let free = available.width - span;
                let x = match horizontal {
                    Horizontal::Center => free * 0.5,
                    Horizontal::Right => free,
                    Horizontal::Start if rtl => free,
                    Horizontal::End if !rtl => free,
                    _ => 0.0,
                };
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
