use crate::{TextPosition, text_area::grapheme_boundaries};
use std::{collections::BTreeMap, ops::Range};
use unicode_bidi::BidiInfo;

/// One grapheme's logical byte range and its two visual edges.
#[derive(Clone, Debug)]
pub struct VisualGrapheme {
    pub bytes: Range<usize>,
    pub leading: f32,
    pub trailing: f32,
    pub rtl: bool,
}

impl VisualGrapheme {
    pub fn span(&self) -> Range<f32> {
        self.leading.min(self.trailing)..self.leading.max(self.trailing)
    }

    fn edge(&self, right: bool) -> TextPosition {
        let trailing = right != self.rtl;
        TextPosition {
            offset: if trailing {
                self.bytes.end
            } else {
                self.bytes.start
            },
            trailing,
        }
    }
}

/// Maps logical grapheme boundaries to the advances of a shaped visual line.
#[derive(Clone, Debug)]
pub struct VisualTextLine {
    cells: Vec<VisualGrapheme>,
    visual: Vec<usize>,
    width: f32,
    rtl: bool,
}

impl VisualTextLine {
    /// Each glyph supplies its source byte index and horizontal advance in drawing order.
    pub fn new(text: &str, glyphs: impl IntoIterator<Item = (usize, f32)>) -> Self {
        let bidi = BidiInfo::new(text, None);
        let paragraph = bidi.paragraphs.first();
        let rtl = paragraph.is_some_and(|paragraph| paragraph.level.is_rtl());
        let levels = paragraph
            .filter(|_| bidi.has_rtl())
            .map(|paragraph| bidi.reordered_levels(paragraph, paragraph.range.clone()));
        let boundaries = grapheme_boundaries(text);
        let mut cells: Vec<_> = boundaries
            .windows(2)
            .map(|pair| VisualGrapheme {
                bytes: pair[0]..pair[1],
                leading: 0.0,
                trailing: 0.0,
                rtl: levels
                    .as_ref()
                    .and_then(|levels| levels.get(pair[0]))
                    .is_some_and(|level| level.is_rtl()),
            })
            .collect();

        let mut clusters: BTreeMap<usize, Range<f32>> = BTreeMap::new();
        let mut pen = 0.0_f32;
        for (byte, advance) in glyphs {
            if byte >= text.len() || !advance.is_finite() {
                continue;
            }
            let next = pen + advance;
            let first = boundaries.partition_point(|&boundary| boundary <= byte) - 1;
            let start = pen.min(next);
            let end = pen.max(next);
            clusters
                .entry(first)
                .and_modify(|span| {
                    span.start = span.start.min(start);
                    span.end = span.end.max(end);
                })
                .or_insert(start..end);
            pen = next;
        }

        if let Some((&first, span)) = clusters.first_key_value() {
            let x = if cells[first].rtl {
                span.end
            } else {
                span.start
            };
            for cell in &mut cells[..first] {
                cell.leading = x;
                cell.trailing = x;
            }
        }
        let mut clusters = clusters.into_iter().peekable();
        let cell_count = cells.len();
        while let Some((start, span)) = clusters.next() {
            let end = clusters.peek().map_or(cell_count, |(start, _)| *start);
            let count = (end - start) as f32;
            let rtl = cells[start].rtl;
            let (first, last) = if rtl {
                (span.end, span.start)
            } else {
                (span.start, span.end)
            };
            // Shapers can emit one cluster for several graphemes, as with ligatures.
            // Interpolate its internal stops without changing the measured cluster edges.
            let x = |index: usize| first + (last - first) * (index as f32 / count);
            for (index, cell) in cells[start..end].iter_mut().enumerate() {
                cell.leading = x(index);
                cell.trailing = x(index + 1);
                cell.rtl = rtl;
            }
        }
        let mut visual: Vec<_> = cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| (cell.leading != cell.trailing).then_some(index))
            .collect();
        visual.sort_by(|&left, &right| {
            cells[left]
                .span()
                .start
                .total_cmp(&cells[right].span().start)
        });
        Self {
            cells,
            visual,
            width: pen.max(0.0),
            rtl,
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }
    pub fn is_rtl(&self) -> bool {
        self.rtl
    }
    pub fn graphemes(&self) -> &[VisualGrapheme] {
        &self.cells
    }

