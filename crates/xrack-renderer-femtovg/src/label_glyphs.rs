use crate::{
    Paint, Point, TextSystem,
    text::{
        AsciiMetrics, LabelFont, LabelWordIndex, PreparedLabel, ascii_word_bounds,
        atlas_size_range, single_ltr_shaping_run,
    },
};
use femtovg::{Baseline, FontId, PositionedGlyph, TextContext, TextMetrics};
#[cfg(xrack_femtovg_glyph_samples)]
use std::collections::HashSet;
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, VecDeque},
    ops::Range,
    rc::{Rc, Weak},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
struct Glyph {
    id: u16,
    advance: [f32; 2],
    offset: [f32; 2],
    bearing_y: f32,
    height: f32,
}

#[derive(Default)]
struct Run {
    font: Option<FontId>,
    baseline: f32,
    glyphs: Vec<Glyph>,
}

impl Run {
    fn from_metrics(metrics: TextMetrics) -> Option<Self> {
        let mut run = Self {
            baseline: metrics.baseline(),
            glyphs: Vec::with_capacity(metrics.glyphs.len()),
            ..Self::default()
        };
        for glyph in metrics.glyphs {
            if run.font.is_some_and(|font| font != glyph.font_id) {
                return None;
            }
            run.font = Some(glyph.font_id);
            run.glyphs.push(Glyph {
                id: glyph.glyph_id,
                advance: [glyph.advance_x, glyph.advance_y],
                offset: [glyph.offset_x, glyph.offset_y],
                bearing_y: glyph.bearing_y,
                height: glyph.height,
            });
        }
        Some(run)
    }

    fn place(
        &self,
        origin: Point,
        scale: f32,
        mut push: impl FnMut(FontId, PositionedGlyph) -> bool,
    ) -> Option<[f32; 2]> {
        let mut x = origin.x * scale;
        let mut y = origin.y * scale;
        let mut top = y;
        let mut bottom = y;
        let inverse = 1.0 / scale;
        if let Some(font) = self.font {
            // Match FemtoVG's arithmetic in shaping space before converting
            // each position and the measured ink bounds to logical pixels.
            for glyph in &self.glyphs {
                let gx = x + glyph.offset[0];
                let gy = y + self.baseline + glyph.offset[1];
                let ink_top = gy - glyph.bearing_y;
                top = top.min(ink_top);
                bottom = bottom.max(ink_top + glyph.height);
                if !push(
                    font,
                    PositionedGlyph {
                        x: gx * inverse,
                        y: gy * inverse,
                        glyph_id: glyph.id,
                    },
                ) {
                    return None;
                }
                x += glyph.advance[0] + 0.0;
                y += glyph.advance[1];
            }
        }
        Some([top * inverse, (bottom - top) * inverse])
    }
}

struct StoredRuns<T> {
    entries: HashMap<Rc<str>, (T, usize)>,
    order: VecDeque<Rc<str>>,
    bytes: usize,
}

impl<T> Default for StoredRuns<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
        }
    }
}

impl<T> StoredRuns<T> {
    fn insert(&mut self, text: &str, run: T, glyph_bytes: usize, max_bytes: usize) {
        let bytes = text.len() + size_of::<T>() + glyph_bytes;
        if bytes > max_bytes {
            return;
        }
        while self.bytes + bytes > max_bytes {
            let Some(key) = self.order.pop_front() else {
                return;
            };
            if let Some((_, cost)) = self.entries.remove(key.as_ref()) {
                self.bytes -= cost;
            }
        }
        let key: Rc<str> = text.into();
        self.order.push_back(key.clone());
        self.entries.insert(key, (run, bytes));
        self.bytes += bytes;
    }
}

