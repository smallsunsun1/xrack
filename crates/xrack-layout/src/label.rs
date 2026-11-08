use crate::{
    Point, Size, font_size,
    text_area::{fitting_prefix, grapheme_boundaries, paragraphs},
};
use std::borrow::Cow;
use unicode_bidi::{Direction, get_base_direction};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{
    Node, TextHorizontalAlignment, TextOverflow, TextVerticalAlignment, TextWrap, Value,
};

pub fn uses_label_layout(node: &Node) -> bool {
    node.kind == "Label"
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
    pub fn new(node: &'a Node, available: Size, mut advance: impl FnMut(&str) -> f32) -> Self {
        use TextHorizontalAlignment as Horizontal;
        use TextVerticalAlignment as Vertical;

        let text = node.text("text").unwrap_or("");
        let line_height = font_size(node) * 1.4;
        let elide =
            node.properties.get("overflow") == Some(&Value::TextOverflow(TextOverflow::Elide));
        let horizontal = match node.properties.get("horizontal-alignment") {
            Some(Value::TextHorizontalAlignment(alignment)) => *alignment,
            _ if node.kind == "Label" => Horizontal::Start,
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
            by_height.floor()
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
            let content = &text[range];
            let rtl = matches!(horizontal, Horizontal::Start | Horizontal::End)
                && get_base_direction(content) == Direction::Rtl;
            if wrap == TextWrap::NoWrap || content.is_empty() {
                raw.push((content, rtl));
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
                    advance(&content[graphemes[start]..graphemes[index]]) <= available.width
                })
                .max(start + 1);
                let word_end = words.partition_point(|&byte| byte <= graphemes[fit]);
                let next = word_end
                    .checked_sub(1)
                    .and_then(|index| graphemes.binary_search(&words[index]).ok())
                    .filter(|&index| index > start)
                    .unwrap_or(fit);
                raw.push((content[graphemes[start]..graphemes[next]].trim_end(), rtl));
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
            .map(|(index, (content, rtl))| {
                let mut span = advance(content);
                let text = if elide && (span > available.width || truncated && index == last) {
                    let shown = elided(content, available.width, &mut advance);
                    span = advance(&shown);
                    Cow::Owned(shown)
                } else {
                    Cow::Borrowed(content)
                };
                width = width.max(span);
                let free = available.width - span;
                let x = match horizontal {
                    Horizontal::Center => free * 0.5,
                    Horizontal::Right => free,
                    Horizontal::Start if rtl => free,
                    Horizontal::End if !rtl => free,
                    _ => 0.0,
                };
                LabelLine {
                    text,
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

fn elided(text: &str, width: f32, advance: &mut impl FnMut(&str) -> f32) -> String {
    const ELLIPSIS: char = '…';
    if advance("…") > width {
        return String::new();
    }
    let boundaries = grapheme_boundaries(text);
    let mut candidate = String::new();
    let end = fitting_prefix(0, boundaries.len() - 1, |index| {
        candidate.clear();
        candidate.push_str(text[..boundaries[index]].trim_end());
        candidate.push(ELLIPSIS);
        advance(&candidate) <= width
    });
    candidate.clear();
    candidate.push_str(text[..boundaries[end]].trim_end());
    candidate.push(ELLIPSIS);
    candidate
}
