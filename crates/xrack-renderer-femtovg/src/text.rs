use femtovg::{
    Align, Baseline, Canvas, Color, ErrorKind, FillRule, FontId, Paint, Path, TextContext,
    TextDecoration, TextMetrics, renderer::WGPURenderer,
};
use fontdb::{Database, Family, Query, Style, Weight};
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, VecDeque},
    hash::BuildHasher,
    num::NonZeroUsize,
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};
use xrack_layout::{
    LabelMeasurements, LabelPlan, LabelStyle, Point, Rect, Size, StyledTextLayout, TextAlignment,
    TextAreaLayout, TextMeasurer, VisualTextLine, text_editor_viewport,
};
use xrack_runtime::{
    EmbeddedFonts, LayoutDirection, Node, SharedNodeIdentity, TextStyle, TextWrap,
};

pub(super) use cell_shaping::AsciiMetrics;
use unicode_bidi::{BidiClass, bidi_class};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(target_os = "android")]
use crate::{AndroidGlyphBitmap, android_emoji::Emoji};
#[cfg(target_os = "ios")]
use crate::{color_glyphs::ColorGlyphs, ios_font::NativeFont};

pub(crate) fn fill_selected_text(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    clip: Rect,
    origin: Point,
    text: &str,
    paint: &Paint,
    spans: &[Range<f32>],
    selected_color: Option<Color>,
) -> Result<(), ErrorKind> {
    fill_with_selection(
        canvas,
        clip,
        origin.x,
        paint,
        spans,
        selected_color,
        |canvas, paint| {
            text_system
                .fill_text(canvas, origin.x, origin.y, text, paint)
                .map(|_| ())
        },
    )
}