#[derive(Default)]
pub(super) struct Cache {
    font: Option<(LabelFont, f32, [usize; 2])>,
    words: StoredRuns<Run>,
    lines: StoredRuns<Run>,
    unicode_words: StoredRuns<Rc<UnicodeRun>>,
    unicode_word_index: Weak<LabelWordIndex>,
    reflow_word_index: Option<Rc<LabelWordIndex>>,
    previous_reflow_label: Weak<PreparedLabel>,
    unicode_word_slots: Vec<IndexedUnicodeWord>,
    unicode_lines: StoredRuns<Rc<UnicodeLine>>,
    unicode_ranges: UnicodeLineRanges,
    #[cfg(xrack_femtovg_glyph_samples)]
    atlas_glyphs: HashSet<(FontId, u16)>,
    #[cfg(not(xrack_femtovg_glyph_samples))]
    atlas_phases: HashMap<(FontId, u16), u32>,
    outline_font_ascents: HashMap<FontId, Option<f32>>,
    recent_positions: VecDeque<(Weak<PreparedLabel>, Placement)>,
    positioned: VecDeque<PositionedBatch>,
    unicode: Option<UnicodeLabel>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Placement {
    pub(super) origin: [u32; 2],
    pub(super) scale: u32,
    pub(super) target_height: u32,
    pub(super) registered_fonts: [usize; 2],
}

impl Placement {
    fn same_horizontal_geometry(self, other: Self) -> bool {
        self.origin[0] == other.origin[0]
            && self.scale == other.scale
            && self.registered_fonts == other.registered_fonts
    }
}

pub(super) struct Reuse {
    pub(super) positioned: bool,
    pub(super) unicode: bool,
}

#[derive(Default)]
pub(super) struct BatchScratch {
    pub(super) runs: Vec<FontRun>,
    pub(super) visible_runs: Vec<FontRun>,
    pub(super) positions: Vec<PositionedGlyph>,
}

pub(super) struct FontRun {
    pub(super) font: FontId,
    pub(super) glyphs: Range<usize>,
}

pub(super) struct PositionedBatch {
    label: Weak<PreparedLabel>,
    placement: Placement,
    pub(super) runs: Vec<FontRun>,
    pub(super) glyphs: Rc<Vec<PositionedGlyph>>,
    pub(super) lines: u64,
}

#[derive(Clone, Copy)]
struct UnicodeGlyph {
    font: FontId,
    id: u16,
    offset_x: f32,
    offset_y: f32,
    advance_x: f32,
    advance_y: f32,
    control: bool,
}

#[derive(Default)]
struct UnicodeRun {
    baseline: f32,
    glyphs: Vec<UnicodeGlyph>,
    aligned_edges: Option<[f32; 2]>,
    atlas_ready: Cell<bool>,
}

#[derive(Default)]
struct IndexedUnicodeWord {
    run: Weak<UnicodeRun>,
    #[cfg(xrack_femtovg_glyph_samples)]
    outline: WordOutline,
    #[cfg(not(xrack_femtovg_glyph_samples))]
    positioned: Option<PositionedWord>,
}

#[derive(Clone, Copy, Default)]
struct WordOutline {
    edges: [f32; 2],
    baseline: f32,
    glyph_count: u32,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
#[derive(Clone, Copy)]
struct PositionedWord {
    start: u32,
    end: f32,
    outline: WordOutline,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
#[derive(Clone, Copy)]
struct AtlasGlyphPhases {
    font: FontId,
    glyph: u16,
    phases: u32,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
pub(super) struct KnownAtlasPhases<const SLOTS: usize> {
    entries: [Option<AtlasGlyphPhases>; SLOTS],
}

#[cfg(not(xrack_femtovg_glyph_samples))]
impl<const SLOTS: usize> Default for KnownAtlasPhases<SLOTS> {
    fn default() -> Self {
        Self {
            entries: [None; SLOTS],
        }
    }
}

#[cfg(not(xrack_femtovg_glyph_samples))]
impl<const SLOTS: usize> KnownAtlasPhases<SLOTS> {
    pub(super) fn contains(
        &mut self,
        font: FontId,
        glyph: u16,
        x: f32,
        scale: f32,
        read: impl FnOnce() -> u32,
    ) -> bool {
        let slot = &mut self.entries[usize::from(glyph) % SLOTS];
        if slot.is_none_or(|entry| entry.font != font || entry.glyph != glyph) {
            *slot = Some(AtlasGlyphPhases {
                font,
                glyph,
                phases: read(),
            });
        }
        let phase = crate::glyph_masks::subpixel_phase((x * scale).fract());
        slot.is_some_and(|entry| entry.phases & (1 << (phase + 10)) != 0)
    }
}

fn visit_ltr_words(value: &str, mut visit: impl FnMut(&str) -> Option<()>) -> Option<()> {
    if let Some(mut words) = ascii_word_bounds(value) {
        words.try_for_each(visit)
    } else if single_ltr_shaping_run(value) {
        value.split_word_bounds().try_for_each(&mut visit)
    } else {
        None
    }
}

impl UnicodeRun {
    fn from_metrics(metrics: TextMetrics) -> Self {
        let baseline = metrics.baseline();
        let glyphs = metrics
            .glyphs
            .into_iter()
            .map(|glyph| UnicodeGlyph {
                font: glyph.font_id,
                id: glyph.glyph_id,
                offset_x: glyph.offset_x,
                offset_y: glyph.offset_y,
                advance_x: glyph.advance_x,
                advance_y: glyph.advance_y,
                control: glyph.c.is_control(),
            })
            .collect();
        Self {
            baseline,
            glyphs,
            aligned_edges: None,
            atlas_ready: Cell::new(false),
        }
    }
}

#[derive(Clone, Copy)]
struct VerticalGlyphInk {
    font: FontId,
    glyph: u16,
    // An unsupported glyph has no bound. An empty outline has an empty range.
    edges: Option<[f32; 2]>,
}

fn glyph_bounds<'a>(
    glyphs: impl Iterator<Item = &'a UnicodeGlyph>,
    paint: &UnicodePaint<'_>,
    baseline: f32,
) -> Option<[f32; 2]> {
    let mut ink = paint.ink.borrow_mut();
    let physical_scale = paint.scale / paint.measurement_scale;
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for glyph in glyphs {
        if glyph.advance_y != 0.0 {
            return None;
        }
        if !glyph.control {
            let slot = &mut ink[usize::from(glyph.id) % GLYPH_INK_SLOTS];
            if slot.is_none_or(|entry| entry.font != glyph.font || entry.glyph != glyph.id) {
                let edges = paint
                    .text
                    .outline_glyph(glyph.font, glyph.id)
                    .map(|outline| {
                        outline.atlas_extents(paint.size_range).map_or(
                            [f32::INFINITY, f32::NEG_INFINITY],
                            |extents| {
                                let [_, low, _, high] = extents.padded_edges();
                                [low, high]
                            },
                        )
                    });
                *slot = Some(VerticalGlyphInk {
                    font: glyph.font,
                    glyph: glyph.id,
                    edges,
                });
            }
            let [low, high] = slot.as_ref()?.edges?;
            let y = (baseline + glyph.offset_y) * physical_scale;
            top = top.min(y + low);
            bottom = bottom.max(y + high);
        }
    }
    Some([top, bottom])
}

struct UnicodeWords {
    runs: Vec<Rc<UnicodeRun>>,
    glyph_count: usize,
    baseline: f32,
    aligned_edges: Option<[f32; 2]>,
    atlas_ready: bool,
}

impl UnicodeWords {
    fn push(&mut self, word: Rc<UnicodeRun>) {
        self.glyph_count += word.glyphs.len();
        self.baseline = self.baseline.max(word.baseline);
        self.aligned_edges = self
            .aligned_edges
            .zip(word.aligned_edges)
            .map(|([top, bottom], [low, high])| [top.min(low), bottom.max(high)]);
        self.runs.push(word);
    }
}

enum UnicodeLineGlyphs {
    Shaped(UnicodeRun),
    Words(UnicodeWords),
}

impl UnicodeLineGlyphs {
    fn iter(&self) -> impl Iterator<Item = &UnicodeGlyph> {
        let (shaped, words): (&[_], &[_]) = match self {
            Self::Shaped(run) => (&run.glyphs, &[]),
            Self::Words(words) => (&[], &words.runs),
        };
        shaped
            .iter()
            .chain(words.iter().flat_map(|word| word.glyphs.iter()))
    }

    fn baseline(&self) -> f32 {
        match self {
            Self::Shaped(run) => run.baseline,
            Self::Words(words) => words.baseline,
        }
    }

    fn atlas_ready(&self) -> bool {
        match self {
            Self::Shaped(run) => run.atlas_ready.get(),
            Self::Words(words) => words.atlas_ready,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Shaped(run) => run.glyphs.len(),
            Self::Words(words) => words.glyph_count,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Shaped(run) => run.glyphs.capacity() * size_of::<UnicodeGlyph>(),
            // Retain a charge for glyph data even after the word cache evicts it.
            Self::Words(words) => {
                words.glyph_count * size_of::<UnicodeGlyph>()
                    + words.runs.capacity() * size_of::<Rc<UnicodeRun>>()
            }
        }
    }

    fn bounds(&self, paint: &UnicodePaint<'_>) -> Option<[f32; 2]> {
        let physical_scale = paint.scale / paint.measurement_scale;
        let aligned_edges = match self {
            Self::Shaped(run) => run.aligned_edges,
            Self::Words(words) => words.aligned_edges,
        };
        if let Some([top, bottom]) = aligned_edges {
            // Add the common baseline after combining the word bounds.
            let y = self.baseline() * physical_scale;
            return Some([y + top, y + bottom]);
        }
        glyph_bounds(self.iter(), paint, self.baseline())
    }
}

pub(super) struct UnicodeLine {
    glyphs: UnicodeLineGlyphs,
    bounds: Option<[f32; 2]>,
    atlas_origin_x: Cell<Option<u32>>,
}

pub(super) struct PlacedGlyph {
    pub(super) font: FontId,
    pub(super) position: PositionedGlyph,
    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) starts_run: bool,
}

impl UnicodeLine {
    pub(super) fn atlas_ready_at(&self, x: f32) -> bool {
        self.glyphs.atlas_ready() || self.atlas_origin_x.get() == Some(x.to_bits())
    }

