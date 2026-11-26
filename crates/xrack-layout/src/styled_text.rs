use crate::{
    LabelPlan, PlannedLabelLine, Point, Rect, Size, StyledFragments, TextAlignment, TextAreaMotion,
    TextEdit, TextPosition, VisualTextLine,
    label::{PreparedLabel, WidthInterval, same_break_characters},
};
use std::{
    borrow::Cow,
    cell::Cell,
    collections::{HashMap, HashSet},
    hash::{BuildHasher, RandomState},
    num::NonZeroUsize,
    ops::Range,
    sync::{Arc, Weak},
};
use unicode_bidi::{
    BidiClass, BidiInfo, Direction, Level, ParagraphBidiInfo, ParagraphInfo, get_base_direction,
};
use xrack_runtime::{Color, LayoutDirection, Node, StyledText, TextHorizontalAlignment, TextStyle};

struct Piece {
    range: Range<usize>,
    rtl_mark: bool,
    style: TextStyle,
    link: Option<usize>,
}

#[derive(Default)]
struct StyledBidi {
    paragraphs: Vec<ParagraphInfo>,
    classes: Vec<BidiClass>,
    levels: Vec<Level>,
}

impl StyledBidi {
    fn new(text: &str) -> Self {
        if text.len() < 4096 {
            return Self::from_bidi_info(text);
        }
        let mut result = Self::default();
        let mut previous: Option<Range<usize>> = None;
        let mut pending = 0;
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.len();
            if !matches!(line, "\n" | "\r\n") {
                if let Some(source) = &previous
                    && line.ends_with('\n')
                    && same_break_characters(line, &text[source.clone()])
                {
                    if result.classes.is_empty() {
                        result.classes.reserve_exact(text.len());
                        result.levels.reserve_exact(text.len());
                    }
                    result.append_chunk(text, pending..start);
                    result.append_repeated(source.clone(), start);
                    pending = end;
                }
                previous = Some(start..end);
            }
            start = end;
        }
        if pending == 0 {
            return Self::from_bidi_info(text);
        }
        result.append_chunk(text, pending..text.len());
        result
    }

    fn from_bidi_info(text: &str) -> Self {
        let BidiInfo {
            paragraphs,
            original_classes,
            levels,
            ..
        } = BidiInfo::new(text, None);
        Self {
            paragraphs,
            classes: original_classes,
            levels,
        }
    }

    fn append_chunk(&mut self, text: &str, range: Range<usize>) {
        debug_assert_eq!(self.classes.len(), range.start);
        let chunk = &text[range.clone()];
        if chunk.bytes().all(|byte| matches!(byte, b'\r' | b'\n')) {
            for start in range {
                self.paragraphs.push(ParagraphInfo {
                    range: start..start + 1,
                    level: Level::ltr(),
                });
                self.classes.push(BidiClass::B);
                self.levels.push(Level::ltr());
            }
            return;
        }
        let parsed = Self::from_bidi_info(chunk);
        self.classes.extend(parsed.classes);
        self.levels.extend(parsed.levels);
        self.paragraphs.extend(
            parsed
                .paragraphs
                .into_iter()
                .map(|paragraph| ParagraphInfo {
                    range: paragraph.range.start + range.start..paragraph.range.end + range.start,
                    level: paragraph.level,
                }),
        );
    }

    fn append_repeated(&mut self, source: Range<usize>, destination: usize) {
        // LF ends a bidi paragraph and resets directional state. Substituting
        // ASCII digits preserves both bidi classes and bracket matching.
        debug_assert_eq!(self.classes.len(), destination);
        self.classes.extend_from_within(source.clone());
        self.levels.extend_from_within(source.clone());
        let start = self
            .paragraphs
            .partition_point(|para| para.range.end <= source.start);
        let end = self
            .paragraphs
            .partition_point(|para| para.range.start < source.end);
        for index in start..end {
            let source_paragraph = &self.paragraphs[index];
            let start = destination + (source_paragraph.range.start - source.start);
            let end = destination + (source_paragraph.range.end - source.start);
            self.paragraphs.push(ParagraphInfo {
                range: start..end,
                level: source_paragraph.level,
            });
        }
    }
}

struct LineBidi<'a> {
    paragraph: ParagraphBidiInfo<'a>,
    runs: Vec<(Range<usize>, Level)>,
}

impl LineBidi<'_> {
    fn new() -> Self {
        Self {
            paragraph: ParagraphBidiInfo::new("", None),
            runs: Vec::new(),
        }
    }
}

/// Source-order spans and byte-page lookups shared by every width of a document.
struct PreparedSpans {
    styles: Vec<StyleSpan>,
    advances: Option<Vec<Cell<[f32; 2]>>>,
    level_ends: Vec<u32>,
    l1_limits: Vec<u32>,
    pages: Vec<SpanPage>,
}

struct StyleSpan {
    end: u32,
    style: TextStyle,
    link: u32,
}

struct SpanPage {
    paragraph: u32,
    level: u32,
    style: u32,
}

impl PreparedSpans {
    const PAGE_BYTES: usize = 64;

    fn new(document: &StyledText, bidi: &StyledBidi) -> Option<Self> {
        let len = document.text().len();
        if len == 0
            || len > 1024 * 1024
            || document.runs().len().saturating_add(document.links().len()) > 65_536
        {
            return None;
        }

        let mut styles = Vec::new();
        visit_style_spans(document, 0..len, |range, style, link| {
            styles.push(StyleSpan {
                end: range.end as u32,
                style,
                link: link.map_or(0, |link| link as u32 + 1),
            });
        });
        let mut level_ends: Vec<_> = bidi
            .levels
            .windows(2)
            .enumerate()
            .filter_map(|(index, pair)| (pair[0] != pair[1]).then_some(index as u32 + 1))
            .collect();
        level_ends.push(len as u32);

        // Outside explicit directional formatting and separators, rule L1
        // only resets trailing whitespace. Record the first class that
        // requires the full algorithm in each paragraph.
        let l1_limits = bidi
            .paragraphs
            .iter()
            .map(|paragraph| {
                let start = paragraph.range.start;
                let end = paragraph.range.end;
                let special = bidi.classes[start..end].iter().position(|class| {
                    use BidiClass::*;
                    matches!(
                        class,
                        B | S | FSI | LRI | RLI | PDI | RLE | LRE | RLO | LRO | PDF | BN
                    )
                });
                special.map_or(end, |offset| start + offset) as u32
            })
            .collect();

        let mut paragraph = 0;
        let mut level = 0;
        let mut style = 0;
        let mut pages = Vec::with_capacity(len.div_ceil(Self::PAGE_BYTES));
        for start in (0..len).step_by(Self::PAGE_BYTES) {
            while bidi
                .paragraphs
                .get(paragraph)
                .is_some_and(|item| item.range.end <= start)
            {
                paragraph += 1;
            }
            while level_ends[level] as usize <= start {
                level += 1;
            }
            while styles[style].end as usize <= start {
                style += 1;
            }
            pages.push(SpanPage {
                paragraph: paragraph as u32,
                level: level as u32,
                style: style as u32,
            });
        }
        Some(Self {
            styles,
            advances: None,
            level_ends,
            l1_limits,
            pages,
        })
    }

    fn paragraph(&self, bidi: &StyledBidi, start: usize) -> Option<usize> {
        let mut index = self.pages.get(start / Self::PAGE_BYTES)?.paragraph as usize;
        while bidi
            .paragraphs
            .get(index)
            .is_some_and(|item| item.range.end <= start)
        {
            index += 1;
        }
        Some(index)
    }

    fn level_run(&self, start: usize) -> usize {
        let mut index = self.pages[start / Self::PAGE_BYTES].level as usize;
        while self.level_ends[index] as usize <= start {
            index += 1;
        }
        index
    }

    fn level_end(&self, start: usize) -> usize {
        self.level_ends[self.level_run(start)] as usize
    }

    fn visit_styles(
        &self,
        range: Range<usize>,
        mut visit: impl FnMut(Range<usize>, TextStyle, Option<usize>),
    ) {
        let mut start = range.start;
        let index = self.pages[start / Self::PAGE_BYTES].style as usize;
        for span in &self.styles[index..] {
            let end = (span.end as usize).min(range.end);
            if end <= start {
                continue;
            }
            visit(
                start..end,
                span.style,
                span.link.checked_sub(1).map(|link| link as usize),
            );
            if end == range.end {
                break;
            }
            start = end;
        }
    }

    fn first_style(&self, byte: usize) -> usize {
        let mut index = self.pages[byte / Self::PAGE_BYTES].style as usize;
        while self.styles[index].end as usize <= byte {
            index += 1;
        }
        index
    }

    fn clear_advances(&self) {
        if let Some(advances) = &self.advances {
            for advance in advances {
                advance.set([f32::NAN; 2]);
            }
        }
    }

    fn complete_span_index(&self, range: &Range<usize>) -> Option<usize> {
        if range.is_empty() {
            return None;
        }
        let index = self.first_style(range.start);
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.styles[previous].end as usize);
        (start == range.start && self.styles[index].end as usize == range.end).then_some(index)
    }

    fn same_style_slices(&self, left: &Range<usize>, right: &Range<usize>) -> bool {
        let styles = |range: &Range<usize>| {
            let (start, end) = (range.start, range.end);
            self.styles[self.first_style(start)..]
                .iter()
                .map(move |span| ((span.end as usize).min(end) - start, span.style))
        };
        for ((left_end, left_style), (right_end, right_style)) in styles(left).zip(styles(right)) {
            if left_end != right_end || left_style != right_style {
                return false;
            }
            if left_end == left.len() {
                return true;
            }
        }
        false
    }
}

/// Width-independent inputs and previously measured lines of an immutable document.
#[doc(hidden)]
pub struct StyledTextMeasurements {
    document: StyledText,
    bidi: StyledBidi,
    prepared: Option<PreparedLabel>,
    spans: Option<PreparedSpans>,
    widths: RetainedWidths,
    lines: RetainedLines,
    shared_widths: SharedWidths,
}

#[derive(Clone, Copy, Default)]
struct SharedShape {
    hash: u64,
    start: u32,
    end: u32,
}

impl SharedShape {
    fn new(range: &Range<usize>, hash: u64) -> Self {
        Self {
            hash,
            start: range.start as u32,
            end: range.end as u32,
        }
    }

