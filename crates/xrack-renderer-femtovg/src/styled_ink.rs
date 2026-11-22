//! Clip long rich-text draws against the ink of their measured font runs.
use crate::{
    canvas_transform,
    styled_text::{document_bytes, document_identity, unchanged_paragraph_prefix},
    text::TextSystem,
};
use femtovg::{Canvas, Color, Paint, renderer::WGPURenderer};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    rc::Rc,
};
use xrack_layout::{LayoutNode, StyledFragment, TextMeasurer};
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

struct CachedInk {
    // Keep the storage addresses in the key alive.
    source: StyledText,
    document: [(usize, usize); 3],
    typography: Rc<()>,
    registered_fonts: [usize; 2],
    family: String,
    weight: u16,
    italic: bool,
    size_baseline: [u32; 2],
    runs: Vec<Run>,
    ink: Option<Ink>,
    bytes: usize,
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
        let inverse = canvas_transform(canvas).inverse()?;
        let [_, y_from_x, _, y_from_y, _, _] = inverse.coefficients();
        // Glyph metrics are in text space. Keep slack for atlas padding there
        // and for the projection of a device-space pixel onto the text's y axis.
        let fringe = 4.0 * (y_from_x.abs() + y_from_y.abs()).max(1.0);
        if !fringe.is_finite() {
            return None;
        }
        let clip = layout.clip.intersect(layout.bounds);
        let fragments = &layout.styled_text.as_ref()?.fragments;
        let document = node.styled_text("text")?;
        if clip.height >= layout.bounds.height
            || fragments.len() < MIN_FRAGMENTS
            || !fragments
                .windows(2)
                .all(|pair| pair[0].bounds.y <= pair[1].bounds.y)
        {
            return None;
        }

        let identity = document_identity(document);
        let typography = system.typography_revision()?;
        let registered_fonts = system.registered_font_counts();
        let (family, weight, italic) = system.font_attributes(Some(node));
        let size_baseline = [system.font_size(node), baseline].map(f32::to_bits);
        let same_font = |entry: &CachedInk| {
            Rc::ptr_eq(&entry.typography, &typography)
                && entry.registered_fonts == registered_fonts
                && entry.family == family
                && entry.weight == weight
                && entry.italic == italic
                && entry.size_baseline == size_baseline
        };
        let known = self.entries.iter().rposition(|entry| {
            entry.document == identity
                && same_font(entry)
                && entry.runs.len() == fragments.len()
                && entry
                    .runs
                    .iter()
                    .zip(fragments)
                    .all(|(run, frag)| *run == Run::for_fragment(frag))
        });
        let entry = match known.and_then(|index| self.entries.remove(index)) {
            Some(entry) => entry,
            None => {
                let bytes = document_bytes(document)
                    .saturating_add(fragments.len().saturating_mul(std::mem::size_of::<Run>()));
                if bytes > CACHE_BYTES {
                    return None;
                }
                let prefix = if self.prefix_disabled {
                    None
                } else {
                    self.entries
                        .iter()
                        .rev()
                        .filter(|entry| same_font(entry))
                        .find_map(|entry| {
                            let ink = entry.ink?;
                            let end = unchanged_paragraph_prefix(&entry.source, document);
                            if end < 4096 {
                                return None;
                            }
                            let shared = entry
                                .runs
                                .iter()
                                .zip(fragments)
                                .take_while(|(run, fragment)| {
                                    run.source.0.as_ref().is_some_and(|range| range.end <= end)
                                        && *run == &Run::for_fragment(fragment)
                                })
                                .count();
                            (shared > 0).then_some((ink, shared))
                        })
                };
                let reused = prefix.and_then(|(mut ink, shared)| {
                    let tail = measure_ink(system, node, document, &fragments[shared..], baseline)?;
                    if system.registered_font_counts() != registered_fonts {
                        return None;
                    }
                    ink.add(tail.top, tail.bottom)?;
                    Some(ink)
                });
                let ink =
                    reused.or_else(|| measure_ink(system, node, document, fragments, baseline));
                CachedInk {
                    source: document.clone(),
                    document: identity,
                    typography,
                    registered_fonts: system.registered_font_counts(),
                    family: family.into(),
                    weight,
                    italic,
                    size_baseline,
                    runs: fragments.iter().map(Run::for_fragment).collect(),
                    ink,
                    bytes,
                }
            }
        };
        let ink = entry.ink;
        let mut resident = entry.bytes
            + self
                .entries
                .iter()
                .map(|cached| cached.bytes)
                .sum::<usize>();
        while self.entries.len() >= MAX_DOCUMENTS || resident > CACHE_BYTES {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            resident -= oldest.bytes;
        }
        self.entries.push_back(entry);
        let ink = ink?;

        let top = ink.top - fringe;
        // Selection backgrounds can extend below the glyph ink.
        let bottom = if has_selection {
            fragments
                .iter()
                .map(|fragment| fragment.bounds.height)
                .fold(ink.bottom, f32::max)
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
) -> Option<Ink> {
    let mut ink = Ink::default();
    let mut paints: HashMap<TextStyle, Paint> = HashMap::new();
    // Register the document's faces before measuring any fallback runs.
    for fragment in fragments {
        let style = Run::for_fragment(fragment).style;
        paints
            .entry(style)
            .or_insert_with(|| system.paint_styled(node, Color::white(), style));
    }
    let mut seen = HashSet::new();
    for fragment in fragments {
        let style = Run::for_fragment(fragment).style;
        let text = fragment.text(document);
        if !seen.insert((text, style)) {
            continue;
        }
        let paint = paints.get(&style)?;
        // Layout and shaping use font-unit metrics. Measure in user space so
        // zoom and rotation can reuse extents without reshaping the document.
        let metrics = system
            .context()
            .measure_text(0.0, baseline, fragment.text(document), paint)
            .ok()?;
        ink.add(metrics.y, metrics.y + metrics.height())?;
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