pub(crate) fn fill_with_selection(
    canvas: &mut Canvas<WGPURenderer>,
    clip: Rect,
    origin_x: f32,
    paint: &Paint,
    spans: &[Range<f32>],
    selected_color: Option<Color>,
    mut draw: impl FnMut(&mut Canvas<WGPURenderer>, &Paint) -> Result<(), ErrorKind>,
) -> Result<(), ErrorKind> {
    let Some(selected_color) = selected_color.filter(|_| !spans.is_empty()) else {
        return draw(canvas, paint);
    };

    // Draw the complete shaped line through disjoint clips. Splitting the
    // string here would reshape ligatures and could reorder bidi fragments.
    let mut normal = Path::new();
    let mut selected = Path::new();
    let right = clip.x + clip.width;
    let mut left = clip.x;
    for span in spans {
        let start = (origin_x + span.start).max(left);
        let end = (origin_x + span.end).min(right);
        if end <= start {
            continue;
        }
        if start > left {
            normal.rect(left, clip.y, start - left, clip.height);
        }
        selected.rect(start, clip.y, end - start, clip.height);
        left = end;
    }
    if selected.is_empty() {
        return draw(canvas, paint);
    }
    if left < right {
        normal.rect(left, clip.y, right - left, clip.height);
    }
    let mut fill = |path: &Path, paint: &Paint| {
        canvas.save();
        canvas.clip_path(path, FillRule::NonZero);
        let result = draw(canvas, paint);
        canvas.restore();
        result
    };
    if !normal.is_empty() {
        fill(&normal, paint)?;
    }
    if selected_color.a > 0.0 {
        fill(&selected, &paint.clone().with_color(selected_color))?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FontRequest {
    family: String,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct StyledFace {
    id: Option<FontId>,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct LabelFont {
    size: f32,
    face: Option<StyledFace>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct LabelSettings {
    available: [u32; 2],
    style: LabelStyle,
    font_size: u32,
    face: Option<StyledFace>,
    registered_fonts: [usize; 2],
}

struct CachedLabel {
    settings: LabelSettings,
    label: Rc<PreparedLabel>,
    reusable_widths: Option<Range<f32>>,
    bytes: usize,
}

impl Drop for CachedLabel {
    fn drop(&mut self) {
        if let Some(source) = &self.label.source {
            source.cached_plans.set(source.cached_plans.get() - 1);
        }
    }
}

struct LabelSource {
    text: Box<str>,
    fingerprint: u64,
    cached_plans: Cell<usize>,
}

impl LabelSource {
    fn bytes(&self) -> usize {
        size_of::<Self>() + self.text.len()
    }
}

struct ReusableLabel {
    widths: Range<f32>,
    settings: LabelSettings,
    label: Weak<PreparedLabel>,
}

struct LabelSnapshot {
    // An admitted plain label keeps its own color, if any. Resolve the theme
    // and enabled state from the current frame.
    plain_tint: Cell<Option<Option<xrack_runtime::Color>>>,
    node: SharedNodeIdentity,
    available: Size,
    direction: Option<LayoutDirection>,
    registered_fonts: [usize; 2],
    label: Weak<PreparedLabel>,
}

#[derive(Default)]
struct LabelSnapshotSlot {
    seen: usize,
    prepared: Option<LabelSnapshot>,
}

enum PreparedLineText {
    Source(Range<u32>),
    Owned(usize),
}

struct PreparedLabelLine {
    origin: Point,
    text: PreparedLineText,
}

pub(crate) struct PreparedLabel {
    size: Size,
    lines: Vec<PreparedLabelLine>,
    owned_lines: Vec<String>,
    source: Option<Rc<LabelSource>>,
    pub(super) font: LabelFont,
    paint: Paint,
    glyphs: RefCell<Option<(f32, crate::cell_glyphs::Run)>>,
}

impl PreparedLabel {
    const MAX_CACHED_TEXT_BYTES: usize = 64;

    pub(super) fn source_text(&self) -> Option<&str> {
        self.source.as_ref().map(|source| source.text.as_ref())
    }

    pub(super) fn shares_text_with(&self, other: &Self) -> bool {
        self.source
            .as_ref()
            .zip(other.source.as_ref())
            .is_some_and(|(this, other)| Rc::ptr_eq(this, other) || this.text == other.text)
    }

    fn line_text(&self, line: &PreparedLabelLine) -> &str {
        match &line.text {
            PreparedLineText::Source(range) => self
                .source_text()
                .and_then(|source| source.get(range.start as usize..range.end as usize))
                .unwrap_or(""),
            PreparedLineText::Owned(index) => &self.owned_lines[*index],
        }
    }

    pub(super) fn lines(&self) -> impl ExactSizeIterator<Item = (Point, &str)> {
        self.lines
            .iter()
            .map(|line| (line.origin, self.line_text(line)))
    }

    pub(super) fn only_line(&self) -> Option<(Point, &str)> {
        let [line] = self.lines.as_slice() else {
            return None;
        };
        Some((line.origin, self.line_text(line)))
    }

    pub(crate) fn paint(&self, tint: Color) -> Paint {
        self.paint.clone().with_color(tint)
    }

    pub(super) fn visit_glyphs(
        &self,
        context: &TextContext,
        origin: Point,
        scale: f32,
        mut visit: impl FnMut(FontId, u16, f32, f32) -> bool,
    ) -> Option<bool> {
        let (_, text) = self.only_line()?;
        if text.len() > Self::MAX_CACHED_TEXT_BYTES || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let mut cached = self.glyphs.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|(measured_scale, _)| *measured_scale != scale)
        {
            let paint = self
                .paint
                .clone()
                .with_font_size(self.paint.font_size() * scale);
            let metrics = context.measure_text(0.0, 0.0, text, &paint).ok()?;
            if metrics.glyphs.len() > text.len() {
                return None;
            }
            *cached = Some((scale, crate::cell_glyphs::Run::from_metrics(&metrics)));
        }
        let (_, run) = cached.as_ref()?;
        Some(
            crate::cell_glyphs::placed_glyphs(
                run.glyphs.iter().copied(),
                run.baseline,
                [origin.x, origin.y],
                scale,
            )
            .all(|(font, glyph, x, y)| visit(font, glyph, x, y)),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LabelAdvanceFont {
    face: Option<StyledFace>,
    size: u32,
    registered_fonts: [usize; 2],
}

#[derive(Default)]
struct LabelAdvances {
    font: Option<LabelAdvanceFont>,
    widths: HashMap<Box<str>, f32>,
    token_widths: HashMap<Box<str>, f32>,
    word_advances: Option<LabelWordAdvances>,
    bytes: usize,
    unretained: u16,
}

impl LabelAdvances {
    const CACHE_PRESSURE: u16 = 1024;
    const MAX_TOKEN_BYTES: usize = 48;

    fn select(&mut self, font: LabelAdvanceFont) {
        if self.font != Some(font) {
            self.font = Some(font);
            self.widths.clear();
            self.token_widths.clear();
            self.word_advances = None;
            self.bytes = 0;
            self.unretained = 0;
        }
    }

    fn retain_word_advances(&mut self, source: &str) -> bool {
        if self.unretained < Self::CACHE_PRESSURE {
            return false;
        }
        if self
            .word_advances
            .as_ref()
            .is_none_or(|index| index.source.as_ref() != source)
        {
            self.word_advances = Some(LabelWordAdvances::new(source));
        }
        true
    }

    fn measure(
        &mut self,
        text: &str,
        source: Option<&str>,
        context: &TextContext,
        measure: impl Fn(&str) -> f32,
    ) -> f32 {
        let indexed = source
            .filter(|_| text.len() >= 16)
            .zip(self.word_advances.as_mut());
        // Varied ASCII ranges rarely share a cache entry across paragraphs.
        // Use their source offsets first. Keep whole-string hits first for
        // other text, where the same phrases can recur many times.
        let index_first = indexed.is_some() && text.is_ascii();
        if !index_first && let Some(&width) = self.widths.get(text) {
            return width;
        }
        if let Some((source, index)) = indexed
            && let Some(width) = index.measure(source, text, |word| {
                self.widths
                    .get(word)
                    .copied()
                    .unwrap_or_else(|| Self::measure_token(&mut self.token_widths, word, &measure))
            })
        {
            return if index_first {
                width
            } else {
                self.remember(text, width, context)
            };
        }
        if index_first && let Some(&width) = self.widths.get(text) {
            return width;
        }
        let under_pressure = self.unretained >= Self::CACHE_PRESSURE;
        if text.len() >= 16
            && (under_pressure || cfg!(not(xrack_femtovg_glyph_samples)) || !text.is_ascii())
        {
            let width = if let Some(words) = ascii_word_bounds(text) {
                self.measure_words(words, under_pressure, context, &measure)
            } else if single_ltr_shaping_run(text) {
                self.measure_words(text.split_word_bounds(), under_pressure, context, &measure)
            } else {
                None
            };
            if let Some(width) = width {
                return self.remember(text, width, context);
            }
        }
        if under_pressure && text.len() <= 16 {
            return Self::measure_token(&mut self.token_widths, text, &measure);
        }
        self.remember(text, measure(text), context)
    }

    fn measure_words<'a>(
        &mut self,
        words: impl Iterator<Item = &'a str>,
        under_pressure: bool,
        context: &TextContext,
        measure: &impl Fn(&str) -> f32,
    ) -> Option<f32> {
        let mut words = words.peekable();
        let first = words.next()?;
        words.peek()?;
        // FemtoVG shapes words independently within a single LTR run.
        // Preserve its original addition order.
        Some(
            std::iter::once(first)
                .chain(words)
                .fold(0.0, |width, word| {
                    width
                        + match self.widths.get(word) {
                            Some(&advance) => advance,
                            None if under_pressure => {
                                Self::measure_token(&mut self.token_widths, word, measure)
                            }
                            None => self.remember(word, measure(word), context),
                        }
                }),
        )
    }

    fn measure_token(
        token_widths: &mut HashMap<Box<str>, f32>,
        text: &str,
        measure: &impl Fn(&str) -> f32,
    ) -> f32 {
        if let Some(&width) = token_widths.get(text) {
            return width;
        }
        let width = measure(text);
        // Whole probe strings can fill the general cache. Keep the words
        // and short probes that still recur in otherwise new ranges.
        if text.len() <= Self::MAX_TOKEN_BYTES && token_widths.len() < 16_384 {
            token_widths.insert(text.into(), width);
        }
        width
    }

    fn remember(&mut self, text: &str, width: f32, context: &TextContext) -> f32 {
        const MAX_ADVANCES: usize = 8192;
        const MAX_TEXT_BYTES: usize = 512 * 1024;
        if self.widths.len() < MAX_ADVANCES && text.len() <= MAX_TEXT_BYTES - self.bytes {
            self.widths.insert(text.into(), width);
            self.bytes += text.len();
        } else {
            self.unretained = self.unretained.saturating_add(1);
            if self.unretained == Self::CACHE_PRESSURE {
                // Reflow probes exceed the advance cache on long, varied text.
                // Keep frequently shaped words available across those probes.
                reserve_reflow_words(context);
            }
        }
        width
    }
}

pub(super) fn reserve_reflow_words(context: &TextContext) {
    context.resize_shaped_words_cache(NonZeroUsize::new(32_768).unwrap_or(NonZeroUsize::MIN));
}

struct LabelWordAdvances {
    source: Box<str>,
    index: Rc<LabelWordIndex>,
    widths: Vec<f32>,
    probe_start: Option<usize>,
    probe_prefixes: Vec<f32>,
}

impl LabelWordAdvances {
    fn new(source: &str) -> Self {
        let index = Rc::new(LabelWordIndex::new(source));
        Self {
            source: source.into(),
            widths: vec![f32::NAN; index.len()],
            index,
            probe_start: None,
            probe_prefixes: vec![0.0],
        }
    }

    fn measure(
        &mut self,
        source: &str,
        probe: &str,
        mut advance: impl FnMut(&str) -> f32,
    ) -> Option<f32> {
        let span = self.index.span(source, probe)?;
        if span.first == span.last {
            return None;
        }
        let partial_end = self.index.words[span.last].end as usize != span.end;
        let prefix_count = span.last - span.first + usize::from(!partial_end);
        if self.probe_start != Some(span.start) {
            self.probe_start = Some(span.start);
            self.probe_prefixes.truncate(1);
        }
        // Line fitting probes several ends from the same start. Retain each
        // prefix in the original f32 addition order, excluding the cut at the end.
        let probe_range = span.start..span.end;
        let first_start = self.index.word_start(span.first);
        if self.probe_prefixes.len() == 1 && span.start != first_start {
            let mut width = 0.0;
            self.index.words[span.first].visit(
                source,
                span.first,
                first_start,
                &probe_range,
                |word, _| {
                    width += advance(word);
                    Some(())
                },
            )?;
            self.probe_prefixes.push(width);
        }
        for offset in self.probe_prefixes.len() - 1..prefix_count {
            let index = span.first + offset;
            let cached = &mut self.widths[index];
            if cached.is_nan() {
                let start = self.index.word_start(index);
                let end = self.index.words[index].end as usize;
                *cached = advance(&source[start..end]);
            }
            self.probe_prefixes
                .push(self.probe_prefixes[offset] + *cached);
        }
        let mut width = self.probe_prefixes[prefix_count];
        if partial_end {
            self.index.words[span.last].visit(
                source,
                span.last,
                self.index.word_start(span.last),
                &probe_range,
                |word, _| {
                    width += advance(word);
                    Some(())
                },
            )?;
        }
        Some(width)
    }
}

pub(super) struct LabelWordIndex {
    byte_to_word: Box<[u32]>,
    words: Vec<IndexedLabelWord>,
}

struct IndexedLabelWord {
    end: u32,
    group: u32,
    may_split: bool,
}

impl IndexedLabelWord {
    fn visit(
        &self,
        source: &str,
        index: usize,
        word_start: usize,
        probe: &Range<usize>,
        mut visit: impl FnMut(&str, Option<usize>) -> Option<()>,
    ) -> Option<()> {
        let word_end = self.end as usize;
        let from = word_start.max(probe.start);
        let to = word_end.min(probe.end);
        let fragment = &source[from..to];
        if from == word_start && to == word_end {
            visit(fragment, Some(index))
        } else if self.may_split {
            // A cut through contextual punctuation can split the word.
            fragment
                .split_word_bounds()
                .try_for_each(|piece| visit(piece, None))
        } else {
            visit(fragment, None)
        }
    }
}

pub(super) struct IndexedWordSpan {
    start: usize,
    end: usize,
    first: usize,
    last: usize,
}

impl LabelWordIndex {
    pub(super) fn new(source: &str) -> Self {
        let mut byte_to_word = Vec::with_capacity(source.len());
        let mut words = Vec::new();
        let mut group = 0;
        let mut chars = source.char_indices().peekable();
        while let Some((start, first)) = chars.next() {
            let supported = indexed_word_char(first);
            while chars
                .next_if(|&(_, ch)| indexed_word_char(ch) == supported)
                .is_some()
            {}
            let end = chars.peek().map_or(source.len(), |&(offset, _)| offset);
            if !supported {
                byte_to_word.resize(end, words.len() as u32);
                words.push(IndexedLabelWord {
                    end: end as u32,
                    group: 0,
                    may_split: false,
                });
                continue;
            }
            // Each nonempty group is separated by an unsupported byte.
            // Group and word IDs cannot exceed the retained source length.
            group += 1;
            let segment = &source[start..end];
            let mut record = |word: &str| {
                let end = byte_to_word.len() + word.len();
                byte_to_word.resize(end, words.len() as u32);
                words.push(IndexedLabelWord {
                    end: end as u32,
                    group,
                    may_split: !word.bytes().all(simple_ascii),
                });
            };
            if let Some(words) = ascii_word_bounds(segment) {
                words.for_each(&mut record);
            } else {
                segment.split_word_bounds().for_each(record);
            }
        }
        Self {
            byte_to_word: byte_to_word.into_boxed_slice(),
            words,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.words.len()
    }

    fn word_start(&self, index: usize) -> usize {
        index
            .checked_sub(1)
            .map_or(0, |i| self.words[i].end as usize)
    }

    pub(super) fn span(&self, source: &str, probe: &str) -> Option<IndexedWordSpan> {
        // Borrowed probes and prepared lines are slices of the current
        // source. Owned text, such as an ellipsized line, has no source range.
        let start = probe.as_ptr().addr().checked_sub(source.as_ptr().addr())?;
        let end = start.checked_add(probe.len())?;
        if start == end {
            return None;
        }
        let first = *self.byte_to_word.get(start)? as usize;
        let last = *self.byte_to_word.get(end - 1)? as usize;
        let group = self.words[first].group;
        (group != 0 && self.words[last].group == group).then_some(IndexedWordSpan {
            start,
            end,
            first,
            last,
        })
    }

    #[cfg(xrack_femtovg_glyph_samples)]
    pub(super) fn full_words(&self, source: &str, value: &str) -> Option<Range<usize>> {
        let span = self.span(source, value)?;
        (span.start == self.word_start(span.first)
            && span.end == self.words[span.last].end as usize)
            .then_some(span.first..span.last + 1)
    }

    pub(super) fn visit(
        &self,
        source: &str,
        span: IndexedWordSpan,
        mut visit: impl FnMut(&str, Option<usize>) -> Option<()>,
    ) -> Option<()> {
        let IndexedWordSpan {
            start,
            end,
            first,
            last,
        } = span;
        let probe = start..end;
        let mut word_start = self.word_start(first);
        for (offset, word) in self.words[first..=last].iter().enumerate() {
            word.visit(source, first + offset, word_start, &probe, &mut visit)?;
            word_start = word.end as usize;
        }
        Some(())
    }
}

fn indexed_word_char(ch: char) -> bool {
    // These LTR characters have no extending marks, joiners, or regional
    // indicators. Cutting a probe can split its edge words, while complete
    // interior words retain their boundaries.
    matches!(
        ch,
        ' '..='~' | '\u{3000}'..='\u{3002}' | '\u{3008}'..='\u{3011}'
            | '\u{4e00}'..='\u{9fff}' | '\u{ff01}'..='\u{ff5e}'
    )
}

fn simple_ascii(byte: u8) -> bool {
    (b' '..=b'~').contains(&byte) && !matches!(byte, b'"' | b'\'' | b',' | b'.' | b':' | b';')
}

fn ascii_word_len(bytes: &[u8]) -> usize {
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    match bytes[0] {
        b' ' => bytes.iter().take_while(|&&byte| byte == b' ').count(),
        byte if word(byte) => bytes.iter().take_while(|&&byte| word(byte)).count(),
        _ => 1,
    }
}

pub(super) fn ascii_word_bounds(mut text: &str) -> Option<impl Iterator<Item = &str>> {
    // The contextual punctuation categories in UAX #29 need the general
    // iterator. The remaining printable ASCII forms words, space runs, or
    // single-character punctuation.
    if !text.bytes().all(simple_ascii) {
        return None;
    }
    Some(std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let (part, rest) = text.split_at(ascii_word_len(text.as_bytes()));
        text = rest;
        Some(part)
    }))
}

pub(super) fn single_ltr_shaping_run(text: &str) -> bool {
    if text.is_ascii() {
        return text.bytes().all(|byte| (b' '..=b'~').contains(&byte));
    }
    text.chars().all(|character| {
        matches!(
            bidi_class(character),
            BidiClass::L
                | BidiClass::EN
                | BidiClass::ES
                | BidiClass::ET
                | BidiClass::CS
                | BidiClass::ON
                | BidiClass::WS
                | BidiClass::NSM
        )
    })
}

#[derive(Default)]
struct LabelCache {
    advances: LabelAdvances,
    recent_wrap: Option<u64>,
    rejected_wrap: Option<u64>,
    recent_source: Weak<LabelSource>,
    measurements: Option<(LabelAdvanceFont, LabelMeasurements)>,
    reusable: VecDeque<ReusableLabel>,
    entries: HashMap<u64, CachedLabel>,
    snapshots: Vec<LabelSnapshotSlot>,
    snapshot_probes: Cell<usize>,
    snapshot_hits: Cell<usize>,
    low_hit_frames: u8,
    retry_snapshots_in: u8,
    order: VecDeque<u64>,
    bytes: usize,
}

impl LabelCache {
    const SNAPSHOT_SLOTS: usize = 4096;
    const RECENT_WIDTH_INTERVALS: usize = 16;

    fn clear(&mut self) {
        self.advances = LabelAdvances::default();
        self.recent_wrap = None;
        self.rejected_wrap = None;
        self.recent_source = Weak::new();
        self.measurements = None;
        self.reusable.clear();
        self.entries.clear();
        self.snapshots.clear();
        self.snapshot_probes.set(0);
        self.snapshot_hits.set(0);
        self.low_hit_frames = 0;
        self.retry_snapshots_in = 0;
        self.order.clear();
        self.bytes = 0;
    }

    fn begin_snapshot_frame(&mut self) -> bool {
        if self.retry_snapshots_in > 0 {
            self.retry_snapshots_in -= 1;
            return self.retry_snapshots_in == 0;
        }
        let probes = self.snapshot_probes.replace(0);
        let hits = self.snapshot_hits.replace(0);
        // Allow initial admission and warming. A workload that keeps
        // rebuilding most labels should spend its time in the text cache.
        if probes >= 64 && hits < probes.div_ceil(8) {
            self.low_hit_frames += 1;
        } else {
            self.low_hit_frames = 0;
        }
        if self.low_hit_frames < 3 {
            return true;
        }
        self.low_hit_frames = 0;
        self.retry_snapshots_in = 64;
        self.snapshots.clear();
        false
    }

    fn snapshot_key(node: &Node) -> (usize, usize) {
        let address = std::ptr::from_ref(&**node).addr();
        let slot = ((address >> 4) ^ (address >> 16)) % Self::SNAPSHOT_SLOTS;
        (address, slot)
    }

    fn snapshot(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
        registered_fonts: impl FnOnce() -> [usize; 2],
    ) -> Option<Rc<PreparedLabel>> {
        self.snapshot_probes.set(self.snapshot_probes.get() + 1);
        let (_, slot) = Self::snapshot_key(node);
        let entry = self.snapshots.get(slot)?.prepared.as_ref()?;
        if !entry.node.matches(node) || entry.available != available || entry.direction != direction
        {
            return None;
        }
        // The text/layout cache still owns the plan and its memory budget.
        let label = entry.label.upgrade()?;
        if entry.registered_fonts != registered_fonts() {
            return None;
        }
        self.snapshot_hits.set(self.snapshot_hits.get() + 1);
        Some(label)
    }

    fn remember_snapshot(
        &mut self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
        registered_fonts: [usize; 2],
        label: &Rc<PreparedLabel>,
    ) {
        let (address, index) = Self::snapshot_key(node);
        if self.snapshots.is_empty() {
            self.snapshots
                .resize_with(Self::SNAPSHOT_SLOTS, LabelSnapshotSlot::default);
        }
        let slot = &mut self.snapshots[index];
        // A first sighting alone should not retain weak references to a
        // freshly rebuilt node or its label plan.
        if std::mem::replace(&mut slot.seen, address) != address {
            return;
        }
        slot.prepared = Some(LabelSnapshot {
            plain_tint: Cell::new(None),
            node: node.snapshot_identity(),
            available,
            direction,
            registered_fonts,
            label: Rc::downgrade(label),
        });
    }

    fn get(
        &mut self,
        text: &str,
        settings: LabelSettings,
        build: impl FnOnce(&mut LabelAdvances, Option<&LabelMeasurements>) -> (LabelPlan, Paint),
    ) -> Rc<PreparedLabel> {
        const MAX_ENTRIES: usize = 2048;
        const MAX_BYTES: usize = LabelMeasurements::MAX_SOURCE_BYTES;

        let shared_settings = LabelSettings {
            available: [0, settings.available[1]],
            ..settings
        };
        let width = f32::from_bits(settings.available[0]);
        let reusable = self.reusable.iter().enumerate().find_map(|(index, entry)| {
            if entry.settings != shared_settings || !entry.widths.contains(&width) {
                return None;
            }
            let label = entry.label.upgrade()?;
            (label.source_text() == Some(text)).then_some((index, label))
        });
        if let Some((index, label)) = reusable {
            if index != 0
                && let Some(entry) = self.reusable.remove(index)
            {
                self.reusable.push_front(entry);
            }
            return label;
        }
        let reflow_document = (1024..=MAX_BYTES).contains(&text.len()) && settings.style.reflows();
        let shared_source = if reflow_document {
            self.recent_source
                .upgrade()
                .filter(|source| source.text.as_ref() == text)
        } else {
            None
        };
        // Resize frames change the settings while retaining the same bytes.
        // Once their source matches, use its existing fingerprint for both
        // the layout cache key and the admission of retained measurements.
        let fingerprint = reflow_document.then(|| {
            shared_source.as_ref().map_or_else(
                || self.entries.hasher().hash_one(text),
                |source| source.fingerprint,
            )
        });
        let key = match fingerprint {
            Some(fingerprint) => self.entries.hasher().hash_one((fingerprint, settings)),
            None => self.entries.hasher().hash_one((text, settings)),
        };
        // Compare the full inputs on lookup, including collisions of the hash.
        if let Some(entry) = self.entries.get(&key)
            && entry.settings == settings
            && (shared_source
                .as_ref()
                .zip(entry.label.source.as_ref())
                .is_some_and(|(current, cached)| Rc::ptr_eq(current, cached))
                || entry.label.source_text() == Some(text))
        {
            let label = entry.label.clone();
            if let Some(widths) = entry.reusable_widths.clone() {
                self.remember_reusable(widths, shared_settings, &label);
            }
            return label;
        }
        let mut measurements = None;
        if let Some(source) = fingerprint {
            let repeated = self.recent_wrap.replace(source) == Some(source);
            let font = LabelAdvanceFont {
                face: settings.face,
                size: settings.font_size,
                registered_fonts: settings.registered_fonts,
            };
            // Admit a document that reflows more than once. Consecutive edits
            // to different documents keep the existing small layout path.
            // Remember a document exceeding the preparation budget so the
            // next width change does not repeat its boundary scan.
            let mut matches = self
                .measurements
                .as_ref()
                .is_some_and(|(_, cached)| cached.matches(text, settings.style));
            if repeated && self.rejected_wrap != Some(source) && !matches {
                self.measurements = LabelMeasurements::new(text, settings.style)
                    .map(|measurements| (font, measurements));
                matches = self.measurements.is_some();
                if !matches {
                    self.rejected_wrap = Some(source);
                }
                self.reusable.clear();
            }
            if matches && let Some((previous_font, cached)) = &mut self.measurements {
                if *previous_font != font {
                    cached.clear_widths();
                    *previous_font = font;
                    self.reusable.clear();
                }
                measurements = Some(&*cached);
            }
        }
        let (plan, paint) = build(&mut self.advances, measurements);
        let reusable_widths = measurements.and_then(LabelMeasurements::reusable_widths);
        let source_cost = text.len().saturating_add(size_of::<LabelSource>());
        let line_cost = plan
            .lines
            .len()
            .saturating_mul(size_of::<PreparedLabelLine>());
        let source = (source_cost.saturating_add(line_cost) <= MAX_BYTES).then(|| {
            shared_source.unwrap_or_else(|| {
                Rc::new(LabelSource {
                    text: text.into(),
                    fingerprint: fingerprint.unwrap_or_default(),
                    cached_plans: Cell::new(0),
                })
            })
        });
        let mut owned_lines = Vec::new();
        let lines: Vec<_> = plan
            .lines
            .into_iter()
            .map(|line| {
                let content = if source.is_some() && !line.ellipsis {
                    PreparedLineText::Source(line.range.start as u32..line.range.end as u32)
                } else {
                    let index = owned_lines.len();
                    owned_lines.push(line.text(text).into_owned());
                    PreparedLineText::Owned(index)
                };
                PreparedLabelLine {
                    origin: line.origin,
                    text: content,
                }
            })
            .collect();
        let label = Rc::new(PreparedLabel {
            size: plan.size,
            lines,
            owned_lines,
            source,
            font: LabelFont {
                size: f32::from_bits(settings.font_size),
                face: settings.face,
            },
            paint,
            glyphs: RefCell::new(None),
        });
        let Some(source) = &label.source else {
            return label;
        };
        let glyph_bytes = label.only_line().map_or(0, |(_, text)| {
            // Reserve the short line's glyph allocation in the cache budget.
            if text.len() <= PreparedLabel::MAX_CACHED_TEXT_BYTES {
                text.len() * size_of::<crate::cell_glyphs::Glyph>()
            } else {
                0
            }
        });
        let bytes = size_of::<CachedLabel>()
            .saturating_add(size_of::<PreparedLabel>())
            .saturating_add(label.lines.capacity() * size_of::<PreparedLabelLine>())
            .saturating_add(label.owned_lines.capacity() * size_of::<String>())
            .saturating_add(
                label
                    .owned_lines
                    .iter()
                    .map(String::capacity)
                    .sum::<usize>(),
            )
            .saturating_add(glyph_bytes);
        if bytes.saturating_add(source.bytes()) > MAX_BYTES {
            return label;
        }
        if self.remove_entry(key) {
            self.order.retain(|old| *old != key);
        }
        let source_charge = || {
            if source.cached_plans.get() == 0 {
                source.bytes()
            } else {
                0
            }
        };
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes + source_charge() > MAX_BYTES
        {
            let Some(old) = self.order.pop_front() else {
                return label;
            };
            self.remove_entry(old);
        }
        self.bytes += bytes + source_charge();
        source.cached_plans.set(source.cached_plans.get() + 1);
        if reflow_document {
            self.recent_source = Rc::downgrade(source);
        }
        self.entries.insert(
            key,
            CachedLabel {
                settings,
                label: label.clone(),
                reusable_widths: reusable_widths.clone(),
                bytes,
            },
        );
        self.order.push_back(key);
        if let Some(widths) = reusable_widths {
            self.remember_reusable(widths, shared_settings, &label);
        }
        label
    }

    fn remember_reusable(
        &mut self,
        widths: Range<f32>,
        settings: LabelSettings,
        label: &Rc<PreparedLabel>,
    ) {
        // Width animations can return to an earlier set of line breaks. The
        // normal layout cache still owns the plans and their memory budget.
        let label = Rc::downgrade(label);
        self.reusable
            .retain(|entry| entry.label.strong_count() != 0 && !entry.label.ptr_eq(&label));
        self.reusable.truncate(Self::RECENT_WIDTH_INTERVALS - 1);
        self.reusable.push_front(ReusableLabel {
            widths,
            settings,
            label,
        });
    }

    fn remove_entry(&mut self, key: u64) -> bool {
        let Some(entry) = self.entries.remove(&key) else {
            return false;
        };
        self.bytes -= entry.bytes;
        if let Some(source) = &entry.label.source
            && source.cached_plans.get() == 1
        {
            self.bytes -= source.bytes();
        }
        true
    }
}

struct FontCatalog {
    db: Database,
    primary: String,
    loaded: HashMap<fontdb::ID, Option<FontId>>,
    // Glyph bounds use the same bytes as the registered font.
    outline_faces: HashMap<FontId, FontSource>,
    matches: HashMap<FontRequest, Option<FontId>>,
}

#[derive(Clone, Copy)]
pub(crate) struct OutlineGlyph {
    pub(crate) bounds: Option<ttf_parser::Rect>,
    pub(crate) units_per_em: u16,
}

#[derive(Clone, Copy)]
pub(super) struct GlyphExtents {
    pub(super) min_bearing: [f32; 2],
    pub(super) max_bearing: [f32; 2],
    pub(super) size: [f32; 2],
}

pub(super) fn atlas_size_range(size: f32, scale: f32) -> [f32; 2] {
    // A previous paint in the same FemtoVG size bin may have produced
    // slightly different rounded bearings and texel extents.
    let size_bin = (size * scale * 10.0).trunc();
    [
        (size_bin / 10.0 - 0.0001).max(0.0),
        (size_bin + 1.0) / 10.0 + 0.0001,
    ]
}

impl OutlineGlyph {
    pub(super) fn atlas_extents(self, size_range: [f32; 2]) -> Option<GlyphExtents> {
        let ink = self.bounds?;
        let [small, large] = size_range.map(|size| size / f32::from(self.units_per_em));
        let bearing = [f32::from(ink.x_min), -f32::from(ink.y_max)];
        Some(GlyphExtents {
            min_bearing: bearing.map(|value| (value * small).min(value * large).round()),
            max_bearing: bearing.map(|value| (value * small).max(value * large).round()),
            size: [ink.width(), ink.height()].map(|extent| (f32::from(extent) * large).ceil()),
        })
    }
}

impl GlyphExtents {
    pub(super) fn padded_edges(self) -> [f32; 4] {
        [
            self.min_bearing[0] - 1.0,
            self.min_bearing[1] - 1.0,
            self.max_bearing[0] + self.size[0] + 1.0,
            self.max_bearing[1] + self.size[1] + 1.0,
        ]
    }
}

impl FontCatalog {
    fn outline_glyph(&self, font: FontId, glyph: u16) -> Option<OutlineGlyph> {
        let source = self.outline_faces.get(&font)?;
        let face = ttf_parser::Face::parse(source.bytes.as_ref(), source.index).ok()?;
        let glyph = ttf_parser::GlyphId(glyph);
        if face.is_variable()
            || face.is_color_glyph(glyph)
            || face.glyph_svg_image(glyph).is_some()
            || face.glyph_raster_image(glyph, u16::MAX).is_some()
        {
            return None;
        }
        Some(OutlineGlyph {
            bounds: face.glyph_bounding_box(glyph),
            units_per_em: face.units_per_em(),
        })
    }

    fn new(
        db: Database,
        loaded: HashMap<fontdb::ID, Option<FontId>>,
        first: Option<FontId>,
        fallback: &[FallbackFace],
    ) -> Self {
        let primary = db
            .faces()
            .find(|face| first.is_some() && loaded.get(&face.id) == Some(&first))
            .or_else(|| db.faces().next())
            .and_then(|face| face.families.first())
            .map(|(family, _)| family.clone())
            .unwrap_or_default();
        let outline_faces = fallback
            .iter()
            .filter_map(|fallback| {
                let font = loaded.get(&fallback.id?).copied().flatten()?;
                Some((font, fallback.source.clone()))
            })
            .collect();
        Self {
            db,
            primary,
            loaded,
            outline_faces,
            matches: HashMap::new(),
        }
    }

    fn resolve(
        &mut self,
        context: &TextContext,
        request: &FontRequest,
        fallback: bool,
    ) -> Option<FontId> {
        if let Some(font) = self.matches.get(request) {
            return *font;
        }
        let family = match request.family.as_str() {
            "" => self.primary.as_str(),
            "sans-serif" => self.db.family_name(&Family::SansSerif),
            "serif" => self.db.family_name(&Family::Serif),
            "monospace" => self
                .db
                .faces()
                .find(|face| face.monospaced)
                .and_then(|face| face.families.first())
                .map_or(self.db.family_name(&Family::Monospace), |(name, _)| name),
            name => self
                .db
                .faces()
                .flat_map(|face| &face.families)
                .find(|(family, _)| family.eq_ignore_ascii_case(name))
                .map_or(name, |(family, _)| family),
        };
        let families = [Family::Name(family), Family::Name(&self.primary)];
        let id = self.db.query(&Query {
            families: &families[..if fallback { 2 } else { 1 }],
            weight: Weight(request.weight),
            style: if request.italic {
                Style::Italic
            } else {
                Style::Normal
            },
            ..Default::default()
        });
        let font = id.and_then(|id| {
            *self.loaded.entry(id).or_insert_with(|| {
                let source = FontSource::from_database(&self.db, id)?;
                let font = source.load(context).ok()?;
                self.outline_faces.insert(font, source);
                Some(font)
            })
        });
        // Bound memoization of requests supplied by dynamic application bindings.
        if self.matches.len() >= 128 {
            self.matches.clear();
        }
        self.matches.insert(request.clone(), font);
        font
    }
}

#[derive(Clone)]
struct FontBytes(Arc<dyn AsRef<[u8]> + Send + Sync>);

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref().as_ref()
    }
}

#[derive(Clone)]
struct FontSource {
    bytes: FontBytes,
    index: u32,
}

impl FontSource {
    fn from_database(db: &Database, id: fontdb::ID) -> Option<Self> {
        let face = db.face(id)?;
        let bytes = if let fontdb::Source::Binary(data) = &face.source {
            FontBytes(data.clone())
        } else {
            db.with_face_data(id, |data, _| FontBytes(Arc::new(Arc::<[u8]>::from(data))))?
        };
        Some(Self {
            bytes,
            index: face.index,
        })
    }

    fn load(&self, context: &TextContext) -> Result<FontId, String> {
        context
            .add_shared_font_with_index(self.bytes.clone(), self.index)
            .map_err(|error| error.to_string())
    }
}

struct FallbackFace {
    id: Option<fontdb::ID>,
    source: FontSource,
}

#[derive(Clone, PartialEq)]
enum SceneFonts {
    Embedded(&'static EmbeddedFonts),
    Loaded(xrack_runtime::LoadedFonts),
}

impl SceneFonts {
    fn from_scene(scene: &Node) -> Option<Self> {
        scene
            .fonts
            .map(Self::Embedded)
            .or_else(|| scene.loaded_fonts.clone().map(Self::Loaded))
    }

    fn load(&self, database: &mut Database) {
        match self {
            Self::Embedded(fonts) => {
                for &source in fonts.sources() {
                    database.load_font_source(fontdb::Source::Binary(Arc::new(source)));
                }
            }
            Self::Loaded(fonts) => {
                for source in fonts.sources() {
                    database.load_font_source(fontdb::Source::Binary(source.clone()));
                }
            }
        }
    }
}

#[cfg(target_os = "ios")]
struct NativeFallback {
    regular: (FontId, Rc<NativeFont>),
    bold: Option<(FontId, Rc<NativeFont>)>,
    bold_fonts: Vec<FontId>,
}

struct FontContext {
    identity: Rc<()>,
    families: Vec<String>,
    context: TextContext,
    fallback: Vec<FontId>,
    catalog: RefCell<FontCatalog>,
    bundled: Option<RefCell<FontCatalog>>,
    #[cfg(target_os = "android")]
    emoji_font: Option<(FontId, std::path::PathBuf, u32)>,
    #[cfg(target_os = "ios")]
    native_fallback: Option<NativeFallback>,
    #[cfg(target_os = "ios")]
    native_emoji: Option<(FontId, Rc<NativeFont>)>,
}

#[cfg(target_os = "android")]
fn android_emoji_font(
    db: &Database,
    loaded: &HashMap<fontdb::ID, Option<FontId>>,
) -> Option<(FontId, std::path::PathBuf, u32)> {
    let id = db.query(&Query {
        families: &[Family::Name("Noto Color Emoji")],
        ..Default::default()
    })?;
    let font = loaded.get(&id).copied().flatten()?;
    let face = db.face(id)?;
    let fontdb::Source::File(path) = &face.source else {
        return None;
    };
    Some((font, path.clone(), face.index))
}

impl FontContext {
    fn with_bundle(&self, sources: &[FallbackFace], fonts: &SceneFonts) -> Self {
        // FemtoVG tries its registered faces after the paint's fallback list.
        // A separate context keeps fonts from a previous scene out of that search.
        let context = TextContext::default();
        let mut loaded = HashMap::new();
        let fallback: Vec<_> = sources
            .iter()
            .filter_map(|source| {
                let font = source.source.load(&context).ok();
                if let Some(id) = source.id {
                    loaded.insert(id, font);
                }
                font
            })
            .collect();
        let system = self.catalog.borrow().db.clone();
        let mut db = Database::new();
        fonts.load(&mut db);
        let families = font_family_names(&system, Some(&db));
        #[cfg(target_os = "android")]
        let emoji_font = android_emoji_font(&system, &loaded);
        let catalog = RefCell::new(FontCatalog::new(
            system,
            loaded,
            fallback.first().copied(),
            sources,
        ));
        let bundled = Some(RefCell::new(FontCatalog::new(
            db,
            HashMap::new(),
            None,
            &[],
        )));
        let result = Self {
            identity: Rc::new(()),
            families,
            context,
            fallback,
            catalog,
            bundled,
            #[cfg(target_os = "android")]
            emoji_font,
            #[cfg(target_os = "ios")]
            native_fallback: None,
            #[cfg(target_os = "ios")]
            native_emoji: None,
        };
        #[cfg(target_os = "ios")]
        let result = if let Some(native) = &self.native_fallback {
            result.with_native_fallback(
                native.regular.1.clone(),
                native.bold.as_ref().map(|(_, font)| font.clone()),
            )
        } else {
            result
        };
        #[cfg(target_os = "ios")]
        let result = if let Some((_, emoji)) = &self.native_emoji {
            result.with_native_emoji(emoji.clone())
        } else {
            result
        };
        result
    }

    #[cfg(target_os = "ios")]
    fn with_native_fallback(
        mut self,
        regular: Rc<NativeFont>,
        bold: Option<Rc<NativeFont>>,
    ) -> Self {
        let Ok(regular_id) = self
            .context
            .add_shared_font_with_index(regular.data.clone(), 0)
        else {
            return self;
        };
        let position = self.fallback.len().min(1);
        self.fallback.insert(position, regular_id);
        self.fallback.truncate(8);
        let bold = bold.and_then(|font| {
            let id = self
                .context
                .add_shared_font_with_index(font.data.clone(), 0)
                .ok()?;
            Some((id, font))
        });
        let mut bold_fonts = self.fallback.clone();
        if let Some((id, _)) = &bold {
            bold_fonts[position] = *id;
        }
        self.native_fallback = Some(NativeFallback {
            regular: (regular_id, regular),
            bold,
            bold_fonts,
        });
        self
    }

    #[cfg(target_os = "ios")]
    fn with_native_emoji(mut self, font: Rc<NativeFont>) -> Self {
        let Ok(id) = self
            .context
            .add_shared_font_with_index(font.data.clone(), 0)
        else {
            return self;
        };
        let position = if self.native_fallback.is_some() { 2 } else { 1 };
        let insert = |fonts: &mut Vec<FontId>| {
            fonts.insert(position.min(fonts.len()), id);
            fonts.truncate(8);
        };
        insert(&mut self.fallback);
        if let Some(native) = &mut self.native_fallback {
            insert(&mut native.bold_fonts);
        }
        let catalog = self.catalog.get_mut();
        for face in catalog
            .db
            .faces()
            .filter(|face| face.post_script_name == "AppleColorEmoji")
        {
            catalog.loaded.insert(face.id, Some(id));
        }
        self.native_emoji = Some((id, font));
        self
    }
}

struct EditLayout {
    text: String,
    size: f32,
    face: Option<StyledFace>,
    wrap: TextWrap,
    alignment: TextAlignment,
    layout: TextAreaLayout,
}

struct EditLine {
    text: String,
    size: f32,
    face: Option<StyledFace>,
    geometry: Rc<VisualTextLine>,
}

fn font_family_names(system: &Database, bundled: Option<&Database>) -> Vec<String> {
    let mut families: Vec<_> = system
        .faces()
        .chain(bundled.into_iter().flat_map(Database::faces))
        .filter_map(|face| face.families.first())
        .map(|(family, _)| family.clone())
        .collect();
    families.sort_by_cached_key(|family| (family.to_ascii_lowercase(), family.clone()));
    families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    families
}

mod cell_shaping;

pub(super) type CellFonts = [Option<FontId>; 8];

pub(super) struct CellPaint {
    pub(super) paint: Paint,
    pub(super) fonts: CellFonts,
}

/// One font registry and shaping cache shared by measurement and drawing.
pub struct TextSystem {
    base: FontContext,
    fallback_sources: Vec<FallbackFace>,
    bundled: Option<(SceneFonts, FontContext)>,
    active_fonts: Option<SceneFonts>,
    default_family: String,
    default_weight: u16,
    default_italic: bool,
    default_size: Option<f32>,
    text_scale: f32,
    system_bold_text: bool,
    typography: Rc<()>,
    intrinsic_measurements: xrack_layout::IntrinsicMeasurements,
    edit_layout: RefCell<Option<EditLayout>>,
    edit_line: RefCell<Option<EditLine>>,
    label_layouts: RefCell<LabelCache>,
    pub(super) label_glyphs: RefCell<crate::label_glyphs::Cache>,
    label_snapshots_enabled: bool,
    outline_glyphs: RefCell<HashMap<(FontId, u16), Option<OutlineGlyph>>>,
    cell_shapes: RefCell<cell_shaping::Cache>,
    pub(super) cell_glyphs: RefCell<crate::cell_glyphs::Cache>,
    styled_layouts: RefCell<crate::styled_text::LayoutCache>,
    retained_styled_text_enabled: bool,
    retained_text: RefCell<Option<crate::retained_text::Cache>>,
    pub(crate) styled_ink: RefCell<crate::styled_ink::InkCache>,
    #[cfg(target_os = "android")]
    android_emoji: Option<RefCell<Emoji>>,
    #[cfg(target_os = "ios")]
    color_glyphs: RefCell<ColorGlyphs>,
}

impl TextSystem {
    /// Use only the fonts embedded in the scene's UI library.
    pub fn bundled_only() -> Self {
        Self::new(
            Database::new(),
            TextContext::default(),
            Vec::new(),
            HashMap::new(),
            Vec::new(),
        )
    }

    pub fn system() -> Result<Self, String> {
        let mut db = Database::new();
        db.load_system_fonts();
        // fontdb's Apple system font discovery targets macOS.
        #[cfg(target_os = "ios")]
        db.load_fonts_dir("/System/Library/Fonts");
        #[cfg(target_os = "android")]
        {
            db.load_fonts_dir("/system/fonts");
            db.set_sans_serif_family("Roboto");
        }
        let mut selected = Vec::new();
        if let Some(id) = db.query(&Query {
            families: &[Family::SansSerif],
            ..Default::default()
        }) {
            selected.push(id);
        }
        for family in [
            "Arial",
            "Segoe UI",
            "PingFang SC",
            "Microsoft YaHei",
            "Noto Sans CJK SC",
            "Noto Sans SC",
            "WenQuanYi Micro Hei",
            "Hiragino Sans",
            "Noto Sans Arabic",
            "Apple Color Emoji",
            "Segoe UI Emoji",
            "Noto Color Emoji",
        ] {
            if let Some(id) = db.query(&Query {
                families: &[Family::Name(family)],
                ..Default::default()
            }) && !selected.contains(&id)
            {
                selected.push(id);
            }
        }
        for face in db.faces() {
            if !selected.contains(&face.id) {
                selected.push(face.id);
            }
        }
        let context = TextContext::default();
        let mut fonts = Vec::new();
        let mut loaded = HashMap::new();
        let mut sources = Vec::new();
        #[cfg(target_os = "ios")]
        let color_emoji = NativeFont::with_post_script_name("AppleColorEmoji");
        // FemtoVG's fallback list holds at most eight font faces.
        for id in selected {
            // Core Text supplies Apple Color Emoji's layout independently of the bitmap strikes.
            #[cfg(target_os = "ios")]
            if color_emoji.is_some()
                && db
                    .face(id)
                    .is_some_and(|face| face.post_script_name == "AppleColorEmoji")
            {
                continue;
            }
            if let Some(source) = FontSource::from_database(&db, id) {
                let font = source.load(&context).ok();
                loaded.insert(id, font);
                if let Some(font) = font {
                    sources.push(FallbackFace {
                        id: Some(id),
                        source,
                    });
                    fonts.push(font);
                }
            }
            if fonts.len() == 8 {
                break;
            }
        }
        let system = Self::new(db, context, fonts, loaded, sources);
        #[cfg(target_os = "ios")]
        let system = if let Some(regular) = NativeFont::with_post_script_name("PingFangSC-Regular")
        {
            let mut system = system;
            let bold = NativeFont::with_post_script_name("PingFangSC-Semibold").map(Rc::new);
            system.base = system.base.with_native_fallback(Rc::new(regular), bold);
            system
        } else {
            system
        };
        #[cfg(target_os = "ios")]
        let system = if let Some(emoji) = color_emoji {
            let mut system = system;
            system.base = system.base.with_native_emoji(Rc::new(emoji));
            system
        } else {
            system
        };
        Ok(system)
    }

    pub fn from_fonts(fonts: impl IntoIterator<Item = (Vec<u8>, u32)>) -> Result<Self, String> {
        let context = TextContext::default();
        let mut db = Database::new();
        let mut loaded = HashMap::new();
        let mut fallback = Vec::new();
        let mut sources = Vec::new();
        for (data, index) in fonts.into_iter().take(8) {
            let source = FontSource {
                bytes: FontBytes(Arc::new(data)),
                index,
            };
            let font = source.load(&context)?;
            let ids = db.load_font_source(fontdb::Source::Binary(source.bytes.0.clone()));
            let id = ids
                .into_iter()
                .find(|&id| db.face(id).is_some_and(|face| face.index == index));
            if let Some(id) = id {
                loaded.insert(id, Some(font));
            }
            fallback.push(font);
            sources.push(FallbackFace { id, source });
        }
        if fallback.is_empty() {
            return Err("at least one font is required".into());
        }
        Ok(Self::new(db, context, fallback, loaded, sources))
    }

    fn new(
        db: Database,
        context: TextContext,
        fallback: Vec<FontId>,
        loaded: HashMap<fontdb::ID, Option<FontId>>,
        fallback_sources: Vec<FallbackFace>,
    ) -> Self {
        let families = font_family_names(&db, None);
        #[cfg(target_os = "android")]
        let emoji_font = android_emoji_font(&db, &loaded);
        let catalog = RefCell::new(FontCatalog::new(
            db,
            loaded,
            fallback.first().copied(),
            &fallback_sources,
        ));
        Self {
            base: FontContext {
                identity: Rc::new(()),
                families,
                context,
                fallback,
                catalog,
                bundled: None,
                #[cfg(target_os = "android")]
                emoji_font,
                #[cfg(target_os = "ios")]
                native_fallback: None,
                #[cfg(target_os = "ios")]
                native_emoji: None,
            },
            fallback_sources,
            bundled: None,
            active_fonts: None,
            default_family: String::new(),
            default_weight: 400,
            default_italic: false,
            default_size: None,
            text_scale: 1.0,
            system_bold_text: false,
            typography: Rc::new(()),
            intrinsic_measurements: Default::default(),
            edit_layout: RefCell::default(),
            edit_line: RefCell::default(),
            label_layouts: RefCell::default(),
            label_glyphs: RefCell::default(),
            label_snapshots_enabled: true,
            outline_glyphs: RefCell::default(),
            cell_shapes: RefCell::default(),
            cell_glyphs: RefCell::default(),
            styled_layouts: RefCell::default(),
            retained_styled_text_enabled: false,
            retained_text: RefCell::default(),
            styled_ink: RefCell::default(),
            #[cfg(target_os = "android")]
            android_emoji: None,
            #[cfg(target_os = "ios")]
            color_glyphs: RefCell::default(),
        }
    }

    pub(super) fn set_text_scale(&mut self, scale: f32) -> bool {
        if !scale.is_finite() || scale <= 0.0 || self.text_scale == scale {
            return false;
        }
        self.text_scale = scale;
        self.invalidate_intrinsic_measurements();
        self.edit_layout.get_mut().take();
        self.edit_line.get_mut().take();
        self.styled_layouts.get_mut().clear();
        true
    }

    pub(super) fn set_system_bold_text(&mut self, enabled: bool) -> bool {
        if self.system_bold_text == enabled {
            return false;
        }
        self.system_bold_text = enabled;
        self.invalidate_intrinsic_measurements();
        self.edit_layout.get_mut().take();
        self.edit_line.get_mut().take();
        self.styled_layouts.get_mut().clear();
        true
    }

    fn effective_weight(&self, weight: u16) -> u16 {
        if self.system_bold_text {
            (weight + 200).min(900)
        } else {
            weight
        }
    }

    pub fn set_window(&mut self, scene: &Node) {
        let fonts = SceneFonts::from_scene(scene);
        let fonts_changed = fonts != self.active_fonts;
        if fonts_changed {
            if let Some(fonts) = &fonts
                && self.bundled.as_ref().is_none_or(|(last, _)| last != fonts)
            {
                self.bundled = Some((
                    fonts.clone(),
                    self.base.with_bundle(&self.fallback_sources, fonts),
                ));
            }
            self.active_fonts = fonts;
            self.edit_layout.get_mut().take();
            self.edit_line.get_mut().take();
            self.styled_layouts.get_mut().clear();
        }
        let font = scene.font("default-font");
        let family = scene
            .text("default-font-family")
            .or_else(|| font.map(|font| font.family.as_str()))
            .unwrap_or("")
            .trim();
        let weight = weight(
            scene
                .int("default-font-weight")
                .or_else(|| font.map(|font| font.weight)),
            400,
        );
        let italic = scene.boolean("default-font-italic", font.is_some_and(|font| font.italic));
        let size = scene
            .length("default-font-size")
            .or_else(|| font.and_then(xrack_runtime::Font::finite_size))
            .filter(|size| *size > 0.0);
        if fonts_changed
            || self.default_family != family
            || self.default_weight != weight
            || self.default_italic != italic
            || self.default_size != size
        {
            self.invalidate_intrinsic_measurements();
        }
        if self.default_family != family {
            self.default_family = family.to_owned();
        }
        self.default_weight = weight;
        self.default_italic = italic;
        self.default_size = size;
    }

    fn invalidate_intrinsic_measurements(&mut self) {
        self.typography = Rc::new(());
        self.intrinsic_measurements = Default::default();
        self.label_layouts.get_mut().clear();
        *self.label_glyphs.get_mut() = Default::default();
        self.label_snapshots_enabled = true;
        self.outline_glyphs.get_mut().clear();
        self.cell_glyphs.get_mut().clear();
    }

    pub(crate) fn outline_glyph(&self, font: FontId, glyph: u16) -> Option<OutlineGlyph> {
        const MAX_OUTLINES: usize = 4096;
        let mut outlines = self.outline_glyphs.borrow_mut();
        let key = (font, glyph);
        if let Some(outline) = outlines.get(&key) {
            return *outline;
        }
        let context = self.font_context();
        let outline = context
            .bundled
            .as_ref()
            .and_then(|catalog| catalog.borrow().outline_glyph(font, glyph))
            .or_else(|| context.catalog.borrow().outline_glyph(font, glyph));
        if outlines.len() >= MAX_OUTLINES {
            outlines.clear();
        }
        outlines.insert(key, outline);
        outline
    }

    fn font_context(&self) -> &FontContext {
        match (&self.bundled, &self.active_fonts) {
            (Some((_, context)), Some(_)) => context,
            _ => &self.base,
        }
    }

    pub(super) fn shape_cell(
        &self,
        text: &str,
        size: f32,
        fonts: &CellFonts,
    ) -> Option<crate::cell_glyphs::Run> {
        if cfg!(any(
            xrack_femtovg_glyph_samples,
            target_os = "android",
            target_os = "ios"
        )) {
            return None;
        }
        self.cell_shapes.borrow_mut().shape(self, text, size, fonts)
    }

    pub(super) fn ascii_metrics_for_word<'a>(
        &self,
        font: LabelFont,
        text: &str,
        size: f32,
        cached: &'a OnceCell<Option<AsciiMetrics>>,
    ) -> Option<&'a AsciiMetrics> {
        if text.is_empty() || text.len() > LabelAdvances::MAX_TOKEN_BYTES || !text.is_ascii() {
            return None;
        }
        cached
            .get_or_init(|| {
                self.cell_shapes
                    .borrow_mut()
                    .ascii_metrics(self, size, font.face?.id?)
            })
            .as_ref()
    }

    pub(super) fn with_primary_label_word<T>(
        &self,
        font: LabelFont,
        text: &str,
        size: f32,
        visit: impl FnOnce(&[crate::cell_glyphs::Glyph], f32) -> T,
    ) -> Option<T> {
        if cfg!(any(
            xrack_femtovg_glyph_samples,
            target_os = "android",
            target_os = "ios"
        )) || text.is_empty()
            || text.len() > LabelAdvances::MAX_TOKEN_BYTES
            || !text.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return None;
        }
        // These tokens are single LTR shaping runs. The paint puts this
        // face first, and the word cache declines missing or variable glyphs.
        let primary = font.face.and_then(|face| face.id)?;
        let mut shapes = self.cell_shapes.borrow_mut();
        let word = shapes.label_word(self, text, primary)?;
        Some(visit(&word.glyphs, size / word.units_per_em))
    }

    pub(super) fn measure_cell(
        &self,
        text: &str,
        paint: &Paint,
        fonts: Option<&CellFonts>,
    ) -> Option<crate::cell_glyphs::Measurement> {
        use crate::cell_glyphs::Measurement;
        if let Some(fonts) = fonts
            && let Some(run) = self.shape_cell(text, paint.font_size(), fonts)
        {
            return Some(Measurement::Portable(run));
        }
        self.context()
            .measure_text(0.0, 0.0, text, paint)
            .ok()
            .map(Measurement::Legacy)
    }

    pub(crate) fn context(&self) -> &TextContext {
        &self.font_context().context
    }

    pub(crate) fn set_styled_text_incremental_enabled(&mut self, enabled: bool) {
        self.styled_layouts
            .get_mut()
            .set_incremental_enabled(enabled);
        self.styled_ink.get_mut().set_incremental_enabled(enabled);
    }

    pub(crate) fn styled_text_reused_fragments(&self) -> u64 {
        self.styled_layouts.borrow().reused_fragments
    }

    pub(crate) fn reused_styled_prefix(
        &self,
        plan: &Rc<StyledTextLayout>,
        document: [(usize, usize); 3],
    ) -> Option<crate::styled_text::ReusedPrefix> {
        self.styled_layouts.borrow().reused_prefix(plan, document)
    }

    pub(crate) fn registered_font_counts(&self) -> [usize; 2] {
        let fonts = self.font_context();
        [
            fonts.catalog.borrow().loaded.len(),
            fonts
                .bundled
                .as_ref()
                .map_or(0, |catalog| catalog.borrow().loaded.len()),
        ]
    }

    #[cfg(target_os = "android")]
    #[doc(hidden)]
    pub fn set_android_glyph_rasterizer(
        &mut self,
        rasterize: impl Fn(&std::path::Path, u32, u16, u16) -> Option<AndroidGlyphBitmap> + 'static,
    ) {
        self.android_emoji = Some(RefCell::new(Emoji::new(Box::new(rasterize))));
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) fn begin_color_glyph_frame(&mut self, canvas_changed: bool) {
        #[cfg(target_os = "android")]
        if let Some(emoji) = &mut self.android_emoji {
            emoji.get_mut().begin_frame(canvas_changed);
        }
        #[cfg(target_os = "ios")]
        self.color_glyphs.get_mut().begin_frame(canvas_changed);
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) fn end_color_glyph_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        #[cfg(target_os = "android")]
        if let Some(emoji) = &mut self.android_emoji {
            emoji.get_mut().end_frame(canvas);
        }
        #[cfg(target_os = "ios")]
        self.color_glyphs.get_mut().end_frame(canvas);
    }

    pub(crate) fn fill_text(
        &self,
        canvas: &mut Canvas<WGPURenderer>,
        x: f32,
        y: f32,
        text: &str,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        let metrics = canvas.fill_text(x, y, text, paint)?;
        #[cfg(target_os = "android")]
        if let Some((emoji, font)) = self
            .android_emoji
            .as_ref()
            .zip(self.font_context().emoji_font.as_ref())
        {
            emoji.borrow_mut().draw(canvas, &metrics, paint, font);
        }
        #[cfg(target_os = "ios")]
        if let Some((id, font)) = &self.font_context().native_emoji {
            self.color_glyphs
                .borrow_mut()
                .draw(canvas, &metrics, paint, *id, |glyph, size| {
                    font.rasterize_color_glyph(glyph, size)
                });
        }
        #[cfg(target_os = "ios")]
        if let Some(native) = &self.font_context().native_fallback {
            for (font_id, font) in std::iter::once(&native.regular).chain(native.bold.as_ref()) {
                for glyph in metrics
                    .glyphs
                    .iter()
                    .filter(|glyph| glyph.font_id == *font_id)
                {
                    font.draw(
                        canvas,
                        glyph.glyph_id,
                        Point {
                            x: glyph.x,
                            y: glyph.y,
                        },
                        paint,
                    );
                }
            }
        }
        Ok(metrics)
    }

    pub(crate) fn context_key(&self) -> &Rc<()> {
        &self.font_context().identity
    }

    pub(crate) fn default_style(&self) -> (&str, u16, bool, Option<f32>) {
        (
            &self.default_family,
            self.effective_weight(self.default_weight),
            self.default_italic,
            Some(self.default_size.unwrap_or(xrack_layout::DEFAULT_FONT_SIZE) * self.text_scale),
        )
    }

    pub(crate) fn font_attributes<'a>(&'a self, node: Option<&'a Node>) -> (&'a str, u16, bool) {
        let font = node.and_then(|node| node.font("font"));
        let family = node
            .and_then(|node| node.text("font-family"))
            .or_else(|| font.map(|font| font.family.as_str()))
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .unwrap_or(&self.default_family);
        let weight = weight(
            node.and_then(|node| node.int("font-weight"))
                .or_else(|| font.map(|font| font.weight)),
            self.default_weight,
        );
        let italic = node.map_or(self.default_italic, |node| {
            node.boolean(
                "font-italic",
                font.map_or(self.default_italic, |font| font.italic),
            )
        });
        (family, weight, italic)
    }

    pub(crate) fn set_retained_styled_text_enabled(&mut self, enabled: bool) -> bool {
        if self.retained_styled_text_enabled == enabled {
            return false;
        }
        self.retained_styled_text_enabled = enabled;
        self.invalidate_intrinsic_measurements();
        self.styled_layouts.get_mut().clear();
        true
    }

    fn retained_styled_text_available(&self) -> bool {
        self.retained_styled_text_enabled
            && !cfg!(any(
                target_os = "ios",
                target_os = "android",
                target_arch = "wasm32"
            ))
            && self.active_fonts.is_none()
            && !self.base.fallback.is_empty()
    }

    pub(crate) fn retained_text_font<'a>(&'a self, node: &'a Node) -> (&'a str, u16, bool) {
        let (family, weight, italic) = self.font_attributes(Some(node));
        (family, self.effective_weight(weight), italic)
    }

    pub(crate) fn retained_glyph_font(
        &self,
        font: &xrack_layout::StyledGlyphFont,
    ) -> Result<FontId, ErrorKind> {
        self.retained_text
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .register_font(self, font)
    }

    fn face(&self, node: Option<&Node>, style: TextStyle) -> Option<StyledFace> {
        self.resolve_face(self.font_attributes(node), style)
    }

    fn resolve_face(
        &self,
        (family, weight, italic): (&str, u16, bool),
        style: TextStyle,
    ) -> Option<StyledFace> {
        let family = if style.monospace { "monospace" } else { family };
        let weight = if style.bold { weight.max(700) } else { weight };
        let weight = self.effective_weight(weight);
        let italic = italic || style.italic;
        let fonts = self.font_context();
        if family.is_empty() && weight == 400 && !italic && !fonts.fallback.is_empty() {
            return None;
        }
        let request = FontRequest {
            family: family.into(),
            weight,
            italic,
        };
        let id = fonts
            .bundled
            .as_ref()
            .filter(|_| !family.is_empty() || fonts.fallback.is_empty())
            .and_then(|catalog| {
                catalog
                    .borrow_mut()
                    .resolve(&fonts.context, &request, fonts.fallback.is_empty())
            })
            .or_else(|| {
                fonts
                    .catalog
                    .borrow_mut()
                    .resolve(&fonts.context, &request, true)
            });
        Some(StyledFace { id, weight, italic })
    }

    pub fn paint(&self, font_size: f32, color: Color, centered: bool) -> Paint {
        self.paint_face(
            self.face(None, TextStyle::default()),
            font_size,
            color,
            centered,
        )
    }

    pub(crate) fn paint_node(&self, node: &Node, color: Color, centered: bool) -> Paint {
        self.paint_node_at_size(node, self.font_size(node), color, centered)
    }

    pub(crate) fn paint_node_at_size(
        &self,
        node: &Node,
        size: f32,
        color: Color,
        centered: bool,
    ) -> Paint {
        self.paint_face(
            self.face(Some(node), TextStyle::default()),
            size,
            color,
            centered,
        )
    }

    pub(super) fn cell_paint(&self, node: &Node, size: f32, color: Color) -> CellPaint {
        self.paint_with_fonts(
            self.face(Some(node), TextStyle::default()),
            size,
            color,
            false,
        )
    }

    pub(crate) fn paint_styled(&self, node: &Node, tint: Color, style: TextStyle) -> Paint {
        let mut paint = self.paint_face(
            self.face(Some(node), style),
            self.font_size(node),
            tint,
            false,
        );
        paint.set_text_baseline(Baseline::Alphabetic);
        paint.set_text_decoration(TextDecoration {
            underline: style.underline,
            strikethrough: style.strikethrough,
            ..Default::default()
        });
        paint
    }

    fn paint_face(
        &self,
        face: Option<StyledFace>,
        font_size: f32,
        color: Color,
        centered: bool,
    ) -> Paint {
        self.paint_with_fonts(face, font_size, color, centered)
            .paint
    }

    fn paint_with_fonts(
        &self,
        face: Option<StyledFace>,
        font_size: f32,
        color: Color,
        centered: bool,
    ) -> CellPaint {
        let mut paint = Paint::color(color);
        let fallback = &self.font_context().fallback;
        #[cfg(target_os = "ios")]
        let fallback = self
            .font_context()
            .native_fallback
            .as_ref()
            .filter(|_| self.system_bold_text)
            .map_or(fallback, |native| &native.bold_fonts);
        let primary = face.and_then(|face| face.id);
        let ordered = primary.into_iter().chain(
            fallback
                .iter()
                .copied()
                .filter(|font| Some(*font) != primary),
        );
        let mut fonts = [None; 8];
        for (slot, font) in fonts.iter_mut().zip(ordered) {
            *slot = Some(font);
        }
        if let Some(first) = fonts[0] {
            let paint_fonts = fonts.map(|font| font.unwrap_or(first));
            let count = fonts.iter().take_while(|font| font.is_some()).count();
            paint.set_font(&paint_fonts[..count]);
        }
        if let Some(face) = face {
            paint.set_font_weight(f32::from(face.weight));
            paint.set_font_italic(face.italic);
        }
        paint.set_font_size(font_size);
        paint.set_text_baseline(Baseline::Top);
        paint.set_text_align(if centered { Align::Center } else { Align::Left });
        CellPaint { paint, fonts }
    }

    pub fn shape(&self, text: &str, size: f32) -> Result<TextMetrics, String> {
        self.context()
            .measure_text(0.0, 0.0, text, &self.paint(size, Color::white(), false))
            .map_err(|e| e.to_string())
    }

    pub fn advance(&self, text: &str, size: f32) -> f32 {
        self.advance_painted(text, &self.paint(size, Color::white(), false))
    }

    pub fn advance_node(&self, node: &Node, text: &str) -> f32 {
        self.advance_painted(text, &self.paint_node(node, Color::white(), false))
    }

    pub(crate) fn advance_painted(&self, text: &str, paint: &Paint) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            self.context()
                .measure_text_width(text, paint)
                .unwrap_or(0.0)
        }
        #[cfg(not(xrack_femtovg_glyph_samples))]
        {
            self.context()
                .measure_text(0.0, 0.0, text, paint)
                .map_or(0.0, |metrics| metrics.width())
        }
    }

    pub(crate) fn shape_visual_line(&self, text: &str, paint: &Paint) -> VisualTextLine {
        let metrics = self
            .context()
            .measure_text(0.0, 0.0, text, paint)
            .unwrap_or_default();
        VisualTextLine::new(
            text,
            metrics
                .glyphs
                .into_iter()
                .map(|glyph| (glyph.byte_index, glyph.advance_x)),
        )
    }

    pub fn single_line_layout(&self, node: &Node, text: &str) -> Rc<VisualTextLine> {
        self.single_line_layout_at_size(node, text, self.font_size(node))
    }

    #[doc(hidden)]
    pub fn single_line_layout_at_size(
        &self,
        node: &Node,
        text: &str,
        size: f32,
    ) -> Rc<VisualTextLine> {
        let face = self.face(Some(node), TextStyle::default());
        if let Some(last) = &*self.edit_line.borrow()
            && last.size == size
            && last.face == face
            && last.text == text
        {
            return last.geometry.clone();
        }
        let paint = self.paint_face(face, size, Color::white(), false);
        let geometry = Rc::new(self.shape_visual_line(text, &paint));
        self.edit_line.replace(Some(EditLine {
            text: text.into(),
            size,
            face,
            geometry: geometry.clone(),
        }));
        geometry
    }

    pub fn text_area_layout(&self, node: &Node, bounds: Rect, text: &str) -> TextAreaLayout {
        self.text_area_layout_in_direction(node, bounds, text, node.layout_direction())
    }

    #[doc(hidden)]
    pub fn text_area_layout_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        text: &str,
        direction: Option<LayoutDirection>,
    ) -> TextAreaLayout {
        let alignment = TextAlignment::editor(node, direction);
        let area = text_editor_viewport(node, bounds);
        let size = self.font_size(node);
        let face = self.face(Some(node), TextStyle::default());
        let wrap = node.text_wrap();
        if let Some(last) = &*self.edit_layout.borrow()
            && last.layout.area.width == area.width
            && last.size == size
            && last.face == face
            && last.wrap == wrap
            && last.alignment == alignment
            && last.text == text
        {
            return last.layout.clone().with_viewport(area);
        }
        let paint = self.paint_face(face, size, Color::white(), false);
        let layout = TextAreaLayout::shaped(
            text,
            area,
            size * 1.4,
            wrap,
            |text| self.advance_painted(text, &paint),
            |text| {
                self.context()
                    .break_text(area.width + 0.01, text, &paint)
                    .unwrap_or(text.len())
            },
            |text| self.shape_visual_line(text, &paint),
        )
        .aligned(text, alignment, |text| self.advance_painted(text, &paint));
        self.edit_layout.replace(Some(EditLayout {
            text: text.into(),
            size,
            face,
            wrap,
            alignment,
            layout: layout.clone(),
        }));
        layout
    }

    pub(crate) fn lines(&self, text: &str, paint: &Paint, width: f32) -> Vec<Range<usize>> {
        if text.is_empty() || width <= 0.0 {
            return vec![];
        }
        // FemtoVG wraps on equality. Allow rounding when layout reapplies measured widths.
        self.context()
            .break_text_vec(width + 0.001, text, paint)
            .unwrap_or_else(|_| std::iter::once(0..text.len()).collect())
    }

    pub(crate) fn prepared_label(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
    ) -> Rc<PreparedLabel> {
        if let Some(label) = self.cached_prepared_label(node, available, direction) {
            return label;
        }
        let font = self.label_font(self.font_attributes(Some(node)), self.font_size(node));
        self.prepared_label_with_font(node, available, direction, font)
    }

    pub(super) fn plain_label_tint(
        &self,
        node: &Node,
        classify: impl FnOnce() -> bool,
    ) -> Option<Option<xrack_runtime::Color>> {
        if self.label_snapshots_enabled && node.has_shared_storage() {
            let cache = self.label_layouts.borrow();
            let (_, index) = LabelCache::snapshot_key(node);
            if let Some(entry) = cache
                .snapshots
                .get(index)
                .and_then(|slot| slot.prepared.as_ref())
                && entry.node.matches(node)
            {
                if let Some(tint) = entry.plain_tint.get() {
                    return Some(tint);
                }
                let tint = classify().then(|| node.color("color"))?;
                entry.plain_tint.set(Some(tint));
                return Some(tint);
            }
        }
        classify().then(|| node.color("color"))
    }

    pub(super) fn cached_prepared_label(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
    ) -> Option<Rc<PreparedLabel>> {
        if !self.label_snapshots_enabled || !node.has_shared_storage() {
            return None;
        }
        self.label_layouts
            .borrow()
            .snapshot(node, available, direction, || self.registered_font_counts())
    }

    pub(super) fn label_word_index(&self, source: &str) -> Option<Rc<LabelWordIndex>> {
        let cache = self.label_layouts.borrow();
        let retained = cache.advances.word_advances.as_ref()?;
        (retained.source.as_ref() == source).then(|| retained.index.clone())
    }

    pub(super) fn label_font(&self, attributes: (&str, u16, bool), size: f32) -> LabelFont {
        LabelFont {
            size,
            face: self.resolve_face(attributes, TextStyle::default()),
        }
    }

    pub(super) fn prepared_label_with_font(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
        LabelFont { size, face }: LabelFont,
    ) -> Rc<PreparedLabel> {
        let text = node.text("text").unwrap_or("");
        let style = LabelStyle::for_node(node, direction);
        let settings = LabelSettings {
            available: [available.width.to_bits(), available.height.to_bits()],
            style,
            font_size: size.to_bits(),
            face,
            registered_fonts: self.registered_font_counts(),
        };
        let mut cache = self.label_layouts.borrow_mut();
        let label = cache.get(text, settings, |advances, measurements| {
            let paint = self.paint_face(face, size, Color::white(), false);
            // Width changes can revisit the same word-wrap probes many times.
            // Keep their advances across line plans for long labels.
            let mut retained = (text.len() >= 1024).then(|| {
                advances.select(LabelAdvanceFont {
                    face,
                    size: size.to_bits(),
                    registered_fonts: self.registered_font_counts(),
                });
                advances
            });
            let context = self.context();
            let indexed_source = (measurements.is_some()
                && retained
                    .as_mut()
                    .is_some_and(|advances| advances.retain_word_advances(text)))
            .then_some(text);
            let ascii_metrics = OnceCell::new();
            let layout =
                LabelPlan::measured_text(text, available, size, style, measurements, |value| {
                    let measure = |word: &str| {
                        if indexed_source.is_some()
                            && let Some(width) = self
                                .ascii_metrics_for_word(
                                    LabelFont { size, face },
                                    word,
                                    size,
                                    &ascii_metrics,
                                )
                                .and_then(|metrics| metrics.width(word))
                        {
                            return width;
                        }
                        if indexed_source.is_some()
                            && let Some(width) = self.with_primary_label_word(
                                LabelFont { size, face },
                                word,
                                size,
                                |glyphs, scale| {
                                    glyphs.iter().fold(0.0, |width, glyph| {
                                        width + (glyph.advance[0] * scale + 0.0)
                                    })
                                },
                            )
                        {
                            return width;
                        }
                        self.advance_painted(word, &paint)
                    };
                    if let Some(advances) = &mut retained {
                        advances.measure(value, indexed_source, context, measure)
                    } else {
                        measure(value)
                    }
                });
            if let Some(advances) = retained
                && advances.unretained >= LabelAdvances::CACHE_PRESSURE
                && let Some(measurements) = measurements
            {
                measurements.retain_range_advances();
            }
            (layout, paint)
        });
        if self.label_snapshots_enabled && node.has_shared_storage() {
            cache.remember_snapshot(
                node,
                available,
                direction,
                settings.registered_fonts,
                &label,
            );
        }
        label
    }

    pub(super) fn begin_label_frame(&mut self) {
        self.label_snapshots_enabled = self.label_layouts.get_mut().begin_snapshot_frame();
    }

    fn measure_painted(&self, text: &str, paint: &Paint, font_size: f32, max_width: f32) -> Size {
        if max_width <= 0.0 {
            return Size::default();
        }
        let lines = self.lines(text, paint, max_width);
        let width = lines
            .iter()
            .filter_map(|range| {
                self.context()
                    .measure_text(0.0, 0.0, &text[range.clone()], paint)
                    .ok()
            })
            .map(|m| m.width())
            .fold(0.0, f32::max);
        Size::new(
            width.min(max_width),
            lines.len().max(1) as f32 * font_size * 1.4,
        )
    }
}

