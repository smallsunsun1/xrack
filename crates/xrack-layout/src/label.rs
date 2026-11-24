use crate::{
    Point, Size, TextMeasurer, font_size,
    text_area::{fitting_prefix, fitting_prefix_until, grapheme_boundaries, paragraphs},
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::HashMap,
    num::NonZeroU32,
    ops::Range,
};
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
    pub(crate) reusable_widths: Cell<Option<WidthInterval>>,
    pub(crate) skip_stable_word_probes: bool,
    repeated: Option<RepeatedParagraphs>,
    pub(crate) replayed_widths: Cell<bool>,
    line_breaks: RefCell<RetainedLineBreaks>,
    replayed_breaks: Cell<bool>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WidthInterval {
    pub(crate) minimum: f32,
    pub(crate) maximum: f32,
}

impl WidthInterval {
    fn contains(self, width: f32) -> bool {
        self.minimum <= width && width < self.maximum
    }

    fn intersect(&mut self, other: Self) {
        self.minimum = self.minimum.max(other.minimum);
        self.maximum = self.maximum.min(other.maximum);
    }

    // Every recorded comparison must make the same decision at a reused width.
    // The lower bound is inclusive and the upper bound is exclusive.
    fn include_probe(&mut self, width: f32, fits: bool) {
        if fits {
            self.minimum = self.minimum.max(width);
        } else {
            self.maximum = self.maximum.min(width);
        }
    }
}

#[derive(Default)]
struct RetainedLineBreaks {
    generation: u32,
    entries: Vec<Option<LineBreak>>,
}

impl RetainedLineBreaks {
    const SLOTS: usize = 32_768;

    fn prepare(&mut self, generation: u32) {
        self.entries.resize(Self::SLOTS, None);
        if self.generation != generation {
            self.entries.fill(None);
            self.generation = generation;
        }
    }

    fn slot(&mut self, position: u32) -> &mut Option<LineBreak> {
        let index = position.wrapping_mul(0x9e3779b1) >> (u32::BITS - Self::SLOTS.ilog2());
        &mut self.entries[index as usize]
    }
}

#[derive(Clone, Copy)]
struct LineBreak {
    // PreparedLabel bounds the number of grapheme and word offsets.
    position: u32,
    next: u32,
    end: usize,
    widths: WidthInterval,
}

struct RepeatedParagraphs {
    valid: Cell<bool>,
    shapes: Vec<Option<ParagraphShape>>,
}

#[derive(Clone, Copy)]
struct ParagraphShape {
    group: usize,
    heading: usize,
}

struct RawLine {
    range: Range<usize>,
    rtl: bool,
    width_source: Option<NonZeroU32>,
}

impl RawLine {
    fn new(range: Range<usize>, rtl: bool) -> Self {
        Self {
            range,
            rtl,
            width_source: None,
        }
    }
}

struct RepeatedWrap {
    source: usize,
    lines: Range<usize>,
    heading_probes: u8,
    interval: WidthInterval,
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
            reusable_widths: Cell::new(None),
            skip_stable_word_probes: false,
            repeated: None,
            replayed_widths: Cell::new(false),
            line_breaks: RefCell::default(),
            replayed_breaks: Cell::new(false),
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