    pub fn caret_x(&self, position: TextPosition) -> f32 {
        let index = self
            .cells
            .partition_point(|cell| cell.bytes.start < position.offset);
        if let Some(cell) = index
            .checked_sub(1)
            .and_then(|previous| self.cells.get(previous))
            && (position.trailing || index == self.cells.len() || position.offset < cell.bytes.end)
        {
            cell.trailing
        } else {
            self.cells.get(index).map_or(0.0, |cell| cell.leading)
        }
    }

    pub fn hit(&self, x: f32) -> TextPosition {
        let index = self
            .visual
            .partition_point(|&index| self.cells[index].span().end <= x);
        let Some(&index) = self.visual.get(index).or_else(|| self.visual.last()) else {
            return TextPosition::default();
        };
        let cell = &self.cells[index];
        cell.edge(x > cell.leading.midpoint(cell.trailing))
    }

    pub fn edge(&self, right: bool) -> TextPosition {
        self.hit(if right {
            f32::INFINITY
        } else {
            f32::NEG_INFINITY
        })
    }

    pub fn step(&self, position: TextPosition, right: bool) -> Option<TextPosition> {
        let x = self.caret_x(position);
        let index = if right {
            self.visual
                .partition_point(|&index| self.cells[index].span().end <= x + 0.001)
        } else {
            self.visual
                .partition_point(|&index| self.cells[index].span().start < x - 0.001)
                .checked_sub(1)?
        };
        self.visual
            .get(index)
            .map(|&index| self.cells[index].edge(right))
    }

    fn selected(&self, bytes: Range<usize>) -> impl DoubleEndedIterator<Item = &VisualGrapheme> {
        self.visual
            .iter()
            .map(|&index| &self.cells[index])
            .filter(move |cell| {
                !bytes.is_empty() && bytes.start < cell.bytes.end && cell.bytes.start < bytes.end
            })
    }

    pub fn selection_edge(&self, bytes: Range<usize>, right: bool) -> Option<TextPosition> {
        if bytes.is_empty() {
            return None;
        }
        let mut selected = self.selected(bytes);
        let cell = if right {
            selected.next_back()
        } else {
            selected.next()
        }?;
        Some(cell.edge(right))
    }

    pub fn selection_spans(&self, bytes: Range<usize>) -> Vec<Range<f32>> {
        let mut spans: Vec<Range<f32>> = Vec::new();
        if bytes.is_empty() {
            return spans;
        }
        for cell in self.selected(bytes) {
            let span = cell.span();
            if let Some(last) = spans.last_mut()
                && span.start <= last.end + 0.001
            {
                last.end = last.end.max(span.end);
            } else {
                spans.push(span);
            }
        }
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_runs_keep_distinct_caret_edges_and_split_logical_selections() {
        let line = VisualTextLine::new("aאבb", [0, 3, 1, 5].map(|index| (index, 10.0)));
        let before = TextPosition {
            offset: 1,
            trailing: true,
        };
        let after = TextPosition {
            offset: 1,
            trailing: false,
        };
        assert_eq!(line.caret_x(before), 10.0);
        assert_eq!(line.caret_x(after), 30.0);
        assert_eq!(
            line.hit(22.0),
            TextPosition {
                offset: 3,
                trailing: true
            }
        );
        assert_eq!(
            line.step(before, true),
            Some(TextPosition {
                offset: 3,
                trailing: false
            })
        );
        assert_eq!(line.selection_spans(0..3), [0.0..10.0, 20.0..30.0]);
    }

    #[test]
    fn ligatures_and_combining_marks_preserve_grapheme_boundaries() {
        let line = VisualTextLine::new("fi", [(0, 12.0)]);
        assert_eq!(line.hit(6.0).offset, 1);
        let line = VisualTextLine::new("a\u{301}b", [(0, 8.0), (1, 0.0), (3, 8.0)]);
        assert_eq!(line.hit(7.0).offset, 3);
        assert_eq!(line.selection_spans(0..3), [0.0..8.0]);
    }
}