impl TextMeasurer for TextSystem {
    fn begin_window(&mut self, scene: &Node) {
        self.set_window(scene);
    }

    fn typography_revision(&self) -> Option<Rc<()>> {
        Some(self.typography.clone())
    }

    fn intrinsic_measurements(&mut self) -> Option<&mut xrack_layout::IntrinsicMeasurements> {
        Some(&mut self.intrinsic_measurements)
    }

    fn font_families(&self) -> &[String] {
        &self.font_context().families
    }

    fn default_font_size(&self) -> Option<f32> {
        self.default_size
    }

    fn text_scale(&self) -> f32 {
        self.text_scale
    }

    fn layout_styled_text(&mut self, node: &Node, available: Size) -> Option<StyledTextLayout> {
        self.shared_styled_text_layout(node, available)
            .map(|plan| (*plan).clone())
    }

    fn shared_styled_text_layout(
        &mut self,
        node: &Node,
        available: Size,
    ) -> Option<Rc<StyledTextLayout>> {
        if self.retained_styled_text_available()
            && let Some(plan) = self
                .retained_text
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .layout(self, node, available)
        {
            return Some(plan);
        }
        self.styled_layouts
            .borrow_mut()
            .get(self, node, available, true)
    }

    fn measure_label(&mut self, node: &Node, max_width: f32) -> Size {
        let available = Size::new(max_width, f32::INFINITY);
        if self.retained_styled_text_available()
            && let Some(size) = self
                .retained_text
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .measure(self, node, max_width)
        {
            return size;
        }
        self.styled_layouts
            .borrow_mut()
            .get(self, node, available, false)
            .map_or_else(
                || {
                    self.prepared_label(node, available, node.layout_direction())
                        .size
                },
                |plan| plan.size,
            )
    }

