use crate::{
    PaintFrame, color,
    text::{CellPaint, TextSystem, fill_selected_text},
};
use femtovg::{Canvas, Paint, Path, renderer::WGPURenderer};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, VecDeque, hash_map::Entry},
    hash::BuildHasher,
    num::NonZeroUsize,
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};
use xrack_layout::{
    LayoutNode, Point, Size, StyledFragment, StyledSelectionSpans, StyledTextLayout,
    StyledTextLineIndex, StyledTextMeasurements, TextMeasurer, VisualTextLine,
};
use xrack_runtime::{
    Node, StyledLink, StyledRun, StyledText, TextHorizontalAlignment, TextOverflow, TextStyle,
    TextVerticalAlignment, TextWrap, Value,
};

const MAX_LAYOUTS: usize = 4096;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    // The entry owns a clone of the immutable document, keeping these addresses live.
    document: [(usize, usize); 3],
    settings: LayoutSettings,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutSettings {
    kind: &'static str,
    available: [u32; 2],
    font: LayoutFont,
    wrap: u8,
    overflow: u8,
    max_lines: i64,
    horizontal: u8,
    vertical: u8,
    direction: Option<u8>,
    selectable: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutFont {
    size: u32,
    family: String,
    weight: u16,
    italic: bool,
}

struct FontScope {
    font: LayoutFont,
    context: Rc<()>,
    typography: Rc<()>,
}

impl FontScope {
    fn matches(&self, font: &LayoutFont, context: &Rc<()>, typography: &Rc<()>) -> bool {
        self.font == *font
            && Rc::ptr_eq(&self.context, context)
            && Rc::ptr_eq(&self.typography, typography)
    }
}

struct ReflowMeasurements {
    document: [(usize, usize); 3],
    scope: FontScope,
    registered_fonts: [usize; 2],
    measurements: StyledTextMeasurements,
}

struct ShapedSpans {
    scope: FontScope,
    registered_fonts: [usize; 2],
    span_widths: SpanWidths,
    short_widths: Vec<ShortWidth>,
    selection: SelectionShapes,
    selection_document: Option<SelectionDocument>,
}

struct SelectionDocument {
    identity: [(usize, usize); 3],
    spans: StyledSelectionSpans,
}

const SHORT_WIDTH_CACHE_SLOTS: usize = 4096;

type SpanWidths = HashMap<u64, SpanMeasurement>;

pub(crate) struct CachedStyledInk<'a>(std::cell::Ref<'a, SpanWidths>);

impl CachedStyledInk<'_> {
    pub(crate) fn get(&self, text: &str, style: TextStyle) -> Option<[f32; 2]> {
        let style = shaping_style_index(style);
        let key = self.0.hasher().hash_one((style, text));
        let measurement = self.0.get(&key)?;
        (measurement.style == style && measurement.text.as_ref() == text)
            .then_some(measurement.ink)
            .flatten()
    }
}

struct SpanMeasurement {
    style: usize,
    text: Box<str>,
    width: f32,
    ink: Option<[f32; 2]>,
}
#[derive(Clone, Copy, Default)]
struct ShortWidth {
    bytes: u64,
    kind: u8,
    width: f32,
}

fn short_width_key(text: &str, style_index: usize, capacity: usize) -> Option<(usize, u64, u8)> {
    if capacity == 0 || !(1..=8).contains(&text.len()) {
        return None;
    }
    // Store the complete UTF-8 sequence and its length. Even two keys
    // that hash to the same slot can never alias each other's advance.
    let mut bytes = [0_u8; 8];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    let encoded = u64::from_le_bytes(bytes);
    let kind = (style_index as u8) << 4 | text.len() as u8;
    let mixed = (encoded ^ u64::from(kind).wrapping_mul(0x9e3779b97f4a7c15))
        .wrapping_mul(0xd6e8feb86659fd93);
    let index = ((mixed ^ (mixed >> 32)) as usize) & (capacity - 1);
    Some((index, encoded, kind))
}

type ShapingPaints = [Option<CellPaint>; 8];

pub(crate) fn shaping_style_index(style: TextStyle) -> usize {
    usize::from(style.bold) | (usize::from(style.italic) << 1) | (usize::from(style.monospace) << 2)
}

fn shaping_paint(system: &TextSystem, node: &Node, style: TextStyle) -> CellPaint {
    // Shaping uses the font variant. Color and decoration belong to painting.
    system.styled_paint_with_fonts(
        node,
        femtovg::Color::white(),
        TextStyle {
            bold: style.bold,
            italic: style.italic,
            monospace: style.monospace,
            ..TextStyle::default()
        },
    )
}

impl ShapedSpans {
    fn sync_fonts(&mut self, registered_fonts: [usize; 2]) {
        if self.registered_fonts != registered_fonts {
            self.span_widths.clear();
            self.short_widths.fill(ShortWidth::default());
            self.clear_selection();
            self.registered_fonts = registered_fonts;
        }
    }

    fn clear_selection(&mut self) {
        self.selection = SelectionShapes::default();
        self.selection_document = None;
    }

    fn selection_caches(
        &mut self,
        document: &StyledText,
        identity: [(usize, usize); 3],
    ) -> (&mut SelectionShapes, &mut StyledSelectionSpans) {
        let make_document = || SelectionDocument {
            identity,
            spans: StyledSelectionSpans::new(document.clone()),
        };
        let document = self.selection_document.get_or_insert_with(make_document);
        if document.identity != identity {
            *document = make_document();
        }
        (&mut self.selection, &mut document.spans)
    }
}

struct SelectionShape {
    text: Box<str>,
    style: TextStyle,
    geometry: Arc<VisualTextLine>,
}