    fn matching_source(
        &self,
        text: &str,
        bidi: &StyledBidi,
        spans: &PreparedSpans,
        range: &Range<usize>,
        hash: u64,
    ) -> Option<Range<usize>> {
        let previous = self.start as usize..self.end as usize;
        (self.hash == hash
            && !previous.is_empty()
            && text[previous.clone()] == text[range.clone()]
            && same_bidi_slice(bidi, spans, &previous, range)
            && spans.same_style_slices(&previous, range))
        .then_some(previous)
    }
}

#[derive(Clone, Copy, Default)]
struct SharedWidth {
    shape: SharedShape,
    width: f32,
    leading: Option<u32>,
}

#[derive(Clone, Copy, Default)]
struct SharedTemplate {
    shape: SharedShape,
    start: u32,
    leading: Option<u32>,
}

struct SharedLineProbe {
    tail: Range<usize>,
    hash: u64,
    leading: Option<u32>,
}

#[derive(Default)]
struct SharedWidths {
    hash: RandomState,
    slots: Vec<SharedWidth>,
    templates: Vec<SharedTemplate>,
    leading_widths: Vec<f32>,
}

impl SharedWidths {
    const SLOTS: usize = 4096;

    fn clear(&mut self) {
        self.slots.fill(SharedWidth::default());
        self.templates.fill(SharedTemplate::default());
        self.leading_widths.fill(f32::NAN);
    }

    fn probe(
        &self,
        text: &str,
        bidi: &StyledBidi,
        spans: &PreparedSpans,
        range: &Range<usize>,
        ellipsis: bool,
    ) -> Option<SharedLineProbe> {
        if ellipsis || !(4..=128).contains(&range.len()) {
            return None;
        }
        let paragraph_index = spans.paragraph(bidi, range.start)?;
        let paragraph = bidi.paragraphs.get(paragraph_index)?;
        if range.start < paragraph.range.start || range.end > paragraph.range.end {
            return None;
        }
        if range.start > paragraph.range.start {
            return Some(SharedLineProbe {
                tail: range.clone(),
                hash: self.hash.hash_one(&text[range.clone()]),
                leading: None,
            });
        }

        // A complete leading LTR style span is an independent shaping piece.
        // Comparing advance bits preserves the order and rounding of the sum
        // even when the first piece has different text, style, or link.
        let width = *self.leading_widths.get(paragraph_index)?;
        if !width.is_finite() {
            return None;
        }
        let tail_start = spans.styles[spans.first_style(range.start)].end as usize;
        if tail_start >= range.end {
            return None;
        }
        let tail = tail_start..range.end;
        Some(SharedLineProbe {
            hash: self.hash.hash_one((width.to_bits(), &text[tail.clone()])),
            tail,
            leading: Some(width.to_bits()),
        })
    }

    fn remember_leading(
        &mut self,
        bidi: &StyledBidi,
        spans: &PreparedSpans,
        range: &Range<usize>,
        fragment: &StyledFragment,
    ) {
        if range.start != fragment.range.start || fragment.range.is_empty() {
            return;
        }
        let Some(index) = spans.paragraph(bidi, range.start) else {
            return;
        };
        let Some(paragraph) = bidi.paragraphs.get(index) else {
            return;
        };
        // L1 leaves a leading span at the paragraph's LTR base level, and
        // L2 cannot reorder a later run across that span.
        if range.start != paragraph.range.start
            || paragraph.level.is_rtl()
            || bidi.levels[range.start] != paragraph.level
            || fragment.range.end > spans.level_end(range.start)
            || fragment.range.end != spans.styles[spans.first_style(range.start)].end as usize
            || fragment.range.end >= paragraph.range.end
        {
            return;
        }
        if self.leading_widths.is_empty() {
            self.leading_widths.resize(bidi.paragraphs.len(), f32::NAN);
        }
        self.leading_widths[index] = fragment.bounds.width;
    }

    fn lookup(
        &self,
        text: &str,
        bidi: &StyledBidi,
        spans: &PreparedSpans,
        probe: &SharedLineProbe,
    ) -> Option<f32> {
        let cached = self.slots.get(probe.hash as usize & (Self::SLOTS - 1))?;
        if cached.leading != probe.leading {
            return None;
        }
        cached
            .shape
            .matching_source(text, bidi, spans, &probe.tail, probe.hash)
            .map(|_| cached.width)
    }

    fn remember(&mut self, probe: &SharedLineProbe, width: f32) {
        if self.slots.is_empty() {
            self.slots.resize(Self::SLOTS, SharedWidth::default());
        }
        self.slots[probe.hash as usize & (Self::SLOTS - 1)] = SharedWidth {
            shape: SharedShape::new(&probe.tail, probe.hash),
            width,
            leading: probe.leading,
        };
    }

    fn template(
        &self,
        text: &str,
        bidi: &StyledBidi,
        spans: &PreparedSpans,
        probe: &SharedLineProbe,
    ) -> Option<(Range<usize>, usize)> {
        let cached = self
            .templates
            .get(probe.hash as usize & (Self::SLOTS - 1))?;
        if cached.leading != probe.leading {
            return None;
        }
        let tail = cached
            .shape
            .matching_source(text, bidi, spans, &probe.tail, probe.hash)?;
        Some((cached.start as usize..tail.end, tail.start))
    }

    fn remember_template(&mut self, range: &Range<usize>, probe: &SharedLineProbe) {
        if self.templates.is_empty() {
            self.templates
                .resize(Self::SLOTS, SharedTemplate::default());
        }
        self.templates[probe.hash as usize & (Self::SLOTS - 1)] = SharedTemplate {
            shape: SharedShape::new(&probe.tail, probe.hash),
            start: range.start as u32,
            leading: probe.leading,
        };
    }
}

fn same_bidi_slice(
    bidi: &StyledBidi,
    spans: &PreparedSpans,
    left: &Range<usize>,
    right: &Range<usize>,
) -> bool {
    let paragraph = |range: &Range<usize>| {
        bidi.paragraphs
            .get(spans.paragraph(bidi, range.start)?)
            .filter(|paragraph| paragraph.range.end >= range.end)
    };
    let (Some(a), Some(b)) = (paragraph(left), paragraph(right)) else {
        return false;
    };
    let uniform = bidi.levels[left.start] == a.level
        && bidi.levels[right.start] == b.level
        && spans.level_end(left.start) >= left.end
        && spans.level_end(right.start) >= right.end;
    a.level == b.level
        && (uniform
            || (bidi.levels[left.clone()] == bidi.levels[right.clone()]
                && bidi.classes[left.clone()] == bidi.classes[right.clone()]))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct LineKey(u64);

impl LineKey {
    fn new(range: &Range<usize>, ellipsis: bool) -> Option<Self> {
        let start = u32::try_from(range.start).ok()?;
        let end = u32::try_from(range.end).ok()?.checked_mul(2)?;
        Some(Self(
            u64::from(start) << 32 | u64::from(end | u32::from(ellipsis)),
        ))
    }
}

struct RetainedWidth {
    width: f32,
    generation: u32,
}

#[derive(Default)]
struct RetainedWidths {
    values: HashMap<LineKey, RetainedWidth>,
    generation: u32,
    trimmed: bool,
}

impl RetainedWidths {
    const MAX_PROBES: usize = 224 * 1024;

    fn begin_layout(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.trimmed = false;
    }

    fn clear(&mut self) {
        self.values.clear();
    }

    fn get(&mut self, key: &LineKey) -> Option<f32> {
        let value = self.values.get_mut(key)?;
        value.generation = self.generation;
        Some(value.width)
    }

    fn insert(&mut self, key: LineKey, width: f32) {
        if self.values.len() >= Self::MAX_PROBES && !self.trimmed {
            // Reserve room for new widths while keeping the probes used by
            // this layout or the preceding layout.
            self.values
                .retain(|_, value| self.generation.wrapping_sub(value.generation) <= 1);
            self.trimmed = true;
        }
        if self.values.len() < Self::MAX_PROBES {
            self.values.insert(
                key,
                RetainedWidth {
                    width,
                    generation: self.generation,
                },
            );
        }
    }
}

struct RetainedFragment {
    range: Range<u32>,
    link: u32,
    color: Color,
    flags: u8,
    width: f32,
}

impl RetainedFragment {
    fn new(fragment: &StyledFragment) -> Option<Self> {
        Some(Self {
            range: u32::try_from(fragment.range.start).ok()?
                ..u32::try_from(fragment.range.end).ok()?,
            link: u32::try_from(fragment.link.map_or(0, NonZeroUsize::get)).ok()?,
            color: fragment.color,
            flags: fragment.flags,
            width: fragment.bounds.width,
        })
    }

    fn place(&self, x: f32, y: f32, height: f32) -> StyledFragment {
        StyledFragment {
            range: self.range.start as usize..self.range.end as usize,
            link: NonZeroUsize::new(self.link as usize),
            color: self.color,
            flags: self.flags,
            bounds: Rect::new(x, y, self.width, height),
            visual: None,
            glyphs: None,
        }
    }
}

#[derive(Default)]
struct RetainedLines {
    ranges: HashMap<LineKey, Range<u32>>,
    fragments: Vec<RetainedFragment>,
}

impl RetainedLines {
    fn clear(&mut self) {
        self.ranges.clear();
        self.fragments.clear();
    }

    fn get(&self, key: &LineKey) -> Option<&[RetainedFragment]> {
        self.ranges
            .get(key)
            .map(|range| &self.fragments[range.start as usize..range.end as usize])
    }

    fn insert(&mut self, key: LineKey, fragments: &[StyledFragment]) {
        const MAX_FRAGMENTS: usize = 262_144;
        let start = self.fragments.len();
        let end = start + fragments.len();
        if end <= MAX_FRAGMENTS && !fragments.is_empty() {
            self.fragments
                .extend(fragments.iter().map_while(RetainedFragment::new));
            if self.fragments.len() == end {
                self.ranges.insert(key, start as u32..end as u32);
            } else {
                self.fragments.truncate(start);
            }
        }
    }
}

impl StyledTextMeasurements {
    pub fn new(document: StyledText) -> Self {
        let bidi = StyledBidi::new(document.text());
        let mut prepared = PreparedLabel::new(document.text());
        let spans = PreparedSpans::new(&document, &bidi);
        if let (Some(prepared), Some(spans)) = (&mut prepared, &spans) {
            let text = document.text();
            let hasher = RandomState::new();
            prepared.prepare_repeated(
                |range| {
                    if range.is_empty() {
                        return None;
                    }
                    let paragraph = &bidi.paragraphs[spans.paragraph(&bidi, range.start)?];
                    if range.start != paragraph.range.start || range.end > paragraph.range.end {
                        return None;
                    }
                    // A complete LTR base-level style span is an independent
                    // shaping run. Group its suffix before measuring the heading.
                    let end = spans.styles[spans.first_style(range.start)].end as usize;
                    let heading = if end < range.end
                        && end <= spans.level_end(range.start)
                        && !paragraph.level.is_rtl()
                        && bidi.levels[range.start] == paragraph.level
                    {
                        end - range.start
                    } else {
                        0
                    };
                    Some((
                        heading,
                        hasher.hash_one(&text[range.start + heading..range.end]),
                    ))
                },
                |a, b, heading| {
                    let a = a.start + heading..a.end;
                    let b = b.start + heading..b.end;
                    text[a.clone()] == text[b.clone()]
                        && same_bidi_slice(&bidi, spans, &a, &b)
                        && spans.same_style_slices(&a, &b)
                },
            );
        }
        Self {
            document,
            bidi,
            prepared,
            spans,
            widths: RetainedWidths::default(),
            lines: RetainedLines::default(),
            shared_widths: SharedWidths::default(),
        }
    }

    /// Retain complete style-span advances when the renderer's width cache is full.
    #[doc(hidden)]
    pub fn retain_span_advances(&mut self) {
        let Some(spans) = &mut self.spans else {
            return;
        };
        if spans.advances.is_some() {
            return;
        }
        let advances = vec![Cell::new([f32::NAN; 2]); spans.styles.len()];
        // The current lines have already measured many complete spans.
        // Seed the new cache from the same font generation.
        for fragment in &self.lines.fragments {
            let range = fragment.range.start as usize..fragment.range.end as usize;
            if let Some(index) = spans.complete_span_index(&range) {
                let mut widths = advances[index].get();
                widths[usize::from(fragment.flags & StyledFragment::RTL_MARK != 0)] =
                    fragment.width;
                advances[index].set(widths);
            }
        }
        spans.advances = Some(advances);
        if let Some(prepared) = &mut self.prepared {
            prepared.skip_stable_word_probes = true;
        }
    }

    pub fn clear_widths(&mut self) {
        self.widths.clear();
        self.lines.clear();
        self.shared_widths.clear();
        if let Some(spans) = &self.spans {
            spans.clear_advances();
        }
        if let Some(prepared) = &self.prepared {
            prepared.invalidate_widths();
        }
    }
}

struct RetainedMeasurements<'a> {
    bidi: &'a StyledBidi,
    prepared: Option<&'a PreparedLabel>,
    spans: Option<&'a PreparedSpans>,
    widths: &'a mut RetainedWidths,
    lines: &'a mut RetainedLines,
    shared_widths: &'a mut SharedWidths,
    shared_hit: bool,
    invalidated: &'a Cell<bool>,
}