    pub(super) fn remember_atlas_origin(&self, x: f32) {
        self.atlas_origin_x.set(Some(x.to_bits()));
    }

    pub(super) fn offscreen(&self, y: f32, scale: f32, target_height: f32) -> bool {
        self.bounds
            .is_some_and(|bounds| outside_target(bounds, y * scale, target_height))
    }

    pub(super) fn place<'a>(
        &'a self,
        origin: Point,
        measurement_scale: f32,
        mut needs_position: impl FnMut(FontId, u16, f32) -> bool + 'a,
    ) -> impl Iterator<Item = PlacedGlyph> + 'a {
        let mut x = origin.x * measurement_scale;
        let mut y = origin.y * measurement_scale;
        let inverse = 1.0 / measurement_scale;
        let baseline = self.glyphs.baseline();
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let mut previous_font = None;
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let mut starts_run = true;
        self.glyphs.iter().filter_map(move |glyph| {
            let placed_x = (x + glyph.offset_x) * inverse;
            let glyph_y = y;
            x += glyph.advance_x + 0.0;
            y += glyph.advance_y;
            if glyph.control {
                return None;
            }
            #[cfg(not(xrack_femtovg_glyph_samples))]
            {
                // A font boundary still applies when every glyph in that run
                // is skipped. Carry it to the next emitted glyph.
                starts_run |= previous_font.replace(glyph.font) != Some(glyph.font);
            }
            if !needs_position(glyph.font, glyph.id, placed_x) {
                return None;
            }
            Some(PlacedGlyph {
                font: glyph.font,
                position: PositionedGlyph {
                    x: placed_x,
                    y: (glyph_y + baseline + glyph.offset_y) * inverse,
                    glyph_id: glyph.id,
                },
                #[cfg(not(xrack_femtovg_glyph_samples))]
                starts_run: std::mem::replace(&mut starts_run, false),
            })
        })
    }
}

fn outside_target([top, bottom]: [f32; 2], y: f32, height: f32) -> bool {
    // Leave room for rounding in the glyph's final device position.
    y.abs().max(top.abs()).max(bottom.abs()) <= 1_048_576.0
        && (y + bottom < -2.0 || y + top > height + 2.0)
}

const GLYPH_INK_SLOTS: usize = 256;

pub(super) struct UnicodePaint<'a> {
    text: &'a TextSystem,
    font: LabelFont,
    paint: Paint,
    pub(super) measurement_scale: f32,
    scale: f32,
    size_range: [f32; 2],
    ink: RefCell<[Option<VerticalGlyphInk>; GLYPH_INK_SLOTS]>,
    height: f32,
    source: Option<&'a str>,
    word_index: Option<Rc<LabelWordIndex>>,
    indexed_measurement: bool,
    ascii_metrics: OnceCell<Option<AsciiMetrics>>,
    #[cfg(not(xrack_femtovg_glyph_samples))]
    native_atlas_phases: RefCell<KnownAtlasPhases<512>>,
}

impl<'a> UnicodePaint<'a> {
    pub(super) fn new(
        text: &'a TextSystem,
        paint: &Paint,
        scale: f32,
        height: f32,
        label: &'a PreparedLabel,
    ) -> Self {
        let measurement_scale = crate::cell_glyphs::measurement_scale(scale);
        let source = label.source_text();
        let word_index = source.and_then(|source| text.label_word_index(source));
        Self {
            text,
            font: label.font,
            paint: paint
                .clone()
                .with_font_size(paint.font_size() * measurement_scale),
            measurement_scale,
            scale,
            size_range: atlas_size_range(paint.font_size(), scale),
            // Repeated glyphs across freshly wrapped words share the same
            // physical atlas padding, including when their positions differ.
            ink: RefCell::new([None; GLYPH_INK_SLOTS]),
            height,
            source,
            indexed_measurement: word_index.is_some(),
            word_index,
            ascii_metrics: OnceCell::new(),
            #[cfg(not(xrack_femtovg_glyph_samples))]
            native_atlas_phases: RefCell::default(),
        }
    }

    pub(super) fn select_word_index(
        &mut self,
        cache: &mut Cache,
        label: &Rc<PreparedLabel>,
    ) -> bool {
        if self.word_index.is_some() {
            cache.reflow_word_index = None;
        } else {
            self.word_index = cache.word_index_for_reflow(label);
        }
        let Some(index) = &self.word_index else {
            return false;
        };
        if !std::ptr::eq(cache.unicode_word_index.as_ptr(), Rc::as_ptr(index)) {
            cache.unicode_word_index = Rc::downgrade(index);
            cache.unicode_word_slots.clear();
            cache
                .unicode_word_slots
                .resize_with(index.len(), IndexedUnicodeWord::default);
        }
        true
    }

    fn visit_words(
        &self,
        value: &str,
        mut visit: impl FnMut(&str, Option<usize>) -> Option<()>,
    ) -> Option<()> {
        if let Some(source) = self.source
            && let Some(index) = &self.word_index
            && let Some(span) = index.span(source, value)
        {
            return index.visit(source, span, visit);
        }
        visit_ltr_words(value, |word| visit(word, None))
    }

    fn shape(&self, cache: &mut Cache, value: &str) -> Result<Option<Rc<UnicodeLine>>, String> {
        cache.measure_unicode_line(value, self)
    }
}

pub(super) enum UnicodeLabelLine {
    Shaped(Rc<UnicodeLine>),
    Deferred([f32; 2]),
    #[cfg(not(xrack_femtovg_glyph_samples))]
    Seeds(Box<OffscreenSeeds>),
}

#[cfg(not(xrack_femtovg_glyph_samples))]
pub(super) struct OffscreenSeeds {
    bounds: [f32; 2],
    baseline: f32,
    glyphs: Vec<OffscreenGlyph>,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
struct OffscreenGlyph {
    font: FontId,
    id: u16,
    x: f32,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
impl OffscreenSeeds {
    pub(super) fn positions(
        self,
        origin_y: f32,
        measurement_scale: f32,
    ) -> impl Iterator<Item = (FontId, PositionedGlyph)> {
        let y = (origin_y * measurement_scale + self.baseline) * (1.0 / measurement_scale);
        self.glyphs.into_iter().map(move |glyph| {
            (
                glyph.font,
                PositionedGlyph {
                    x: glyph.x,
                    y,
                    glyph_id: glyph.id,
                },
            )
        })
    }
}

#[derive(Default)]
struct UnicodeLineRanges {
    source: Weak<PreparedLabel>,
    entries: Vec<Option<UnicodeRangeEntry>>,
}

struct UnicodeRangeEntry {
    key: u64,
    line: UnicodeRangeLine,
}

enum UnicodeRangeLine {
    Shaped(Weak<UnicodeLine>),
    Deferred {
        bounds: [f32; 2],
        count: usize,
        #[cfg(not(xrack_femtovg_glyph_samples))]
        origin_x: u32,
    },
}

impl UnicodeLineRanges {
    const SLOTS: usize = 8192;

