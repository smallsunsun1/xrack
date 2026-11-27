use crate::{
    Point, Size, TextMeasurer, font_size,
    text_area::{
        fitting_prefix, fitting_prefix_from, fitting_prefix_until, grapheme_boundaries,
        is_basic_han_text, paragraphs,
    },
};
use std::{
    borrow::Cow,
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, hash_map::RandomState},
    hash::BuildHasher,
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
        advance: impl FnMut(&str) -> f32,
    ) -> Self {
        Self::with_measurements(text, available, size, style, None, advance)
    }

    #[doc(hidden)]
    pub fn with_measurements(
        text: &'a str,
        available: Size,
        size: f32,
        style: LabelStyle,
        measurements: Option<&LabelMeasurements>,
        advance: impl FnMut(&str) -> f32,
    ) -> Self {
        let plan = LabelPlan::measured_text(text, available, size, style, measurements, advance);
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

impl LabelPlan {
    #[doc(hidden)]
    pub fn measured_text(
        text: &str,
        available: Size,
        size: f32,
        style: LabelStyle,
        measurements: Option<&LabelMeasurements>,
        mut advance: impl FnMut(&str) -> f32,
    ) -> Self {
        let measurements = measurements.filter(|measurements| measurements.matches(text, style));
        let prepared = measurements.map(|measurements| &measurements.prepared);
        if let Some(prepared) = prepared {
            prepared.reusable_widths.set(None);
        }
        Self::plan(
            text,
            available,
            size,
            style,
            |range, ellipsis| {
                if !ellipsis
                    && !range.is_empty()
                    && let Some(measurements) = measurements
                    && let Some(advances) = measurements.advances.get()
                {
                    return advances.measure(
                        range.clone(),
                        &measurements.prepared.font_generation,
                        || advance(&text[range]),
                    );
                }
                advance(&line_text(text, range, ellipsis))
            },
            None,
            prepared,
        )
    }
}

/// Measurements retained by the plain-label renderer across width changes.
#[doc(hidden)]
pub struct LabelMeasurements {
    source: Box<str>,
    prepared: PreparedLabel,
    advances: OnceCell<LabelRangeAdvances>,
}

impl LabelMeasurements {
    /// Source budget for a retained label and its prepared reflow metadata.
    #[doc(hidden)]
    pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;

    pub fn new(text: &str, style: LabelStyle) -> Option<Self> {
        if !style.reflows() || text.len() > Self::MAX_SOURCE_BYTES {
            return None;
        }
        let mut prepared = PreparedLabel::new(text)?;
        prepared.skip_stable_word_probes = true;
        prepared.retain_plain_wraps = true;
        let hasher = RandomState::new();
        // Identical plain paragraphs share the complete width proof.
        prepared.prepare_repeated(
            |range| (!range.is_empty()).then(|| (0, hasher.hash_one(&text[range.clone()]))),
            |left, right, _| (text[left.clone()] == text[right.clone()]).then_some(0),
        );
        Some(Self {
            source: text.into(),
            prepared,
            advances: OnceCell::new(),
        })
    }

    pub fn matches(&self, text: &str, style: LabelStyle) -> bool {
        style.reflows() && self.source.as_ref() == text
    }

    pub fn clear_widths(&self) {
        self.prepared.invalidate_widths();
        if let Some(advances) = self.advances.get() {
            advances.clear();
        }
    }

    /// Retain measured ranges after repeated reflows exceed the renderer's
    /// general width cache.
    #[doc(hidden)]
    pub fn retain_range_advances(&self) {
        self.advances
            .get_or_init(|| LabelRangeAdvances::new(self.source.len()));
    }

    /// Widths producing identical lines and geometry at the measured height.
    pub fn reusable_widths(&self) -> Option<Range<f32>> {
        self.prepared
            .reusable_widths
            .get()
            .map(|widths| widths.minimum..widths.maximum)
    }
}

#[derive(Clone, Copy)]
struct LabelRangeAdvance {
    key: u64,
    width: f32,
}

impl LabelRangeAdvance {
    // No retained source reaches this offset.
    const EMPTY: Self = Self {
        key: u64::MAX,
        width: 0.0,
    };
}

struct LabelRangeAdvances {
    entries: Box<[Cell<LabelRangeAdvance>]>,
    shift: u32,
}

impl LabelRangeAdvances {
    fn new(bytes: usize) -> Self {
        let slots = bytes.next_power_of_two().clamp(4096, 65_536);
        Self {
            entries: vec![Cell::new(LabelRangeAdvance::EMPTY); slots].into_boxed_slice(),
            shift: u64::BITS - slots.ilog2(),
        }
    }

    fn clear(&self) {
        for entry in &self.entries {
            entry.set(LabelRangeAdvance::EMPTY);
        }
    }

    fn measure(
        &self,
        range: Range<usize>,
        font_generation: &Cell<u32>,
        advance: impl FnOnce() -> f32,
    ) -> f32 {
        let key = ((range.start as u64) << 32) | range.end as u64;
        let slot = (key.wrapping_mul(0x9e3779b97f4a7c15) >> self.shift) as usize;
        let entry = &self.entries[slot];
        let cached = entry.get();
        if cached.key == key {
            return cached.width;
        }
        let generation = font_generation.get();
        let width = advance();
        if generation == font_generation.get() {
            entry.set(LabelRangeAdvance { key, width });
        }
        width
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
    pub fn reflows(self) -> bool {
        self.wrap != TextWrap::NoWrap
    }

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
    // Nearest word ending at each grapheme, using paragraph-local indices.
    // Zero means there is no usable word ending at this position.
    word_ends: Vec<u32>,
    first_line_probes: Vec<Cell<FirstLineProbes>>,
    font_generation: Cell<u32>,
    pub(crate) reusable_widths: Cell<Option<WidthInterval>>,
    pub(crate) skip_stable_word_probes: bool,
    retain_plain_wraps: bool,
    repeated: Vec<Option<ParagraphShape>>,
    pub(crate) replayed_widths: Cell<bool>,
    line_breaks: RefCell<RetainedLineBreaks>,
    replayed_breaks: Cell<bool>,
    replayed_paragraph_at: Cell<Option<u32>>,
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
        if self.generation != generation {
            self.entries.fill(None);
            self.generation = generation;
        }
    }

    fn slot(&mut self, position: NonZeroU32) -> &mut Option<LineBreak> {
        if self.entries.is_empty() {
            self.entries.resize(Self::SLOTS, None);
        }
        let index =
            (position.get() - 1).wrapping_mul(0x9e3779b1) >> (u32::BITS - Self::SLOTS.ilog2());
        &mut self.entries[index as usize]
    }
}

#[derive(Clone, Copy)]
struct LineBreak {
    // PreparedLabel bounds the number of grapheme and word offsets.
    position: NonZeroU32,
    next: u32,
    len: u32,
    advance: f32,
    widths: WidthInterval,
    prefix_end: u32,
}

#[derive(Clone, Copy)]
struct ParagraphShape {
    group: usize,
    heading_bytes: usize,
    heading_graphemes: usize,
    common_heading_bytes: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct ReplayedLine {
    source: NonZeroU32,
    pub(crate) replace_heading: bool,
}

impl ReplayedLine {
    pub(crate) fn source(self) -> usize {
        self.source.get() as usize - 1
    }
}

pub(crate) struct LineMeasurement {
    pub(crate) max_width: f32,
    pub(crate) replay: Option<ReplayedLine>,
}

struct RawLine {
    range: Range<usize>,
    rtl: bool,
    width_source: Option<ReplayedLine>,
    retained_break: Option<NonZeroU32>,
    advance: f32,
}

impl RawLine {
    fn new(range: Range<usize>, rtl: bool) -> Self {
        Self {
            range,
            rtl,
            width_source: None,
            retained_break: None,
            advance: f32::NAN,
        }
    }
}

struct RepeatedWrap {
    source: usize,
    paragraph: usize,
    common_heading_bytes: usize,
    lines: Range<usize>,
    heading_probes: u32,
    one_line: bool,
    leading: Option<u32>,
    font_generation: u32,
    interval: WidthInterval,
}

struct PreparedParagraph {
    source: Range<usize>,
    trimmed_end: usize,
    rtl: bool,
    graphemes: Range<usize>,
    // Width probes belong to this paragraph even when its boundaries are shared.
    probe_start: u32,
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

    fn slot(index: usize, last: usize) -> Option<usize> {
        // The first line probes powers of two, capped at the last grapheme.
        // Probes made by binary search can have other indices.
        (index.is_power_of_two() || index == last)
            .then(|| index.next_power_of_two().trailing_zeros() as usize)
            .filter(|slot| *slot < FIRST_LINE_PROBE_COUNT)
    }

    fn cached(&self, index: usize, last: usize, generation: u32) -> Option<f32> {
        let width = self.widths[Self::slot(index, last)?];
        (self.font_generation == generation && !width.is_nan()).then_some(width)
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
        if let Some(slot) = Self::slot(index, last) {
            let width = &mut self.widths[slot];
            if width.is_nan() {
                *width = advance();
            }
            *width
        } else {
            advance()
        }
    }
}

pub(crate) fn same_break_characters(first: &str, second: &str) -> bool {
    first.len() == second.len()
        && first
            .bytes()
            .zip(second.bytes())
            .all(|(a, b)| a == b || (a.is_ascii_digit() && b.is_ascii_digit()))
}

impl PreparedLabel {
    pub(crate) fn new(text: &str) -> Option<Self> {
        const MAX_PARAGRAPHS: usize = 65_536;
        const MAX_BOUNDARIES: usize = 512 * 1024;
        let mut result = Self {
            paragraphs: Vec::new(),
            graphemes: Vec::new(),
            word_ends: Vec::new(),
            first_line_probes: Vec::new(),
            font_generation: Cell::new(0),
            reusable_widths: Cell::new(None),
            skip_stable_word_probes: false,
            retain_plain_wraps: false,
            repeated: Vec::new(),
            replayed_widths: Cell::new(false),
            line_breaks: RefCell::default(),
            replayed_breaks: Cell::new(false),
            replayed_paragraph_at: Cell::new(None),
        };
        let mut word_count = 0;
        let mut grapheme_count = 0;
        let mut previous_nonempty: Option<(usize, usize)> = None;
        for source in paragraphs(text) {
            let content = &text[source.clone()];
            if result.paragraphs.len() == MAX_PARAGRAPHS || content.len() > MAX_BOUNDARIES {
                return None;
            }
            let grapheme_start = result.graphemes.len();
            let before = word_count;
            let template = previous_nonempty
                .map(|(index, count)| (&result.paragraphs[index], count))
                .filter(|(previous, _)| {
                    same_break_characters(content, &text[previous.source.clone()])
                });
            let (graphemes, rtl) = if let Some((previous, count)) = template {
                // ASCII digits share Unicode word, grapheme and bidi classes.
                // Width probes still measure the original text of each paragraph.
                word_count += count;
                (previous.graphemes.clone(), previous.rtl)
            } else {
                if is_basic_han_text(content) {
                    // Each character is a three-byte grapheme and a complete word.
                    let count = content.len() / 3;
                    result.graphemes.extend((0..=content.len()).step_by(3));
                    result.word_ends.extend((0..=count).map(|i| i as u32));
                    word_count += count;
                } else {
                    result
                        .graphemes
                        .extend(content.grapheme_indices(true).map(|(byte, _)| byte));
                    result.graphemes.push(content.len());
                    let mut words = content
                        .split_word_bound_indices()
                        .map(|(byte, word)| byte + word.len())
                        .peekable();
                    let mut end = 0;
                    for (index, &boundary) in result.graphemes[grapheme_start..].iter().enumerate()
                    {
                        while let Some(word) = words.next_if(|&word| word <= boundary) {
                            word_count += 1;
                            // A word boundary can land inside a grapheme. The
                            // existing wrap falls back to the grapheme in that case.
                            end = if word == boundary { index as u32 } else { 0 };
                        }
                        result.word_ends.push(end);
                    }
                }
                (
                    grapheme_start..result.graphemes.len(),
                    get_base_direction(content) == Direction::Rtl,
                )
            };
            let probe_start = grapheme_count as u32;
            grapheme_count += graphemes.len();
            if grapheme_count + word_count > MAX_BOUNDARIES {
                return None;
            }
            if !content.is_empty() {
                previous_nonempty = Some((result.paragraphs.len(), word_count - before));
            }
            result.paragraphs.push(PreparedParagraph {
                trimmed_end: source.start + content.trim_end().len(),
                source,
                rtl,
                graphemes,
                probe_start,
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

    // Equivalence certifies the suffix and reports how many leading bytes
    // share the source paragraph's text, styles and direction.
    pub(crate) fn prepare_repeated(
        &mut self,
        mut key: impl FnMut(&Range<usize>) -> Option<(usize, u64)>,
        mut equivalent: impl FnMut(&Range<usize>, &Range<usize>, usize) -> Option<usize>,
    ) {
        if self.paragraphs.len() < 128 {
            return;
        }
        let mut candidates = HashMap::<(usize, u64), usize>::new();
        let mut shapes = vec![None; self.paragraphs.len()];
        let mut reused = false;
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            let Some(signature) = key(&paragraph.source) else {
                continue;
            };
            let heading_bytes = signature.0;
            let boundaries = &self.graphemes[paragraph.graphemes.clone()];
            let Ok(heading_graphemes) = boundaries.binary_search(&heading_bytes) else {
                continue;
            };
            if heading_graphemes > u32::BITS as usize {
                continue;
            }
            if let Some(&old) = candidates.get(&signature) {
                let reference = &self.paragraphs[old];
                if let Some(common_heading_bytes) =
                    equivalent(&reference.source, &paragraph.source, heading_bytes)
                    && self.graphemes[reference.graphemes.clone()] == *boundaries
                    && self.word_ends[reference.graphemes.clone()]
                        == self.word_ends[paragraph.graphemes.clone()]
                {
                    let shape = ParagraphShape {
                        group: old,
                        heading_bytes,
                        heading_graphemes,
                        common_heading_bytes,
                    };
                    shapes[index] = Some(shape);
                    shapes[old] = Some(ParagraphShape {
                        common_heading_bytes: heading_bytes,
                        ..shape
                    });
                    reused = true;
                    continue;
                }
            }
            candidates.insert(signature, index);
        }
        if reused {
            self.repeated = shapes;
        }
    }

    pub(crate) fn font_generation(&self) -> u32 {
        self.font_generation.get()
    }

    pub(crate) fn invalidate_widths(&self) {
        self.reusable_widths.set(None);
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
        advance: impl FnMut(Range<usize>, bool) -> f32,
        measurements: Option<&mut Vec<LineMeasurement>>,
        prepared: Option<&PreparedLabel>,
    ) -> Self {
        Self::plan(
            node.text("text").unwrap_or(""),
            available,
            size,
            LabelStyle::for_node(node, direction),
            advance,
            measurements,
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
        mut measurements: Option<&mut Vec<LineMeasurement>>,
        prepared: Option<&PreparedLabel>,
    ) -> Self {
        let generation = prepared.map(|prepared| {
            prepared.replayed_breaks.set(false);
            prepared.font_generation.get()
        });
        let measurement_count = measurements.as_ref().map_or(0, |lines| lines.len());
        let plan = Self::build_plan(
            text,
            available,
            size,
            style,
            &mut advance,
            measurements.as_deref_mut(),
            prepared,
        );
        if let Some(prepared) = prepared
            && (prepared.replayed_breaks.replace(false)
                && generation != Some(prepared.font_generation.get())
                || prepared
                    .replayed_paragraph_at
                    .take()
                    .is_some_and(|used| used != prepared.font_generation.get()))
        {
            // A later fallback can change wrapping decisions already replayed
            // from the previous font set. Rebuild without replaying decisions.
            if let Some(lines) = &mut measurements {
                lines.truncate(measurement_count);
            }
            return Self::build_plan(text, available, size, style, advance, measurements, None);
        }
        plan
    }

    fn build_plan(
        text: &str,
        available: Size,
        size: f32,
        style: LabelStyle,
        mut advance: impl FnMut(Range<usize>, bool) -> f32,
        mut measurements: Option<&mut Vec<LineMeasurement>>,
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
        let font_generation_at_start = prepared.map(|prepared| {
            prepared.replayed_paragraph_at.set(None);
            prepared.font_generation.get()
        });
        let mut reusable = (prepared.is_some_and(|prepared| {
            prepared.retain_plain_wraps
                || available.height == f32::INFINITY
                    && vertical == Vertical::Top
                    && max_lines == usize::MAX
        }) && available.width.is_finite()
            && !elide)
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
        let mut generated_graphemes = Vec::new();
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

        let mut memo = HashMap::<usize, RepeatedWrap>::new();
        let mut line_breaks = prepared
            .filter(|prepared| {
                wrap == TextWrap::WordWrap
                    && (prepared.retain_plain_wraps
                        || prepared.paragraphs.len() >= 128 && reusable.is_some())
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
            let group = prepared
                .and_then(|prepared| prepared.repeated.get(paragraph_index).copied().flatten());
            if let Some(shape) = group
                && let Some(saved) = memo.get(&shape.group)
                && saved.font_generation == font_generation
                && let Some((prepared, paragraph)) = retained
            {
                let cache = &prepared.first_line_probes[paragraph_index];
                let mut probes = cache.get();
                let mut mask = saved.heading_probes & !1;
                let last = paragraph.graphemes.len() - 1;
                // Both paragraphs were compared with the same group source.
                let common_heading = shape.common_heading_bytes.min(saved.common_heading_bytes);
                let reference_probes = prepared.first_line_probes[saved.paragraph].get();
                let mut interval = saved.interval;
                let mut matches = true;
                let graphemes = &prepared.graphemes[paragraph.graphemes.clone()];
                while mask != 0 {
                    let offset = mask.trailing_zeros() as usize;
                    mask &= mask - 1;
                    let end = graphemes[offset];
                    let generation = prepared.font_generation.get();
                    let width = probes.measure(offset, last, generation, || {
                        if end <= common_heading
                            && let Some(width) = reference_probes.cached(offset, last, generation)
                        {
                            width
                        } else {
                            advance(range.start..range.start + end, false)
                        }
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
                if matches && shape.heading_bytes > 0 {
                    let leading = advance(range.start..range.start + shape.heading_bytes, false);
                    matches = leading.is_finite() && Some(leading.to_bits()) == saved.leading;
                }
                if matches && font_generation == prepared.font_generation.get() {
                    // The heading's complete advance and its observed prefix
                    // decisions agree. The rest of the paragraph is equivalent.
                    if prepared.replayed_paragraph_at.get().is_none() {
                        prepared.replayed_paragraph_at.set(Some(font_generation));
                    }
                    if let Some(proof) = &mut reusable {
                        proof.intersect(interval);
                    }
                    if saved.one_line {
                        paragraph.one_line.set(Some(OneLineFit {
                            font_generation,
                            wrap,
                            minimum_width: interval.minimum,
                        }));
                    }
                    has_width_sources |= !elide;
                    for index in saved.lines.clone() {
                        let line = &raw[index].range;
                        let start = range.start + (line.start - saved.source);
                        // Prepared documents bound the line index well below u32::MAX.
                        let width_source = if elide || line.end < saved.source + shape.heading_bytes
                        {
                            None
                        } else {
                            NonZeroU32::new(index as u32 + 1).map(|source| ReplayedLine {
                                source,
                                replace_heading: shape.heading_bytes > 0
                                    && line.start == saved.source,
                            })
                        };
                        raw.push(RawLine {
                            range: start..start + line.len(),
                            rtl,
                            width_source,
                            retained_break: None,
                            advance: f32::NAN,
                        });
                        if raw.len() > limit {
                            break 'paragraphs;
                        }
                    }
                    continue;
                }
            }
            let first_raw = raw.len();
            let mut heading_probes = 0_u32;
            let mut memo_proof = WidthInterval {
                minimum: 0.0,
                maximum: f32::INFINITY,
            };
            let mut can_store = group.is_some();
            let mut one_line = false;
            let han_boundaries = retained.is_none() && is_basic_han_text(content);
            let graphemes = if let Some((prepared, paragraph)) = retained {
                &prepared.graphemes[paragraph.graphemes.clone()]
            } else {
                generated_graphemes.clear();
                if content.is_ascii() {
                    // Paragraph splitting already removed CR and LF.
                    generated_graphemes.extend(0..=content.len());
                } else if han_boundaries {
                    generated_graphemes.extend((0..=content.len()).step_by(3));
                } else {
                    generated_graphemes
                        .extend(content.grapheme_indices(true).map(|(byte, _)| byte));
                    generated_graphemes.push(content.len());
                }
                &generated_graphemes
            };
            let indexed_ends = retained
                .filter(|_| wrap == TextWrap::WordWrap)
                .map(|(prepared, paragraph)| &prepared.word_ends[paragraph.graphemes.clone()]);
            let mut words = None;
            let last = graphemes.len() - 1;
            let mut start = 0;
            while start < last {
                let from = range.start + graphemes[start];
                let break_slot = retained
                    .filter(|(prepared, _)| {
                        (group.is_none() || prepared.retain_plain_wraps)
                            && Some(prepared.font_generation.get()) == font_generation_at_start
                    })
                    .and_then(|(prepared, paragraph)| {
                        let position = NonZeroU32::new(paragraph.probe_start + start as u32 + 1)?;
                        let slot = line_breaks.as_mut()?.slot(position);
                        Some((slot, position, prepared))
                    });
                let retained_break = break_slot
                    .as_ref()
                    .filter(|(_, _, prepared)| prepared.retain_plain_wraps)
                    .map(|(_, position, _)| *position);
                if let Some((slot, position, prepared)) = &break_slot
                    && let Some(saved) = **slot
                    && saved.position == *position
                    && saved.widths.contains(available.width)
                {
                    if let Some(interval) = &mut reusable {
                        interval.intersect(saved.widths);
                    }
                    if group.is_some() {
                        memo_proof.intersect(saved.widths);
                    }
                    prepared.replayed_breaks.set(true);
                    raw.push(RawLine {
                        range: from..from + saved.len as usize,
                        rtl,
                        width_source: None,
                        retained_break,
                        advance: saved.advance,
                    });
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
                let retain_prefix = start != 0
                    && break_slot
                        .as_ref()
                        .is_some_and(|(_, _, prepared)| prepared.retain_plain_wraps);
                let mut prefix_end = start as u32;
                if retain_prefix
                    && let Some((slot, position, _)) = &break_slot
                    && let Some(saved) = slot.filter(|saved| saved.position == *position)
                    && saved.prefix_end as usize > start
                    && saved.widths.minimum <= available.width
                {
                    // The existing lower bound covers every skipped probe.
                    // Carry it forward into the new width certificates.
                    prefix_end = saved.prefix_end;
                    if let Some(interval) = &mut observed {
                        interval.include_probe(saved.widths.minimum, true);
                    }
                    if let Some(interval) = &mut reusable {
                        interval.include_probe(saved.widths.minimum, true);
                    }
                    if group.is_some() {
                        memo_proof.include_probe(saved.widths.minimum, true);
                    }
                }
                let stop = |fit: usize, highest_fit: usize| {
                    // Both ends of the remaining search choose the same
                    // usable word boundary, even with nonmonotonic advances.
                    short_word_search
                        && indexed_ends.is_some_and(|ends| {
                            let end = ends[fit.max(start + 1)];
                            end as usize > start && ends[highest_fit] == end
                        })
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
                        if start == 0 && index < shape.heading_graphemes {
                            heading_probes |= 1 << index;
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
                } else if retain_prefix {
                    let fit = fitting_prefix_from(
                        start,
                        last,
                        prefix_end as usize,
                        |index, growing| {
                            let to = range.start + graphemes[index];
                            let width = advance(from..to, false);
                            let accepted = fits(index, width);
                            if growing && accepted {
                                prefix_end = index as u32;
                            }
                            accepted
                        },
                        stop,
                    );
                    (fit, false)
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
                let word_end = if fit == last || wrap == TextWrap::CharWrap || han_boundaries {
                    fit
                } else if let Some(ends) = indexed_ends {
                    ends[fit] as usize
                } else {
                    // The end of the paragraph is always a word boundary.
                    // Only segment words when a probe ends inside it.
                    let words = words.get_or_insert_with(|| {
                        content
                            .split_word_bound_indices()
                            .map(|(start, word)| start + word.len())
                            .collect::<Vec<_>>()
                    });
                    let word = words.partition_point(|&byte| byte <= graphemes[fit]);
                    word.checked_sub(1)
                        .and_then(|index| graphemes.binary_search(&words[index]).ok())
                        .unwrap_or(0)
                };
                let next = if word_end > start { word_end } else { fit };
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
                    one_line = true;
                }
                // Only save wraps that consume the complete heading in the
                // first line. Every probe inside that heading must have passed.
                if start == 0 && group.is_some_and(|shape| next < shape.heading_graphemes) {
                    can_store = false;
                }
                let to = range.start + graphemes[next];
                let line = trim_trailing_space(text, from..to);
                // Retained paragraphs bound each line to MAX_BOUNDARIES bytes.
                let len = line.len() as u32;
                let retained_advance = break_slot
                    .as_ref()
                    .and_then(|(slot, position, _)| {
                        slot.filter(|saved| saved.position == *position && saved.len == len)
                            .map(|saved| saved.advance)
                    })
                    .unwrap_or(f32::NAN);
                raw.push(RawLine {
                    range: line,
                    rtl,
                    width_source: None,
                    retained_break,
                    advance: retained_advance,
                });
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
                        len,
                        advance: retained_advance,
                        widths,
                        prefix_end,
                    });
                }
            }
            if can_store
                && raw.len() > first_raw
                && let Some(shape) = group
                && let Some(prepared) = prepared
                && prepared.font_generation.get() == font_generation
            {
                let leading = (shape.heading_bytes > 0).then(|| {
                    advance(range.start..range.start + shape.heading_bytes, false).to_bits()
                });
                if prepared.font_generation.get() == font_generation {
                    memo.insert(
                        shape.group,
                        RepeatedWrap {
                            source: range.start,
                            paragraph: paragraph_index,
                            common_heading_bytes: shape.common_heading_bytes,
                            lines: first_raw..raw.len(),
                            heading_probes,
                            one_line,
                            leading,
                            font_generation,
                            interval: memo_proof,
                        },
                    );
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
                        retained_break,
                        advance: retained_advance,
                    },
                )| {
                    let replay = width_source.filter(|_| {
                        prepared.is_some_and(|prepared| {
                            let current = prepared.font_generation.get();
                            prepared.replayed_paragraph_at.get() == Some(current)
                        })
                    });
                    if replay.is_some()
                        && let Some(prepared) = prepared
                    {
                        prepared.replayed_widths.set(true);
                    }
                    let mut span = replay
                        .map(|replay| measured_widths[replay.source()])
                        .unwrap_or_else(|| {
                            if !retained_advance.is_nan()
                                && prepared.map(|p| p.font_generation.get())
                                    == font_generation_at_start
                            {
                                return retained_advance;
                            }
                            let width = advance(range.clone(), false);
                            if let Some(position) = retained_break
                                && prepared.map(|p| p.font_generation.get())
                                    == font_generation_at_start
                                && let Some(breaks) = line_breaks.as_deref_mut()
                                && let Some(entry) = breaks.slot(position).as_mut()
                                && entry.position == position
                                && entry.len as usize == range.len()
                            {
                                entry.advance = width;
                            }
                            width
                        });
                    if has_width_sources {
                        measured_widths.push(span);
                    }
                    let mut ellipsis = false;
                    if elide && (span > available.width || truncated && index == last) {
                        (range, ellipsis) = elided(text, range, available.width, &mut advance);
                        span = advance(range.clone(), ellipsis);
                    }
                    width = width.max(span);
                    if let Some(lines) = measurements.as_deref_mut() {
                        lines.push(LineMeasurement {
                            max_width: width.min(available.width),
                            replay,
                        });
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
        if let Some(prepared) = prepared
            && Some(prepared.font_generation.get()) == font_generation_at_start
            && let Some(mut interval) = reusable
        {
            // The cached settings include the height and line limit. A
            // clipped, non-elided plan is reusable when its break decisions
            // and geometry are stable, even if some source lines are hidden.
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
            |range| (range.len() > 2).then_some((2, 0)),
            |a, b, heading| {
                (text[a.start + heading..a.end] == text[b.start + heading..b.end]).then_some(0)
            },
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
        assert!(!prepared.repeated.is_empty());
    }

    #[test]
    fn replayed_single_lines_keep_their_width_proof() {
        let text = vec!["the same words in every paragraph"; 150].join("\n");
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        let size = Size::new(380.0, f32::INFINITY);
        let advance = |line: &str| line.len() as f32 * 5.0;
        let fresh = LabelPlan::measured_text(&text, size, 12.0, style, None, advance);
        let retained =
            LabelPlan::measured_text(&text, size, 12.0, style, Some(&measurements), advance);
        assert_eq!(fresh.lines, retained.lines);
        assert!(measurements.prepared.replayed_widths.get());
        assert!(measurements.prepared.paragraphs.iter().all(|paragraph| {
            paragraph.one_line.get().is_some_and(|fit| {
                fit.minimum_width == advance("the same words in every paragraph")
            })
        }));
    }

    #[test]
    fn font_registration_rechecks_paragraphs_replayed_earlier_in_the_layout() {
        let mut text = "the same words in every paragraph\n".repeat(150);
        text.push_str("⚑ late fallback");
        let mut node = Node::new("Label", None);
        node.properties.insert("text", text.clone().into());
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        let prepared = &measurements.prepared;
        let size = Size::new(99.0, f32::INFINITY);
        let mut registered = false;
        let mut replayed_before_registration = false;
        let actual =
            LabelPlan::measured_text(&text, size, 12.0, style, Some(&measurements), |line| {
                if !registered && line.contains('⚑') {
                    replayed_before_registration = prepared.replayed_paragraph_at.get().is_some();
                    prepared.invalidate_widths();
                    registered = true;
                }
                line.chars().count() as f32 * if registered { 7.0 } else { 3.0 }
            });
        let expected = LabelPlan::measured_text(&text, size, 12.0, style, None, |line| {
            line.chars().count() as f32 * 7.0
        });
        assert!(registered && replayed_before_registration);
        assert_eq!(actual.lines, expected.lines);
        assert_eq!(actual.size, expected.size);
    }

    #[test]
    fn shared_paragraph_boundaries_preserve_bidi_and_unicode_clusters() {
        let text =
            "04 שלום👩‍🚀 ١ é 1️⃣ 工具  \r\n\r\n11 שלום👩‍🚀 ١ é 9️⃣ 工具  \n12 שלום👩‍🚀 ٢ é 9️⃣ 工具  ";
        let prepared = PreparedLabel::new(text).unwrap();
        for paragraph in &prepared.paragraphs {
            let independent = PreparedLabel::new(&text[paragraph.source.clone()]).unwrap();
            let fresh = &independent.paragraphs[0];
            assert_eq!(
                prepared.graphemes[paragraph.graphemes.clone()],
                independent.graphemes
            );
            assert_eq!(
                prepared.word_ends[paragraph.graphemes.clone()],
                independent.word_ends
            );
            assert_eq!(paragraph.rtl, fresh.rtl);
            assert_eq!(
                paragraph.trimmed_end - paragraph.source.start,
                fresh.trimmed_end
            );
        }
    }

    #[test]
    fn prepared_word_ends_preserve_the_grapheme_fallback() {
        let prepared = PreparedLabel::new("a\u{600}  \r\nhi there").unwrap();
        let first = prepared.paragraphs[0].graphemes.clone();
        // The word ending at byte 3 is inside the second grapheme, so a
        // fit at byte 4 must fall back to the grapheme instead of snapping.
        assert_eq!(&prepared.graphemes[first.clone()], &[0, 1, 4, 5]);
        assert_eq!(&prepared.word_ends[first], &[0, 0, 0, 3]);
        let second = prepared.paragraphs[1].graphemes.clone();
        assert_eq!(&prepared.word_ends[second], &[0, 0, 2, 3, 3, 3, 3, 3, 8]);
    }

    #[test]
    fn shared_paragraph_boundaries_keep_separate_wrap_measurements() {
        let text = "构建 1111 other words\r\n\n构建 8888 other words";
        let mut prepared = PreparedLabel::new(text).unwrap();
        prepared.retain_plain_wraps = true;
        assert_eq!(
            prepared.paragraphs[0].graphemes,
            prepared.paragraphs[2].graphemes
        );
        let mut node = Node::new("Label", None);
        node.properties.insert("text", text.to_owned().into());
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        for width in [48.0, 32.0, 82.0, 48.0, 32.0] {
            let layout = |prepared| {
                LabelPlan::with_widths(
                    &node,
                    Size::new(width, f32::INFINITY),
                    12.0,
                    None,
                    |range, _| {
                        text[range]
                            .chars()
                            .map(|character| if character == '8' { 12.0 } else { 5.0 })
                            .sum()
                    },
                    None,
                    prepared,
                )
            };
            let fresh = layout(None);
            let cached = layout(Some(&prepared));
            assert_eq!(fresh.lines, cached.lines, "width {width}");
            assert_eq!(fresh.size, cached.size);
        }
    }

    #[test]
    fn on_demand_boundaries_match_prepared_wraps() {
        let text =
            "ASCII  with a tab\tand a hyphen-word  \r\n汉字快速换行\nשלום 👩‍🚀  café   \na\u{600}  ";
        let mut node = Node::new("StyledText", None);
        node.properties.insert(
            "text",
            xrack_runtime::StyledText::from_plain_text(text).into(),
        );
        node.properties
            .insert("overflow", TextOverflow::Elide.into());
        let prepared = PreparedLabel::new(text).unwrap();
        for wrap in [TextWrap::WordWrap, TextWrap::CharWrap] {
            node.properties.insert("wrap", wrap.into());
            for width in [8.0, 26.0, 66.0, 200.0] {
                for height in [36.0, f32::INFINITY] {
                    let plan = |prepared| {
                        LabelPlan::with_widths(
                            &node,
                            Size::new(width, height),
                            12.0,
                            None,
                            |range, ellipsis| {
                                // Full fit can differ from the earlier probes.
                                text[range].chars().count() as f32 * 5.0
                                    + if ellipsis { 6.0 } else { 0.0 }
                            },
                            None,
                            prepared,
                        )
                    };
                    let fresh = plan(None);
                    let retained = plan(Some(&prepared));
                    assert_eq!(fresh.lines, retained.lines);
                    assert_eq!(fresh.size, retained.size);
                }
            }
        }
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
    fn retained_plain_wraps_match_independent_layouts_at_new_widths() {
        let text = "A long word-wrap line with 👩‍🚀 and joined-word clusters.\r\nא שלום\n".repeat(16);
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        node.properties
            .insert("overflow", TextOverflow::Elide.into());
        node.properties
            .insert("vertical-alignment", TextVerticalAlignment::Center.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        let cached_ranges = LabelMeasurements::new(&text, style).unwrap();
        cached_ranges.retain_range_advances();
        let mut reference_calls = 0;
        let mut retained_calls = 0;
        let mut cached_range_calls = 0;
        for size in [10.0, 15.0] {
            measurements.clear_widths();
            cached_ranges.clear_widths();
            for width in 72..92 {
                let available = Size::new(width as f32, 116.0);
                let advance = |part: &str| {
                    // Shaping need not have monotonically increasing advances.
                    ((part.len() * 59 + part.bytes().last().unwrap_or(0) as usize) % 89) as f32
                        * size
                        / 10.0
                };
                let expected = LabelLayout::with_style(&text, available, size, style, |part| {
                    reference_calls += 1;
                    advance(part)
                });
                for (measurements, calls) in [
                    (&measurements, &mut retained_calls),
                    (&cached_ranges, &mut cached_range_calls),
                ] {
                    let actual = LabelLayout::with_measurements(
                        &text,
                        available,
                        size,
                        style,
                        Some(measurements),
                        |part| {
                            *calls += 1;
                            advance(part)
                        },
                    );
                    assert_eq!(actual.size, expected.size);
                    assert_eq!(actual.lines.len(), expected.lines.len());
                    for (actual, expected) in actual.lines.iter().zip(&expected.lines) {
                        assert_eq!(actual.text, expected.text, "width {width}, size {size}");
                        assert_eq!(actual.origin, expected.origin);
                    }
                }
            }
        }
        assert!(retained_calls < reference_calls);
        assert!(cached_range_calls < retained_calls);
    }

    #[test]
    fn identical_plain_paragraphs_reuse_wraps_across_font_changes() {
        let text = "release-ab12cd34 followed by 混排文档 and punctuation.\n".repeat(144);
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        measurements.retain_range_advances();

        for size in [12.0, 17.0] {
            measurements.clear_widths();
            for width in [90.0, 99.0, 95.0, 123.0] {
                let area = Size::new(width, 340.0);
                let advance = |part: &str| {
                    part.chars().count() as f32 * size * 0.5
                        + (part.bytes().last().unwrap_or(0) % 3) as f32
                };
                measurements.prepared.replayed_widths.set(false);
                let retained = LabelPlan::measured_text(
                    &text,
                    area,
                    size,
                    style,
                    Some(&measurements),
                    advance,
                );
                let fresh = LabelPlan::measured_text(&text, area, size, style, None, advance);
                assert_eq!(retained.size, fresh.size);
                assert_eq!(retained.lines, fresh.lines);
                assert!(measurements.prepared.replayed_widths.get());
            }
        }
    }

    #[test]
    fn resumed_prefix_preserves_search_with_nonmonotonic_advances() {
        assert_eq!(size_of::<Option<LineBreak>>(), size_of::<LineBreak>());
        let widths = [
            0.0, 0.0, 0.0, 8.0, 18.0, 2.0, 9.0, 32.0, 12.0, 4.0, 28.0, 40.0,
        ];
        let start = 2;
        let last = widths.len() - 1;
        let stop = |fit: usize, highest: usize| fit / 3 == highest / 3;
        let mut prefix_end = start;
        let mut interval = WidthInterval {
            minimum: 0.0,
            maximum: f32::INFINITY,
        };
        fitting_prefix_from(
            start,
            last,
            start,
            |index, growing| {
                let fits = widths[index] <= 20.0;
                interval.include_probe(widths[index], fits);
                if growing && fits {
                    prefix_end = index;
                }
                fits
            },
            stop,
        );
        assert_eq!(prefix_end, 6);
        assert_eq!(interval.minimum, 18.0);

        for width in [7.0, 12.0, 18.0, 20.0, 25.0, 29.0, 40.0] {
            let mut reference_calls = 0;
            let expected = fitting_prefix_until(
                start,
                last,
                |index| {
                    reference_calls += 1;
                    widths[index] <= width
                },
                stop,
            );
            let can_resume = interval.minimum <= width;
            let seed = if can_resume { prefix_end } else { start };
            let mut calls = 0;
            let actual = fitting_prefix_from(
                start,
                last,
                seed,
                |index, _| {
                    calls += 1;
                    widths[index] <= width
                },
                stop,
            );
            assert_eq!(actual, expected, "width {width}");
            assert_eq!(reference_calls - calls, if can_resume { 3 } else { 0 });
        }
    }

    #[test]
    fn retained_range_advances_distinguish_offsets_above_64_kib() {
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let text = "word and words\n".repeat(5_000);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        measurements.retain_range_advances();
        let cache = measurements.advances.get().unwrap();
        let generation = &measurements.prepared.font_generation;
        let early = 7..11;
        let late = (early.start + 65_536)..(early.end + 65_536);
        for (range, width) in [(early, 7.0), (late, 12.0)] {
            let mut calls = 0;
            for _ in 0..2 {
                assert_eq!(
                    cache.measure(range.clone(), generation, || {
                        calls += 1;
                        width
                    }),
                    width
                );
            }
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn retained_breaks_reuse_line_advances_until_fonts_change() {
        let text = "word and words ".repeat(12);
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(&text, style).unwrap();
        let advance = |text: &str| text.len() as f32;
        let first = LabelLayout::with_measurements(
            &text,
            Size::new(20.0, 500.0),
            12.0,
            style,
            Some(&measurements),
            advance,
        );
        let width = 20.0f32.next_up();
        assert!(measurements.reusable_widths().unwrap().contains(&width));
        let available = Size::new(width, 500.0);
        let replayed = LabelLayout::with_measurements(
            &text,
            available,
            12.0,
            style,
            Some(&measurements),
            |_| panic!("the retained breaks already contain their advances"),
        );
        assert_eq!(first.size, replayed.size);
        assert_eq!(
            first
                .lines
                .iter()
                .map(|line| (&line.text, line.origin))
                .collect::<Vec<_>>(),
            replayed
                .lines
                .iter()
                .map(|line| (&line.text, line.origin))
                .collect::<Vec<_>>(),
        );

        measurements.clear_widths();
        let wider_font = |text: &str| advance(text) * 1.3;
        let updated = LabelLayout::with_measurements(
            &text,
            available,
            12.0,
            style,
            Some(&measurements),
            wider_font,
        );
        let expected = LabelLayout::with_style(&text, available, 12.0, style, wider_font);
        assert_eq!(updated.size, expected.size);
        assert_eq!(
            updated
                .lines
                .iter()
                .map(|line| (&line.text, line.origin))
                .collect::<Vec<_>>(),
            expected
                .lines
                .iter()
                .map(|line| (&line.text, line.origin))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn plain_wrap_width_range_preserves_the_finite_height_layout() {
        let text = "Keep each measured word.\nUse the same line positions across widths.";
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        node.properties
            .insert("vertical-alignment", TextVerticalAlignment::Bottom.into());
        let style = LabelStyle::for_node(&node, None);
        let measurements = LabelMeasurements::new(text, style).unwrap();
        let advance = |text: &str| 5.0 * text.len() as f32;
        for (height, limits) in [
            (180.0, style),
            (33.0, style),
            (
                180.0,
                LabelStyle {
                    max_lines: 2,
                    ..style
                },
            ),
        ] {
            let measured = LabelLayout::with_measurements(
                text,
                Size::new(83.0, height),
                12.0,
                limits,
                Some(&measurements),
                advance,
            );
            let widths = measurements.reusable_widths().unwrap();
            assert!(widths.contains(&83.0));
            if height == 33.0 || limits.max_lines == 2 {
                assert_eq!(measured.lines.len(), 2);
            }
            for width in [widths.start, widths.end.next_down()] {
                let exact =
                    LabelLayout::with_style(text, Size::new(width, height), 12.0, limits, advance);
                assert_eq!(measured.size, exact.size);
                assert_eq!(measured.lines.len(), exact.lines.len());
                for (measured, exact) in measured.lines.iter().zip(&exact.lines) {
                    assert_eq!(measured.text, exact.text);
                    assert_eq!(measured.origin, exact.origin);
                }
            }
        }

        for width_sensitive in [
            LabelStyle {
                horizontal: TextAlignment {
                    mode: TextHorizontalAlignment::Center,
                    direction: None,
                },
                ..style
            },
            LabelStyle {
                elide: true,
                ..style
            },
        ] {
            LabelLayout::with_measurements(
                text,
                Size::new(83.0, 180.0),
                12.0,
                width_sensitive,
                Some(&measurements),
                advance,
            );
            assert!(measurements.reusable_widths().is_none());
        }
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
