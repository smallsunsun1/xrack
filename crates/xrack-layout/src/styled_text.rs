use crate::{
    LabelPlan, Point, Rect, Size, TextAlignment, TextAreaMotion, TextEdit, TextPosition,
    VisualTextLine,
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};
use unicode_bidi::{Direction, InitialInfo, ParagraphBidiInfo, get_base_direction};
use xrack_runtime::{LayoutDirection, Node, StyledText, TextHorizontalAlignment, TextStyle};

#[derive(Clone, Debug, PartialEq)]
struct Piece {
    range: Option<Range<usize>>,
    rtl_mark: bool,
    style: TextStyle,
    link: Option<usize>,
}

impl Piece {
    fn text<'a>(&self, document: &'a StyledText) -> Cow<'a, str> {
        let Some(range) = &self.range else {
            return Cow::Borrowed("…");
        };
        let text = &document.text()[range.clone()];
        if self.rtl_mark {
            Cow::Owned(format!("\u{200f}{text}"))
        } else {
            Cow::Borrowed(text)
        }
    }
}

/// One shaped span in visual order. Bounds are local to the label.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledFragment {
    piece: Piece,
    pub bounds: Rect,
    visual: Option<Arc<VisualTextLine>>,
}

impl StyledFragment {
    pub(crate) fn contains_text_offset(&self, offset: usize) -> bool {
        self.piece
            .range
            .as_ref()
            .is_some_and(|range| range.contains(&offset))
    }

    pub fn text<'a>(&self, document: &'a StyledText) -> Cow<'a, str> {
        self.piece.text(document)
    }

    /// Identifies the painted substring and its optional bidi prefix.
    #[doc(hidden)]
    pub fn source_span(&self) -> (Option<Range<usize>>, bool) {
        (self.piece.range.clone(), self.piece.rtl_mark)
    }

    pub fn style(&self) -> TextStyle {
        self.piece.style
    }

    pub fn link(&self) -> Option<usize> {
        self.piece.link
    }

    #[doc(hidden)]
    pub fn selection_geometry(&self) -> Option<(&VisualTextLine, Range<usize>, usize)> {
        Some((
            self.visual.as_deref()?,
            self.piece.range.clone()?,
            if self.piece.rtl_mark {
                '\u{200f}'.len_utf8()
            } else {
                0
            },
        ))
    }

    fn position(&self, mut local: TextPosition) -> Option<TextPosition> {
        let (_, range, prefix) = self.selection_geometry()?;
        local.offset = range.start + local.offset.saturating_sub(prefix).min(range.len());
        Some(local)
    }
}

/// Final text geometry consumed by painting, hit testing and accessibility.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledTextLayout {
    pub size: Size,
    pub fragments: Vec<StyledFragment>,
}

#[derive(Clone, Copy)]
struct IndexedLine {
    source_start: usize,
    first_fragment: usize,
    max_width: f32,
}

/// Line starts and prefix widths for a top-aligned, unbounded rich-text layout.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct StyledTextLineIndex {
    lines: Vec<IndexedLine>,
}

impl StyledTextLineIndex {
    pub fn allocated_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<IndexedLine>()
    }
}

impl StyledTextLayout {
    #[doc(hidden)]
    pub fn allocated_bytes(&self) -> usize {
        let mut counted = HashSet::new();
        self.fragments.capacity() * std::mem::size_of::<StyledFragment>()
            + self
                .fragments
                .iter()
                .filter_map(|fragment| fragment.visual.as_ref())
                .filter(|line| counted.insert(Arc::as_ptr(line)))
                .map(|line| line.allocated_bytes())
                .sum::<usize>()
    }

    #[doc(hidden)]
    pub fn shape_selection(
        &mut self,
        document: &StyledText,
        shape: impl FnMut(&str, TextStyle) -> VisualTextLine,
    ) {
        self.shape_selection_from(document, 0, shape);
    }