    fn prepare<'a>(&mut self, label: &'a Rc<PreparedLabel>) -> Option<&'a str> {
        let source = label.source_text().filter(|source| source.len() >= 1024)?;
        if !std::ptr::eq(self.source.as_ptr(), Rc::as_ptr(label))
            && !self
                .source
                .upgrade()
                .is_some_and(|old| old.shares_text_with(label))
        {
            self.entries.fill_with(|| None);
        }
        self.source = Rc::downgrade(label);
        self.entries.resize_with(Self::SLOTS, || None);
        Some(source)
    }

    fn key(source: &str, line: &str) -> Option<u64> {
        // The prepared plan owns the source. Generated text, such as an
        // ellipsized line, does not borrow a range from it.
        let start = line.as_ptr().addr().checked_sub(source.as_ptr().addr())?;
        let end = start.checked_add(line.len())?;
        (start < end && end <= source.len()).then(|| ((start as u64) << 32) | end as u64)
    }

    fn slot(key: u64) -> usize {
        (key.wrapping_mul(0x9e3779b97f4a7c15) >> (u64::BITS - Self::SLOTS.ilog2())) as usize
    }

    fn get(
        &self,
        key: u64,
        origin: Point,
        paint: &UnicodePaint<'_>,
    ) -> Option<(UnicodeLabelLine, usize)> {
        let entry = self.entries[Self::slot(key)].as_ref()?;
        if entry.key != key {
            return None;
        }
        match &entry.line {
            UnicodeRangeLine::Shaped(line) => {
                let line = line.upgrade()?;
                let count = line.glyphs.len();
                Some((UnicodeLabelLine::Shaped(line), count))
            }
            UnicodeRangeLine::Deferred {
                bounds,
                count,
                #[cfg(not(xrack_femtovg_glyph_samples))]
                origin_x,
            } => {
                // Stock FemtoVG keys its masks by the subpixel phase. The
                // proof covers the exact horizontal position that seeded it.
                #[cfg(not(xrack_femtovg_glyph_samples))]
                if *origin_x != origin.x.to_bits() {
                    return None;
                }
                outside_target(*bounds, origin.y * paint.scale, paint.height)
                    .then_some((UnicodeLabelLine::Deferred(*bounds), *count))
            }
        }
    }

    fn insert(&mut self, key: u64, line: &UnicodeLabelLine, count: usize, _origin_x: f32) {
        let line = match line {
            UnicodeLabelLine::Shaped(line) => UnicodeRangeLine::Shaped(Rc::downgrade(line)),
            UnicodeLabelLine::Deferred(bounds) => UnicodeRangeLine::Deferred {
                bounds: *bounds,
                count,
                #[cfg(not(xrack_femtovg_glyph_samples))]
                origin_x: _origin_x.to_bits(),
            },
            // The saved glyphs refer to tiles that the frame still has to seed.
            #[cfg(not(xrack_femtovg_glyph_samples))]
            UnicodeLabelLine::Seeds(_) => return,
        };
        self.entries[Self::slot(key)] = Some(UnicodeRangeEntry { key, line });
    }
}

impl UnicodeLabelLine {
    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) fn take_offscreen_seeds(
        &mut self,
        paint: &UnicodePaint<'_>,
        y: f32,
    ) -> Option<OffscreenSeeds> {
        let Self::Seeds(seeds) = self else {
            return None;
        };
        let bounds = seeds.bounds;
        if !outside_target(bounds, y * paint.scale, paint.height) {
            return None;
        }
        match std::mem::replace(self, Self::Deferred(bounds)) {
            Self::Seeds(seeds) => Some(*seeds),
            _ => None,
        }
    }

    pub(super) fn still_offscreen(&self, paint: &UnicodePaint<'_>, y: f32) -> bool {
        matches!(self, Self::Deferred(bounds) if outside_target(*bounds, y * paint.scale, paint.height))
    }

    pub(super) fn shape(
        &mut self,
        cache: &mut Cache,
        paint: &UnicodePaint<'_>,
        value: &str,
    ) -> Result<Option<&UnicodeLine>, String> {
        if !matches!(self, Self::Shaped(_)) {
            let Some(line) = paint.shape(cache, value)? else {
                return Ok(None);
            };
            *self = Self::Shaped(line);
        }
        match self {
            Self::Shaped(line) => Ok(Some(line)),
            _ => Ok(None),
        }
    }

    pub(super) fn remember_atlas_origin(&self, x: f32) {
        if let Self::Shaped(line) = self {
            line.remember_atlas_origin(x);
        }
    }
}

pub(super) struct UnicodeLabel {
    label: Weak<PreparedLabel>,
    placement: Placement,
    pub(super) lines: Vec<UnicodeLabelLine>,
    pub(super) allow_outline_masks: bool,
    pub(super) drew_outline_masks: bool,
}

impl UnicodeLabel {
    pub(super) fn measure(
        cache: &mut Cache,
        label: &Rc<PreparedLabel>,
        placement: Placement,
        lines: &[(Point, &str)],
        paint: &UnicodePaint<'_>,
    ) -> Result<Option<Self>, String> {
        const MAX_LINES: usize = 4096;
        const DEFAULT_GLYPH_BUDGET: usize = 64 * 1024;
        if lines.len() > MAX_LINES {
            return Ok(None);
        }
        let mut glyph_count = 0;
        let mut measured = Vec::with_capacity(lines.len());
        let source = cache.unicode_ranges.prepare(label);
        // The cached source already has a byte budget. Allow a proportional
        // shaping budget so long outline labels can still cull offscreen runs.
        let glyph_budget = source.map_or(DEFAULT_GLYPH_BUDGET, |source| {
            source.len().max(DEFAULT_GLYPH_BUDGET)
        });
        for (origin, value) in lines {
            let key = source.and_then(|source| UnicodeLineRanges::key(source, value));
            let cached = key.and_then(|key| cache.unicode_ranges.get(key, *origin, paint));
            let (line, count) = if let Some(cached) = cached {
                cached
            } else {
                let (line, count) = if let Some((line, _)) = cache.unicode_lines.entries.get(*value)
                {
                    (UnicodeLabelLine::Shaped(line.clone()), line.glyphs.len())
                } else if let Some(line) = cache.deferred_outline(value, *origin, paint) {
                    line
                } else {
                    let Some(line) = paint.shape(cache, value)? else {
                        return Ok(None);
                    };
                    let count = line.glyphs.len();
                    (UnicodeLabelLine::Shaped(line), count)
                };
                if let Some(key) = key {
                    cache.unicode_ranges.insert(key, &line, count, origin.x);
                }
                (line, count)
            };
            glyph_count += count;
            if glyph_count > glyph_budget {
                return Ok(None);
            }
            measured.push(line);
        }
        Ok(Some(Self {
            label: Rc::downgrade(label),
            placement,
            lines: measured,
            allow_outline_masks: true,
            drew_outline_masks: false,
        }))
    }