#[derive(Default)]
struct SelectionShapes {
    entries: HashMap<u64, SelectionShape>,
    bytes: usize,
}

impl SelectionShapes {
    fn get_or_shape(
        &mut self,
        text: &str,
        style: TextStyle,
        shape: impl FnOnce() -> Arc<VisualTextLine>,
    ) -> Arc<VisualTextLine> {
        const MAX_BYTES: usize = 4 * 1024 * 1024;
        if text.len() > 256 {
            return shape();
        }
        let key = self.entries.hasher().hash_one((text, style));
        match self.entries.entry(key) {
            Entry::Occupied(entry) => {
                let retained = entry.get();
                if retained.style == style && retained.text.as_ref() == text {
                    retained.geometry.clone()
                } else {
                    shape()
                }
            }
            Entry::Vacant(entry) => {
                let geometry = shape();
                let bytes = std::mem::size_of::<(u64, SelectionShape)>()
                    + text.len()
                    + geometry.allocated_bytes();
                if self.bytes + bytes <= MAX_BYTES {
                    entry.insert(SelectionShape {
                        text: text.into(),
                        style,
                        geometry: geometry.clone(),
                    });
                    self.bytes += bytes;
                }
                geometry
            }
        }
    }
}

struct CachedLayout {
    document: StyledText,
    plan: Rc<StyledTextLayout>,
    lines: Option<Rc<StyledTextLineIndex>>,
    shaped_fragments: usize,
    selection_allocations: Vec<SelectionAllocation>,
    registered_fonts: [usize; 2],
    reusable_widths: Option<Range<f32>>,
    prefix: Option<ReusedPrefix>,
    last_used: u64,
    eviction_stamp: u64,
    bytes: usize,
}

#[derive(Clone, Copy)]
struct SelectionAllocation {
    first_fragment: usize,
    total_bytes: usize,
}

#[derive(Clone)]
pub(crate) struct ReusedPrefix {
    pub(crate) source: Weak<StyledTextLayout>,
    pub(crate) source_document: [(usize, usize); 3],
    pub(crate) fragments: usize,
}

pub(crate) fn document_bytes(document: &StyledText) -> usize {
    document
        .text()
        .len()
        .saturating_add(std::mem::size_of_val(document.runs()))
        .saturating_add(std::mem::size_of_val(document.links()))
        .saturating_add(document.links().iter().map(|link| link.href.len()).sum())
}

pub(crate) fn document_identity(document: &StyledText) -> [(usize, usize); 3] {
    [
        (document.text().as_ptr() as usize, document.text().len()),
        (document.runs().as_ptr() as usize, document.runs().len()),
        (document.links().as_ptr() as usize, document.links().len()),
    ]
}

/// Complete paragraphs whose bytes, inline styles and link destinations still
/// match after parsing a later document revision.
pub(crate) fn unchanged_paragraph_prefix(previous: &StyledText, next: &StyledText) -> usize {
    let (verified_bytes, verified_runs, verified_links) =
        next.verified_prefix_from(previous).unwrap_or_default();
    let old = previous.text().as_bytes();
    let old_tail = &old[verified_bytes..];
    let new_tail = &next.text().as_bytes()[verified_bytes..];
    let common = if new_tail.starts_with(old_tail) {
        old.len()
    } else {
        verified_bytes
            + old_tail
                .iter()
                .zip(new_tail)
                .take_while(|(a, b)| *a == *b)
                .count()
    };
    let Some(newline) = old[..common].iter().rposition(|byte| *byte == b'\n') else {
        return 0;
    };
    let end = newline + 1;
    let styles_match = previous.runs()[verified_runs..]
        .iter()
        .take_while(|run| run.start < end)
        .map(|run| (run.start, run.end.min(end), run.style))
        .eq(next.runs()[verified_runs..]
            .iter()
            .take_while(|run| run.start < end)
            .map(|run| (run.start, run.end.min(end), run.style)));
    let links_match = previous.links()[verified_links..]
        .iter()
        .take_while(|link| link.start < end)
        .map(|link| (link.start, link.end.min(end), &link.href))
        .eq(next.links()[verified_links..]
            .iter()
            .take_while(|link| link.start < end)
            .map(|link| (link.start, link.end.min(end), &link.href)));
    if styles_match && links_match { end } else { 0 }
}

/// Reuses local text geometry across scroll positions and window frames.
#[derive(Default)]
pub(crate) struct LayoutCache {
    entries: HashMap<LayoutKey, CachedLayout>,
    // Creation order searches recent document revisions for a reusable prefix.
    order: VecDeque<LayoutKey>,
    eviction_order: BTreeMap<u64, LayoutKey>,
    bytes: usize,
    use_clock: u64,
    prefix_disabled: bool,
    reflow: Option<ReflowMeasurements>,
    shaped_spans: Option<ShapedSpans>,
    pub(crate) reused_fragments: u64,
}