    fn font_size(&self, node: &Node) -> f32 {
        let size = node
            .font_size()
            .or(self.default_size)
            .unwrap_or(xrack_layout::DEFAULT_FONT_SIZE);
        (size * self.text_scale).clamp(1.0, 512.0)
    }

    fn measure_node(&mut self, node: &Node, text: &str, max_width: f32) -> Size {
        let size = self.font_size(node);
        self.measure_node_at_size(node, text, size, max_width)
    }

    fn measure_node_at_size(&mut self, node: &Node, text: &str, size: f32, max_width: f32) -> Size {
        let paint = self.paint_node_at_size(node, size, Color::white(), false);
        self.measure_painted(text, &paint, size, max_width)
    }

    fn measure(&mut self, text: &str, size: f32, max_width: f32) -> Size {
        self.measure_painted(
            text,
            &self.paint(size, Color::white(), false),
            size,
            max_width,
        )
    }
}

fn weight(value: Option<i64>, fallback: u16) -> u16 {
    value
        .filter(|&weight| weight != 0)
        .map_or(fallback, |weight| weight.clamp(100, 900) as u16)
}

mod word_advances_tests {
    #[test]
    fn returning_to_an_earlier_width_interval_reuses_the_clipped_plan() {
        use super::{Color, LabelCache, LabelPlan, LabelSettings, LabelStyle, Node, Paint, Size};
        use std::rc::Rc;
        use xrack_runtime::TextWrap;

        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let style = LabelStyle::for_node(&node, None);
        let source = "Keep each measured word in place.\n".repeat(128);
        let mut cache = LabelCache::default();
        let plan = |cache: &mut LabelCache, width: f32, height: f32| {
            cache.get(
                &source,
                LabelSettings {
                    available: [width.to_bits(), height.to_bits()],
                    style,
                    font_size: 12.0f32.to_bits(),
                    face: None,
                    registered_fonts: [0; 2],
                },
                |_, measurements| {
                    let layout = LabelPlan::measured_text(
                        &source,
                        Size::new(width, height),
                        12.0,
                        style,
                        measurements,
                        |text| text.len() as f32 * 5.0,
                    );
                    (layout, Paint::color(Color::white()))
                },
            )
        };

        let _ = plan(&mut cache, 60.0, 33.0);
        let narrow = plan(&mut cache, 83.0, 33.0);
        assert_eq!(narrow.lines().len(), 2);
        let interval = cache.reusable.front().unwrap().widths.clone();
        let wide = plan(&mut cache, interval.end + 30.0, 33.0);
        assert!(!Rc::ptr_eq(&wide, &narrow));
        let new_width = if interval.start == 83.0 {
            interval.start.next_up()
        } else {
            interval.start
        };
        assert!(interval.contains(&new_width));
        assert!(Rc::ptr_eq(&plan(&mut cache, new_width, 33.0), &narrow));
        assert!(!Rc::ptr_eq(&plan(&mut cache, new_width, 50.0), &narrow));
    }