    pub(super) fn update_placement(&mut self, placement: Placement) -> bool {
        let moved_vertically = self.placement.origin[1] != placement.origin[1];
        self.placement = placement;
        moved_vertically
    }
}

impl Cache {
    fn word_index_for_reflow(&mut self, label: &Rc<PreparedLabel>) -> Option<Rc<LabelWordIndex>> {
        // The index has a slot for each source byte and each word. Retain
        // one bounded document when its text survives a change of line plan.
        let source = label
            .source_text()
            .filter(|source| (4096..=256 * 1024).contains(&source.len()));
        let previous = self.previous_reflow_label.upgrade();
        self.previous_reflow_label = Rc::downgrade(label);
        match (source, previous) {
            (Some(source), Some(previous)) if previous.shares_text_with(label) => {
                if !Rc::ptr_eq(&previous, label) {
                    self.reflow_word_index
                        .get_or_insert_with(|| Rc::new(LabelWordIndex::new(source)));
                }
            }
            _ => self.reflow_word_index = None,
        }
        self.reflow_word_index.clone()
    }

    fn with_unicode_word<T>(
        &mut self,
        word: &str,
        id: Option<usize>,
        visit: impl FnOnce(&Self, &Rc<UnicodeRun>) -> T,
    ) -> Option<T> {
        if let Some(run) = id
            .and_then(|id| self.unicode_word_slots.get(id))
            .and_then(|slot| slot.run.upgrade())
        {
            return Some(visit(self, &run));
        }
        let (run, _) = self.unicode_words.entries.get(word)?;
        if let Some(slot) = id.and_then(|id| self.unicode_word_slots.get_mut(id)) {
            slot.run = Rc::downgrade(run);
        }
        Some(visit(self, run))
    }

    #[cfg(xrack_femtovg_glyph_samples)]
    fn outline_for_run(&self, run: &UnicodeRun) -> Option<WordOutline> {
        let edges = run.aligned_edges?;
        self.run_in_atlas(run).then_some(WordOutline {
            edges,
            baseline: run.baseline,
            glyph_count: run.glyphs.len() as u32,
        })
    }

    #[cfg(xrack_femtovg_glyph_samples)]
    fn word_outline(&mut self, word: &str, id: Option<usize>) -> Option<WordOutline> {
        if let Some(slot) = id.and_then(|id| self.unicode_word_slots.get(id))
            && slot.outline.glyph_count != 0
        {
            return Some(slot.outline);
        }
        let outline =
            self.with_unicode_word(word, id, |cache, run| cache.outline_for_run(run))??;
        if let Some(slot) = id.and_then(|id| self.unicode_word_slots.get_mut(id)) {
            // Atlas residency lasts for the selected canvas and font scale.
            // Keep the bounds even after the shaped glyph array is evicted.
            slot.outline = outline;
        }
        Some(outline)
    }