impl LayoutCache {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.eviction_order.clear();
        self.bytes = 0;
        self.use_clock = 0;
        self.reflow = None;
        self.shaped_spans = None;
    }

    pub(crate) fn set_incremental_enabled(&mut self, enabled: bool) {
        self.prefix_disabled = !enabled;
    }

    pub(crate) fn reused_prefix(
        &self,
        plan: &Rc<StyledTextLayout>,
        document: [(usize, usize); 3],
    ) -> Option<ReusedPrefix> {
        self.order.iter().rev().take(32).find_map(|key| {
            if key.document != document {
                return None;
            }
            let entry = self.entries.get(key)?;
            if Rc::ptr_eq(&entry.plan, plan) {
                entry.prefix.clone()
            } else {
                None
            }
        })
    }

    pub(crate) fn get(
        &mut self,
        system: &TextSystem,
        node: &Node,
        available: Size,
        selection_geometry: bool,
    ) -> Option<Rc<StyledTextLayout>> {
        let document = node.styled_text("text")?;
        let selectable = node.selectable_styled_text();
        let selection = selection_geometry && selectable;
        let (family, weight, italic) = system.font_attributes(Some(node));
        let horizontal = match node.properties.get("horizontal-alignment") {
            Some(Value::TextHorizontalAlignment(value)) => *value,
            _ if matches!(node.kind, "Label" | "StyledText") => TextHorizontalAlignment::Start,
            _ => TextHorizontalAlignment::Left,
        };
        let vertical = match node.properties.get("vertical-alignment") {
            Some(Value::TextVerticalAlignment(value)) => *value,
            _ => TextVerticalAlignment::Top,
        };
        let wrap = match node.properties.get("wrap") {
            Some(Value::TextWrap(value)) => *value,
            _ if node.kind == "Label" => TextWrap::NoWrap,
            _ => TextWrap::WordWrap,
        };
        let overflow = match node.properties.get("overflow") {
            Some(Value::TextOverflow(value)) => *value,
            _ => TextOverflow::Clip,
        };
        let mut key = LayoutKey {
            document: document_identity(document),
            settings: LayoutSettings {
                kind: node.kind,
                available: [available.width.to_bits(), available.height.to_bits()],
                font: LayoutFont {
                    size: system.font_size(node).to_bits(),
                    family: family.into(),
                    weight,
                    italic,
                },
                wrap: wrap as u8,
                overflow: overflow as u8,
                max_lines: node.int("max-lines").unwrap_or_default(),
                horizontal: horizontal as u8,
                vertical: vertical as u8,
                direction: node.layout_direction().map(|direction| direction as u8),
                selectable,
            },
        };

        // A fully fitting, top-aligned paragraph has the same geometry as its
        // intrinsic measurement. Use that plan for placement when available.
        if !self.entries.contains_key(&key)
            && available.height.is_finite()
            && vertical == TextVerticalAlignment::Top
        {
            let mut intrinsic = key.clone();
            intrinsic.settings.available[1] = f32::INFINITY.to_bits();
            if let Some(cached) = self.entries.get(&intrinsic)
                && available.height >= cached.plan.size.height
            {
                key = intrinsic;
            }
        }
        if !self.prefix_disabled
            && document.text().len() >= 4096
            && available.width > 0.0
            && !self.entries.contains_key(&key)
            && let Some(source) = self.reusable_width_key(system, &key, available)
        {
            key = source;
        }
        if let Some(cached) = self.entries.get_mut(&key)
            && (!selection || cached.shaped_fragments == cached.plan.fragments.len())
        {
            self.use_clock += 1;
            cached.last_used = self.use_clock;
            return Some(cached.plan.clone());
        }

        let mut paints = ShapingPaints::default();
        let share_spans = self.prepare_shaped_spans(system, document, &key.settings.font);
        let mut cached = self.take(&key).unwrap_or_else(|| {
            let can_index = !self.prefix_disabled
                && key.settings.available[1] == f32::INFINITY.to_bits()
                && available.width.is_finite()
                && available.width > 0.0
                && vertical == TextVerticalAlignment::Top
                && overflow != TextOverflow::Elide
                && key.settings.max_lines <= 0
                && document.text().len() >= 4096;
            if can_index
                && let Some((cached, reused)) =
                    self.continue_layout(system, node, document, &key, &mut paints, share_spans)
            {
                self.reused_fragments = self.reused_fragments.saturating_add(reused as u64);
                return cached;
            }
            if can_index && document.text().len() >= 64 * 1024 {
                // The default word cache can evict earlier shaping results
                // during a long layout, before the first ink scan can use them.
                crate::text::reserve_reflow_words(system.context());
            }
            let reflow = can_index && self.prepare_reflow(system, document, &key);
            let mode = if let Some(reflow) = self.reflow.as_mut().filter(|_| reflow) {
                LayoutMode::Reflow(reflow)
            } else if can_index {
                LayoutMode::Indexed
            } else {
                LayoutMode::Regular
            };
            let (plan, lines) = layout(
                system,
                node,
                document,
                available,
                &mut paints,
                mode,
                self.shaped_spans.as_mut().filter(|_| share_spans),
            );
            CachedLayout {
                plan: Rc::new(plan),
                document: document.clone(),
                reusable_widths: lines
                    .as_ref()
                    .and_then(StyledTextLineIndex::reusable_widths),
                lines: lines.map(Rc::new),
                shaped_fragments: 0,
                selection_allocations: Vec::new(),
                registered_fonts: system.registered_font_counts(),
                prefix: None,
                last_used: 0,
                eviction_stamp: 0,
                bytes: 0,
            }
        });
        if selection {
            let plan = Rc::make_mut(&mut cached.plan);
            // Resolve all used faces before sharing a shaped span. Shaping
            // the same text with the same style then uses a stable font set.
            for fragment in plan
                .fragments
                .iter_range(cached.shaped_fragments..plan.fragments.len())
            {
                let style = fragment.style();
                paints[shaping_style_index(style)]
                    .get_or_insert_with(|| shaping_paint(system, node, style));
            }
            let registered_fonts = system.registered_font_counts();
            if registered_fonts != cached.registered_fonts {
                cached.reusable_widths = None;
            }
            let allocations = &mut cached.selection_allocations;
            let mut bytes = allocations.last().map_or(0, |entry| entry.total_bytes);
            let allocate = |first_fragment, allocation_bytes| {
                bytes = bytes.saturating_add(allocation_bytes);
                allocations.push(SelectionAllocation {
                    first_fragment,
                    total_bytes: bytes,
                });
            };
            let mut shape = |text: &str, style: TextStyle| {
                let paint = paints[shaping_style_index(style)]
                    .get_or_insert_with(|| shaping_paint(system, node, style));
                Arc::new(system.shape_visual_line(text, &paint.paint))
            };
            if let Some(spans) = self
                .shaped_spans
                .as_mut()
                .filter(|_| share_spans && document.text().len() >= 4096)
            {
                spans.sync_fonts(registered_fonts);
                if cached.shaped_fragments == 0
                    && (spans
                        .selection_document
                        .as_ref()
                        .is_some_and(|entry| entry.identity == key.document)
                        || (document.text().len() <= 1024 * 1024
                            && document_bytes(document) <= 8 * 1024 * 1024))
                {
                    let (selection, source) = spans.selection_caches(document, key.document);
                    plan.shape_selection_recording_by_span(
                        0,
                        source,
                        |text, style| selection.get_or_shape(text, style, || shape(text, style)),
                        allocate,
                    );
                } else {
                    plan.shape_selection_recording_shared(
                        document,
                        cached.shaped_fragments,
                        |text, style| {
                            spans
                                .selection
                                .get_or_shape(text, style, || shape(text, style))
                        },
                        allocate,
                    );
                }
            } else {
                plan.shape_selection_recording_shared(
                    document,
                    cached.shaped_fragments,
                    shape,
                    allocate,
                );
            }
            cached.shaped_fragments = plan.fragments.len();
            cached.registered_fonts = registered_fonts;
        }
        let plan = cached.plan.clone();
        self.insert(key, cached);
        Some(plan)
    }

    fn prepare_shaped_spans(
        &mut self,
        system: &TextSystem,
        document: &StyledText,
        font: &LayoutFont,
    ) -> bool {
        if self.prefix_disabled {
            return false;
        }
        let Some(typography) = system.typography_revision() else {
            return false;
        };
        let context = system.context_key();
        let registered_fonts = system.registered_font_counts();
        if let Some(spans) = self.shaped_spans.as_mut()
            && spans.scope.matches(font, context, &typography)
        {
            spans.sync_fonts(registered_fonts);
        } else if document.text().len() < 4096 {
            // Short siblings can reuse the active width cache without
            // displacing a differently styled document's reflow state.
            return false;
        } else {
            self.shaped_spans = Some(ShapedSpans {
                scope: FontScope {
                    font: font.clone(),
                    context: context.clone(),
                    typography,
                },
                registered_fonts,
                span_widths: SpanWidths::new(),
                short_widths: Vec::new(),
                selection: SelectionShapes::default(),
                selection_document: None,
            });
        }
        true
    }

    pub(crate) fn cached_ink<'a>(
        cache: std::cell::Ref<'a, Self>,
        system: &TextSystem,
        node: &Node,
    ) -> Option<CachedStyledInk<'a>> {
        let typography = system.typography_revision()?;
        let context = system.context_key();
        let (family, weight, italic) = system.font_attributes(Some(node));
        let size = system.font_size(node).to_bits();
        let fonts = system.registered_font_counts();
        std::cell::Ref::filter_map(cache, |cache| {
            let spans = cache.shaped_spans.as_ref()?;
            let font = &spans.scope.font;
            (spans.registered_fonts == fonts
                && Rc::ptr_eq(&spans.scope.context, context)
                && Rc::ptr_eq(&spans.scope.typography, &typography)
                && font.size == size
                && font.family == family
                && font.weight == weight
                && font.italic == italic)
                .then_some(&spans.span_widths)
        })
        .ok()
        .map(CachedStyledInk)
    }

    fn prepare_reflow(
        &mut self,
        system: &TextSystem,
        document: &StyledText,
        key: &LayoutKey,
    ) -> bool {
        let context = system.context_key();
        let Some(typography) = system.typography_revision() else {
            return false;
        };
        let registered_fonts = system.registered_font_counts();
        if !self.reflow.as_ref().is_some_and(|entry| {
            entry.document == key.document
                && entry
                    .scope
                    .matches(&key.settings.font, context, &typography)
        }) {
            // Capture the first full layout's probe results for a later resize.
            // Incremental tails bypass this path.
            if document.text().len() > 1024 * 1024 || document_bytes(document) > 8 * 1024 * 1024 {
                return false;
            }
            self.reflow = Some(ReflowMeasurements {
                document: key.document,
                scope: FontScope {
                    font: key.settings.font.clone(),
                    context: context.clone(),
                    typography,
                },
                registered_fonts,
                measurements: StyledTextMeasurements::new(document.clone()),
            });
        }
        if let Some(reflow) = self.reflow.as_mut()
            && reflow.registered_fonts != registered_fonts
        {
            reflow.measurements.clear_widths();
            reflow.registered_fonts = registered_fonts;
        }
        true
    }

    fn continue_layout(
        &mut self,
        system: &TextSystem,
        node: &Node,
        document: &StyledText,
        key: &LayoutKey,
        paints: &mut ShapingPaints,
        share_spans: bool,
    ) -> Option<(CachedLayout, usize)> {
        let registered_fonts = system.registered_font_counts();
        let (old, split) = self.order.iter().rev().take(32).find_map(|previous| {
            if previous.settings != key.settings {
                return None;
            }
            let old = self.entries.get(previous)?;
            if old.lines.is_none() || old.registered_fonts != registered_fonts {
                return None;
            }
            let split = unchanged_paragraph_prefix(&old.document, document);
            (split >= 4096).then_some((old, split))
        })?;
        let first_run = document.runs().partition_point(|run| run.end <= split);
        let first_link = document.links().partition_point(|link| link.end <= split);
        let suffix = StyledText::from_parts(
            document.text().get(split..)?,
            document.runs()[first_run..]
                .iter()
                .map(|run| StyledRun {
                    start: run.start.saturating_sub(split),
                    end: run.end - split,
                    style: run.style,
                })
                .collect(),
            document.links()[first_link..]
                .iter()
                .map(|link| StyledLink {
                    start: link.start.saturating_sub(split),
                    end: link.end - split,
                    href: link.href.clone(),
                })
                .collect(),
        )
        .ok()?;
        let mut suffix_node = node.clone();
        suffix_node.properties.insert("text", suffix.clone().into());
        let (tail, lines) = layout(
            system,
            &suffix_node,
            &suffix,
            Size::new(f32::from_bits(key.settings.available[0]), f32::INFINITY),
            paints,
            LayoutMode::Indexed,
            self.shaped_spans.as_mut().filter(|_| share_spans),
        );
        if system.registered_font_counts() != registered_fonts {
            return None;
        }
        let (plan, lines, reused) = old.plan.replace_tail(
            old.lines.as_deref()?,
            split,
            (tail, lines?),
            first_link,
            system.font_size(node),
        );
        let retained_shapes = old
            .selection_allocations
            .partition_point(|entry| entry.first_fragment < reused);
        Some((
            CachedLayout {
                document: document.clone(),
                plan: Rc::new(plan),
                lines: Some(Rc::new(lines)),
                shaped_fragments: old.shaped_fragments.min(reused),
                selection_allocations: old.selection_allocations[..retained_shapes].to_vec(),
                registered_fonts,
                reusable_widths: None,
                prefix: Some(ReusedPrefix {
                    source: Rc::downgrade(&old.plan),
                    source_document: document_identity(&old.document),
                    fragments: reused,
                }),
                last_used: 0,
                eviction_stamp: 0,
                bytes: 0,
            },
            reused,
        ))
    }

    fn reusable_width_key(
        &self,
        system: &TextSystem,
        key: &LayoutKey,
        available: Size,
    ) -> Option<LayoutKey> {
        let mut comparison = key.clone();
        self.order
            .iter()
            .rev()
            .take(16)
            .find(|source| {
                if source.document != key.document {
                    return false;
                }
                comparison.settings.available = source.settings.available;
                comparison == **source
                    && self.entries.get(*source).is_some_and(|cached| {
                        cached
                            .reusable_widths
                            .as_ref()
                            .is_some_and(|range| range.contains(&available.width))
                            && available.height >= cached.plan.size.height
                            && cached.registered_fonts == system.registered_font_counts()
                    })
            })
            .cloned()
    }

    fn take(&mut self, key: &LayoutKey) -> Option<CachedLayout> {
        let cached = self.entries.remove(key)?;
        self.bytes -= cached.bytes;
        self.eviction_order.remove(&cached.eviction_stamp);
        if self.order.front() == Some(key) {
            self.order.pop_front();
        } else if let Some(index) = self.order.iter().rposition(|previous| previous == key) {
            self.order.remove(index);
        }
        Some(cached)
    }

    fn insert(&mut self, key: LayoutKey, mut cached: CachedLayout) {
        let visual_bytes = cached
            .selection_allocations
            .last()
            .map_or(0, |entry| entry.total_bytes);
        cached.bytes = document_bytes(&cached.document)
            .saturating_add(cached.plan.fragments.allocated_bytes())
            .saturating_add(
                cached.selection_allocations.capacity()
                    * std::mem::size_of::<SelectionAllocation>(),
            )
            .saturating_add(visual_bytes)
            .saturating_add(
                cached
                    .lines
                    .as_deref()
                    .map_or(0, StyledTextLineIndex::allocated_bytes),
            );
        let bytes = cached.bytes;
        if bytes > MAX_CACHE_BYTES {
            return;
        }
        self.evict_for(bytes);
        self.use_clock += 1;
        cached.last_used = self.use_clock;
        cached.eviction_stamp = self.use_clock;
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.eviction_order.insert(self.use_clock, key.clone());
        self.entries.insert(key, cached);
    }

    fn evict_for(&mut self, incoming_bytes: usize) {
        while self.entries.len() >= MAX_LAYOUTS || self.bytes + incoming_bytes > MAX_CACHE_BYTES {
            let Some((stamp, key)) = self.eviction_order.pop_first() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            // Refresh a stale timestamp only when the entry becomes a
            // candidate. Cache hits don't have to update the ordered index.
            if stamp != entry.last_used {
                entry.eviction_stamp = entry.last_used;
                self.eviction_order.insert(entry.last_used, key);
            } else {
                self.take(&key);
            }
        }
    }
}