    #[test]
    fn indexed_glyph_words_assign_ids_to_complete_tokens() {
        use super::LabelWordIndex;

        let source = "汉a.b c.d尾";
        let start = source.find(".b").unwrap();
        let end = source.find(".d").unwrap() + 1;
        let index = LabelWordIndex::new(source);
        let span = index.span(source, &source[start..end]).unwrap();
        let mut words = Vec::new();
        index
            .visit(source, span, |word, id| {
                words.push((word.to_owned(), id.is_some()));
                Some(())
            })
            .unwrap();
        assert_eq!(
            words
                .iter()
                .map(|(word, full)| (word.as_str(), *full))
                .collect::<Vec<_>>(),
            [
                (".", false),
                ("b", false),
                (" ", true),
                ("c", false),
                (".", false)
            ]
        );
    }

    #[test]
    fn indexed_widths_preserve_contextual_word_cuts() {
        use super::{LabelWordAdvances, UnicodeSegmentation};

        let advance = |word: &str| {
            let hash = word
                .bytes()
                .fold(0_u32, |hash, byte| hash.rotate_left(5) ^ u32::from(byte));
            (hash % 37) as f32 / 7.0
        };
        let source =
            "文档：build.v1.2 ３，４％ Ａ．Ｂ ｐｋｇ＿２ can't a:b——发布。\n下一段 release-v2";
        let mut index = LabelWordAdvances::new(source);
        let boundaries: Vec<_> = source
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(source.len()))
            .collect();
        let mut indexed = 0;
        for (position, &start) in boundaries.iter().enumerate() {
            let ends = &boundaries[position + 1..];
            // Alternate growing and shrinking prefixes as the line search does.
            for i in 0..ends.len() {
                let end = ends[if i.is_multiple_of(2) {
                    i / 2
                } else {
                    ends.len() - 1 - i / 2
                }];
                let probe = &source[start..end];
                if let Some(actual) = index.measure(source, probe, advance) {
                    let expected = probe
                        .split_word_bounds()
                        .fold(0.0, |width, word| width + advance(word));
                    assert_eq!(actual.to_bits(), expected.to_bits(), "{probe:?}");
                    indexed += 1;
                }
            }
        }
        assert!(indexed > 500);
    }

    #[test]
    fn indexed_words_keep_large_document_offsets_and_ids() {
        use super::{LabelWordAdvances, UnicodeSegmentation};

        let mut source = "#-".repeat(32_770);
        let start = source.len();
        source.push_str("汉a.b c.d尾\n下一段");
        let mut advances = LabelWordAdvances::new(&source);
        let probe = &source[start..start + "汉a.b c.".len()];
        let span = advances.index.span(&source, probe).unwrap();
        assert!(span.first > usize::from(u16::MAX));
        let advance = |word: &str| word.len() as f32 / 7.0;
        let expected = probe
            .split_word_bounds()
            .fold(0.0, |width, word| width + advance(word));
        assert_eq!(
            advances.measure(&source, probe, advance).map(f32::to_bits),
            Some(expected.to_bits())
        );
        assert!(advances.index.span(&source, &source[start..]).is_none());
    }

    #[test]
    fn indexed_widths_leave_joiners_and_bidi_to_shaping() {
        use super::LabelWordAdvances;

        let advance = |_: &str| 0.0;
        for source in ["release 👩‍💻 文档", "a אבג release", "é Ａ．Ｂ"] {
            let mut index = LabelWordAdvances::new(source);
            assert_eq!(index.measure(source, source, advance), None);
        }
        let source = "中文：release-v2";
        let mut index = LabelWordAdvances::new(source);
        assert!(index.measure(source, source, advance).is_some());
        assert_eq!(index.measure(source, &source.to_owned(), advance), None);
    }
}

