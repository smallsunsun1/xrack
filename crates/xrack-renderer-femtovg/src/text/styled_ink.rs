//! Clip long rich-text draws against the ink of their measured font runs.
use crate::{
    canvas_transform,
    styled_text::{document_bytes, document_identity, unchanged_paragraph_prefix},
    text::TextSystem,
};
use femtovg::{Canvas, Color, Paint, renderer::WGPURenderer};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    hash::{BuildHasher, RandomState},
    ops::Range,
    rc::{Rc, Weak},
};
use xrack_layout::{LayoutNode, StyledFragment, StyledFragments, StyledTextLayout, TextMeasurer};
use xrack_runtime::{Node, StyledText, TextStyle};

const MAX_DOCUMENTS: usize = 8;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const MIN_FRAGMENTS: usize = 128;
const MIN_COVERAGE_FRAGMENTS: usize = 4096;
const MAX_COVERED_RUNS: usize = 65_536;
const RUN_OFFSET_BITS: u32 = 29;
const _: () = assert!(CACHE_BYTES < (1 << RUN_OFFSET_BITS));

#[derive(PartialEq, Eq)]
struct Run {
    source: (Option<Range<usize>>, bool),
    style: TextStyle,
}

impl Run {
    fn for_fragment(fragment: &StyledFragment) -> Self {
        Self {
            source: fragment.source_span(),
            style: ink_style(fragment),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Ink {
    top: f32,
    bottom: f32,
}

impl Ink {
    fn add(&mut self, top: f32, bottom: f32) -> Option<()> {
        if !top.is_finite() || !bottom.is_finite() {
            return None;
        }
        self.top = self.top.min(top);
        self.bottom = self.bottom.max(bottom);
        Some(())
    }
}

struct InkSpans {
    entries: HashMap<u64, (TextStyle, Box<str>, Ink)>,
    bytes: usize,
    max_bytes: usize,
    covered_runs: HashSet<u64>,
    covered_ink: Ink,
}

impl InkSpans {
    fn new(document_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            covered_runs: HashSet::new(),
            covered_ink: Ink::default(),
            max_bytes: CACHE_BYTES
                .saturating_sub(document_bytes)
                .min(2 * 1024 * 1024),
        }
    }

    fn clear(&mut self) {
        self.entries = HashMap::new();
        self.bytes = 0;
        self.covered_runs = HashSet::new();
        self.covered_ink = Ink::default();
    }

    fn cover<'a>(&mut self, ink: Ink, fragments: impl Iterator<Item = &'a StyledFragment>) {
        self.covered_ink = ink;
        let room = MAX_COVERED_RUNS - self.covered_runs.len();
        self.covered_runs
            .extend(fragments.take(room).map(source_run_key));
    }

    fn measure(
        &mut self,
        key: u64,
        text: &str,
        style: TextStyle,
        measure: impl FnOnce() -> Option<Ink>,
    ) -> Option<Ink> {
        if text.len() > 128 {
            return measure();
        }
        match self.entries.entry(key) {
            Entry::Occupied(entry) => {
                let (cached_style, cached_text, ink) = entry.get();
                if *cached_style == style && cached_text.as_ref() == text {
                    Some(*ink)
                } else {
                    measure()
                }
            }
            Entry::Vacant(entry) => {
                let ink = measure()?;
                let bytes = std::mem::size_of::<(u64, (TextStyle, Box<str>, Ink))>() + text.len();
                if self.bytes + bytes <= self.max_bytes {
                    entry.insert((style, text.into(), ink));
                    self.bytes += bytes;
                }
                Some(ink)
            }
        }
    }
}

struct CachedInk {
    // Keep the storage addresses in the key alive.
    source: StyledText,
    document: [(usize, usize); 3],
    context: Rc<()>,
    typography: Rc<()>,
    registered_fonts: [usize; 2],
    family: String,
    weight: u16,
    italic: bool,
    size_baseline: [u32; 2],
    plan: Weak<StyledTextLayout>,
    max_fragment_height: f32,
    ink: Option<Ink>,
    bytes: usize,
    spans: Option<InkSpans>,
}

impl CachedInk {
    fn resident_bytes(&self) -> usize {
        self.bytes
            + self.spans.as_ref().map_or(0, |spans| {
                spans.bytes + spans.covered_runs.capacity() * std::mem::size_of::<u64>()
            })
    }
}

#[derive(Default)]
pub(crate) struct InkCache {
    entries: VecDeque<CachedInk>,
    disabled: bool,
    prefix_disabled: bool,
    pub(crate) culled: u64,
}

impl InkCache {
    pub(crate) fn enabled(&self) -> bool {
        !self.disabled
    }
    pub(crate) fn set_incremental_enabled(&mut self, enabled: bool) {
        self.prefix_disabled = !enabled;
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.disabled = !enabled;
        self.entries.clear();
    }

