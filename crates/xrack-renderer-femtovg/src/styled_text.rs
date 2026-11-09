use crate::{color, text::TextSystem};
use femtovg::{Canvas, Paint, renderer::WGPURenderer};
use std::{borrow::Cow, collections::HashMap, ops::Range};
use unicode_bidi::{BidiInfo, Direction, get_base_direction};
use xrack_layout::{LabelPlan, Rect, Size, TextMeasurer};
use xrack_runtime::{Color, Node, StyledText, TextStyle};

struct Piece<'a> {
    text: Cow<'a, str>,
    style: TextStyle,
}

struct Shaper<'a> {
    system: &'a TextSystem,
    node: &'a Node,
    text: &'a StyledText,
    bidi: Option<BidiInfo<'a>>,
    tint: Color,
    enabled: bool,
    paints: HashMap<TextStyle, Paint>,
}

impl<'a> Shaper<'a> {
    fn new(
        system: &'a TextSystem,
        node: &'a Node,
        text: &'a StyledText,
        tint: Color,
        enabled: bool,
    ) -> Self {
        let bidi = BidiInfo::new(text.text(), None);
        Self {
            system,
            node,
            text,
            bidi: bidi.has_rtl().then_some(bidi),
            tint,
            enabled,
            paints: HashMap::new(),
        }
    }

    fn paint(&mut self, style: TextStyle) -> &Paint {
        self.paints.entry(style).or_insert_with(|| {
            let tint = if self.enabled {
                style.color.unwrap_or(self.tint)
            } else {
                self.tint
            };
            self.system.paint_styled(self.node, color(tint), style)
        })
    }

    fn plan(&mut self, available: Size) -> LabelPlan {
        let system = self.system;
        LabelPlan::with_font_size(
            self.node,
            available,
            system.font_size(self.node),
            |range, ellipsis| {
                self.pieces(range, ellipsis)
                    .iter()
                    .map(|piece| system.advance_painted(&piece.text, self.paint(piece.style)))
                    .sum()
            },
        )
    }

    fn pieces(&self, range: Range<usize>, ellipsis: bool) -> Vec<Piece<'a>> {
        let mut pieces = Vec::new();
        let mut rtl = false;
        if let Some(bidi) = &self.bidi {
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
                    self.split_run(run.clone(), levels[run.start].is_rtl(), &mut pieces);
                }
            }
        } else {
            self.split_run(range, false, &mut pieces);
        }
        if ellipsis {
            let mark = Piece {
                text: Cow::Borrowed("…"),
                style: TextStyle::default(),
            };
            if rtl {
                pieces.insert(0, mark);
            } else {
                pieces.push(mark);
            }
        }
        pieces
    }

    fn split_run(&self, range: Range<usize>, rtl: bool, pieces: &mut Vec<Piece<'a>>) {
        let first = pieces.len();
        let runs = self.text.runs();
        let index = runs.partition_point(|run| run.end <= range.start);
        let mut start = range.start;
        for run in &runs[index..] {
            if run.start >= range.end {
                break;
            }
            self.push_piece(
                start..run.start.min(range.end),
                rtl,
                TextStyle::default(),
                pieces,
            );
            let end = run.end.min(range.end);
            self.push_piece(start.max(run.start)..end, rtl, run.style, pieces);
            start = end;
        }
        self.push_piece(start..range.end, rtl, TextStyle::default(), pieces);
        if rtl {
            pieces[first..].reverse();
        }
    }

    fn push_piece(
        &self,
        range: Range<usize>,
        rtl: bool,
        style: TextStyle,
        pieces: &mut Vec<Piece<'a>>,
    ) {
        if range.is_empty() {
            return;
        }
        let text = &self.text.text()[range];
        let text = if !rtl || get_base_direction(text) == Direction::Rtl {
            Cow::Borrowed(text)
        } else {
            // Keep neutral fragments in their resolved direction when shaped separately.
            Cow::Owned(format!("\u{200f}{text}"))
        };
        pieces.push(Piece { text, style });
    }
}

pub(crate) fn measure(
    system: &TextSystem,
    node: &Node,
    text: &StyledText,
    available: Size,
) -> Size {
    Shaper::new(system, node, text, Color::default(), true)
        .plan(available)
        .size
}

pub(crate) fn draw(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    node: &Node,
    text: &StyledText,
    bounds: Rect,
    tint: Color,
    enabled: bool,
) -> Result<(), String> {
    let mut shaper = Shaper::new(system, node, text, tint, enabled);
    let plan = shaper.plan(Size::new(bounds.width, bounds.height));
    let baseline = system
        .context()
        .measure_font(shaper.paint(TextStyle::default()))
        .map_or_else(|_| system.font_size(node), |metrics| metrics.ascender());
    for line in plan.lines {
        let mut x = bounds.x + line.origin.x;
        let y = bounds.y + line.origin.y + baseline;
        for piece in shaper.pieces(line.range, line.ellipsis) {
            let metrics = canvas
                .fill_text(x, y, &piece.text, shaper.paint(piece.style))
                .map_err(|error| format!("styled text drawing failed: {error}"))?;
            x += metrics.width();
        }
    }
    Ok(())
}
