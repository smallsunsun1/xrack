use crate::{LabelPlan, Rect, Size};
use std::{borrow::Cow, ops::Range};
use unicode_bidi::{BidiInfo, Direction, get_base_direction};
use xrack_runtime::{Node, StyledText, TextStyle};

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
}

/// Final text geometry consumed by painting, hit testing and accessibility.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledTextLayout {
    pub size: Size,
    pub fragments: Vec<StyledFragment>,
}

impl StyledTextLayout {
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
                fragments.push(StyledFragment { piece, bounds });
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