    pub(crate) fn visible_fragments(
        &mut self,
        canvas: &Canvas<WGPURenderer>,
        system: &TextSystem,
        node: &Node,
        layout: &LayoutNode,
        baseline: f32,
        has_selection: bool,
    ) -> Option<Range<usize>> {
        // Mobile hosts may paint extra native glyphs beyond FemtoVG's metrics.
        if cfg!(any(target_os = "ios", target_os = "android")) || self.disabled {
            return None;
        }
        let clip = layout.clip.intersect(layout.bounds);
        let plan = layout.styled_text.as_ref()?;
        let fragments = &plan.fragments;
        if clip.height >= layout.bounds.height || fragments.len() < MIN_FRAGMENTS {
            return None;
        }
        let inverse = canvas_transform(canvas).inverse()?;
        let [_, y_from_x, _, y_from_y, _, _] = inverse.coefficients();
        // Glyph metrics are in text space. Keep slack for atlas padding there
        // and for the projection of a device-space pixel onto the text's y axis.
        let fringe = 4.0 * (y_from_x.abs() + y_from_y.abs()).max(1.0);
        if !fringe.is_finite() {
            return None;
        }
        let document = node.styled_text("text")?;
        let plan_key = Rc::downgrade(plan);
        let identity = document_identity(document);
        let context = system.context_key();
        let typography = system.typography_revision()?;
        let registered_fonts = system.registered_font_counts();
        let (family, weight, italic) = system.font_attributes(Some(node));
        let size_baseline = [system.font_size(node), baseline].map(f32::to_bits);
        let same_font = |entry: &CachedInk| {
            Rc::ptr_eq(&entry.context, context)
                && Rc::ptr_eq(&entry.typography, &typography)
                && entry.registered_fonts == registered_fonts
                && entry.family == family
                && entry.weight == weight
                && entry.italic == italic
                && entry.size_baseline == size_baseline
        };
        let known = self.entries.iter().rposition(|entry| {
            entry.document == identity
                && same_font(entry)
                && (entry.plan.ptr_eq(&plan_key)
                    || entry.plan.upgrade().is_some_and(|previous| {
                        previous.fragments.len() == fragments.len()
                            && previous
                                .fragments
                                .iter_range(0..fragments.len())
                                .zip(fragments.iter_range(0..fragments.len()))
                                .all(|(old, new)| Run::for_fragment(old) == Run::for_fragment(new))
                    }))
        });
        // Layout has already checked the source, typography and geometry of
        // this prefix. The previous ink and height are conservative bounds
        // even when part of the previous tail was replaced.
        let continued = if !self.prefix_disabled && known.is_none() {
            system
                .reused_styled_prefix(plan, identity)
                .and_then(|prefix| {
                    let entry = self.entries.iter().rev().find(|entry| {
                        entry.plan.ptr_eq(&prefix.source)
                            && entry.document == prefix.source_document
                            && same_font(entry)
                    })?;
                    Some((entry.ink?, prefix.fragments, entry.max_fragment_height))
                })
        } else {
            None
        };
        // The plan owns local geometry. An unchanged plan keeps the same ink
        // across scroll offsets, including the full height of selection spans.
        let max_fragment_height = if let Some(entry) = known
            .and_then(|index| self.entries.get(index))
            .filter(|entry| entry.plan.ptr_eq(&plan_key))
        {
            entry.max_fragment_height
        } else {
            let (prefix, mut max_height) =
                continued.map_or((0, 0.0), |(_, count, height)| (count, height));
            let mut last_y = fragments
                .iter_range(prefix.saturating_sub(1)..prefix)
                .next()
                .map_or(f32::NEG_INFINITY, |fragment| fragment.bounds.y);
            for fragment in fragments.iter_range(prefix..fragments.len()) {
                let bounds = fragment.bounds;
                if bounds.y < last_y || !bounds.y.is_finite() {
                    return None;
                }
                last_y = bounds.y;
                max_height = max_height.max(bounds.height);
            }
            max_height
        };
        let entry = match known.and_then(|index| self.entries.remove(index)) {
            Some(mut entry) => {
                entry.plan = plan_key;
                entry.max_fragment_height = max_fragment_height;
                entry
            }
            None => {
                let bytes = document_bytes(document);
                if bytes > CACHE_BYTES {
                    return None;
                }
                // A new width can change the fragment boundaries. Reuse the ink
                // of spans already seen in this document at the same typography.
                let same_document =
                    |entry: &CachedInk| entry.document == identity && same_font(entry);
                let mut spans = if self.prefix_disabled {
                    None
                } else {
                    self.entries
                        .iter_mut()
                        .rev()
                        .filter(|entry| same_document(entry))
                        .find_map(|entry| entry.spans.take())
                };
                let prefix = continued.map(|(ink, count, _)| (ink, count)).or_else(|| {
                    if self.prefix_disabled {
                        return None;
                    }
                    self.entries
                        .iter()
                        .rev()
                        .filter(|entry| same_font(entry))
                        .find_map(|entry| {
                            let ink = entry.ink?;
                            let previous = entry.plan.upgrade()?;
                            let end = unchanged_paragraph_prefix(&entry.source, document);
                            if end < 4096 {
                                return None;
                            }
                            let shared = previous
                                .fragments
                                .iter_range(0..previous.fragments.len())
                                .zip(fragments.iter_range(0..fragments.len()))
                                .take_while(|(old, new)| {
                                    let run = Run::for_fragment(old);
                                    run.source.0.as_ref().is_some_and(|range| range.end <= end)
                                        && run == Run::for_fragment(new)
                                })
                                .count();
                            (shared > 0).then_some((ink, shared))
                        })
                });
                let reused = prefix.and_then(|(mut ink, shared)| {
                    let tail = measure_ink(
                        system,
                        node,
                        document,
                        (fragments, shared..fragments.len()),
                        baseline,
                        spans.as_mut(),
                    )?;
                    if system.registered_font_counts() != registered_fonts {
                        return None;
                    }
                    ink.add(tail.top, tail.bottom)?;
                    Some(ink)
                });
                let ink = reused.or_else(|| {
                    // Keep the first full scan's measurements for reflow as well.
                    if !self.prefix_disabled && spans.is_none() {
                        spans = Some(InkSpans::new(bytes));
                    }
                    measure_ink(
                        system,
                        node,
                        document,
                        (fragments, 0..fragments.len()),
                        baseline,
                        spans.as_mut(),
                    )
                });
                CachedInk {
                    source: document.clone(),
                    document: identity,
                    context: context.clone(),
                    typography,
                    registered_fonts: system.registered_font_counts(),
                    family: family.into(),
                    weight,
                    italic,
                    size_baseline,
                    plan: plan_key,
                    max_fragment_height,
                    ink,
                    bytes,
                    spans,
                }
            }
        };
        let ink = entry.ink;
        let mut resident = entry.resident_bytes()
            + self
                .entries
                .iter()
                .map(CachedInk::resident_bytes)
                .sum::<usize>();
        while self.entries.len() >= MAX_DOCUMENTS || resident > CACHE_BYTES {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            resident -= oldest.resident_bytes();
        }
        self.entries.push_back(entry);
        let ink = ink?;

        let top = ink.top - fringe;
        // Selection backgrounds can extend below the glyph ink.
        let bottom = if has_selection {
            ink.bottom.max(max_fragment_height)
        } else {
            ink.bottom
        } + fringe;
        let y = layout.bounds.y;
        let first = fragments.partition_point(|fragment| y + fragment.bounds.y + bottom < clip.y);
        let last = fragments
            .partition_point(|fragment| y + fragment.bounds.y + top <= clip.y + clip.height);
        let range = first..last.max(first);
        self.culled = self
            .culled
            .saturating_add((fragments.len() - range.len()) as u64);
        Some(range)
    }
}

fn ink_style(fragment: &StyledFragment) -> TextStyle {
    let mut style = fragment.style();
    style.color = None;
    style.underline |= fragment.link().is_some();
    style
}

fn paint_slot(style: TextStyle) -> usize {
    let TextStyle {
        bold,
        italic,
        monospace,
        underline,
        strikethrough,
        color: _,
    } = style;
    usize::from(bold)
        | (usize::from(italic) << 1)
        | (usize::from(monospace) << 2)
        | (usize::from(underline) << 3)
        | (usize::from(strikethrough) << 4)
}

// Admitted documents fit both offsets. The empty span identifies an ellipsis.
fn source_run_key(fragment: &StyledFragment) -> u64 {
    let (source, rtl) = fragment.source_span();
    let (start, end) = source.map_or((0, 0), |range| (range.start as u64, range.end as u64));
    start
        | (end << RUN_OFFSET_BITS)
        | ((paint_slot(ink_style(fragment)) as u64) << (2 * RUN_OFFSET_BITS))
        | (u64::from(rtl) << 63)
}

fn measure_ink(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    fragments: (&StyledFragments, Range<usize>),
    baseline: f32,
    spans: Option<&mut InkSpans>,
) -> Option<Ink> {
    let (plan, range) = fragments;
    let count = range.len();
    if range.start == 0
        && count == plan.len()
        && let Some(representatives) = plan.ink_representatives()
    {
        return measure_ink_candidates(
            system,
            node,
            document,
            representatives,
            count,
            baseline,
            spans,
        );
    }
    measure_ink_candidates(
        system,
        node,
        document,
        plan.iter_range(range),
        count,
        baseline,
        spans,
    )
}

fn measure_ink_candidates<'a>(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    fragments: impl Iterator<Item = &'a StyledFragment> + Clone,
    count: usize,
    baseline: f32,
    spans: Option<&mut InkSpans>,
) -> Option<Ink> {
    let Some(spans) = spans else {
        return measure_ink_runs(system, node, document, fragments, baseline, None);
    };
    if count < MIN_COVERAGE_FRAGMENTS {
        return measure_ink_runs(system, node, document, fragments, baseline, Some(spans));
    }
    if !spans.covered_runs.is_empty() {
        // Across widths of the same immutable document, an exact run keeps
        // its ink relative to the baseline. A wider envelope is safe to clip.
        let pending: Vec<_> = fragments
            .clone()
            .filter(|fragment| !spans.covered_runs.contains(&source_run_key(fragment)))
            .collect();
        if pending.is_empty() {
            return Some(spans.covered_ink);
        }
        let registered_fonts = system.registered_font_counts();
        let pending_ink = measure_ink_runs(
            system,
            node,
            document,
            pending.iter().copied(),
            baseline,
            Some(spans),
        );
        if system.registered_font_counts() == registered_fonts {
            let mut ink = spans.covered_ink;
            let pending_ink = pending_ink?;
            ink.add(pending_ink.top, pending_ink.bottom)?;
            spans.cover(ink, pending.into_iter());
            return Some(ink);
        }
        // A new fallback face can change the ink of previously covered runs.
    }
    let ink = measure_ink_runs(
        system,
        node,
        document,
        fragments.clone(),
        baseline,
        Some(spans),
    )?;
    // Store coverage only for the run ranges that have been measured. A
    // later reflow may expose a different representative and measure its ink.
    spans.cover(ink, fragments);
    Some(ink)
}

