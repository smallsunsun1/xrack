use crate::{
    LabelPlan, Point, Rect, Size, TextAlignment, TextAreaMotion, TextEdit, TextPosition,
    VisualTextLine,
};
use std::{borrow::Cow, ops::Range};
use unicode_bidi::{BidiInfo, Direction, get_base_direction};
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
    visual: Option<VisualTextLine>,
}

impl StyledFragment {
    pub fn text<'a>(&self, document: &'a StyledText) -> Cow<'a, str> {
        self.piece.text(document)
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
            self.visual.as_ref()?,
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

impl StyledTextLayout {
    #[doc(hidden)]
    pub fn shape_selection(
        &mut self,
        document: &StyledText,
        mut shape: impl FnMut(&str, TextStyle) -> VisualTextLine,
    ) {
        for fragment in &mut self.fragments {
            if fragment.piece.range.is_some() {
                fragment.visual = Some(shape(&fragment.text(document), fragment.style()));
            }
        }
    }

    #[doc(hidden)]
    pub fn selection_hit(&self, point: Point) -> Option<TextPosition> {
        let distance = |point: f32, start: f32, size: f32| {
            (start - point).max(point - (start + size)).max(0.0)
        };
        let fragment = self
            .fragments
            .iter()
            .filter(|fragment| fragment.visual.is_some())
            .min_by(|a, b| {
                let y = |f: &StyledFragment| distance(point.y, f.bounds.y, f.bounds.height);
                let x = |f: &StyledFragment| distance(point.x, f.bounds.x, f.bounds.width);
                y(a).total_cmp(&y(b)).then_with(|| x(a).total_cmp(&x(b)))
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
        mut advance: impl FnMut(&str, TextStyle) -> f32,
    ) -> Self {
        let bidi = BidiInfo::new(document.text(), None);
        let bidi = bidi.has_rtl().then_some(&bidi);
        let mut width = |piece: &Piece| advance(&piece.text(document), piece.style);
        let plan = LabelPlan::with_font_size(node, available, font_size, |range, ellipsis| {
            pieces(document, bidi, range, ellipsis)
                .iter()
                .map(&mut width)
                .sum()
        });
        let mut fragments = Vec::new();
        for line in plan.lines {
            let mut x = line.origin.x;
            for piece in pieces(document, bidi, line.range, line.ellipsis) {
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
    bidi: Option<&BidiInfo<'_>>,
    range: Range<usize>,
    ellipsis: bool,
) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut rtl = false;
    if let Some(bidi) = bidi {
        let first = bidi
            .paragraphs
            .partition_point(|para| para.range.end <= range.start);
        for para in &bidi.paragraphs[first..] {
            let line = para.range.start.max(range.start)..para.range.end.min(range.end);
            if line.start > range.end {
                break;
            }
            if pieces.is_empty() {
                rtl = para.level.is_rtl();
            }
            if line.is_empty() {
                continue;
            }
            let (levels, runs) = bidi.visual_runs(para, line);
            for run in runs {
                let rtl = levels[run.start].is_rtl();
                split_run(document, run, rtl, &mut pieces);
            }
        }
    } else {
        split_run(document, range, false, &mut pieces);
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