mod prepared_labels_tests {
    #[test]
    fn reflow_plans_share_a_source_and_charge_it_once() {
        use super::{
            LabelCache, LabelPlan, LabelSettings, LabelStyle, Node, Paint, Point, Rc, Size,
            TextWrap,
        };
        use xrack_layout::PlannedLabelLine;

        let text = "源".repeat(400);
        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        let mut settings = LabelSettings {
            available: [320.0f32.to_bits(), 480.0f32.to_bits()],
            style: LabelStyle::for_node(&node, None),
            font_size: 14.0f32.to_bits(),
            face: None,
            registered_fonts: [0; 2],
        };
        let plan = |ellipsis| LabelPlan {
            size: Size::new(50.0, 20.0),
            lines: vec![PlannedLabelLine {
                range: 0..6,
                ellipsis,
                origin: Point { x: 10.0, y: 5.0 },
            }],
        };
        let mut cache = LabelCache::default();
        let first = cache.get(&text, settings, |_, _| (plan(false), Paint::default()));
        settings.available[0] = 360.0f32.to_bits();
        let second = cache.get(&text, settings, |_, _| (plan(true), Paint::default()));
        let source = first.source.as_ref().unwrap();
        assert!(Rc::ptr_eq(source, second.source.as_ref().unwrap()));
        assert_eq!(source.cached_plans.get(), 2);
        let plan_bytes: usize = cache.entries.values().map(|entry| entry.bytes).sum();
        assert_eq!(cache.bytes, plan_bytes + source.bytes());
        assert_eq!(first.only_line(), Some((Point { x: 10.0, y: 5.0 }, "源源")));
        assert_eq!(
            second.only_line(),
            Some((Point { x: 10.0, y: 5.0 }, "源源…"))
        );

        let oldest = cache.order.pop_front().unwrap();
        assert!(cache.remove_entry(oldest));
        assert_eq!(source.cached_plans.get(), 1);
        assert_eq!(
            cache.bytes,
            cache.entries.values().next().unwrap().bytes + source.bytes()
        );
        let newest = cache.order.pop_front().unwrap();
        assert!(cache.remove_entry(newest));
        assert_eq!(source.cached_plans.get(), 0);
        assert_eq!(cache.bytes, 0);
        let reopened = cache.get(&text, settings, |_, _| (plan(true), Paint::default()));
        assert!(Rc::ptr_eq(source, reopened.source.as_ref().unwrap()));
        assert_eq!(source.cached_plans.get(), 1);
        cache.clear();
        assert_eq!(source.cached_plans.get(), 0);
        assert_eq!(second.only_line().unwrap().1, "源源…");
    }

