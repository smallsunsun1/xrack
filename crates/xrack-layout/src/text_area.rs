use crate::{Point, Rect, TextEdit, TextPosition};
use std::{ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::TextWrap;

#[derive(Clone, Copy, Debug)]
pub enum TextAreaMotion {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// The same visual lines drive hit testing, selection, and drawing.
#[derive(Clone)]
pub struct TextAreaLayout {
    pub area: Rect,
    pub line_height: f32,
    pub lines: Arc<[Range<usize>]>,
    pub max_scroll: Point,
}

pub(crate) fn grapheme_boundaries(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect()
}

pub(crate) fn fitting_prefix(
    start: usize,
    last: usize,
    mut fits: impl FnMut(usize) -> bool,
) -> usize {
    let mut fit = start;
    let mut probe = (start + 1).min(last);
    while probe > fit && fits(probe) {
        fit = probe;
        probe = (start + (fit - start).saturating_mul(2)).min(last);
    }
    while probe > fit + 1 {
        let middle = fit + (probe - fit) / 2;
        if fits(middle) {
            fit = middle;
        } else {
            probe = middle;
        }
    }
    fit
}

pub(crate) fn paragraphs(text: &str) -> impl Iterator<Item = Range<usize>> {
    let mut separators = text.match_indices(['\r', '\n']).peekable();
    let mut start = Some(0);
    std::iter::from_fn(move || {
        let begin = start.take()?;
        let Some((end, separator)) = separators.next() else {
            return Some(begin..text.len());
        };
        let crlf = separator == "\r" && separators.next_if_eq(&(end + 1, "\n")).is_some();
        start = Some(end + if crlf { 2 } else { 1 });
        Some(begin..end)
    })
}

impl TextAreaLayout {
    pub fn new(
        text: &str,
        area: Rect,
        line_height: f32,
        wrap: TextWrap,
        advance: impl Fn(&str) -> f32,
        word_break: impl Fn(&str) -> usize,
    ) -> Self {
        let mut lines = Vec::new();
        let mut paragraph = |range: Range<usize>| {
            let content = &text[range.clone()];
            if content.is_empty() || wrap == TextWrap::NoWrap || area.width <= 0.0 {
                lines.push(range);
                return;
            }
            let breaks = grapheme_boundaries(content);
            let mut start = 0;
            while start < breaks.len() - 1 {
                let next = match wrap {
                    TextWrap::CharWrap => fitting_prefix(start, breaks.len() - 1, |index| {
                        advance(&content[breaks[start]..breaks[index]]) <= area.width
                    })
                    .max(start + 1),
                    TextWrap::WordWrap => {
                        let end =
                            breaks[start].saturating_add(word_break(&content[breaks[start]..]));
                        breaks
                            .partition_point(|&byte| byte <= end)
                            .saturating_sub(1)
                            .max(start + 1)
                    }
                    TextWrap::NoWrap => unreachable!(),
                };
                lines.push(range.start + breaks[start]..range.start + breaks[next]);
                start = next;
            }
        };
        for range in paragraphs(text) {
            paragraph(range);
        }
        let width = lines
            .iter()
            .map(|line| advance(&text[line.clone()]))
            .fold(0.0, f32::max);
        let max_scroll = Point {
            x: (width + 1.0 - area.width).max(0.0),
            y: (lines.len() as f32 * line_height - area.height).max(0.0),
        };
        Self {
            area,
            line_height,
            lines: lines.into(),
            max_scroll,
        }
    }

    pub fn with_viewport(mut self, area: Rect) -> Self {
        debug_assert_eq!(area.width, self.area.width);
        self.area = area;
        self.max_scroll.y = (self.lines.len() as f32 * self.line_height - area.height).max(0.0);
        self
    }

    pub fn clamp_scroll(&self, scroll: Point) -> Point {
        Point {
            x: scroll.x.clamp(0.0, self.max_scroll.x),
            y: scroll.y.clamp(0.0, self.max_scroll.y),
        }
    }

    pub fn keep_visible(&self, scroll: Point, caret: Point) -> Point {
        self.clamp_scroll(Point {
            x: scroll.x.max(caret.x + 1.0 - self.area.width).min(caret.x),
            y: scroll
                .y
                .max(caret.y + self.line_height - self.area.height)
                .min(caret.y),
        })
    }

    pub fn visible_lines(&self, scroll: Point) -> Range<usize> {
        let top = (scroll.y / self.line_height).floor() as usize;
        let bottom = ((scroll.y + self.area.height) / self.line_height).ceil() as usize;
        top.min(self.lines.len())..bottom.min(self.lines.len())
    }

    pub fn line_index(&self, position: TextPosition) -> usize {
        let line = self
            .lines
            .partition_point(|line| line.start <= position.offset)
            .saturating_sub(1);
        if position.trailing && line > 0 && self.lines[line - 1].end == position.offset {
            line - 1
        } else {
            line
        }
    }

    pub fn caret_point(
        &self,
        text: &str,
        position: TextPosition,
        advance: impl Fn(&str) -> f32,
    ) -> Point {
        let line = self.line_index(position);
        let range = &self.lines[line];
        let end = position.offset.clamp(range.start, range.end);
        Point {
            x: advance(&text[range.start..end]),
            y: line as f32 * self.line_height,
        }
    }

    fn position_on_line(
        &self,
        text: &str,
        line: usize,
        x: f32,
        advance: impl Fn(&str) -> f32,
    ) -> TextPosition {
        let range = &self.lines[line];
        let content = &text[range.clone()];
        let breaks = grapheme_boundaries(content);
        let after = breaks.partition_point(|&byte| advance(&content[..byte]) < x);
        let right = breaks[after.min(breaks.len() - 1)];
        let left = breaks[after.saturating_sub(1).min(breaks.len() - 1)];
        let offset = if x - advance(&content[..left]) <= advance(&content[..right]) - x {
            left
        } else {
            right
        };
        TextPosition {
            offset: range.start + offset,
            trailing: offset != 0 && offset == content.len(),
        }
    }

    pub fn hit(
        &self,
        text: &str,
        point: Point,
        scroll: Point,
        advance: impl Fn(&str) -> f32,
    ) -> TextPosition {
        let line = ((point.y - self.area.y + scroll.y) / self.line_height).floor() as usize;
        self.position_on_line(
            text,
            line.min(self.lines.len() - 1),
            point.x - self.area.x + scroll.x,
            advance,
        )
    }

    pub fn move_caret(
        &self,
        text: &str,
        editor: &mut TextEdit,
        motion: TextAreaMotion,
        selecting: bool,
        advance: impl Fn(&str) -> f32,
    ) {
        let caret = editor.position(text);
        let line = self.line_index(caret);
        let range = &self.lines[line];
        match motion {
            TextAreaMotion::Home => editor.place_caret(
                text,
                TextPosition {
                    offset: range.start,
                    trailing: false,
                },
                selecting,
            ),
            TextAreaMotion::End => editor.place_caret(
                text,
                TextPosition {
                    offset: range.end,
                    trailing: true,
                },
                selecting,
            ),
            _ => {
                let page = (self.area.height / self.line_height).floor().max(1.0) as usize;
                let next = match motion {
                    TextAreaMotion::Up => line.saturating_sub(1),
                    TextAreaMotion::Down => (line + 1).min(self.lines.len() - 1),
                    TextAreaMotion::PageUp => line.saturating_sub(page),
                    TextAreaMotion::PageDown => (line + page).min(self.lines.len() - 1),
                    TextAreaMotion::Home | TextAreaMotion::End => unreachable!(),
                };
                let x = editor
                    .preferred_x()
                    .unwrap_or_else(|| self.caret_point(text, caret, &advance).x);
                let position = self.position_on_line(text, next, x, advance);
                editor.place_caret(text, position, selecting);
                editor.set_preferred_x(x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditAction;

    fn advance(text: &str) -> f32 {
        text.graphemes(true).count() as f32
    }

    #[test]
    fn wrapping_preserves_graphemes_empty_lines_and_soft_wrap_affinity() {
        let text = "ab👩‍🚀c\r\n\n末\n";
        let layout = TextAreaLayout::new(
            text,
            Rect::new(0.0, 0.0, 2.0, 20.0),
            10.0,
            TextWrap::CharWrap,
            advance,
            str::len,
        );
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|range| &text[range.clone()])
                .collect::<Vec<_>>(),
            ["ab", "👩‍🚀c", "", "末", ""]
        );
        let upstream = layout.hit(text, Point { x: 2.0, y: 1.0 }, Point::default(), advance);
        let downstream = layout.hit(text, Point { x: 0.0, y: 11.0 }, Point::default(), advance);
        assert_eq!(upstream.offset, downstream.offset);
        assert!(upstream.trailing);
        assert!(!downstream.trailing);
        assert_eq!(
            layout.caret_point(text, upstream, advance),
            Point { x: 2.0, y: 0.0 }
        );
        assert_eq!(
            layout.caret_point(text, downstream, advance),
            Point { x: 0.0, y: 10.0 }
        );
        assert_eq!(layout.visible_lines(Point { x: 0.0, y: 25.0 }), 2..5);
    }

    #[test]
    fn vertical_selection_keeps_the_preferred_column_through_short_lines() {
        let text = "abcdefghij\nx\nabcdefghij";
        let layout = TextAreaLayout::new(
            text,
            Rect::new(0.0, 0.0, 100.0, 20.0),
            10.0,
            TextWrap::NoWrap,
            advance,
            str::len,
        );
        let mut editor = TextEdit::default();
        editor.place_caret(
            text,
            TextPosition {
                offset: 7,
                trailing: false,
            },
            false,
        );
        layout.move_caret(text, &mut editor, TextAreaMotion::Down, true, advance);
        assert_eq!(editor.caret(text), 12);
        layout.move_caret(text, &mut editor, TextAreaMotion::Down, true, advance);
        assert_eq!(editor.selection(text), 7..20);
        let caret = layout.caret_point(text, editor.position(text), advance);
        assert_eq!(layout.keep_visible(Point::default(), caret).y, 10.0);
        assert_eq!(
            editor.apply_multiline(text, EditAction::Insert("甲\r\n乙")),
            Some("abcdefg甲\n乙hij".into())
        );
        assert_eq!(
            TextEdit::default().apply("", EditAction::Insert("甲\r\n乙")),
            Some("甲乙".into())
        );
    }
}