fn measure_ink_runs<'a>(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    fragments: impl Iterator<Item = &'a StyledFragment> + Clone,
    baseline: f32,
    mut spans: Option<&mut InkSpans>,
) -> Option<Ink> {
    let mut ink = Ink::default();
    let mut paints: [Option<(TextStyle, Paint)>; 32] = std::array::from_fn(|_| None);
    let registered_fonts = spans.as_ref().map(|_| system.registered_font_counts());
    // Preserve first-use order when the paints register fallback fonts.
    for fragment in fragments.clone() {
        let style = ink_style(fragment);
        let slot = &mut paints[paint_slot(style)];
        if slot.is_none() {
            *slot = Some((style, system.paint_styled(node, Color::white(), style)));
        }
    }
    if let Some(registered_fonts) = registered_fonts
        && system.registered_font_counts() != registered_fonts
        && let Some(spans) = &mut spans
    {
        spans.clear();
    }
    // Use the same digest for frame-local deduplication and the retained ink cache.
    let hasher = spans
        .as_ref()
        .map_or_else(RandomState::new, |spans| spans.entries.hasher().clone());
    let cached_ink = (document.text().len() >= 64 * 1024)
        .then(|| system.cached_styled_ink(node))
        .flatten();
    let mut seen = HashMap::<u64, (Cow<'_, str>, TextStyle)>::new();
    for fragment in fragments {
        let style = ink_style(fragment);
        let text = fragment.text(document);
        let key = hasher.hash_one((text.as_ref(), style));
        if seen
            .get(&key)
            .is_some_and(|(old, old_style)| *old == text && *old_style == style)
        {
            continue;
        }
        let (_, paint) = paints[paint_slot(style)].as_ref()?;
        let measure = || {
            let context = system.context();
            let [top, bottom] = if let Some(ink) = cached_ink
                .as_ref()
                .and_then(|cache| cache.get(text.as_ref(), style))
            {
                [baseline + ink[0], baseline + ink[1]]
            } else {
                #[cfg(xrack_femtovg_glyph_samples)]
                {
                    context
                        .alphabetic_ink_bounds(baseline, text.as_ref(), paint)
                        .ok()?
                }
                #[cfg(not(xrack_femtovg_glyph_samples))]
                {
                    let metrics = context
                        .measure_text(0.0, baseline, text.as_ref(), paint)
                        .ok()?;
                    [metrics.y, metrics.y + metrics.height()]
                }
            };
            let mut span = Ink::default();
            span.add(top, bottom)?;
            Some(span)
        };
        let shape = if let Some(spans) = &mut spans {
            spans.measure(key, text.as_ref(), style, measure)
        } else {
            measure()
        }?;
        ink.add(shape.top, shape.bottom)?;
        seen.insert(key, (text, style));
    }
    for (style, paint) in paints.iter().flatten() {
        if style.underline || style.strikethrough {
            let font = system.context().measure_font(paint).ok()?;
            for (enabled, position, thickness) in [
                (
                    style.underline,
                    font.underline_position(),
                    font.underline_thickness(),
                ),
                (
                    style.strikethrough,
                    font.strikeout_position(),
                    font.strikeout_thickness(),
                ),
            ] {
                if enabled {
                    let center = baseline - position;
                    let half = thickness.max(1.0) * 0.5;
                    ink.add(center - half, center + half)?;
                }
            }
        }
    }
    Some(ink)
}

mod tests {
    #[cfg(xrack_femtovg_glyph_samples)]
    #[test]
    fn cached_vertical_ink_matches_positioned_glyphs() -> Result<(), String> {
        use super::{Color, Node, TextStyle, TextSystem};

        let system = TextSystem::system()?;
        let context = system.context();
        let node = Node::new("StyledText", None);
        for style in [
            TextStyle::default(),
            TextStyle {
                bold: true,
                monospace: true,
                ..TextStyle::default()
            },
        ] {
            let mut paint = system.paint_styled(&node, Color::white(), style);
            paint.set_letter_spacing(0.75);
            for text in [
                "",
                "  ",
                "Tests 123?",
                "构建 123",
                "汉字",
                "A • أَبْجَد 123",
                "é e\u{0301} 👩‍🚀",
                "line\nשורה",
            ] {
                for baseline in [0.0, 11.25, -17500.1] {
                    let actual = context
                        .alphabetic_ink_bounds(baseline, text, &paint)
                        .map_err(|err| err.to_string())?;
                    let positioned = context
                        .measure_text(0.0, baseline, text, &paint)
                        .map_err(|err| err.to_string())?;
                    assert_eq!(
                        actual,
                        [positioned.y, positioned.y + positioned.height()],
                        "{text:?} at {baseline}"
                    );
                    let width = context
                        .measure_text_width(text, &paint)
                        .map_err(|err| err.to_string())?;
                    assert_eq!(width.to_bits(), positioned.width().to_bits(), "{text:?}");
                }
            }
        }
        Ok(())
    }
}