    fn deferred_outline(
        &mut self,
        value: &str,
        origin: Point,
        paint: &UnicodePaint<'_>,
    ) -> Option<(UnicodeLabelLine, usize)> {
        #[cfg(not(xrack_femtovg_glyph_samples))]
        {
            self.seed_offscreen_glyphs(value, origin, paint)
        }
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            let y = origin.y * paint.scale;
            if self.atlas_glyphs.is_empty()
                || (0.0..=paint.height).contains(&y)
                || paint.paint.text_baseline() != Baseline::Top
            {
                return None;
            }
            let empty = ([f32::INFINITY, f32::NEG_INFINITY], 0.0, 0);
            let include = |([top, bottom], baseline, count): ([f32; 2], f32, usize),
                           outline: WordOutline| {
                (
                    [top.min(outline.edges[0]), bottom.max(outline.edges[1])],
                    baseline.max(outline.baseline),
                    count + outline.glyph_count as usize,
                )
            };
            let indexed =
                paint
                    .word_index
                    .as_ref()
                    .zip(paint.source)
                    .and_then(|(index, source)| {
                        let words = self
                            .unicode_word_slots
                            .get(index.full_words(source, value)?)?;
                        words.iter().try_fold(empty, |bounds, word| {
                            (word.outline.glyph_count != 0).then(|| include(bounds, word.outline))
                        })
                    });
            let ([top, bottom], baseline, count) = if let Some(bounds) = indexed {
                bounds
            } else {
                // A new line may split a word or use a different fallback
                // glyph. Resolve its exact pieces before using the proof.
                let mut bounds = empty;
                paint.visit_words(value, |word, id| {
                    bounds = include(bounds, self.word_outline(word, id)?);
                    Some(())
                })?;
                bounds
            };
            if baseline < f32::EPSILON {
                return None;
            }
            let baseline = baseline * (paint.scale / paint.measurement_scale);
            let bounds = [baseline + top, baseline + bottom];
            outside_target(bounds, y, paint.height)
                .then_some((UnicodeLabelLine::Deferred(bounds), count))
        }
    }

    #[cfg(not(xrack_femtovg_glyph_samples))]
    fn seed_offscreen_glyphs(
        &mut self,
        value: &str,
        origin: Point,
        paint: &UnicodePaint<'_>,
    ) -> Option<(UnicodeLabelLine, usize)> {
        const MAX_NEW_GLYPHS_PER_LINE: usize = 128;

        let y = origin.y * paint.scale;
        if !paint.indexed_measurement
            || self.atlas_phases.is_empty()
            || (0.0..=paint.height).contains(&y)
            || !origin.x.is_finite()
            || paint.paint.text_baseline() != Baseline::Top
        {
            return None;
        }
        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        let mut baseline = 0.0_f32;
        let mut count = 0;
        let mut x = origin.x * paint.measurement_scale;
        let inverse = 1.0 / paint.measurement_scale;
        let mut glyphs = Vec::new();
        let mut phases = paint.native_atlas_phases.borrow_mut();
        paint.visit_words(value, |word, id| {
            let start = x.to_bits();
            let cached = id
                .and_then(|id| self.unicode_word_slots.get(id))
                .and_then(|word| word.positioned);
            let outline = if let Some(cached) = cached.filter(|word| word.start == start) {
                x = cached.end;
                cached.outline
            } else {
                let before = glyphs.len();
                let outline = self.with_unicode_word(word, id, |cache, run| {
                    // These words have no vertical offsets or advances. The
                    // line's maximum font ascent supplies their baseline.
                    let edges = run.aligned_edges?;
                    for glyph in &run.glyphs {
                        if !glyph.control {
                            let placed = (x + glyph.offset_x) * inverse;
                            if !phases.contains(glyph.font, glyph.id, placed, paint.scale, || {
                                cache.known_atlas_phases(glyph.font, glyph.id)
                            }) {
                                if glyphs.len() == MAX_NEW_GLYPHS_PER_LINE {
                                    return None;
                                }
                                glyphs.push(OffscreenGlyph {
                                    font: glyph.font,
                                    id: glyph.id,
                                    x: placed,
                                });
                            }
                        }
                        x += glyph.advance_x + 0.0;
                    }
                    Some(WordOutline {
                        edges,
                        baseline: run.baseline,
                        glyph_count: run.glyphs.len() as u32,
                    })
                })??;
                // Reuse the exact end position, including additions rounded
                // by the original placement loop. A miss still needs seeds.
                if glyphs.len() == before
                    && let Some(slot) = id.and_then(|id| self.unicode_word_slots.get_mut(id))
                {
                    slot.positioned = Some(PositionedWord {
                        start,
                        end: x,
                        outline,
                    });
                }
                outline
            };
            top = top.min(outline.edges[0]);
            bottom = bottom.max(outline.edges[1]);
            baseline = baseline.max(outline.baseline);
            count += outline.glyph_count as usize;
            Some(())
        })?;
        let baseline_physical = baseline * (paint.scale / paint.measurement_scale);
        let bounds = [top + baseline_physical, bottom + baseline_physical];
        if baseline < f32::EPSILON || !x.is_finite() || !outside_target(bounds, y, paint.height) {
            return None;
        }
        let render = if glyphs.is_empty() {
            UnicodeLabelLine::Deferred(bounds)
        } else {
            UnicodeLabelLine::Seeds(Box::new(OffscreenSeeds {
                bounds,
                baseline,
                glyphs,
            }))
        };
        Some((render, count))
    }

    fn run_in_atlas(&self, run: &UnicodeRun) -> bool {
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            // The patched vector atlas uses one tile per glyph at this font
            // size and scale, independent of the horizontal subpixel phase.
            let ready = run.atlas_ready.get()
                || (!self.atlas_glyphs.is_empty()
                    && run.glyphs.iter().all(|glyph| {
                        glyph.control || self.atlas_glyphs.contains(&(glyph.font, glyph.id))
                    }));
            run.atlas_ready.set(ready);
            ready
        }
        #[cfg(not(xrack_femtovg_glyph_samples))]
        {
            let _ = run;
            false
        }
    }

    pub(super) fn remember_atlas_glyphs(
        &mut self,
        runs: &[FontRun],
        glyphs: &[PositionedGlyph],
        scale: f32,
    ) {
        const MAX_GLYPHS: usize = 16 * 1024;
        #[cfg(xrack_femtovg_glyph_samples)]
        let _ = scale;
        for run in runs {
            for glyph in &glyphs[run.glyphs.clone()] {
                #[cfg(xrack_femtovg_glyph_samples)]
                let count = self.atlas_glyphs.len();
                #[cfg(not(xrack_femtovg_glyph_samples))]
                let count = self.atlas_phases.len();
                if count == MAX_GLYPHS {
                    return;
                }
                #[cfg(xrack_femtovg_glyph_samples)]
                self.atlas_glyphs.insert((run.font, glyph.glyph_id));
                #[cfg(not(xrack_femtovg_glyph_samples))]
                {
                    let phase = crate::glyph_masks::subpixel_phase((glyph.x * scale).fract());
                    *self
                        .atlas_phases
                        .entry((run.font, glyph.glyph_id))
                        .or_default() |= 1 << (phase + 10);
                }
            }
        }
    }

    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) fn known_atlas_phases(&self, font: FontId, glyph: u16) -> u32 {
        self.atlas_phases.get(&(font, glyph)).copied().unwrap_or(0)
    }

    fn measure_unicode_line(
        &mut self,
        value: &str,
        paint: &UnicodePaint<'_>,
    ) -> Result<Option<Rc<UnicodeLine>>, String> {
        if let Some((line, _)) = self.unicode_lines.entries.get(value) {
            return Ok(Some(line.clone()));
        }
        let context = paint.text.context();
        let glyphs = if let Some(words) = self.combine_unicode_words(value, paint) {
            UnicodeLineGlyphs::Words(words)
        } else {
            let metrics = context
                .measure_text(0.0, 0.0, value, &paint.paint)
                .map_err(|error| format!("text shaping failed: {error}"))?;
            let mut previous = None;
            for glyph in &metrics.glyphs {
                if !glyph.c.is_control() && previous != Some(glyph.font_id) {
                    if self
                        .outline_font_ascent(context, glyph.font_id, paint.paint.font_size())
                        .is_none()
                    {
                        return Ok(None);
                    }
                    previous = Some(glyph.font_id);
                }
            }
            let run = UnicodeRun::from_metrics(metrics);
            self.run_in_atlas(&run);
            UnicodeLineGlyphs::Shaped(run)
        };
        let line = UnicodeLine {
            bounds: glyphs.bounds(paint),
            glyphs,
            atlas_origin_x: Cell::new(None),
        };
        let bytes = size_of::<UnicodeLine>() + line.glyphs.bytes();
        let line = Rc::new(line);
        self.unicode_lines
            .insert(value, line.clone(), bytes, 2 * 1024 * 1024);
        Ok(Some(line))
    }

    fn combine_unicode_words(
        &mut self,
        value: &str,
        paint: &UnicodePaint<'_>,
    ) -> Option<UnicodeWords> {
        if paint.paint.text_baseline() != Baseline::Top {
            return None;
        }
        // FemtoVG shapes the UAX #29 words of a single LTR bidi run
        // independently, then uses the largest participating font ascent.
        const MIN_WORD_BYTES: usize = 512 * 1024;
        const MAX_WORD_BYTES: usize = 2 * 1024 * 1024;
        let word_budget =
            paint
                .word_index
                .as_ref()
                .zip(paint.source)
                .map_or(MIN_WORD_BYTES, |(_, source)| {
                    // An actively reflowed document can use the same words at many
                    // widths. Keep more of them without preallocating the budget.
                    source
                        .len()
                        .saturating_mul(size_of::<UnicodeGlyph>())
                        .clamp(MIN_WORD_BYTES, MAX_WORD_BYTES)
                });
        let mut line = UnicodeWords {
            runs: Vec::new(),
            glyph_count: 0,
            baseline: 0.0,
            aligned_edges: Some([f32::INFINITY, f32::NEG_INFINITY]),
            atlas_ready: false,
        };
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            line.atlas_ready = !self.atlas_glyphs.is_empty();
        }
        paint.visit_words(value, |word, id| {
            let cached = self.with_unicode_word(word, id, |cache, run| {
                if line.atlas_ready {
                    line.atlas_ready = cache.run_in_atlas(run);
                }
                line.push(run.clone());
            });
            if cached.is_some() {
                return Some(());
            }
            let mut run = self.shape_unicode_word(word, paint)?;
            if run
                .glyphs
                .iter()
                .all(|glyph| glyph.control || glyph.offset_y == 0.0)
            {
                run.aligned_edges = glyph_bounds(run.glyphs.iter(), paint, 0.0);
            }
            let bytes = size_of::<UnicodeRun>() + run.glyphs.capacity() * size_of::<UnicodeGlyph>();
            if line.atlas_ready {
                line.atlas_ready = self.run_in_atlas(&run);
            }
            #[cfg(xrack_femtovg_glyph_samples)]
            if let Some(id) = id
                && let Some(outline) = self.outline_for_run(&run)
                && let Some(slot) = self.unicode_word_slots.get_mut(id)
            {
                slot.outline = outline;
            }
            let run = Rc::new(run);
            line.push(run.clone());
            if let Some(slot) = id.and_then(|id| self.unicode_word_slots.get_mut(id)) {
                slot.run = Rc::downgrade(&run);
            }
            self.unicode_words.insert(word, run, bytes, word_budget);
            Some(())
        })?;
        // The full shaper supplies the primary font's ascent when needed.
        (line.baseline >= f32::EPSILON).then_some(line)
    }

    fn shape_unicode_word(&mut self, word: &str, paint: &UnicodePaint<'_>) -> Option<UnicodeRun> {
        let context = paint.text.context();
        let size = paint.paint.font_size();
        // Word offsets are useful for combining text even when the width
        // measurement path has not enabled its shaping shortcuts.
        let indexed_glyphs = paint
            .word_index
            .as_ref()
            .filter(|_| paint.indexed_measurement)
            .and_then(|_| {
                paint
                    .text
                    .ascii_metrics_for_word(paint.font, word, size, &paint.ascii_metrics)
                    .and_then(|metrics| {
                        metrics.word(word, |font, id, advance_x| UnicodeGlyph {
                            font,
                            id,
                            offset_x: 0.0,
                            offset_y: 0.0,
                            advance_x,
                            advance_y: 0.0,
                            control: false,
                        })
                    })
                    .or_else(|| {
                        paint.text.with_primary_label_word(
                            paint.font,
                            word,
                            size,
                            |glyphs, scale| {
                                glyphs
                                    .iter()
                                    .map(|glyph| UnicodeGlyph {
                                        font: glyph.font,
                                        id: glyph.id,
                                        offset_x: glyph.offset[0] * scale,
                                        offset_y: glyph.offset[1] * scale,
                                        advance_x: glyph.advance[0] * scale,
                                        advance_y: glyph.advance[1] * scale,
                                        control: !glyph.drawable,
                                    })
                                    .collect()
                            },
                        )
                    })
            });
        if let Some(glyphs) = indexed_glyphs
            && let Some(first) = glyphs.first()
        {
            let baseline = self.outline_font_ascent(context, first.font, size)?;
            return Some(UnicodeRun {
                baseline,
                glyphs,
                ..UnicodeRun::default()
            });
        }
        let metrics = context.measure_text(0.0, 0.0, word, &paint.paint).ok()?;
        let mut ascent: f32 = 0.0;
        let mut previous = None;
        for glyph in &metrics.glyphs {
            if previous != Some(glyph.font_id) {
                ascent = ascent.max(self.outline_font_ascent(context, glyph.font_id, size)?);
                previous = Some(glyph.font_id);
            }
        }
        let mut run = UnicodeRun::from_metrics(metrics);
        run.baseline = ascent;
        Some(run)
    }

    fn outline_font_ascent(
        &mut self,
        context: &TextContext,
        font: FontId,
        size: f32,
    ) -> Option<f32> {
        *self.outline_font_ascents.entry(font).or_insert_with(|| {
            if !context.font_variation_axes(font).ok()?.is_empty() {
                return None;
            }
            // Use the font's unvaried ascent, as FemtoVG does during layout.
            context
                .measure_font(&Paint::default().with_font(&[font]).with_font_size(size))
                .ok()
                .map(|metrics| metrics.ascender())
        })
    }

    pub(super) fn reset_canvas(&mut self) {
        self.recent_positions.clear();
        self.positioned.clear();
        self.unicode = None;
        self.unicode_lines = StoredRuns::default();
        self.unicode_ranges = UnicodeLineRanges::default();
        self.unicode_word_index = Weak::new();
        self.reflow_word_index = None;
        self.previous_reflow_label = Weak::new();
        self.unicode_word_slots.clear();
        #[cfg(not(xrack_femtovg_glyph_samples))]
        self.atlas_phases.clear();
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            self.atlas_glyphs.clear();
            self.unicode_words = StoredRuns::default();
        }
    }

    // A recurring placement can amortize the extra copy needed to retain glyphs.
    pub(super) fn observe_position(
        &mut self,
        label: &Rc<PreparedLabel>,
        placement: Placement,
    ) -> Reuse {
        const OBSERVED_POSITIONS: usize = 8;

        let same_label =
            |previous: &Weak<PreparedLabel>| std::ptr::eq(previous.as_ptr(), Rc::as_ptr(label));
        let unicode = self
            .recent_positions
            .front()
            .is_some_and(|(previous, prior)| {
                prior.same_horizontal_geometry(placement)
                    && (same_label(previous)
                        || previous.upgrade().is_some_and(|old| {
                            old.font == label.font && old.shares_text_with(label)
                        }))
            });
        let previous = self
            .recent_positions
            .iter()
            .position(|(old, prior)| *prior == placement && same_label(old));
        if previous != Some(0) {
            let observed = if let Some(index) = previous {
                self.recent_positions.remove(index)
            } else {
                self.recent_positions
                    .retain(|(old, _)| old.strong_count() != 0);
                None
            };
            self.recent_positions.truncate(OBSERVED_POSITIONS - 1);
            self.recent_positions
                .push_front(observed.unwrap_or_else(|| (Rc::downgrade(label), placement)));
        }
        Reuse {
            positioned: previous.is_some(),
            unicode,
        }
    }

    pub(super) fn take_unicode(
        &mut self,
        label: &Rc<PreparedLabel>,
        placement: Placement,
        indexed_words: bool,
    ) -> Option<UnicodeLabel> {
        let batch = self.unicode.as_ref()?;
        if batch.placement.same_horizontal_geometry(placement)
            && std::ptr::eq(batch.label.as_ptr(), Rc::as_ptr(label))
        {
            return self.unicode.take();
        }
        // The previous lines own shaped words whose indexed entries are weak.
        // Keep those words available while building the new line boundaries.
        if !indexed_words {
            self.unicode = None;
        }
        None
    }

    pub(super) fn remember_unicode(&mut self, batch: UnicodeLabel) {
        self.unicode = Some(batch);
    }

    pub(super) fn positioned(
        &mut self,
        label: &Rc<PreparedLabel>,
        placement: Placement,
    ) -> Option<&PositionedBatch> {
        let index = self.positioned.iter().position(|batch| {
            batch.placement == placement && std::ptr::eq(batch.label.as_ptr(), Rc::as_ptr(label))
        })?;
        if index != 0 {
            let batch = self.positioned.remove(index)?;
            self.positioned.push_front(batch);
        }
        self.positioned.front()
    }

    pub(super) const MAX_POSITIONED_GLYPHS: usize = 16 * 1024;
    pub(super) const MAX_POSITIONED_RUNS: usize = 4 * 1024;

    pub(super) fn remember_positioned(
        &mut self,
        label: &Rc<PreparedLabel>,
        placement: Placement,
        runs: Vec<FontRun>,
        glyphs: Vec<PositionedGlyph>,
        lines: u64,
    ) {
        const MAX_BATCHES: usize = 4;

        if glyphs.capacity() > Self::MAX_POSITIONED_GLYPHS
            || runs.capacity() > Self::MAX_POSITIONED_RUNS
        {
            return;
        }
        // Returning to the same layout and placement can replay earlier glyph
        // positions. All retained batches share the original memory limits.
        self.positioned
            .retain(|batch| batch.label.strong_count() != 0);
        self.positioned.truncate(MAX_BATCHES - 1);
        let mut glyph_slots = glyphs.capacity();
        let mut run_slots = runs.capacity();
        for batch in &self.positioned {
            glyph_slots += batch.glyphs.capacity();
            run_slots += batch.runs.capacity();
        }
        while glyph_slots > Self::MAX_POSITIONED_GLYPHS || run_slots > Self::MAX_POSITIONED_RUNS {
            let Some(evicted) = self.positioned.pop_back() else {
                return;
            };
            glyph_slots -= evicted.glyphs.capacity();
            run_slots -= evicted.runs.capacity();
        }
        self.positioned.push_front(PositionedBatch {
            label: Rc::downgrade(label),
            placement,
            runs,
            glyphs: Rc::new(glyphs),
            lines,
        });
    }

    pub(super) fn select(&mut self, font: LabelFont, scale: f32, registered: [usize; 2]) {
        let key = Some((font, scale, registered));
        if self.font != key {
            *self = Self {
                font: key,
                recent_positions: std::mem::take(&mut self.recent_positions),
                ..Self::default()
            };
        }
    }

    pub(super) fn place(
        &mut self,
        context: &TextContext,
        paint: &Paint,
        text: &str,
        origin: Point,
        scale: f32,
        push: impl FnMut(FontId, PositionedGlyph) -> bool,
    ) -> Option<[f32; 2]> {
        if let Some((line, _)) = self.lines.entries.get(text) {
            return line.place(origin, scale, push);
        }
        // Pinned FemtoVG shapes printable ASCII one UAX #29 word at a time
        // in source order. Reuse those same glyphs on new line boundaries.
        let mut line = Run {
            glyphs: Vec::with_capacity(text.len()),
            ..Run::default()
        };
        for word in text.split_word_bounds() {
            if !self.words.entries.contains_key(word) {
                let run = Run::from_metrics(context.measure_text(0.0, 0.0, word, paint).ok()?)?;
                let bytes = run.glyphs.capacity() * size_of::<Glyph>();
                self.words.insert(word, run, bytes, 256 * 1024);
            }
            let (word, _) = self.words.entries.get(word)?;
            if let Some(font) = word.font {
                if line.font.is_some_and(|previous| previous != font) {
                    return None;
                }
                line.font = Some(font);
                line.baseline = word.baseline;
                line.glyphs.extend_from_slice(&word.glyphs);
            }
        }
        let placed = line.place(origin, scale, push);
        let bytes = line.glyphs.capacity() * size_of::<Glyph>();
        self.lines.insert(text, line, bytes, 2 * 1024 * 1024);
        placed
    }
}