enum LayoutMode<'a> {
    Regular,
    Indexed,
    Reflow(&'a mut ReflowMeasurements),
}

#[cfg(test)]
mod layout_cache_tests {
    use super::*;

    #[test]
    fn byte_pressure_keeps_recently_read_history() {
        let mut cache = LayoutCache::default();
        let mut add = |text: &str| {
            let document = StyledText::from_markdown(text).unwrap();
            let available = Size::new(240.0, f32::INFINITY);
            let plan = Rc::new(StyledTextLayout::new(
                &Node::new("StyledText", None),
                &document,
                available,
                16.0,
                |text, _| text.len() as f32 * 8.0,
            ));
            let key = LayoutKey {
                document: document_identity(&document),
                settings: LayoutSettings {
                    kind: "StyledText",
                    available: [available.width.to_bits(), available.height.to_bits()],
                    font: LayoutFont {
                        size: 16.0_f32.to_bits(),
                        family: String::new(),
                        weight: 400,
                        italic: false,
                    },
                    wrap: TextWrap::WordWrap as u8,
                    overflow: TextOverflow::Clip as u8,
                    max_lines: 0,
                    horizontal: TextHorizontalAlignment::Start as u8,
                    vertical: TextVerticalAlignment::Top as u8,
                    direction: None,
                    selectable: false,
                },
            };
            cache.insert(
                key.clone(),
                CachedLayout {
                    document,
                    plan,
                    lines: None,
                    shaped_fragments: 0,
                    selection_allocations: Vec::new(),
                    registered_fonts: [0; 2],
                    reusable_widths: None,
                    prefix: None,
                    last_used: 0,
                    eviction_stamp: 0,
                    bytes: 0,
                },
            );
            key
        };
        let history = add("Earlier message");
        let obsolete = add("Earlier reply revision");
        let latest = add("Current reply revision");
        cache.use_clock += 1;
        cache.entries.get_mut(&history).unwrap().last_used = cache.use_clock;
        let live_bytes = cache.entries[&history].bytes + cache.entries[&latest].bytes;
        cache.evict_for(MAX_CACHE_BYTES - live_bytes);
        assert!(!cache.entries.contains_key(&obsolete));
        assert_eq!(cache.bytes, live_bytes);
        assert!(cache.order.iter().eq([&history, &latest]));
    }
}