impl RetainedMeasurements<'_> {
    fn invalidate_if_requested(&mut self) -> bool {
        let invalidated = self.invalidated.replace(false);
        if invalidated {
            self.widths.clear();
            self.lines.clear();
            self.shared_widths.clear();
            if let Some(spans) = self.spans {
                spans.clear_advances();
            }
            self.shared_hit = false;
            if let Some(prepared) = self.prepared {
                prepared.invalidate_widths();
            }
        }
        invalidated
    }

    fn measure(
        &mut self,
        text: &str,
        range: &Range<usize>,
        ellipsis: bool,
        key: Option<LineKey>,
        advance: impl FnOnce() -> f32,
    ) -> f32 {
        if let Some(width) = key.and_then(|key| self.widths.get(&key)) {
            return width;
        }
        let shared = self.spans.and_then(|spans| {
            self.shared_widths
                .probe(text, self.bidi, spans, range, ellipsis)
                .map(|probe| (spans, probe))
        });
        let cached = shared
            .as_ref()
            .and_then(|(spans, probe)| self.shared_widths.lookup(text, self.bidi, spans, probe));
        self.shared_hit |= cached.is_some();
        let width = cached.unwrap_or_else(advance);
        // A probe that changed the font set can contain widths from two generations.
        if !self.invalidate_if_requested() {
            if let Some(key) = key {
                self.widths.insert(key, width);
            }
            if cached.is_none()
                && let Some((_, probe)) = shared
            {
                self.shared_widths.remember(&probe, width);
            }
        }
        width
    }
}

fn piece_text<'a>(document: &'a StyledText, range: &Range<usize>, rtl_mark: bool) -> Cow<'a, str> {
    if range.is_empty() {
        return Cow::Borrowed("…");
    }
    let text = &document.text()[range.clone()];
    if rtl_mark {
        Cow::Owned(format!("\u{200f}{text}"))
    } else {
        Cow::Borrowed(text)
    }
}

/// One shaped span in visual order. Bounds are local to the label.
#[derive(Clone, Debug)]
pub struct StyledFragment {
    // Source pieces are nonempty. An empty range represents an ellipsis.
    range: Range<usize>,
    // Store the link index plus one, so None takes no additional space.
    link: Option<NonZeroUsize>,
    color: Color,
    flags: u8,
    pub bounds: Rect,
    visual: Option<Arc<VisualTextLine>>,
    glyphs: Option<Arc<StyledGlyphRun>>,
}

impl PartialEq for StyledFragment {
    fn eq(&self, other: &Self) -> bool {
        self.eq_placed(0.0, other, 0.0)
    }
}

/// An already shaped, positioned glyph relative to the origin of its text run.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyledGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
}

/// A font resource kept alive by positioned text snapshots.
#[doc(hidden)]
#[derive(Clone)]
pub struct StyledGlyphFont {
    pub key: (u64, u32),
    pub bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    pub ink: Option<[f32; 2]>,
}

impl AsRef<[u8]> for StyledGlyphFont {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref().as_ref()
    }
}

impl std::fmt::Debug for StyledGlyphFont {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("StyledGlyphFont")
            .field(&self.key)
            .finish()
    }
}

impl PartialEq for StyledGlyphFont {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key || (self.key.1 == other.key.1 && self.as_ref() == other.as_ref())
    }
}

/// Immutable glyph and caret geometry for an independently positioned run.
#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub struct StyledGlyphRun {
    pub font: Arc<StyledGlyphFont>,
    pub font_size: f32,
    pub baseline: f32,
    pub ink: Option<[f32; 2]>,
    pub variations: Box<[i16]>,
    pub glyphs: Box<[StyledGlyph]>,
    pub underline: [f32; 2],
    pub strikethrough: [f32; 2],
    pub visual: Option<Arc<VisualTextLine>>,
}

impl StyledFragment {
    const BOLD: u8 = 1 << 0;
    const ITALIC: u8 = 1 << 1;
    const MONOSPACE: u8 = 1 << 2;
    const STRIKETHROUGH: u8 = 1 << 3;
    const UNDERLINE: u8 = 1 << 4;
    const COLOR: u8 = 1 << 5;
    const RTL_MARK: u8 = 1 << 6;

    pub(crate) fn eq_placed(&self, y: f32, other: &Self, other_y: f32) -> bool {
        let mut bounds = self.bounds;
        bounds.y += y;
        let mut other_bounds = other.bounds;
        other_bounds.y += other_y;
        bounds == other_bounds
            && self.range == other.range
            && self.link == other.link
            && self.color == other.color
            && self.flags == other.flags
            && (self
                .visual
                .as_ref()
                .zip(other.visual.as_ref())
                .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
                || self.visual == other.visual)
            && (self
                .glyphs
                .as_ref()
                .zip(other.glyphs.as_ref())
                .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
                || self.glyphs == other.glyphs)
    }

    fn from_piece(piece: Piece, bounds: Rect) -> Self {
        let Piece {
            range,
            rtl_mark,
            style,
            link,
        } = piece;
        Self {
            range,
            link: link.and_then(|index| NonZeroUsize::new(index + 1)),
            color: style.color.unwrap_or_default(),
            flags: (u8::from(style.bold) * Self::BOLD)
                | (u8::from(style.italic) * Self::ITALIC)
                | (u8::from(style.monospace) * Self::MONOSPACE)
                | (u8::from(style.strikethrough) * Self::STRIKETHROUGH)
                | (u8::from(style.underline) * Self::UNDERLINE)
                | (u8::from(style.color.is_some()) * Self::COLOR)
                | (u8::from(rtl_mark) * Self::RTL_MARK),
            bounds,
            visual: None,
            glyphs: None,
        }
    }

    #[doc(hidden)]
    pub fn from_shaped_run(
        range: Range<usize>,
        style: TextStyle,
        link: Option<usize>,
        bounds: Rect,
        glyphs: Arc<StyledGlyphRun>,
    ) -> Self {
        let mut fragment = Self::from_piece(
            Piece {
                range,
                style,
                link,
                rtl_mark: false,
            },
            bounds,
        );
        fragment.glyphs = Some(glyphs);
        fragment
    }

    #[doc(hidden)]
    pub fn shaped_run(&self) -> Option<&StyledGlyphRun> {
        self.glyphs.as_deref()
    }

    fn visual(&self) -> Option<&Arc<VisualTextLine>> {
        self.visual
            .as_ref()
            .or_else(|| self.shaped_run()?.visual.as_ref())
    }

    fn rtl_mark(&self) -> bool {
        self.flags & Self::RTL_MARK != 0
    }

    fn source_range(&self) -> Option<Range<usize>> {
        (!self.range.is_empty()).then(|| self.range.clone())
    }

    pub(crate) fn contains_text_offset(&self, offset: usize) -> bool {
        self.range.contains(&offset)
    }