mod tests {
    #[test]
    fn reflow_word_index_follows_the_prepared_source() {
        use super::{Cache, TextSystem};
        use std::rc::Rc;
        use xrack_layout::Size;
        use xrack_runtime::{Node, TextWrap};

        let mut node = Node::new("Label", None);
        node.properties.insert("wrap", TextWrap::WordWrap.into());
        node.properties
            .insert("text", "用同一份文本验证换行。\n".repeat(256).into());
        let system = TextSystem::bundled_only();
        let first = system.prepared_label(&node, Size::new(320.0, 800.0), None);
        let second = system.prepared_label(&node, Size::new(480.0, 900.0), None);
        let mut cache = Cache::default();

        assert!(cache.word_index_for_reflow(&first).is_none());
        assert!(cache.word_index_for_reflow(&first).is_none());
        let after_reflow = cache.word_index_for_reflow(&second).unwrap();
        let restored = cache.word_index_for_reflow(&first).unwrap();
        assert!(Rc::ptr_eq(&after_reflow, &restored));

        node.properties
            .insert("text", "替换文本后重建词边界。\n".repeat(256).into());
        let replaced = system.prepared_label(&node, Size::new(480.0, 900.0), None);
        assert!(cache.word_index_for_reflow(&replaced).is_none());
    }