fn layout(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    available: Size,
    paints: &mut ShapingPaints,
    mode: LayoutMode<'_>,
    mut spans: Option<&mut ShapedSpans>,
) -> (StyledTextLayout, Option<StyledTextLineIndex>) {
    const REFLOW_WIDTH_CACHE_PRESSURE: usize = 1024;
    let reflow = matches!(mode, LayoutMode::Reflow(_));
    // Wrap probes and final placement often measure the same short spans.
    let max_widths = if spans.is_some() { 16_384 } else { 1024 };
    let invalidated = Cell::new(false);
    let mut registered_fonts = if let Some(spans) = &spans {
        Some(spans.registered_fonts)
    } else if let LayoutMode::Reflow(reflow) = &mode {
        Some(reflow.registered_fonts)
    } else {
        None
    };
    // Long clipped documents use these measurements again to bound their
    // visible ink. Small labels only pay for their advances.
    let joint_ink = spans.is_some() && document.text().len() >= 64 * 1024;
    let mut local_widths;
    let mut local_short = Vec::new();
    let (mut widths, short) = if let Some(spans) = &mut spans {
        (Some(&mut spans.span_widths), &mut spans.short_widths)
    } else {
        local_widths = (document.text().len() >= 4096).then(SpanWidths::new);
        (local_widths.as_mut(), &mut local_short)
    };
    #[cfg(xrack_femtovg_glyph_samples)]
    let mut span_misses = [0; 8];
    let mut unretained_widths = 0_usize;
    let advance = |text: &str, style| {
        let style_index = shaping_style_index(style);
        let slot = &mut paints[style_index];
        let paint = match slot {
            Some(paint) => paint,
            None => {
                let paint = shaping_paint(system, node, style);
                // A newly registered face can change fallback for earlier spans.
                let clear_widths = if let Some(registered_fonts) = &mut registered_fonts {
                    let fonts = system.registered_font_counts();
                    let changed = *registered_fonts != fonts;
                    if changed {
                        invalidated.set(true);
                        *registered_fonts = fonts;
                    }
                    changed
                } else {
                    true
                };
                if clear_widths && let Some(widths) = &mut widths {
                    widths.clear();
                }
                if clear_widths {
                    short.fill(ShortWidth::default());
                }
                slot.insert(paint)
            }
        };
        let Some(widths) = widths.as_mut().filter(|_| text.len() <= 128) else {
            return system.advance_painted(text, &paint.paint);
        };
        let short_key = short_width_key(text, style_index, short.len());
        if let Some((index, bytes, kind)) = short_key {
            let previous = short[index];
            if previous.bytes == bytes && previous.kind == kind {
                return previous.width;
            }
        }
        let key = widths.hasher().hash_one((style_index, text));
        let width = if let Some(measurement) = widths.get(&key)
            && measurement.style == style_index
            && measurement.text.as_ref() == text
        {
            measurement.width
        } else {
            #[cfg(xrack_femtovg_glyph_samples)]
            if joint_ink && reflow && style_index != 0 {
                span_misses[style_index] += 1;
                // Wait until the layout has used this face repeatedly. This
                // preserves the normal order of font registration and skips
                // speculative work when the span cache is already warm.
                if span_misses[style_index] == 24 {
                    let words = document
                        .runs()
                        .iter()
                        .filter(|run| shaping_style_index(run.style) == style_index)
                        .flat_map(|run| {
                            document.text()[run.start..run.end].split_ascii_whitespace()
                        });
                    system
                        .context()
                        .prepare_short_ltr_words(words, &paint.paint);
                }
            }
            #[cfg(xrack_femtovg_glyph_samples)]
            let measurement = if joint_ink && widths.len() < max_widths {
                system
                    .context()
                    .measure_ltr_width_and_ink(text, &paint.paint)
            } else {
                None
            };
            #[cfg(not(xrack_femtovg_glyph_samples))]
            let (width, ink) = if joint_ink && widths.len() < max_widths {
                system.measure_styled_ltr(text, paint)
            } else {
                None
            }
            .unwrap_or_else(|| (system.advance_painted(text, &paint.paint), None));
            #[cfg(xrack_femtovg_glyph_samples)]
            let (width, ink) = measurement.map_or_else(
                || (system.advance_painted(text, &paint.paint), None),
                |(width, ink)| (width, Some(ink)),
            );
            if widths.len() < max_widths {
                widths.insert(
                    key,
                    SpanMeasurement {
                        style: style_index,
                        text: text.into(),
                        width,
                        ink,
                    },
                );
            } else {
                unretained_widths += 1;
                if reflow && unretained_widths == REFLOW_WIDTH_CACHE_PRESSURE {
                    // Reflow probes frequently revisit short prefixes and
                    // punctuation after the general width cache fills up.
                    if short.is_empty() {
                        short.resize(SHORT_WIDTH_CACHE_SLOTS, ShortWidth::default());
                    }
                    // Long reflows can evict the shaped runs needed by placement
                    // and drawing before measurement finishes. Give this font
                    // context a larger working set once the width cache is full.
                    let context = system.context();
                    context.resize_shaping_run_cache(
                        NonZeroUsize::new(8_192).unwrap_or(NonZeroUsize::MIN),
                    );
                    crate::text::reserve_reflow_words(context);
                }
            }
            width
        };
        if let Some((index, bytes, kind)) = short_key {
            short[index] = ShortWidth { bytes, kind, width };
        }
        width
    };
    let size = system.font_size(node);
    let plan = match mode {
        LayoutMode::Reflow(reflow) => {
            let (plan, lines) = StyledTextLayout::indexed_with_measurements(
                node,
                available,
                size,
                &mut reflow.measurements,
                &invalidated,
                advance,
            );
            reflow.registered_fonts = system.registered_font_counts();
            // A document with many misses at capacity benefits from retaining
            // complete spans by their source position on its next reflow.
            if unretained_widths >= REFLOW_WIDTH_CACHE_PRESSURE {
                reflow.measurements.retain_span_advances();
            }
            (plan, Some(lines))
        }
        LayoutMode::Indexed => {
            let (plan, lines) = StyledTextLayout::indexed(node, document, available, size, advance);
            (plan, Some(lines))
        }
        LayoutMode::Regular => (
            StyledTextLayout::new(node, document, available, size, advance),
            None,
        ),
    };
    if let Some(spans) = spans {
        let registered_fonts = system.registered_font_counts();
        if spans.registered_fonts != registered_fonts {
            // The measuring callback already discarded stale span widths.
            spans.clear_selection();
            spans.registered_fonts = registered_fonts;
        }
    }
    plan
}