    #[doc(hidden)]
    pub fn shape_selection_from(
        &mut self,
        document: &StyledText,
        start: usize,
        mut shape: impl FnMut(&str, TextStyle) -> VisualTextLine,
    ) -> usize {
        let mut lines = HashMap::new();
        let mut bytes = 0;
        for fragment in &mut self.fragments[start..] {
            if fragment.piece.range.is_some() {
                let key = (fragment.text(document), fragment.style());
                let line = lines.entry(key).or_insert_with_key(|(text, style)| {
                    let line = shape(text, *style);
                    bytes += line.allocated_bytes();
                    Arc::new(line)
                });
                fragment.visual = Some(line.clone());
            }
        }
        bytes
    }

    #[doc(hidden)]
    pub fn selection_hit(&self, point: Point) -> Option<TextPosition> {
        let distance = |point: f32, start: f32, size: f32| {
            (start - point).max(point - (start + size)).max(0.0)
        };
        let (fragment, _) = self
            .fragments
            .iter()
            .filter(|fragment| fragment.visual.is_some())
            .map(|fragment| {
                let y = distance(point.y, fragment.bounds.y, fragment.bounds.height);
                (fragment, y)
            })
            .min_by(|(a, ay), (b, by)| {
                let x = |f: &StyledFragment| distance(point.x, f.bounds.x, f.bounds.width);
                ay.total_cmp(by).then_with(|| x(a).total_cmp(&x(b)))
            })?;
        fragment.position(fragment.visual.as_ref()?.hit(point.x - fragment.bounds.x))
    }

    #[doc(hidden)]
    pub fn move_selection(
        &self,
        text: &str,
        editor: &mut TextEdit,
        motion: TextAreaMotion,
        selecting: bool,
    ) -> bool {
        let caret = editor.position(text);
        let items = self
            .fragments
            .iter()
            .enumerate()
            .filter_map(|(index, fragment)| {
                let (visual, range, prefix) = fragment.selection_geometry()?;
                Some((index, fragment, visual, range, prefix))
            });
        let Some((index, fragment, visual, range, prefix)) =
            items.min_by_key(|(_, _, _, range, _)| {
                let distance = range.start.saturating_sub(caret.offset)
                    + caret.offset.saturating_sub(range.end);
                let wrong_edge = if caret.trailing {
                    caret.offset == range.start
                } else {
                    caret.offset == range.end
                };
                (distance, wrong_edge)
            })
        else {
            return false;
        };
        let local = TextPosition {
            offset: caret.offset.clamp(range.start, range.end) - range.start + prefix,
            trailing: caret.trailing,
        };
        let mut preferred = None;
        let next = match motion {
            TextAreaMotion::Left | TextAreaMotion::Right => {
                let right = matches!(motion, TextAreaMotion::Right);
                visual
                    .step(local, right)
                    .and_then(|position| fragment.position(position))
                    .or_else(|| {
                        let adjacent = if right {
                            self.fragments[index + 1..]
                                .iter()
                                .find(|fragment| fragment.visual.is_some())
                        } else {
                            self.fragments[..index]
                                .iter()
                                .rev()
                                .find(|fragment| fragment.visual.is_some())
                        }?;
                        let visual = adjacent.visual.as_ref()?;
                        let edge = visual.edge(!right);
                        let position = adjacent.position(edge)?;
                        if adjacent.bounds.y == fragment.bounds.y && position.offset == caret.offset
                        {
                            visual
                                .step(edge, right)
                                .and_then(|next| adjacent.position(next))
                                .or(Some(position))
                        } else {
                            Some(position)
                        }
                    })
            }
            TextAreaMotion::Up
            | TextAreaMotion::Down
            | TextAreaMotion::PageUp
            | TextAreaMotion::PageDown => {
                let x = editor
                    .preferred_x()
                    .unwrap_or(fragment.bounds.x + visual.caret_x(local));
                preferred = Some(x);
                let delta = match motion {
                    TextAreaMotion::Up => -fragment.bounds.height,
                    TextAreaMotion::Down => fragment.bounds.height,
                    TextAreaMotion::PageUp => -self.size.height,
                    _ => self.size.height,
                };
                self.selection_hit(Point {
                    x,
                    y: fragment.bounds.y + fragment.bounds.height * 0.5 + delta,
                })
            }
            _ => return false,
        };
        if !selecting
            && matches!(motion, TextAreaMotion::Left | TextAreaMotion::Right)
            && !editor.selection(text).is_empty()
        {
            // Collapse an existing range in the same direction as the text line.
            let range = editor.selection(text);
            let end = matches!(motion, TextAreaMotion::Right) != visual.is_rtl();
            editor.place_caret(
                text,
                TextPosition {
                    offset: if end { range.end } else { range.start },
                    trailing: end,
                },
                false,
            );
        } else if let Some(next) = next {
            editor.place_caret(text, next, selecting);
            if let Some(x) = preferred {
                editor.set_preferred_x(x);
            }
        }
        true
    }