    #[test]
    fn positioned_batches_reuse_earlier_placements_with_a_shared_budget() {
        use super::{Cache, Placement, TextSystem};
        use xrack_layout::Size;
        use xrack_runtime::Node;

        let system = TextSystem::bundled_only();
        let label = system.prepared_label(&Node::new("Label", None), Size::new(100.0, 200.0), None);
        let mut cache = Cache::default();
        let first = Placement {
            origin: [0, 0],
            scale: 1.0f32.to_bits(),
            target_height: 200,
            registered_fonts: [0; 2],
        };
        let second = Placement {
            origin: [0, 1.0f32.to_bits()],
            ..first
        };
        let third = Placement {
            target_height: 240,
            ..first
        };
        assert!(!cache.observe_position(&label, first).positioned);
        assert!(!cache.observe_position(&label, second).positioned);
        assert!(cache.observe_position(&label, first).positioned);
        assert!(!cache.observe_position(&label, third).positioned);

        let half = Cache::MAX_POSITIONED_GLYPHS / 2;
        cache.remember_positioned(&label, first, Vec::new(), Vec::with_capacity(half), 1);
        cache.remember_positioned(&label, second, Vec::new(), Vec::with_capacity(half / 2), 2);
        assert_eq!(cache.positioned(&label, first).unwrap().lines, 1);
        cache.remember_positioned(&label, third, Vec::new(), Vec::with_capacity(half), 3);
        assert!(cache.positioned(&label, second).is_none());
        assert_eq!(cache.positioned(&label, first).unwrap().lines, 1);
        assert_eq!(cache.positioned(&label, third).unwrap().lines, 3);

        cache.reset_canvas();
        assert!(cache.positioned(&label, first).is_none());
        assert!(!cache.observe_position(&label, first).positioned);
    }
}