pub(crate) fn selection_spans(
    fragment: &StyledFragment,
    selection: &Range<usize>,
) -> Vec<Range<f32>> {
    let Some((visual, range, prefix)) = fragment.selection_geometry() else {
        return Vec::new();
    };
    let start = selection.start.max(range.start);
    let end = selection.end.min(range.end);
    if start < end {
        visual.selection_spans(start - range.start + prefix..end - range.start + prefix)
    } else {
        Vec::new()
    }
}

#[cfg(not(any(xrack_femtovg_glyph_samples, target_os = "android", target_os = "ios")))]
fn warm_native_masks<'a>(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    origin: Point,
    baseline: f32,
    fragments: impl Iterator<Item = &'a StyledFragment>,
) {
    use femtovg::{Color, CompositeOperation, FontId, PositionedGlyph};
    use std::collections::HashSet;

    // The caller admits only untransformed text at exact device scales.
    let scale = canvas.transform().0[0];
    let size = system.font_size(node);
    if !(size > 0.0 && size * scale <= 92.0) {
        return;
    }
    let mut runs = Vec::<(FontId, Vec<PositionedGlyph>)>::new();
    let mut unique = HashSet::new();
    let mut paints: ShapingPaints = Default::default();
    let mut remaining_bytes = 16 * 1024_usize;
    let mut outlines = 0;
    'visible: for fragment in fragments {
        let style = fragment.style();
        let slot = &mut paints[shaping_style_index(style)];
        let paint = match slot {
            Some(paint) => paint,
            None => {
                let paint = shaping_paint(system, node, style);
                let Some(primary) = paint.fonts[0] else {
                    break;
                };
                if !canvas
                    .font_variation_axes(primary)
                    .is_ok_and(|axes| axes.is_empty())
                {
                    break;
                }
                slot.insert(paint)
            }
        };
        let text = fragment.text(document);
        let Some(bytes) = remaining_bytes.checked_sub(text.len()) else {
            break;
        };
        remaining_bytes = bytes;
        let Ok(metrics) = canvas.measure_text(
            origin.x + fragment.bounds.x,
            // Keep the same arithmetic order as the actual StyledText draw.
            (origin.y + fragment.bounds.y) + baseline,
            text,
            &paint.paint,
        ) else {
            break;
        };
        for glyph in metrics.glyphs {
            if glyph.c.is_control() {
                continue;
            }
            let phase = crate::glyph_masks::subpixel_phase((glyph.x * scale).fract());
            if !unique.insert((glyph.font_id, glyph.glyph_id, phase)) {
                continue;
            }
            // Leave varying and color glyphs in the regular paint path. A
            // prefix preserves first-use atlas ordering before those glyphs.
            if system
                .outline_glyph(glyph.font_id, glyph.glyph_id)
                .is_none()
                || outlines == 256
            {
                break 'visible;
            }
            let position = PositionedGlyph {
                glyph_id: glyph.glyph_id,
                x: glyph.x,
                y: glyph.y,
            };
            match runs.last_mut() {
                Some((font, positions)) if *font == glyph.font_id => positions.push(position),
                _ => runs.push((glyph.font_id, vec![position])),
            }
            outlines += 1;
        }
    }
    if outlines < 32 {
        return;
    }
    // FemtoVG writes the masks in its internal offscreen pass. The transparent
    // glyph draws preserve the destination and reuse its source-over pipeline.
    let paint = Paint::color(Color::white()).with_font_size(size);
    canvas.save();
    canvas.set_global_alpha(0.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 0));
    canvas.global_composite_operation(CompositeOperation::SourceOver);
    for (font, glyphs) in runs {
        if canvas.fill_glyph_run(font, &[], glyphs, &paint).is_err() {
            break;
        }
    }
    canvas.restore();
}