    pub(crate) fn align_in_direction(
        &mut self,
        node: &Node,
        width: f32,
        direction: LayoutDirection,
    ) {
        let alignment =
            TextAlignment::from_node(node, Some(direction), TextHorizontalAlignment::Start);
        for line in self
            .fragments
            .chunk_by_mut(|left, right| left.bounds.y == right.bounds.y)
        {
            let first = line[0].bounds.x;
            let last = line[line.len() - 1].bounds;
            let shift = alignment.offset(width - (last.x + last.width - first), false) - first;
            for fragment in line {
                fragment.bounds.x += shift;
            }
        }
    }

    pub fn new(
        node: &Node,
        document: &StyledText,
        available: Size,
        font_size: f32,
        advance: impl FnMut(&str, TextStyle) -> f32,
    ) -> Self {
        Self::build(node, document, available, font_size, advance, None)
    }

    #[doc(hidden)]
    pub fn indexed(
        node: &Node,
        document: &StyledText,
        available: Size,
        font_size: f32,
        advance: impl FnMut(&str, TextStyle) -> f32,
    ) -> (Self, StyledTextLineIndex) {
        let mut lines = StyledTextLineIndex::default();
        let layout = Self::build(
            node,
            document,
            available,
            font_size,
            advance,
            Some(&mut lines),
        );
        (layout, lines)
    }

    /// Replace the complete paragraphs starting at `offset`. Both plans
    /// must use the same unbounded, top-aligned geometry and typography.
    #[doc(hidden)]
    pub fn replace_tail(
        &self,
        index: &StyledTextLineIndex,
        offset: usize,
        tail: (Self, StyledTextLineIndex),
        first_link: usize,
        font_size: f32,
    ) -> (Self, StyledTextLineIndex, usize) {
        let (mut tail, mut tail_index) = tail;
        let line_count = index
            .lines
            .partition_point(|line| line.source_start < offset);
        let prefix_fragments = index
            .lines
            .get(line_count)
            .map_or(self.fragments.len(), |line| line.first_fragment);
        let mut lines = index.lines[..line_count].to_vec();
        let max_width = lines.last().map_or(0.0, |line| line.max_width);
        let line_height = font_size * 1.4;
        for line in 0..tail_index.lines.len() {
            let end = tail_index
                .lines
                .get(line + 1)
                .map_or(tail.fragments.len(), |next| next.first_fragment);
            let current = &mut tail_index.lines[line];
            let y = (line_count + line) as f32 * line_height;
            for fragment in &mut tail.fragments[current.first_fragment..end] {
                fragment.bounds.y = y;
                if let Some(range) = &mut fragment.piece.range {
                    range.start += offset;
                    range.end += offset;
                }
                if let Some(link) = &mut fragment.piece.link {
                    *link += first_link;
                }
            }
            current.source_start += offset;
            current.first_fragment += prefix_fragments;
            current.max_width = current.max_width.max(max_width);
        }
        lines.append(&mut tail_index.lines);
        let size = Size::new(
            max_width.max(tail.size.width),
            lines.len() as f32 * line_height,
        );
        let mut fragments = Vec::with_capacity(prefix_fragments + tail.fragments.len());
        fragments.extend_from_slice(&self.fragments[..prefix_fragments]);
        fragments.append(&mut tail.fragments);
        (
            Self { size, fragments },
            StyledTextLineIndex { lines },
            prefix_fragments,
        )
    }