    pub(crate) fn prepare_repeated(
        &mut self,
        mut key: impl FnMut(&Range<usize>) -> Option<(u32, usize, u64)>,
        mut equivalent: impl FnMut(&Range<usize>, &Range<usize>, usize) -> bool,
    ) {
        if self.paragraphs.len() < 128
            || self
                .repeated
                .as_ref()
                .is_some_and(|repeated| repeated.valid.get())
        {
            return;
        }
        let mut candidates = HashMap::<(u32, usize, u64), usize>::new();
        let mut shapes = vec![None; self.paragraphs.len()];
        let mut reused = false;
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            let Some(signature) = key(&paragraph.source) else {
                continue;
            };
            let heading = signature.1;
            if let Some(&old) = candidates.get(&signature) {
                let reference = &self.paragraphs[old];
                if equivalent(&reference.source, &paragraph.source, heading)
                    && self.graphemes[reference.graphemes.clone()]
                        == self.graphemes[paragraph.graphemes.clone()]
                    && self.words[reference.words.clone()] == self.words[paragraph.words.clone()]
                {
                    let shape = ParagraphShape {
                        group: old,
                        heading,
                    };
                    shapes[index] = Some(shape);
                    shapes[old] = Some(shape);
                    reused = true;
                    continue;
                }
            }
            candidates.insert(signature, index);
        }
        self.repeated = Some(RepeatedParagraphs {
            valid: Cell::new(true),
            shapes: if reused { shapes } else { Vec::new() },
        });
    }

    pub(crate) fn invalidate_widths(&self) {
        self.reusable_widths.set(None);
        if let Some(repeated) = &self.repeated {
            repeated.valid.set(false);
        }
        let next = self.font_generation.get().wrapping_add(1);
        if next == 0 {
            for paragraph in &self.paragraphs {
                paragraph.one_line.set(None);
            }
            for probes in &self.first_line_probes {
                probes.set(FirstLineProbes::new(0));
            }
            if let Ok(mut breaks) = self.line_breaks.try_borrow_mut() {
                breaks.entries.clear();
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
        mut advance: impl FnMut(Range<usize>, bool) -> f32,
        mut widths: Option<&mut Vec<f32>>,
        prepared: Option<&PreparedLabel>,
    ) -> Self {
        let text = node.text("text").unwrap_or("");
        let style = LabelStyle::for_node(node, direction);
        let generation = prepared.map(|prepared| {
            prepared.replayed_breaks.set(false);
            prepared.font_generation.get()
        });
        let width_count = widths.as_ref().map_or(0, |widths| widths.len());
        let plan = Self::plan(
            text,
            available,
            size,
            style,
            &mut advance,
            widths.as_deref_mut(),
            prepared,
        );
        if let Some(prepared) = prepared
            && prepared.replayed_breaks.replace(false)
            && generation != Some(prepared.font_generation.get())
        {
            // A fallback font registered during this pass may invalidate an
            // earlier replayed line. Rebuild in the now-current font set.
            if let Some(widths) = &mut widths {
                widths.truncate(width_count);
            }
            return Self::plan(
                text,
                available,
                size,
                style,
                advance,
                widths,
                Some(prepared),
            );
        }
        plan
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
        let font_generation_at_start = prepared.map(|prepared| prepared.font_generation.get());
        let mut reusable = (prepared.is_some()
            && available.height == f32::INFINITY
            && available.width.is_finite()
            && !elide
            && vertical == Vertical::Top
            && max_lines == usize::MAX)
            .then_some(WidthInterval {
                minimum: 0.0,
                maximum: f32::INFINITY,
            });
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
        let mut has_width_sources = false;
        let short_word_search = prepared.is_some_and(|prepared| prepared.skip_stable_word_probes)
            && wrap == TextWrap::WordWrap;
        let mut generated = paragraphs(text);
        let mut retained = prepared.map(|prepared| prepared.paragraphs.iter());
        let sources = std::iter::from_fn(|| match &mut retained {
            Some(retained) => retained
                .next()
                .map(|paragraph| (paragraph.source.clone(), Some(paragraph))),
            None => generated.next().map(|range| (range, None)),
        });
        let repeated = prepared
            .and_then(|prepared| prepared.repeated.as_ref())
            .filter(|repeated| !repeated.shapes.is_empty());
        let mut memo = HashMap::<usize, RepeatedWrap>::new();
        let mut line_breaks = prepared
            .filter(|prepared| {
                prepared.paragraphs.len() >= 128 && reusable.is_some() && wrap == TextWrap::WordWrap
            })
            .map(|prepared| {
                let mut breaks = prepared.line_breaks.borrow_mut();
                breaks.prepare(prepared.font_generation.get());
                breaks
            });
        'paragraphs: for (paragraph_index, (range, paragraph)) in sources.enumerate() {
            let content = &text[range.clone()];
            let rtl = horizontal.direction.is_none()
                && matches!(horizontal.mode, Horizontal::Start | Horizontal::End)
                && paragraph.map_or_else(
                    || get_base_direction(content) == Direction::Rtl,
                    |paragraph| paragraph.rtl,
                );
            if horizontal.factor(rtl) != 0.0 {
                reusable = None;
            }
            if wrap == TextWrap::NoWrap || content.is_empty() {
                raw.push(RawLine::new(range, rtl));
                if raw.len() > limit {
                    break;
                }
                continue;
            }
            let retained = prepared.zip(paragraph);
            let font_generation = prepared.map_or(0, |prepared| prepared.font_generation.get());
            if let Some(paragraph) = paragraph
                && let Some(fit) = paragraph.one_line.get()
                && fit.font_generation == font_generation
                && fit.wrap == wrap
                && available.width >= fit.minimum_width
            {
                if let Some(interval) = &mut reusable {
                    interval.minimum = interval.minimum.max(fit.minimum_width);
                }
                raw.push(RawLine::new(range.start..paragraph.trimmed_end, rtl));
                if raw.len() > limit {
                    break;
                }
                continue;
            }
            let group = repeated
                .filter(|repeated| repeated.valid.get())
                .and_then(|repeated| repeated.shapes[paragraph_index]);
            if let Some(shape) = group
                && let Some(saved) = memo.get(&shape.group)
                && let Some((prepared, paragraph)) = retained
            {
                let cache = &prepared.first_line_probes[paragraph_index];
                let mut probes = cache.get();
                let mask = saved.heading_probes;
                let last = paragraph.graphemes.len() - 1;
                let mut interval = saved.interval;
                let mut matches = true;
                for offset in 1..shape.heading {
                    if mask & (1 << offset) == 0 {
                        continue;
                    }
                    let width =
                        probes.measure(offset, last, prepared.font_generation.get(), || {
                            advance(range.start..range.start + offset, false)
                        });
                    let fits = width <= available.width;
                    if !fits {
                        matches = false;
                        break;
                    }
                    interval.include_probe(width, true);
                }
                if probes.font_generation == prepared.font_generation.get() {
                    cache.set(probes);
                }
                if matches && font_generation == prepared.font_generation.get() {
                    // Widths beyond the heading match the representative.
                    // Keep the target heading's comparisons in the width certificate.
                    if let Some(proof) = &mut reusable {
                        proof.intersect(interval);
                    }
                    has_width_sources |= !elide;
                    for index in saved.lines.clone() {
                        let line = &raw[index].range;
                        let start = range.start + (line.start - saved.source);
                        // Prepared documents bound the line index well below u32::MAX.
                        let width_source = if elide {
                            None
                        } else {
                            NonZeroU32::new(index as u32 + 1)
                        };
                        raw.push(RawLine {
                            range: start..start + line.len(),
                            rtl,
                            width_source,
                        });
                        if raw.len() > limit {
                            break 'paragraphs;
                        }
                    }
                    continue;
                }
            }
            let first_raw = raw.len();
            let mut heading_probes = 0_u8;
            let mut memo_proof = WidthInterval {
                minimum: 0.0,
                maximum: f32::INFINITY,
            };
            let mut can_store = group.is_some();
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
                let from = range.start + graphemes[start];
                let break_slot = retained
                    .filter(|(prepared, _)| {
                        group.is_none()
                            && Some(prepared.font_generation.get()) == font_generation_at_start
                    })
                    .and_then(|(prepared, paragraph)| {
                        let position = (paragraph.graphemes.start + start) as u32;
                        let slot = line_breaks.as_mut()?.slot(position);
                        Some((slot, position, prepared))
                    });
                if let Some((slot, position, prepared)) = &break_slot
                    && let Some(saved) = **slot
                    && saved.position == *position
                    && saved.widths.contains(available.width)
                {
                    if let Some(interval) = &mut reusable {
                        interval.intersect(saved.widths);
                    }
                    prepared.replayed_breaks.set(true);
                    raw.push(RawLine::new(from..saved.end, rtl));
                    if raw.len() > limit {
                        break 'paragraphs;
                    }
                    start = saved.next as usize;
                    continue;
                }
                let mut observed = break_slot.as_ref().map(|_| WidthInterval {
                    minimum: 0.0,
                    maximum: f32::INFINITY,
                });
                let stop = |fit: usize, highest_fit: usize| {
                    short_word_search && same_word_break(graphemes, words, start, fit, highest_fit)
                };
                let mut fits = |index: usize, width: f32| {
                    let accepts_width = width <= available.width;
                    if let Some(interval) = &mut observed {
                        interval.include_probe(width, accepts_width);
                    }
                    if let Some(interval) = &mut reusable {
                        interval.include_probe(width, accepts_width);
                    }
                    if let Some(shape) = group {
                        if start == 0 && graphemes[index] < shape.heading {
                            heading_probes |= 1 << graphemes[index];
                        } else {
                            memo_proof.include_probe(width, accepts_width);
                        }
                    }
                    accepts_width
                };
                let mut minimum_width = f32::NEG_INFINITY;
                let (fit, whole_paragraph_fits) = if let Some((prepared, _)) = retained
                    && start == 0
                {
                    let cache = &prepared.first_line_probes[paragraph_index];
                    let mut probes = cache.get();
                    let fit = fitting_prefix_until(
                        0,
                        last,
                        |index| {
                            let width =
                                probes.measure(index, last, prepared.font_generation.get(), || {
                                    advance(range.start..range.start + graphemes[index], false)
                                });
                            minimum_width = minimum_width.max(width);
                            fits(index, width)
                        },
                        stop,
                    );
                    if probes.font_generation == prepared.font_generation.get() {
                        cache.set(probes);
                    }
                    (fit, fit == last)
                } else {
                    let fit = fitting_prefix_until(
                        start,
                        last,
                        |index| {
                            let from = range.start + graphemes[start];
                            let to = range.start + graphemes[index];
                            fits(index, advance(from..to, false))
                        },
                        stop,
                    );
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
                // Only save wraps that consume the complete heading in the
                // first line. Every probe inside that heading must have passed.
                if start == 0 && group.is_some_and(|shape| graphemes[next] < shape.heading) {
                    can_store = false;
                }
                let to = range.start + graphemes[next];
                let line = trim_trailing_space(text, from..to);
                let end = line.end;
                raw.push(RawLine::new(line, rtl));
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
                if let Some((slot, position, prepared)) = break_slot
                    && Some(prepared.font_generation.get()) == font_generation_at_start
                    && let Some(widths) = observed
                {
                    *slot = Some(LineBreak {
                        position,
                        next: start as u32,
                        end,
                        widths,
                    });
                }
            }
            if can_store
                && raw.len() > first_raw + 1
                && let Some(shape) = group
                && prepared.is_some_and(|p| p.font_generation.get() == font_generation)
            {
                memo.insert(
                    shape.group,
                    RepeatedWrap {
                        source: range.start,
                        lines: first_raw..raw.len(),
                        heading_probes,
                        interval: memo_proof,
                    },
                );
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
        let mut measured_widths = Vec::with_capacity(if has_width_sources { raw.len() } else { 0 });
        let mut width = 0.0_f32;
        let lines = raw
            .into_iter()
            .enumerate()
            .map(
                |(
                    index,
                    RawLine {
                        mut range,
                        rtl,
                        width_source,
                    },
                )| {
                    let span_from_source = width_source.and_then(|source| {
                        let prepared = prepared?;
                        // Whole-heading and tail advances agreed at the start of
                        // the layout. New fallback fonts invalidate that proof.
                        if Some(prepared.font_generation.get()) == font_generation_at_start {
                            prepared.replayed_widths.set(true);
                            return Some(measured_widths[source.get() as usize - 1]);
                        }
                        None
                    });
                    let mut span =
                        span_from_source.unwrap_or_else(|| advance(range.clone(), false));
                    if has_width_sources {
                        measured_widths.push(span);
                    }
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
                },
            )
            .collect();
        if !truncated
            && let Some(prepared) = prepared
            && Some(prepared.font_generation.get()) == font_generation_at_start
            && let Some(mut interval) = reusable
        {
            // The maximum line width and the line index must also remain unclamped.
            interval.minimum = interval.minimum.max(width);
            if interval.contains(available.width) {
                prepared.reusable_widths.set(Some(interval));
            }
        }
        Self {
            size: Size::new(width.min(available.width), height),
            lines,
        }
    }
}

fn same_word_break(
    graphemes: &[usize],
    words: &[usize],
    start: usize,
    fit: usize,
    highest_fit: usize,
) -> bool {
    // Every remaining fit is in this interval, even when shaped advances
    // are nonmonotonic. If both ends select the same valid word boundary,
    // any further probe would leave the final line break unchanged.
    let first = graphemes[fit.max(start + 1)];
    let last = graphemes[highest_fit];
    let next = words.partition_point(|&byte| byte <= first);
    next.checked_sub(1).is_some_and(|index| {
        let end = words[index];
        end > graphemes[start]
            && words.get(next).is_none_or(|&end| end > last)
            && graphemes.binary_search(&end).is_ok()
    })
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
    fn retained_line_breaks_match_fresh_layout_and_retry_after_font_change() {
        let text = "one two three four five six seven eight nine ten\n".repeat(160);
        let mut node = Node::new("StyledText", None);
        node.properties.insert(
            "text",
            xrack_runtime::StyledText::from_plain_text(text.clone()).into(),
        );
        let prepared = PreparedLabel::new(&text).unwrap();
        let plan = |width, prepared, advance: &mut dyn FnMut(Range<usize>, bool) -> f32| {
            LabelPlan::with_widths(
                &node,
                Size::new(width, f32::INFINITY),
                12.0,
                None,
                advance,
                None,
                prepared,
            )
        };
        let mut advance = |range: Range<usize>, _| range.len() as f32 * 3.0;
        plan(49.0, Some(&prepared), &mut advance);
        let mut calls = 0;
        let resized = plan(50.0, Some(&prepared), &mut |range, ellipsis| {
            calls += 1;
            advance(range, ellipsis)
        });
        let mut fresh_calls = 0;
        let fresh = plan(50.0, None, &mut |range, ellipsis| {
            fresh_calls += 1;
            advance(range, ellipsis)
        });
        assert_eq!(resized.lines, fresh.lines);
        assert_eq!(resized.size, fresh.size);
        assert!(calls < fresh_calls);

        let mut registered = false;
        let mut widths = Vec::new();
        let changed = LabelPlan::with_widths(
            &node,
            Size::new(50.0, f32::INFINITY),
            12.0,
            None,
            |range, _| {
                if !registered && prepared.replayed_breaks.get() {
                    prepared.invalidate_widths();
                    registered = true;
                }
                range.len() as f32 * if registered { 4.0 } else { 3.0 }
            },
            Some(&mut widths),
            Some(&prepared),
        );
        let changed_fresh = plan(50.0, None, &mut |range, _| range.len() as f32 * 4.0);
        assert!(registered);
        assert_eq!(changed.lines, changed_fresh.lines);
        assert_eq!(changed.size, changed_fresh.size);
        assert_eq!(widths.len(), changed.lines.len());
    }

    #[test]
    fn repeated_paragraphs_verify_heading_probes_before_reusing_wraps() {
        use std::fmt::Write as _;

        let mut text = String::new();
        for index in 0..160 {
            let heading = if index % 2 == 0 { "AA" } else { "BB" };
            writeln!(
                text,
                "{heading} long identical words and a second group of words"
            )
            .unwrap();
        }
        let mut node = Node::new("StyledText", None);
        node.properties.insert(
            "text",
            xrack_runtime::StyledText::from_plain_text(text.clone()).into(),
        );
        let mut prepared = PreparedLabel::new(&text).unwrap();
        prepared.prepare_repeated(
            |range| (range.len() > 2).then_some((6.0_f32.to_bits(), 2, 0)),
            |a, b, heading| text[a.start + heading..a.end] == text[b.start + heading..b.end],
        );

        let layout = |width, prepared, calls: &mut usize| {
            LabelPlan::with_widths(
                &node,
                Size::new(width, f32::INFINITY),
                12.0,
                None,
                |range, _| {
                    *calls += 1;
                    let part = &text[range];
                    if part == "A" {
                        42.0
                    } else {
                        part.len() as f32 * 3.0
                    }
                },
                None,
                prepared,
            )
        };

        let assert_same = |actual: &LabelPlan, expected: &LabelPlan| {
            assert_eq!(actual.size, expected.size);
            assert_eq!(actual.lines.len(), expected.lines.len());
            for (actual, expected) in actual.lines.iter().zip(&expected.lines) {
                assert_eq!(actual.range, expected.range);
                assert_eq!(actual.origin, expected.origin);
            }
        };

        for width in [20.0, 60.0, 45.0] {
            let mut independent_calls = 0;
            let mut shared_calls = 0;
            let expected = layout(width, None, &mut independent_calls);
            prepared.reusable_widths.set(None);
            prepared.replayed_widths.set(false);
            let actual = layout(width, Some(&prepared), &mut shared_calls);
            assert_same(&actual, &expected);
            assert!(shared_calls < independent_calls);
            if width == 20.0 {
                // The forced first grapheme is wider than the available width.
                // Its reported width is clamped and cannot certify other sizes.
                assert!(prepared.reusable_widths.get().is_none());
                continue;
            }
            assert!(prepared.replayed_widths.get());
            let interval = prepared.reusable_widths.get().unwrap();
            let end = if interval.maximum.is_finite() {
                interval.maximum.next_down()
            } else {
                width + 2.0
            };
            for certified in [interval.minimum.max(0.5), end] {
                assert!(interval.contains(certified));
                assert_same(&actual, &layout(certified, None, &mut independent_calls));
            }
        }

        prepared.invalidate_widths();
        assert!(!prepared.repeated.as_ref().unwrap().valid.get());
    }

    #[test]
    fn ending_word_search_early_preserves_breaks_with_nonmonotonic_widths() {
        let text = "a longidentifier with 👩‍🚀 glyphs and another-word\nא 1234567890, שלום";
        let mut optimized = PreparedLabel::new(text).unwrap();
        optimized.skip_stable_word_probes = true;
        let reference = PreparedLabel::new(text).unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert(
            "text",
            xrack_runtime::StyledText::from_plain_text(text).into(),
        );

        let layout = |prepared, width, calls: &mut usize| {
            LabelPlan::with_widths(
                &node,
                Size::new(width, f32::INFINITY),
                12.0,
                None,
                |range, _| {
                    *calls += 1;
                    ((range.len() * 59 + range.start * 11) % 89) as f32
                },
                None,
                Some(prepared),
            )
        };
        let mut reference_calls = 0;
        let mut optimized_calls = 0;
        for width in 1..100 {
            let expected = layout(&reference, width as f32, &mut reference_calls);
            let actual = layout(&optimized, width as f32, &mut optimized_calls);
            assert_eq!(expected.size, actual.size);
            assert_eq!(expected.lines.len(), actual.lines.len());
            for (expected, actual) in expected.lines.iter().zip(&actual.lines) {
                assert_eq!(expected.range, actual.range, "width {width}");
                assert_eq!(expected.origin, actual.origin);
            }
        }
        assert!(optimized_calls < reference_calls);
    }

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
