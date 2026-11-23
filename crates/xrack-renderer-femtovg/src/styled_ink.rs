//! Clip long rich-text draws against the ink of their measured font runs.
use crate::{
    canvas_transform,
    styled_text::{document_bytes, document_identity, unchanged_paragraph_prefix},
    text::TextSystem,
};
use femtovg::{Canvas, Color, Paint, renderer::WGPURenderer};
use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    hash::BuildHasher,
    ops::Range,
    rc::{Rc, Weak},
};
use xrack_layout::{LayoutNode, StyledFragment, StyledTextLayout, TextMeasurer};
use xrack_runtime::{Node, StyledText, TextStyle};

const MAX_DOCUMENTS: usize = 8;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const MIN_FRAGMENTS: usize = 128;

#[derive(PartialEq, Eq)]
struct Run {
    source: (Option<Range<usize>>, bool),
    style: TextStyle,
}

impl Run {
    fn for_fragment(fragment: &StyledFragment) -> Self {
        let mut style = fragment.style();
        style.color = None;
        style.underline |= fragment.link().is_some();
        Self {
            source: fragment.source_span(),
            style,
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
}

impl InkSpans {
    fn new(document_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            max_bytes: CACHE_BYTES
                .saturating_sub(document_bytes)
                .min(2 * 1024 * 1024),
        }
    }

    fn clear(&mut self) {
        self.entries = HashMap::new();
        self.bytes = 0;
    }

    fn measure(
        &mut self,
        text: &str,
        style: TextStyle,
        measure: impl FnOnce() -> Option<Ink>,
    ) -> Option<Ink> {
        if text.len() > 128 {
            return measure();
        }
        let key = self.entries.hasher().hash_one((text, style));
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
        self.bytes + self.spans.as_ref().map_or(0, |spans| spans.bytes)
    }
}

#[derive(Default)]
pub(crate) struct InkCache {
    entries: VecDeque<CachedInk>,
    disabled: bool,
    prefix_disabled: bool,
    pub(super) culled: u64,
}

impl InkCache {
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
        let known =
            self.entries.iter().rposition(|entry| {
                entry.document == identity
                    && same_font(entry)
                    && (entry.plan.ptr_eq(&plan_key)
                        || entry.plan.upgrade().is_some_and(|previous| {
                            previous.fragments.len() == fragments.len()
                                && previous.fragments.iter().zip(fragments).all(|(old, new)| {
                                    Run::for_fragment(old) == Run::for_fragment(new)
                                })
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
            let mut last_y = fragments[..prefix]
                .last()
                .map_or(f32::NEG_INFINITY, |fragment| fragment.bounds.y);
            for fragment in &fragments[prefix..] {
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
                                .iter()
                                .zip(fragments)
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
                        &fragments[shared..],
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
                    measure_ink(system, node, document, fragments, baseline, spans.as_mut())
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

fn measure_ink(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    fragments: &[StyledFragment],
    baseline: f32,
    mut spans: Option<&mut InkSpans>,
) -> Option<Ink> {
    let mut ink = Ink::default();
    let mut paints: HashMap<TextStyle, Paint> = HashMap::new();
    let registered_fonts = spans.as_ref().map(|_| system.registered_font_counts());
    // Register the document's faces before measuring any fallback runs.
    for fragment in fragments {
        let style = Run::for_fragment(fragment).style;
        paints
            .entry(style)
            .or_insert_with(|| system.paint_styled(node, Color::white(), style));
    }
    if let Some(registered_fonts) = registered_fonts
        && system.registered_font_counts() != registered_fonts
        && let Some(spans) = &mut spans
    {
        spans.clear();
    }
    let mut seen = HashMap::new();
    for fragment in fragments {
        let style = Run::for_fragment(fragment).style;
        let Entry::Vacant(entry) = seen.entry((fragment.text(document), style)) else {
            continue;
        };
        let text = entry.key().0.as_ref();
        let paint = paints.get(&style)?;
        // Layout and shaping use font-unit metrics. Measure in user space so
        // zoom and rotation can reuse extents without reshaping the document.
        let measure = || {
            let metrics = system
                .context()
                .measure_text(0.0, baseline, text, paint)
                .ok()?;
            let mut span = Ink::default();
            span.add(metrics.y, metrics.y + metrics.height())?;
            Some(span)
        };
        let span = if let Some(spans) = &mut spans {
            spans.measure(text, style, measure)
        } else {
            measure()
        }?;
        ink.add(span.top, span.bottom)?;
        entry.insert(());
    }
    // Link underlines and strikeout rules are separate from the glyph ink.
    for (style, paint) in &paints {
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