    fn build(
        node: &Node,
        document: &StyledText,
        available: Size,
        font_size: f32,
        mut advance: impl FnMut(&str, TextStyle) -> f32,
        mut index: Option<&mut StyledTextLineIndex>,
    ) -> Self {
        // unicode-bidi copies its resolved levels for each visual line. Keep
        // those copies paragraph-local, including when measuring wrap points.
        let bidi: Vec<_> = InitialInfo::new(document.text(), None)
            .paragraphs
            .into_iter()
            .map(|paragraph| {
                let text = &document.text()[paragraph.range.clone()];
                let bidi = ParagraphBidiInfo::new(text, Some(paragraph.level));
                (paragraph.range, bidi)
            })
            .collect();
        let mut width = |piece: &Piece| advance(&piece.text(document), piece.style);
        let mut widths = Vec::new();
        let plan = LabelPlan::with_widths(
            node,
            available,
            font_size,
            node.layout_direction(),
            |range, ellipsis| {
                pieces(document, &bidi, range, ellipsis)
                    .iter()
                    .map(&mut width)
                    .sum()
            },
            index.is_some().then_some(&mut widths),
        );
        let mut fragments = Vec::new();
        for (line_index, line) in plan.lines.into_iter().enumerate() {
            if let Some(index) = index.as_deref_mut() {
                index.lines.push(IndexedLine {
                    source_start: line.range.start,
                    first_fragment: fragments.len(),
                    max_width: widths[line_index],
                });
            }
            let mut x = line.origin.x;
            for piece in pieces(document, &bidi, line.range, line.ellipsis) {
                let advance = width(&piece);
                let bounds = Rect::new(x, line.origin.y, advance, font_size * 1.4);
                fragments.push(StyledFragment {
                    piece,
                    bounds,
                    visual: None,
                });
                x += advance;
            }
        }
        Self {
            size: plan.size,
            fragments,
        }
    }
}

fn pieces(
    document: &StyledText,
    paragraphs: &[(Range<usize>, ParagraphBidiInfo<'_>)],
    range: Range<usize>,
    ellipsis: bool,
) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut rtl = false;
    let first = paragraphs.partition_point(|(bounds, _)| bounds.end <= range.start);
    for (bounds, bidi) in &paragraphs[first..] {
        let start = bounds.start;
        let line = start.max(range.start)..bounds.end.min(range.end);
        if line.start > range.end {
            break;
        }
        if pieces.is_empty() {
            rtl = bidi.paragraph_level.is_rtl();
        }
        if line.is_empty() {
            continue;
        }
        if bidi.is_pure_ltr {
            split_run(document, line, false, &mut pieces);
            continue;
        }
        let (levels, runs) = bidi.visual_runs(line.start - start..line.end - start);
        for run in runs {
            let rtl = levels[run.start].is_rtl();
            split_run(
                document,
                start + run.start..start + run.end,
                rtl,
                &mut pieces,
            );
        }
    }
    if ellipsis {
        let mark = Piece {
            range: None,
            rtl_mark: false,
            style: TextStyle::default(),
            link: None,
        };
        if rtl {
            pieces.insert(0, mark);
        } else {
            pieces.push(mark);
        }
    }
    pieces
}

fn split_run(document: &StyledText, range: Range<usize>, rtl: bool, pieces: &mut Vec<Piece>) {
    let first = pieces.len();
    let mut start = range.start;
    let styles = document.runs();
    let links = document.links();
    while start < range.end {
        let style_index = styles.partition_point(|run| run.end <= start);
        let link_index = links.partition_point(|link| link.end <= start);
        let (style, style_end) = match styles.get(style_index) {
            Some(run) if run.start <= start => (run.style, run.end),
            Some(run) => (TextStyle::default(), run.start),
            None => (TextStyle::default(), range.end),
        };
        let (link, link_end) = match links.get(link_index) {
            Some(link) if link.start <= start => (Some(link_index), link.end),
            Some(link) => (None, link.start),
            None => (None, range.end),
        };
        let end = range.end.min(style_end).min(link_end);
        let text = &document.text()[start..end];
        let rtl_mark = rtl && get_base_direction(text) != Direction::Rtl;
        pieces.push(Piece {
            range: Some(start..end),
            rtl_mark,
            style,
            link,
        });
        start = end;
    }
    if rtl {
        pieces[first..].reverse();
    }
}