pub(crate) fn draw(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    node: &Node,
    layout: &LayoutNode,
    frame: PaintFrame<'_>,
    focused: Option<usize>,
    selection: Range<usize>,
) -> Result<(), String> {
    let Some(document) = node.styled_text("text") else {
        return Ok(());
    };
    let deferred;
    let plan = if let Some(plan) = &layout.styled_text {
        plan
    } else {
        deferred = system.measure_styled_text(node, layout);
        let Some(plan) = &deferred else {
            return Ok(());
        };
        plan
    };
    let PaintFrame { theme, scale, .. } = frame;
    let bounds = layout.bounds;
    let tint = node.color("color").unwrap_or(theme.text);
    let link_tint = node.color("link-color").unwrap_or(theme.focus);
    let highlight = Paint::color(color(theme.text_selection(node)));
    let selected_color = node.color("selected-text-color").map(color);
    let prepared = plan.fragments.has_shaped_runs();
    let baseline = if prepared {
        0.0
    } else {
        let base = system.paint_styled(node, color(tint), TextStyle::default());
        system
            .context()
            .measure_font(&base)
            .map_or_else(|_| system.font_size(node), |metrics| metrics.ascender())
    };
    let prepared_clip = if prepared && system.styled_ink.borrow().enabled() {
        crate::retained_text::clip_range(canvas, layout)
    } else {
        None
    };
    let visible = if !prepared && scale.is_finite() && scale > 0.0 {
        system.styled_ink.borrow_mut().visible_fragments(
            canvas,
            system,
            node,
            layout,
            baseline,
            !selection.is_empty(),
        )
    } else {
        None
    };
    #[cfg(not(any(xrack_femtovg_glyph_samples, target_os = "android", target_os = "ios")))]
    if system.native_styled_masks_pending.get()
        && document.text().len() >= 64 * 1024
        && selection.is_empty()
        && let Some(range) = &visible
        && (16..=256).contains(&range.len())
        && matches!(scale, 1.0 | 2.0)
        && !layout.has_ancestor_clip_chain()
        && crate::has_device_scale(crate::canvas_transform(canvas), scale)
    {
        system.native_styled_masks_pending.set(false);
        warm_native_masks(
            canvas,
            system,
            node,
            document,
            Point {
                x: bounds.x,
                y: bounds.y,
            },
            baseline,
            plan.fragments.iter_range(range.clone()),
        );
    }
    let flat = visible.map(|range| plan.fragments.iter_range(range));
    let placed = flat
        .is_none()
        .then(|| plan.fragments.placed_in_y_range(prepared_clip));
    let fragments = flat
        .into_iter()
        .flatten()
        .map(|fragment| (fragment, 0.0))
        .chain(placed.into_iter().flatten());
    let mut paints = HashMap::new();
    for (fragment, offset) in fragments {
        let local_y = fragment.bounds.y + offset;
        if let Some((top, bottom)) = prepared_clip
            && let Some([ink_top, ink_bottom]) = fragment.shaped_run().and_then(|run| run.ink)
            && (local_y + ink_bottom.max(fragment.bounds.height) < top
                || local_y + ink_top.min(0.0) > bottom)
        {
            continue;
        }
        let spans = selection_spans(fragment, &selection);
        let x = bounds.x + fragment.bounds.x;
        let y = bounds.y + local_y;
        if !spans.is_empty() {
            let mut path = Path::new();
            for span in &spans {
                path.rect(
                    x + span.start,
                    y,
                    span.end - span.start,
                    fragment.bounds.height,
                );
            }
            canvas.fill_path(&path, &highlight);
        }
        let link = fragment.link().is_some();
        let mut style = fragment.style();
        let tint = if !layout.enabled {
            theme.disabled_text
        } else {
            style.color.unwrap_or(if link { link_tint } else { tint })
        };
        if prepared {
            crate::retained_text::paint_run(
                canvas,
                system,
                fragment,
                bounds,
                Point { x, y },
                color(tint),
                &spans,
                selected_color,
            )
            .map_err(|error| format!("styled glyph drawing failed: {error}"))?;
            continue;
        }
        style.underline |= link;
        let paint = paints
            .entry((style, tint))
            .or_insert_with(|| system.paint_styled(node, color(tint), style));
        fill_selected_text(
            canvas,
            system,
            bounds,
            Point { x, y: y + baseline },
            &fragment.text(document),
            paint,
            &spans,
            selected_color,
        )
        .map_err(|error| format!("styled text drawing failed: {error}"))?;
    }
    if let Some(index) = focused {
        let mut outline = Path::new();
        for (_, rect) in layout
            .text_link_regions()
            .filter(|(link, _)| *link == index)
        {
            let rect = rect.inset(0.75);
            outline.rect(rect.x, rect.y, rect.width, rect.height);
        }
        let mut paint = Paint::color(color(theme.focus));
        paint.set_line_width(1.5);
        canvas.stroke_path(&outline, &paint);
    }
    Ok(())
}