    #[test]
    fn a_cache_hit_restores_the_width_interval_for_its_own_text() {
        use super::{
            LabelCache, LabelPlan, LabelSettings, LabelStyle, Node, Paint, Point, Rc, Size,
        };
        use xrack_layout::PlannedLabelLine;

        let node = Node::new("Label", None);
        let mut settings = LabelSettings {
            available: [50.0f32.to_bits(), 40.0f32.to_bits()],
            style: LabelStyle::for_node(&node, None),
            font_size: 14.0f32.to_bits(),
            face: None,
            registered_fonts: [0; 2],
        };
        let build = || {
            (
                LabelPlan {
                    size: Size::new(20.0, 18.0),
                    lines: vec![PlannedLabelLine {
                        range: 0..1,
                        ellipsis: false,
                        origin: Point::default(),
                    }],
                },
                Paint::default(),
            )
        };
        let mut cache = LabelCache::default();
        let first = cache.get("a", settings, |_, _| build());
        // Record the interval a retained measurement would have proved.
        cache.entries.values_mut().next().unwrap().reusable_widths = Some(49.0..55.0);
        let hit = cache.get("a", settings, |_, _| {
            panic!("known width must use its plan")
        });
        assert!(Rc::ptr_eq(&first, &hit));
        settings.available[0] = 53.0f32.to_bits();
        let reused = cache.get("a", settings, |_, _| {
            panic!("width is in the restored interval")
        });
        assert!(Rc::ptr_eq(&first, &reused));
        let other = cache.get("b", settings, |_, _| build());
        assert_eq!(other.only_line().unwrap().1, "b");
    }
}