    pub fn text<'a>(&self, document: &'a StyledText) -> Cow<'a, str> {
        piece_text(document, &self.range, self.rtl_mark())
    }

    /// Identifies the painted substring and its optional bidi prefix.
    #[doc(hidden)]
    pub fn source_span(&self) -> (Option<Range<usize>>, bool) {
        (self.source_range(), self.rtl_mark())
    }

    pub fn style(&self) -> TextStyle {
        TextStyle {
            bold: self.flags & Self::BOLD != 0,
            italic: self.flags & Self::ITALIC != 0,
            monospace: self.flags & Self::MONOSPACE != 0,
            strikethrough: self.flags & Self::STRIKETHROUGH != 0,
            underline: self.flags & Self::UNDERLINE != 0,
            color: (self.flags & Self::COLOR != 0).then_some(self.color),
        }
    }

    pub fn link(&self) -> Option<usize> {
        self.link.map(|link| link.get() - 1)
    }

    #[doc(hidden)]
    pub fn selection_geometry(&self) -> Option<(&VisualTextLine, Range<usize>, usize)> {
        Some((
            self.visual()?.as_ref(),
            self.source_range()?,
            if self.rtl_mark() {
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
    pub fragments: StyledFragments,
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
    reusable_widths: Option<WidthInterval>,
    lines: Vec<IndexedLine>,
}

impl StyledTextLineIndex {
    #[doc(hidden)]
    pub fn reusable_widths(&self) -> Option<Range<f32>> {
        self.reusable_widths
            .map(|interval| interval.minimum..interval.maximum)
    }

    pub fn allocated_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<IndexedLine>()
    }
}

/// Selection geometry of exact source spans in one document and font context.
#[doc(hidden)]
pub struct StyledSelectionSpans {
    document: StyledText,
    entries: SelectionEntries,
}

struct SelectionEntry {
    end_and_direction: u32,
    line: Weak<VisualTextLine>,
}

struct SelectionEntries {
    slots: Vec<u32>,
    shapes: Vec<SelectionEntry>,
}

impl SelectionEntries {
    fn get(&self, fragment: &StyledFragment) -> Option<Arc<VisualTextLine>> {
        let slot = self.slots.get(fragment.range.start)?.checked_sub(1)?;
        let entry = &self.shapes[slot as usize];
        debug_assert!(fragment.range.end <= self.slots.len());
        let signature = fragment.range.end as u32 * 2 + u32::from(fragment.rtl_mark());
        if entry.end_and_direction == signature {
            entry.line.upgrade()
        } else {
            None
        }
    }

    fn remember(&mut self, fragment: &StyledFragment, line: &Arc<VisualTextLine>) {
        const MAX_SPANS: usize = 65_536;
        debug_assert!(self.slots.is_empty() || fragment.range.end <= self.slots.len());
        let Some(slot) = self.slots.get_mut(fragment.range.start) else {
            return;
        };
        let signature = fragment.range.end as u32 * 2 + u32::from(fragment.rtl_mark());
        if let Some(index) = slot.checked_sub(1) {
            // At a new wrap width the same start byte can have a different end.
            // Keep the latest exact span so the next layout can reuse it.
            let entry = &mut self.shapes[index as usize];
            entry.end_and_direction = signature;
            entry.line = Arc::downgrade(line);
        } else if self.shapes.len() < MAX_SPANS {
            self.shapes.push(SelectionEntry {
                end_and_direction: signature,
                line: Arc::downgrade(line),
            });
            *slot = self.shapes.len() as u32;
        }
    }
}

impl StyledSelectionSpans {
    pub fn new(document: StyledText) -> Self {
        let len = document.text().len();
        let slots = if len <= 1024 * 1024 {
            vec![0; len]
        } else {
            Vec::new()
        };
        Self {
            document,
            entries: SelectionEntries {
                slots,
                shapes: Vec::new(),
            },
        }
    }
}

impl StyledTextLayout {
    #[doc(hidden)]
    pub fn allocated_bytes(&self) -> usize {
        let mut counted = HashSet::new();
        self.fragments.allocated_bytes()
            + self
                .fragments
                .placed()
                .filter_map(|(fragment, _)| fragment.visual())
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
        shape: impl FnMut(&str, TextStyle) -> VisualTextLine,
    ) -> usize {
        self.shape_selection_recording(document, start, shape, |_, _| {})
    }

    /// Records the size of each new visual line at its first fragment.
    #[doc(hidden)]
    pub fn shape_selection_recording(
        &mut self,
        document: &StyledText,
        start: usize,
        mut shape: impl FnMut(&str, TextStyle) -> VisualTextLine,
        allocated: impl FnMut(usize, usize),
    ) -> usize {
        self.shape_selection_recording_shared(
            document,
            start,
            |text, style| Arc::new(shape(text, style)),
            allocated,
        )
    }

    /// Records the resident bytes of each distinct visual line in this plan.
    #[doc(hidden)]
    pub fn shape_selection_recording_shared(
        &mut self,
        document: &StyledText,
        start: usize,
        shape: impl FnMut(&str, TextStyle) -> Arc<VisualTextLine>,
        allocated: impl FnMut(usize, usize),
    ) -> usize {
        self.shape_selection_recording_impl(document, start, None, shape, allocated)
    }

    /// Restores cached selection geometry before looking up a fragment's text.
    #[doc(hidden)]
    pub fn shape_selection_recording_by_span(
        &mut self,
        start: usize,
        spans: &mut StyledSelectionSpans,
        shape: impl FnMut(&str, TextStyle) -> Arc<VisualTextLine>,
        allocated: impl FnMut(usize, usize),
    ) -> usize {
        let entries = (!spans.entries.slots.is_empty()).then_some(&mut spans.entries);
        self.shape_selection_recording_impl(&spans.document, start, entries, shape, allocated)
    }

    fn shape_selection_recording_impl(
        &mut self,
        document: &StyledText,
        start: usize,
        mut spans: Option<&mut SelectionEntries>,
        mut shape: impl FnMut(&str, TextStyle) -> Arc<VisualTextLine>,
        mut allocated: impl FnMut(usize, usize),
    ) -> usize {
        let mut lines = HashMap::new();
        let mut counted = spans.is_some().then(HashSet::new);
        let mut bytes = 0;
        for (index, fragment) in self.fragments.selection_fragments_mut(start).enumerate() {
            if fragment.range.is_empty() {
                continue;
            }
            let cached = spans.as_ref().and_then(|spans| spans.get(fragment));
            let line = cached.unwrap_or_else(|| {
                let style_key = (fragment.text(document), fragment.style());
                let line = lines
                    .entry(style_key)
                    .or_insert_with_key(|(text, style)| {
                        let line = shape(text, *style);
                        if counted.is_none() {
                            let size = line.allocated_bytes();
                            bytes += size;
                            allocated(start + index, size);
                        }
                        line
                    })
                    .clone();
                if let Some(spans) = &mut spans {
                    spans.remember(fragment, &line);
                }
                line
            });
            if let Some(counted) = &mut counted
                && counted.insert(Arc::as_ptr(&line))
            {
                let size = line.allocated_bytes();
                bytes += size;
                allocated(start + index, size);
            }
            fragment.visual = Some(line);
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
            .placed()
            .filter(|(fragment, _)| fragment.visual().is_some())
            .map(|(fragment, offset)| {
                let y = distance(point.y, fragment.bounds.y + offset, fragment.bounds.height);
                (fragment, y)
            })
            .min_by(|(a, ay), (b, by)| {
                let x = |f: &StyledFragment| distance(point.x, f.bounds.x, f.bounds.width);
                ay.total_cmp(by).then_with(|| x(a).total_cmp(&x(b)))
            })?;
        fragment.position(fragment.visual()?.hit(point.x - fragment.bounds.x))
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
            .placed()
            .enumerate()
            .filter_map(|(index, (fragment, y))| {
                let (visual, range, prefix) = fragment.selection_geometry()?;
                Some((index, fragment, y, visual, range, prefix))
            });
        let Some((index, fragment, y, visual, range, prefix)) =
            items.min_by_key(|(_, _, _, _, range, _)| {
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
                        let (adjacent, adjacent_y) = if right {
                            self.fragments
                                .placed()
                                .skip(index + 1)
                                .find(|(fragment, _)| fragment.visual().is_some())
                        } else {
                            self.fragments
                                .placed()
                                .rev()
                                .skip(self.fragments.len() - index)
                                .find(|(fragment, _)| fragment.visual().is_some())
                        }?;
                        let visual = adjacent.visual()?;
                        let edge = visual.edge(!right);
                        let position = adjacent.position(edge)?;
                        if adjacent.bounds.y + adjacent_y == fragment.bounds.y + y
                            && position.offset == caret.offset
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
                    y: (fragment.bounds.y + y) + fragment.bounds.height * 0.5 + delta,
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
        Self::build(node, document, available, font_size, advance, None, None)
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
            None,
        );
        (layout, lines)
    }

    /// Keep measurements when only the layout constraints change. The measuring
    /// callback sets `invalidated` if it changes the font set during a probe.
    #[doc(hidden)]
    pub fn indexed_with_measurements(
        node: &Node,
        available: Size,
        font_size: f32,
        measurements: &mut StyledTextMeasurements,
        invalidated: &Cell<bool>,
        advance: impl FnMut(&str, TextStyle) -> f32,
    ) -> (Self, StyledTextLineIndex) {
        if invalidated.replace(false) {
            measurements.clear_widths();
        }
        if let Some(prepared) = &measurements.prepared {
            prepared.reusable_widths.set(None);
            prepared.replayed_widths.set(false);
        }
        measurements.widths.begin_layout();
        let mut lines = StyledTextLineIndex::default();
        let layout = Self::build(
            node,
            &measurements.document,
            available,
            font_size,
            advance,
            Some(&mut lines),
            Some(RetainedMeasurements {
                bidi: &measurements.bidi,
                prepared: measurements.prepared.as_ref(),
                spans: measurements.spans.as_ref(),
                widths: &mut measurements.widths,
                lines: &mut measurements.lines,
                shared_widths: &mut measurements.shared_widths,
                shared_hit: false,
                invalidated,
            }),
        );
        if invalidated.replace(false) {
            measurements.clear_widths();
        }
        lines.reusable_widths = measurements
            .prepared
            .as_ref()
            .and_then(|prepared| prepared.reusable_widths.get());
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
                if !fragment.range.is_empty() {
                    fragment.range.start += offset;
                    fragment.range.end += offset;
                }
                fragment.link = fragment.link.and_then(|link| link.checked_add(first_link));
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
        let fragments = self
            .fragments
            .replace_tail(prefix_fragments, tail.fragments.into_vec());
        (
            Self { size, fragments },
            StyledTextLineIndex {
                lines,
                reusable_widths: None,
            },
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
        mut retained: Option<RetainedMeasurements<'_>>,
    ) -> Self {
        let owned_bidi;
        let bidi = if let Some(retained) = &retained {
            retained.bidi
        } else {
            owned_bidi = StyledBidi::new(document.text());
            &owned_bidi
        };
        let mut line_bidi = LineBidi::new();
        let spans = retained.as_ref().and_then(|retained| retained.spans);
        let advance_cache = spans
            .and_then(|spans| spans.advances.as_deref().map(|cache| (spans, cache)))
            .zip(retained.as_ref().map(|entry| entry.invalidated));
        let mut width = |piece: &Piece| {
            let mut measure = || {
                advance(
                    &piece_text(document, &piece.range, piece.rtl_mark),
                    piece.style,
                )
            };
            if let Some(((spans, cache), invalidated)) = advance_cache
                && !invalidated.get()
                && let Some(span) = spans.complete_span_index(&piece.range)
            {
                let slot = &cache[span];
                let index = usize::from(piece.rtl_mark);
                let mut stored = slot.get();
                if stored[index].is_nan() {
                    stored[index] = measure();
                    if !invalidated.get() {
                        slot.set(stored);
                    }
                }
                return stored[index];
            }
            measure()
        };
        let mut measurements = Vec::new();
        // Reuse storage across wrap probes and final line placement.
        let mut line_pieces = Vec::new();
        let prepared = retained
            .as_ref()
            .and_then(|retained| retained.prepared)
            .filter(|_| {
                node.text("text")
                    .is_some_and(|text| std::ptr::eq(text, document.text()))
            });
        let plan = LabelPlan::with_widths(
            node,
            available,
            font_size,
            node.layout_direction(),
            |range, ellipsis| {
                let key = LineKey::new(&range, ellipsis);
                let source = range.clone();
                let measure = || {
                    collect_pieces(
                        document,
                        bidi,
                        spans,
                        &mut line_bidi,
                        range,
                        ellipsis,
                        &mut line_pieces,
                    );
                    line_pieces.iter().map(&mut width).sum()
                };
                match &mut retained {
                    Some(retained) => {
                        retained.measure(document.text(), &source, ellipsis, key, measure)
                    }
                    None => measure(),
                }
            },
            index.is_some().then_some(&mut measurements),
            prepared,
        );
        let generation = prepared.map(PreparedLabel::font_generation);
        // Keep the initial layout on the ordinary placement path unless the
        // measurement traversal has already found reusable suffixes.
        let share_templates = retained.as_ref().is_some_and(|entry| entry.shared_hit)
            || prepared.is_some_and(|prepared| prepared.replayed_widths.get());
        let mut fragments = Vec::new();
        let mut duplicate_ink = Vec::new();
        for (line_index, line) in plan.lines.into_iter().enumerate() {
            if let Some(index) = index.as_deref_mut() {
                index.lines.push(IndexedLine {
                    source_start: line.range.start,
                    first_fragment: fragments.len(),
                    max_width: measurements[line_index].max_width,
                });
            }
            if line.range.is_empty() && !line.ellipsis {
                continue;
            }
            let key = LineKey::new(&line.range, line.ellipsis);
            let mut x = line.origin.x;
            let parts = retained
                .as_ref()
                .zip(key)
                .and_then(|(entry, key)| entry.lines.get(&key));
            if let Some(parts) = parts {
                for part in parts {
                    fragments.push(part.place(x, line.origin.y, font_size * 1.4));
                    x += part.width;
                }
                continue;
            }
            if let Some(replay) = measurements.get(line_index).and_then(|line| line.replay)
                && let Some(index) = &index
                && let Some(spans) = spans
                && let Some(entry) = &mut retained
                && !entry.invalidated.get()
                && prepared.map(PreparedLabel::font_generation) == generation
            {
                let source = &index.lines[replay.source()];
                let end = index.lines[replay.source() + 1].first_fragment;
                let first = fragments.len();
                if replay_line_fragments(
                    &mut fragments,
                    source.first_fragment..end,
                    source.source_start,
                    &line,
                    replay.replace_heading,
                    spans,
                    &mut duplicate_ink,
                ) {
                    if let Some(key) = key {
                        entry.lines.insert(key, &fragments[first..]);
                    }
                    continue;
                }
            }
            let shared = if share_templates {
                spans.zip(retained.as_ref()).and_then(|(spans, retained)| {
                    retained
                        .shared_widths
                        .probe(document.text(), bidi, spans, &line.range, line.ellipsis)
                        .map(|probe| (spans, probe))
                })
            } else {
                None
            };
            if let Some((spans, probe)) = &shared
                && let Some(entry) = retained.as_ref()
                && let Some((source, source_tail)) =
                    entry
                        .shared_widths
                        .template(document.text(), bidi, spans, probe)
                && let Some(parts) =
                    LineKey::new(&source, false).and_then(|key| entry.lines.get(&key))
                && probe.leading.is_none_or(|width| {
                    parts.first().is_some_and(|part| {
                        part.range.start as usize == source.start
                            && part.range.end as usize == source_tail
                            && part.width.to_bits() == width
                    })
                })
            {
                let first = fragments.len();
                let parts = if let Some(width) = probe.leading {
                    let leading = &spans.styles[spans.first_style(line.range.start)];
                    let width = f32::from_bits(width);
                    fragments.push(StyledFragment::from_piece(
                        Piece {
                            range: line.range.start..probe.tail.start,
                            rtl_mark: false,
                            style: leading.style,
                            link: leading.link.checked_sub(1).map(|link| link as usize),
                        },
                        Rect::new(x, line.origin.y, width, font_size * 1.4),
                    ));
                    x += width;
                    &parts[1..]
                } else {
                    parts
                };
                for part in parts {
                    let mut fragment = part.place(x, line.origin.y, font_size * 1.4);
                    let start = probe.tail.start + (fragment.range.start - source_tail);
                    fragment.range = start..start + fragment.range.len();
                    fragment.link =
                        NonZeroUsize::new(spans.styles[spans.first_style(start)].link as usize);
                    fragments.push(fragment);
                    x += part.width;
                }
                // A return to this width can use the rebased fragments directly.
                if let Some(entry) = &mut retained
                    && let Some(key) = key
                {
                    entry.lines.insert(key, &fragments[first..]);
                }
                continue;
            }
            let range = line.range.clone();
            let first = fragments.len();
            collect_pieces(
                document,
                bidi,
                spans,
                &mut line_bidi,
                line.range,
                line.ellipsis,
                &mut line_pieces,
            );
            for piece in line_pieces.drain(..) {
                let advance = width(&piece);
                let bounds = Rect::new(x, line.origin.y, advance, font_size * 1.4);
                fragments.push(StyledFragment::from_piece(piece, bounds));
                x += advance;
            }
            if let Some(retained) = &mut retained
                && !retained.invalidate_if_requested()
                && let Some(key) = key
            {
                retained.lines.insert(key, &fragments[first..]);
                if let Some(spans) = spans
                    && let Some(fragment) = fragments.get(first)
                {
                    retained
                        .shared_widths
                        .remember_leading(bidi, spans, &range, fragment);
                }
                if let Some((_, probe)) = shared
                    && retained.lines.get(&key).is_some()
                {
                    retained.shared_widths.remember_template(&range, &probe);
                }
            }
        }
        Self {
            size: plan.size,
            fragments: StyledFragments::from_replayed_layout(fragments, duplicate_ink),
        }
    }
}

/// Place the fragments of a line whose width and source were certified by the planner.
fn replay_line_fragments(
    fragments: &mut Vec<StyledFragment>,
    mut source: Range<usize>,
    source_start: usize,
    line: &PlannedLabelLine,
    replace_heading: bool,
    spans: &PreparedSpans,
    duplicate_ink: &mut Vec<Range<usize>>,
) -> bool {
    let mut x = line.origin.x;
    if replace_heading {
        let leading = &spans.styles[spans.first_style(line.range.start)];
        let end = leading.end as usize;
        let Some(first) = fragments
            .get(source.clone())
            .and_then(|parts| parts.first())
        else {
            return false;
        };
        if end > line.range.end
            || first.range.start != source_start
            || first.range.len() != end - line.range.start
            || first.rtl_mark()
        {
            return false;
        }
        let bounds = Rect::new(x, line.origin.y, first.bounds.width, first.bounds.height);
        fragments.push(StyledFragment::from_piece(
            Piece {
                range: line.range.start..end,
                rtl_mark: false,
                style: leading.style,
                link: leading.link.checked_sub(1).map(|link| link as usize),
            },
            bounds,
        ));
        x += bounds.width;
        source.start += 1;
    }
    let copied = fragments.len();
    let mut same_decoration = true;
    for position in source {
        let mut fragment = fragments[position].clone();
        let start = line.range.start + (fragment.range.start - source_start);
        let link = NonZeroUsize::new(spans.styles[spans.first_style(start)].link as usize);
        // Links add underlines at paint time. Matching text and style alone
        // doesn't cover a destination that changes the decoration.
        same_decoration &= fragment.link.is_some() == link.is_some();
        fragment.range = start..start + fragment.range.len();
        fragment.link = link;
        fragment.bounds.x = x;
        fragment.bounds.y = line.origin.y;
        x += fragment.bounds.width;
        fragments.push(fragment);
    }
    if same_decoration && copied < fragments.len() {
        duplicate_ink.push(copied..fragments.len());
    }
    true
}

fn collect_pieces<'text>(
    document: &'text StyledText,
    bidi: &StyledBidi,
    spans: Option<&PreparedSpans>,
    line_bidi: &mut LineBidi<'text>,
    range: Range<usize>,
    ellipsis: bool,
    pieces: &mut Vec<Piece>,
) {
    pieces.clear();
    let mut rtl = false;
    let first = spans
        .and_then(|spans| spans.paragraph(bidi, range.start))
        .unwrap_or_else(|| {
            bidi.paragraphs
                .partition_point(|paragraph| paragraph.range.end <= range.start)
        });
    for (number, paragraph) in bidi.paragraphs.iter().enumerate().skip(first) {
        let line = paragraph.range.start.max(range.start)..paragraph.range.end.min(range.end);
        if line.start > range.end {
            break;
        }
        if pieces.is_empty() {
            rtl = paragraph.level.is_rtl();
        }
        if line.is_empty() {
            continue;
        }
        let levels = &bidi.levels[line.clone()];
        let uniform = if let Some(spans) = spans {
            levels[0] == paragraph.level && spans.level_end(line.start) >= line.end
        } else {
            levels.iter().all(|&level| level == paragraph.level)
        };
        if uniform {
            split_run(document, line, paragraph.level.is_rtl(), spans, pieces);
            continue;
        }
        if let Some(spans) = spans
            && line.end <= spans.l1_limits[number] as usize
        {
            simple_visual_runs(bidi, spans, paragraph.level, line, &mut line_bidi.runs);
            for (run, level) in &line_bidi.runs {
                split_run(document, run.clone(), level.is_rtl(), Some(spans), pieces);
            }
            continue;
        }
        // L1 and L2 operate on the line and use the paragraph's resolved
        // levels. Supplying just this slice bounds visual_runs' level copy.
        let start = line.start;
        let info = &mut line_bidi.paragraph;
        info.text = &document.text()[line.clone()];
        info.paragraph_level = paragraph.level;
        info.is_pure_ltr = false;
        info.original_classes.clear();
        info.original_classes
            .extend_from_slice(&bidi.classes[line.clone()]);
        info.levels.clear();
        info.levels.extend_from_slice(levels);
        let (levels, runs) = info.visual_runs(0..line.len());
        for run in runs {
            let rtl = levels[run.start].is_rtl();
            split_run(
                document,
                start + run.start..start + run.end,
                rtl,
                spans,
                pieces,
            );
        }
    }
    if ellipsis {
        let mark = Piece {
            range: 0..0,
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
}

fn simple_visual_runs(
    bidi: &StyledBidi,
    spans: &PreparedSpans,
    base: Level,
    line: Range<usize>,
    runs: &mut Vec<(Range<usize>, Level)>,
) {
    runs.clear();
    // L1 resets only the trailing whitespace in this range.
    let mut suffix = line.end;
    while suffix > line.start && bidi.classes[suffix - 1] == BidiClass::WS {
        suffix -= 1;
    }
    let mut start = line.start;
    for &end in &spans.level_ends[spans.level_run(start)..] {
        let end = (end as usize).min(suffix);
        if end > start {
            runs.push((start..end, bidi.levels[start]));
        }
        start = end;
        if start == suffix {
            break;
        }
    }
    if suffix < line.end {
        if let Some((last, level)) = runs.last_mut()
            && *level == base
        {
            last.end = line.end;
        } else {
            runs.push((suffix..line.end, base));
        }
    }

    let (min, max) = runs.iter().fold((u8::MAX, 0), |(min, max), (_, level)| {
        (min.min(level.number()), max.max(level.number()))
    });
    // L2 reverses contiguous runs at each embedding level down to the lowest odd level.
    for level in ((min | 1)..=max).rev() {
        let mut start = 0;
        while start < runs.len() {
            if runs[start].1.number() < level {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < runs.len() && runs[end].1.number() >= level {
                end += 1;
            }
            runs[start..end].reverse();
            start = end;
        }
    }
}

fn split_run(
    document: &StyledText,
    range: Range<usize>,
    rtl: bool,
    spans: Option<&PreparedSpans>,
    pieces: &mut Vec<Piece>,
) {
    let first = pieces.len();
    let append = |range: Range<usize>, style, link| {
        let rtl_mark = rtl && get_base_direction(&document.text()[range.clone()]) != Direction::Rtl;
        pieces.push(Piece {
            range,
            rtl_mark,
            style,
            link,
        });
    };
    if let Some(spans) = spans {
        spans.visit_styles(range, append);
    } else {
        visit_style_spans(document, range, append);
    }
    if rtl {
        pieces[first..].reverse();
    }
}

fn visit_style_spans(
    document: &StyledText,
    range: Range<usize>,
    mut visit: impl FnMut(Range<usize>, TextStyle, Option<usize>),
) {
    let mut start = range.start;
    let styles = document.runs();
    let links = document.links();
    let mut style_index = styles.partition_point(|run| run.end <= start);
    let mut link_index = links.partition_point(|link| link.end <= start);
    while start < range.end {
        let style_run = styles.get(style_index);
        let link_run = links.get(link_index);
        let (style, style_end) = match style_run {
            Some(run) if run.start <= start => (run.style, run.end),
            Some(run) => (TextStyle::default(), run.start),
            None => (TextStyle::default(), range.end),
        };
        let (link, link_end) = match link_run {
            Some(link) if link.start <= start => (Some(link_index), link.end),
            Some(link) => (None, link.start),
            None => (None, range.end),
        };
        let end = range.end.min(style_end).min(link_end);
        visit(start..end, style, link);
        style_index += usize::from(style_run.is_some_and(|run| run.end == end));
        link_index += usize::from(link_run.is_some_and(|link| link.end == end));
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::{StyledLink, StyledRun};

    #[test]
    fn full_width_cache_keeps_recent_probes_and_admits_new_ones() {
        let mut cache = RetainedWidths::default();
        for index in 0..RetainedWidths::MAX_PROBES {
            cache.insert(LineKey(index as u64), index as f32);
        }

        cache.begin_layout();
        let previous = LineKey(1);
        assert_eq!(cache.get(&previous), Some(1.0));
        cache.begin_layout();
        let current = LineKey(2);
        assert_eq!(cache.get(&current), Some(2.0));
        let new = LineKey(RetainedWidths::MAX_PROBES as u64);
        cache.insert(new, 42.0);

        assert_eq!(cache.get(&previous), Some(1.0));
        assert_eq!(cache.get(&current), Some(2.0));
        assert_eq!(cache.get(&new), Some(42.0));
        assert_eq!(cache.get(&LineKey(3)), None);
        assert_eq!(cache.values.len(), 3);
    }

    #[test]
    fn certified_widths_preserve_geometry_and_exclude_unstable_fonts() {
        use xrack_runtime::TextWrap;

        let document = StyledText::from_markdown(
            "**Alpha** abcdefgh · [go](destination) שלום 12345\n\nBeta 🎧 *(longword)* tail",
        )
        .unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        let invalidated = Cell::new(false);
        let measure = |text: &str, style: TextStyle| {
            ((text.len() * 11 + usize::from(style.bold) * 5) % 37) as f32
        };
        let mut checked = 0;
        let mut bounded = 0;
        for wrap in [TextWrap::WordWrap, TextWrap::CharWrap, TextWrap::NoWrap] {
            node.properties.insert("wrap", wrap.into());
            let mut measurements = StyledTextMeasurements::new(document.clone());
            for width in (8..160).step_by(3) {
                let (layout, lines) = StyledTextLayout::indexed_with_measurements(
                    &node,
                    Size::new(width as f32, f32::INFINITY),
                    12.0,
                    &mut measurements,
                    &invalidated,
                    measure,
                );
                let Some(range) = lines.reusable_widths() else {
                    continue;
                };
                let maximum = if range.end.is_finite() {
                    bounded += 1;
                    range.end.next_down()
                } else {
                    range.start + 60.0
                };
                for width in [range.start.max(0.5), maximum] {
                    assert!(range.contains(&width));
                    let (expected, _) = StyledTextLayout::indexed(
                        &node,
                        &document,
                        Size::new(width, f32::INFINITY),
                        12.0,
                        measure,
                    );
                    assert_eq!(layout, expected, "width={width}, wrap={wrap:?}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 30);
        assert!(bounded > 0);

        let size = Size::new(160.0, f32::INFINITY);
        let mut measurements = StyledTextMeasurements::new(document);
        let changed = Cell::new(false);
        let (_, lines) = StyledTextLayout::indexed_with_measurements(
            &node,
            size,
            12.0,
            &mut measurements,
            &invalidated,
            |text, style| {
                if !changed.replace(true) {
                    invalidated.set(true);
                }
                measure(text, style)
            },
        );
        assert!(changed.get());
        assert!(lines.reusable_widths().is_none());
        node.properties.insert(
            "horizontal-alignment",
            TextHorizontalAlignment::Center.into(),
        );
        let (_, lines) = StyledTextLayout::indexed_with_measurements(
            &node,
            size,
            12.0,
            &mut measurements,
            &invalidated,
            measure,
        );
        assert!(lines.reusable_widths().is_none());
    }

    #[test]
    fn shared_suffix_width_requires_matching_style_splits_and_bidi_levels() {
        let text = "a same\nb same\nc same\nא same";
        let ranges: Vec<_> = text
            .match_indices("same")
            .map(|(start, text)| start..start + text.len())
            .collect();
        let document = StyledText::from_parts(
            text,
            Vec::new(),
            vec![StyledLink {
                start: ranges[1].start + 1,
                end: ranges[1].start + 3,
                href: "https://example.invalid".into(),
            }],
        )
        .unwrap();
        let bidi = StyledBidi::new(text);
        let spans = PreparedSpans::new(&document, &bidi).unwrap();
        let mut memo = SharedWidths::default();
        let probe = memo.probe(text, &bidi, &spans, &ranges[0], false).unwrap();
        memo.remember(&probe, 42.0);
        let with_tail = |index: usize| SharedLineProbe {
            tail: ranges[index].clone(),
            hash: probe.hash,
            leading: None,
        };
        assert_eq!(memo.lookup(text, &bidi, &spans, &with_tail(1)), None);
        assert_eq!(memo.lookup(text, &bidi, &spans, &with_tail(2)), Some(42.0));
        assert_eq!(memo.lookup(text, &bidi, &spans, &with_tail(3)), None);
    }

    #[test]
    fn shared_first_lines_keep_different_prefix_styles_and_source_offsets() {
        let text =
            "a same word שלום after gap\n界 same word שלום after gap\nא same word שלום after gap";
        let mut runs = Vec::new();
        let mut links = Vec::new();
        for (index, prefix) in ["a", "界", "א"].into_iter().enumerate() {
            let start = text.find(prefix).unwrap();
            let end = start + prefix.len();
            runs.push(StyledRun {
                start,
                end,
                style: TextStyle {
                    color: Some(Color(0x112233ff + (index as u32) * 0x11000000)),
                    bold: index == 0,
                    italic: index == 1,
                    ..TextStyle::default()
                },
            });
            links.push(StyledLink {
                start,
                end,
                href: format!("https://example.invalid/prefix/{index}").into(),
            });
            links.push(StyledLink {
                start: end + 1,
                end: end + 5,
                href: format!("https://example.invalid/tail/{index}").into(),
            });
        }
        let document = StyledText::from_parts(text, runs, links).unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        let mut measurements = StyledTextMeasurements::new(document.clone());
        let invalidated = Cell::new(false);
        let advance = |text: &str, _: TextStyle| text.chars().count() as f32 * 8.0;
        StyledTextLayout::indexed_with_measurements(
            &node,
            Size::new(500.0, f32::INFINITY),
            12.0,
            &mut measurements,
            &invalidated,
            advance,
        );
        measurements.retain_span_advances();
        let span = measurements.spans.as_ref().unwrap();
        let first_span = span.complete_span_index(&(0..1)).unwrap();
        assert_eq!(span.advances.as_ref().unwrap()[first_span].get()[0], 8.0);
        let size = Size::new(65.0, f32::INFINITY);
        let (expected, _) = StyledTextLayout::indexed(&node, &document, size, 12.0, advance);
        let (actual, _) = StyledTextLayout::indexed_with_measurements(
            &node,
            size,
            12.0,
            &mut measurements,
            &invalidated,
            advance,
        );
        assert_eq!(actual, expected);

        let start = text.find("界").unwrap();
        let spans = measurements.spans.as_ref().unwrap();
        let memo = &measurements.shared_widths;
        let bidi = &measurements.bidi;
        let probe = memo
            .probe(text, bidi, spans, &(start..start + "界 same".len()), false)
            .unwrap();
        assert_eq!(probe.leading, Some(8.0_f32.to_bits()));
        assert_eq!(memo.template(text, bidi, spans, &probe), Some((0..6, 1)));

        let different_width = SharedLineProbe {
            leading: Some(12.0_f32.to_bits()),
            ..probe
        };
        assert!(memo.lookup(text, bidi, spans, &different_width).is_none());
        assert!(memo.template(text, bidi, spans, &different_width).is_none());

        let rtl = text.find("א").unwrap();
        assert!(
            memo.probe(text, bidi, spans, &(rtl..rtl + "א same".len()), false)
                .is_none()
        );
        measurements.clear_widths();
        assert!(
            measurements
                .spans
                .as_ref()
                .unwrap()
                .advances
                .as_ref()
                .unwrap()[first_span]
                .get()[0]
                .is_nan()
        );
    }

    #[test]
    fn cold_wraps_reuse_unicode_suffixes_after_checking_the_heading() {
        use std::fmt::Write as _;

        let mut markdown = String::new();
        for index in 0..160 {
            writeln!(
                markdown,
                "**构建 {index:04}** alphanumeric שלום [log](log/{index})\n"
            )
            .unwrap();
        }
        let document = StyledText::from_markdown(&markdown).unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        let measure = |text: &str, style: TextStyle| {
            let width = text.chars().count() as f32 * 5.0;
            if style.bold && text.contains('9') {
                width + 15.0
            } else {
                width
            }
        };
        for width in [49.0, 105.0, 250.0] {
            let size = Size::new(width, f32::INFINITY);
            let mut direct_calls = 0;
            let (direct, _) =
                StyledTextLayout::indexed(&node, &document, size, 12.0, |text, style| {
                    direct_calls += 1;
                    measure(text, style)
                });
            let mut measurements = StyledTextMeasurements::new(document.clone());
            let mut calls = 0;
            let (retained, _) = StyledTextLayout::indexed_with_measurements(
                &node,
                size,
                12.0,
                &mut measurements,
                &Cell::new(false),
                |text, style| {
                    calls += 1;
                    measure(text, style)
                },
            );
            assert_eq!(retained, direct, "width {width}");
            assert!(
                calls < direct_calls,
                "width {width}: {calls} versus {direct_calls}"
            );
            if width == 250.0 {
                assert!(
                    measurements
                        .prepared
                        .as_ref()
                        .unwrap()
                        .replayed_widths
                        .get()
                );
                let key = |fragment: &StyledFragment| {
                    let mut style = fragment.style();
                    style.underline |= fragment.link().is_some();
                    (fragment.text(&document), style)
                };
                let expected: HashSet<_> = retained.fragments.iter().map(key).collect();
                let representatives = retained.fragments.ink_representatives().unwrap();
                assert!(representatives.clone().count() < retained.fragments.len() / 2);
                assert_eq!(representatives.map(key).collect::<HashSet<_>>(), expected);

                let prefix = retained.fragments.len() - 3;
                let tail = retained
                    .fragments
                    .iter_range(prefix..retained.fragments.len())
                    .cloned()
                    .collect();
                let mut updated = retained.fragments.replace_tail(prefix, tail);
                assert_eq!(
                    updated
                        .ink_representatives()
                        .unwrap()
                        .map(key)
                        .collect::<HashSet<_>>(),
                    expected
                );
                // A caller replacing a fragment cannot keep an old ink proof.
                updated[0] = retained.fragments[0].clone();
                assert!(updated.ink_representatives().is_none());
            }
        }
    }

    #[test]
    fn reused_suffix_fragments_keep_the_destination_links() {
        let text = "prefix same\nprefix same";
        let links = text
            .match_indices("same")
            .enumerate()
            .map(|(index, (start, word))| StyledLink {
                start,
                end: start + word.len(),
                href: format!("https://example.invalid/{index}").into(),
            })
            .collect();
        let document = StyledText::from_parts(text, Vec::new(), links).unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        let advance = |text: &str, _: TextStyle| text.len() as f32 * 8.0;
        let size = Size::new(65.0, f32::INFINITY);
        let (reference, _) = StyledTextLayout::indexed(&node, &document, size, 12.0, advance);
        let mut measurements = StyledTextMeasurements::new(document);
        let (retained, _) = StyledTextLayout::indexed_with_measurements(
            &node,
            size,
            12.0,
            &mut measurements,
            &Cell::new(false),
            advance,
        );
        assert!(!measurements.shared_widths.templates.is_empty());
        assert_eq!(retained, reference);
        assert_eq!(
            retained
                .fragments
                .iter()
                .filter_map(StyledFragment::link)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn selection_spans_reuse_geometry_and_charge_shared_shapes_once() {
        let document = StyledText::from_plain_text("same\nsame");
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        let mut spans = StyledSelectionSpans::new(document.clone());
        let plan = |width| {
            StyledTextLayout::indexed(
                &node,
                &document,
                Size::new(width, f32::INFINITY),
                12.0,
                |text, _| text.len() as f32 * 5.0,
            )
            .0
        };
        let mut initial = plan(90.0);
        let mut resized = plan(60.0);
        let mut calls = 0;
        let mut shape = |text: &str, _| {
            calls += 1;
            Arc::new(VisualTextLine::new(
                text,
                text.char_indices().map(|(index, _)| (index, 5.0)),
            ))
        };
        let before_bytes =
            initial.shape_selection_recording_by_span(0, &mut spans, &mut shape, |_, _| {});
        let after_bytes =
            resized.shape_selection_recording_by_span(0, &mut spans, &mut shape, |_, _| {});
        assert_eq!(calls, 1);
        assert_eq!(before_bytes, after_bytes);
        let visual = initial.fragments[0].visual.as_ref().unwrap();
        assert_eq!(before_bytes, visual.allocated_bytes());
        for fragment in &resized.fragments {
            assert!(Arc::ptr_eq(fragment.visual.as_ref().unwrap(), visual));
        }
    }

    #[test]
    fn selection_spans_reuse_recent_geometry_after_a_wrap_change() {
        let document = StyledText::from_plain_text("abcdef");
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        node.properties
            .insert("wrap", xrack_runtime::TextWrap::CharWrap.into());
        let mut spans = StyledSelectionSpans::new(document.clone());
        let mut shaped = 0;
        let mut plan = |width| {
            let mut layout = StyledTextLayout::new(
                &node,
                &document,
                Size::new(width, f32::INFINITY),
                12.0,
                |text, _| text.len() as f32 * 5.0,
            );
            layout.shape_selection_recording_by_span(
                0,
                &mut spans,
                |text, _| {
                    shaped += 1;
                    Arc::new(VisualTextLine::new(
                        text,
                        text.char_indices().map(|(index, _)| (index, 5.0)),
                    ))
                },
                |_, _| {},
            );
            layout
        };
        let wide = plan(30.0);
        let narrow = plan(16.0);
        let resized = plan(17.0);

        assert_eq!(wide.fragments[0].source_span().0, Some(0..6));
        assert_eq!(narrow.fragments[0].source_span().0, Some(0..3));
        assert_eq!(narrow, resized);
        assert_eq!(shaped, 3);
        assert!(Arc::ptr_eq(
            narrow.fragments[0].visual.as_ref().unwrap(),
            resized.fragments[0].visual.as_ref().unwrap()
        ));
    }

    #[test]
    fn compact_line_keys_preserve_offsets_and_ellipsis_without_truncation() {
        let limit = (u32::MAX >> 1) as usize;
        let full = LineKey::new(&(limit - 2..limit), false).unwrap();
        assert!(full != LineKey::new(&(limit - 2..limit), true).unwrap());
        assert!(full != LineKey::new(&(limit - 1..limit), false).unwrap());
        assert!(full != LineKey::new(&(limit - 2..limit - 1), false).unwrap());
        assert!(LineKey::new(&(limit..limit + 1), false).is_none());
    }

    #[test]
    fn single_line_shortcut_requires_all_observed_wrap_probes_to_fit() {
        let document = StyledText::from_plain_text("abcd");
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        node.properties
            .insert("wrap", xrack_runtime::TextWrap::CharWrap.into());
        let mut measurements = StyledTextMeasurements::new(document.clone());
        let invalidated = Cell::new(false);
        let measure = |text: &str, _: TextStyle| if text.len() == 1 { 100.0 } else { 20.0 };
        for width in [130.0, 170.0, 70.0] {
            let available = Size::new(width, f32::INFINITY);
            let (reference, _) =
                StyledTextLayout::indexed(&node, &document, available, 10.0, measure);
            let (retained, _) = StyledTextLayout::indexed_with_measurements(
                &node,
                available,
                10.0,
                &mut measurements,
                &invalidated,
                measure,
            );
            assert_eq!(retained, reference);
            assert_eq!(retained.fragments.len(), if width < 100.0 { 4 } else { 1 });
        }
    }

    #[test]
    fn retained_wrap_probes_match_fresh_layout_after_resize_and_font_change() {
        let document = StyledText::from_markdown(
            "**Build** succeeded · \u{2067}אבג 42\u{2069} · [log](log.txt) · `cargo test`",
        )
        .unwrap();
        let mut node = Node::new("StyledText", None);
        node.properties.insert("text", document.clone().into());
        node.properties.insert(
            "horizontal-alignment",
            TextHorizontalAlignment::Center.into(),
        );
        let mut measurements = StyledTextMeasurements::new(document.clone());
        let invalidated = Cell::new(false);
        let mut reused = false;
        for (width, font_size) in [(180.0, 10.0), (210.0, 10.0), (180.0, 20.0)] {
            let available = Size::new(width, f32::INFINITY);
            let measure = |text: &str, style: TextStyle| {
                text.chars().count() as f32 * font_size * if style.bold { 0.7 } else { 0.5 }
            };
            let mut reference_calls = 0;
            let (reference, _) =
                StyledTextLayout::indexed(&node, &document, available, font_size, |text, style| {
                    reference_calls += 1;
                    measure(text, style)
                });
            invalidated.set(font_size == 20.0);
            let mut retained_calls = 0;
            let (retained, _) = StyledTextLayout::indexed_with_measurements(
                &node,
                available,
                font_size,
                &mut measurements,
                &invalidated,
                |text, style| {
                    retained_calls += 1;
                    measure(text, style)
                },
            );
            assert_eq!(retained, reference);
            reused |= font_size == 10.0 && retained_calls < reference_calls;
        }
        assert!(reused);
    }

    #[test]
    fn a_probe_that_registers_fonts_invalidates_all_cached_widths() {
        let mut widths = RetainedWidths::default();
        let bidi = StyledBidi::new("hello");
        let invalidated = Cell::new(false);
        let mut lines = RetainedLines::default();
        let mut shared_widths = SharedWidths::default();
        shared_widths.remember(
            &SharedLineProbe {
                tail: 0..1,
                hash: 0,
                leading: Some(1.0_f32.to_bits()),
            },
            1.0,
        );
        shared_widths.leading_widths.push(1.0);
        let mut retained = RetainedMeasurements {
            bidi: &bidi,
            prepared: None,
            spans: None,
            widths: &mut widths,
            lines: &mut lines,
            shared_widths: &mut shared_widths,
            shared_hit: false,
            invalidated: &invalidated,
        };
        let first = LineKey::new(&(0..1), false).unwrap();
        let second = LineKey::new(&(0..4), false).unwrap();
        assert_eq!(
            retained.measure("hello", &(0..1), false, Some(first), || 1.0),
            1.0
        );
        retained.lines.insert(
            first,
            &[StyledFragment::from_piece(
                Piece {
                    range: 0..1,
                    rtl_mark: false,
                    style: TextStyle::default(),
                    link: None,
                },
                Rect::new(0.0, 0.0, 1.0, 1.0),
            )],
        );
        assert_eq!(
            retained.measure("hello", &(0..4), false, Some(second), || {
                invalidated.set(true);
                4.0
            }),
            4.0
        );
        assert!(retained.lines.get(&first).is_none());
        assert!(retained.shared_widths.leading_widths[0].is_nan());
        assert!(
            retained
                .shared_widths
                .slots
                .iter()
                .all(|slot| slot.shape.start == slot.shape.end)
        );
        assert_eq!(
            retained.measure("hello", &(0..1), false, Some(first), || 2.0),
            2.0
        );
        assert_eq!(
            retained.measure("hello", &(0..4), false, Some(second), || 5.0),
            5.0
        );
    }

    #[test]
    fn repeated_paragraph_bidi_matches_whole_document_analysis() {
        use std::fmt::Write as _;

        let mut text = String::from("preamble 👩‍🚀\r\n");
        for index in 0..400 {
            write!(
                text,
                "\u{2068}(构建 {index:04} שלום)\u{2069} · é\u{0085}سلام 42\r\n\n"
            )
            .unwrap();
            if index % 20 == 0 {
                writeln!(text, "distinct LTR paragraph {index} 🧪").unwrap();
            }
        }
        text.push_str("trailing paragraph without a separator");
        let expected = BidiInfo::new(&text, None);
        let actual = StyledBidi::new(&text);
        assert_eq!(actual.classes, expected.original_classes);
        assert_eq!(actual.levels, expected.levels);
        let ranges = |paragraphs: Vec<ParagraphInfo>| {
            paragraphs
                .into_iter()
                .map(|paragraph| (paragraph.range, paragraph.level))
                .collect::<Vec<_>>()
        };
        assert_eq!(ranges(actual.paragraphs), ranges(expected.paragraphs));
    }

    #[test]
    fn prepared_bidi_runs_match_unicode_bidi_for_all_slice_boundaries() {
        let samples = [
            "English (אב 42) ג and 123  שלום  ",
            "اب 123 (12.3) a b שלום  مرحبا \u{2002} ",
            "start अ́ 🧑‍🔧 שלום 77\u{a0} tail  ",
            "س\tאב (45)\u{2067} isolated\u{2069}\nabc  \u{202e}explicit\u{202c} text  ",
            "English (مرحبا+12) אב 99\u{2028} more שלום\u{2029} tail אב  ",
        ];
        let mut runs = Vec::new();
        let mut compared = 0;
        let mut excluded = 0;
        for text in samples {
            let document = StyledText::from_plain_text(text);
            let text = document.text();
            let original = BidiInfo::new(text, None);
            let bidi = StyledBidi::new(text);
            let spans = PreparedSpans::new(&document, &bidi).unwrap();
            for (number, paragraph) in original.paragraphs.iter().enumerate() {
                let start = paragraph.range.start;
                let offsets: Vec<_> = text[paragraph.range.clone()]
                    .char_indices()
                    .map(|(offset, _)| start + offset)
                    .chain([paragraph.range.end])
                    .collect();
                for (index, &start) in offsets.iter().enumerate() {
                    for &end in &offsets[index + 1..] {
                        if end > spans.l1_limits[number] as usize {
                            excluded += 1;
                            continue;
                        }
                        let line = start..end;
                        simple_visual_runs(&bidi, &spans, paragraph.level, line.clone(), &mut runs);
                        let (levels, reference) = original.visual_runs(paragraph, line.clone());
                        assert!(
                            runs.iter().cloned().eq(reference.into_iter().map(|range| {
                                let level = levels[range.start];
                                (range, level)
                            })),
                            "{text:?}, {line:?}"
                        );
                        compared += 1;
                    }
                }
            }
        }
        assert!(compared > 500);
        assert!(excluded > 0);
    }

    #[test]
    fn bidi_line_slices_match_paragraph_runs_at_wrap_boundaries() {
        let text = format!("{}\nس (42)\tאב ABC \u{2067}ب 77\u{2069} س", "x".repeat(57));
        let document = StyledText::from_plain_text(text);
        let bidi = StyledBidi::new(document.text());
        let spans = PreparedSpans::new(&document, &bidi).unwrap();
        let paragraph = &bidi.paragraphs[1];
        let offset = paragraph.range.start;
        let text = &document.text()[paragraph.range.clone()];
        let reference = ParagraphBidiInfo::new(text, Some(paragraph.level));
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain([text.len()])
            .collect();
        let mut line_bidi = LineBidi::new();
        let mut pieces = Vec::new();
        for (i, &start) in boundaries.iter().enumerate() {
            for &end in &boundaries[i + 1..] {
                let (levels, runs) = reference.visual_runs(start..end);
                collect_pieces(
                    &document,
                    &bidi,
                    Some(&spans),
                    &mut line_bidi,
                    offset + start..offset + end,
                    false,
                    &mut pieces,
                );
                assert!(
                    pieces
                        .iter()
                        .map(|piece| (piece.range.clone(), piece.rtl_mark))
                        .eq(runs.into_iter().map(|run| {
                            let mark = levels[run.start].is_rtl()
                                && get_base_direction(&text[run.clone()]) != Direction::Rtl;
                            (offset + run.start..offset + run.end, mark)
                        })),
                    "wrapped substring {start}..{end}"
                );
            }
        }
    }

    #[test]
    fn split_run_advances_past_overlapping_styles_and_links() {
        let plain = TextStyle::default();
        let bold = TextStyle {
            bold: true,
            ..plain
        };
        let italic = TextStyle {
            italic: true,
            ..plain
        };
        let document = StyledText::from_parts(
            "abcdefghij",
            vec![
                StyledRun {
                    start: 1,
                    end: 4,
                    style: bold,
                },
                StyledRun {
                    start: 6,
                    end: 8,
                    style: italic,
                },
            ],
            [(0, 3), (5, 7), (8, 10)]
                .into_iter()
                .map(|(start, end)| StyledLink {
                    start,
                    end,
                    href: "example".into(),
                })
                .collect(),
        )
        .unwrap();
        let mut pieces = Vec::new();
        let bidi = StyledBidi::new(document.text());
        let spans = PreparedSpans::new(&document, &bidi).unwrap();
        split_run(&document, 2..9, false, Some(&spans), &mut pieces);
        assert_eq!(
            pieces
                .into_iter()
                .map(|piece| (piece.range, piece.style, piece.link))
                .collect::<Vec<_>>(),
            [
                (2..3, bold, Some(0)),
                (3..4, bold, None),
                (4..5, plain, None),
                (5..6, plain, Some(1)),
                (6..7, italic, Some(1)),
                (7..8, italic, None),
                (8..9, plain, Some(2)),
            ]
        );
    }

    #[test]
    fn fragment_metadata_preserves_every_style_flag_and_first_link() {
        for flags in 0..64 {
            let style = TextStyle {
                bold: flags & 1 != 0,
                italic: flags & 2 != 0,
                monospace: flags & 4 != 0,
                strikethrough: flags & 8 != 0,
                underline: flags & 16 != 0,
                color: (flags & 32 != 0).then_some(Color(0)),
            };
            for rtl in [false, true] {
                let fragment = StyledFragment::from_piece(
                    Piece {
                        range: 10..22,
                        rtl_mark: rtl,
                        style,
                        link: Some(0),
                    },
                    Rect::default(),
                );
                assert_eq!(fragment.style(), style);
                assert_eq!(fragment.source_span(), (Some(10..22), rtl));
                assert_eq!(fragment.link(), Some(0));
            }
        }
    }
}
