use crate::{
    PaintFrame, color,
    text::{TextSystem, fill_selected_text},
};
use femtovg::{Canvas, Paint, Path, renderer::WGPURenderer};
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    rc::Rc,
};
use xrack_layout::{
    LayoutNode, Point, Size, StyledFragment, StyledTextLayout, StyledTextLineIndex, TextMeasurer,
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
    font_size: u32,
    family: String,
    weight: u16,
    italic: bool,
    wrap: u8,
    overflow: u8,
    max_lines: i64,
    horizontal: u8,
    vertical: u8,
    direction: Option<u8>,
    selectable: bool,
}

struct CachedLayout {
    document: StyledText,
    plan: Rc<StyledTextLayout>,
    lines: Option<Rc<StyledTextLineIndex>>,
    shaped_fragments: usize,
    registered_fonts: [usize; 2],
    bytes: usize,
}

pub(super) fn document_bytes(document: &StyledText) -> usize {
    document
        .text()
        .len()
        .saturating_add(std::mem::size_of_val(document.runs()))
        .saturating_add(std::mem::size_of_val(document.links()))
        .saturating_add(document.links().iter().map(|link| link.href.len()).sum())
}

pub(super) fn document_identity(document: &StyledText) -> [(usize, usize); 3] {
    [
        (document.text().as_ptr() as usize, document.text().len()),
        (document.runs().as_ptr() as usize, document.runs().len()),
        (document.links().as_ptr() as usize, document.links().len()),
    ]
}

/// Complete paragraphs whose bytes, inline styles and link destinations still
/// match after parsing a later document revision.
pub(super) fn unchanged_paragraph_prefix(previous: &StyledText, next: &StyledText) -> usize {
    let old = previous.text().as_bytes();
    let new = next.text().as_bytes();
    let common = if new.starts_with(old) {
        old.len()
    } else {
        old.iter().zip(new).take_while(|(a, b)| *a == *b).count()
    };
    let Some(newline) = old[..common].iter().rposition(|byte| *byte == b'\n') else {
        return 0;
    };
    let end = newline + 1;
    let styles_match = previous
        .runs()
        .iter()
        .take_while(|run| run.start < end)
        .map(|run| (run.start, run.end.min(end), run.style))
        .eq(next
            .runs()
            .iter()
            .take_while(|run| run.start < end)
            .map(|run| (run.start, run.end.min(end), run.style)));
    let links_match = previous
        .links()
        .iter()
        .take_while(|link| link.start < end)
        .map(|link| (link.start, link.end.min(end), &link.href))
        .eq(next
            .links()
            .iter()
            .take_while(|link| link.start < end)
            .map(|link| (link.start, link.end.min(end), &link.href)));
    if styles_match && links_match { end } else { 0 }
}

/// Reuses local text geometry across scroll positions and window frames.
#[derive(Default)]
pub(crate) struct LayoutCache {
    entries: HashMap<LayoutKey, CachedLayout>,
    order: VecDeque<LayoutKey>,
    bytes: usize,
    prefix_disabled: bool,
    pub(crate) reused_fragments: u64,
}

impl LayoutCache {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    pub(crate) fn set_incremental_enabled(&mut self, enabled: bool) {
        self.prefix_disabled = !enabled;
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
                font_size: system.font_size(node).to_bits(),
                family: family.into(),
                weight,
                italic,
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
        if let Some(cached) = self.entries.get(&key)
            && (!selection || cached.shaped_fragments == cached.plan.fragments.len())
        {
            return Some(cached.plan.clone());
        }

        let mut paints = HashMap::new();
        let mut cached = self.take(&key).unwrap_or_else(|| {
            let can_index = !self.prefix_disabled
                && key.settings.available[1] == f32::INFINITY.to_bits()
                && available.width.is_finite()
                && available.width > 0.0
                && vertical == TextVerticalAlignment::Top
                && overflow != TextOverflow::Elide
                && key.settings.max_lines <= 0
                && document.text().len() >= 4096;
            let continued = can_index
                .then(|| self.continue_layout(system, node, document, &key, &mut paints))
                .flatten();
            let (plan, lines, shaped_fragments) =
                if let Some((plan, lines, reused, shaped)) = continued {
                    self.reused_fragments = self.reused_fragments.saturating_add(reused as u64);
                    (plan, Some(lines), shaped)
                } else {
                    let (plan, lines) =
                        layout(system, node, document, available, &mut paints, can_index);
                    (plan, lines, 0)
                };
            CachedLayout {
                plan: Rc::new(plan),
                document: document.clone(),
                lines: lines.map(Rc::new),
                shaped_fragments,
                registered_fonts: system.registered_font_counts(),
                bytes: 0,
            }
        });
        if selection {
            let plan = Rc::make_mut(&mut cached.plan);
            // Resolve all used faces before sharing a shaped span. Shaping
            // the same text with the same style then uses a stable font set.
            for fragment in &plan.fragments[cached.shaped_fragments..] {
                paints.entry(fragment.style()).or_insert_with(|| {
                    system.paint_styled(node, femtovg::Color::white(), fragment.style())
                });
            }
            let shaped_bytes =
                plan.shape_selection_from(document, cached.shaped_fragments, |text, style| {
                    system.shape_visual_line(text, &paints[&style])
                });
            if cached.bytes != 0 {
                cached.bytes = cached.bytes.saturating_add(shaped_bytes);
            }
            cached.shaped_fragments = plan.fragments.len();
            cached.registered_fonts = system.registered_font_counts();
        }
        let plan = cached.plan.clone();
        self.insert(key, cached);
        Some(plan)
    }

    fn continue_layout(
        &self,
        system: &TextSystem,
        node: &Node,
        document: &StyledText,
        key: &LayoutKey,
        paints: &mut HashMap<TextStyle, Paint>,
    ) -> Option<(StyledTextLayout, StyledTextLineIndex, usize, usize)> {
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
        let first_link = document.links().partition_point(|link| link.end <= split);
        let suffix = StyledText::from_parts(
            document.text().get(split..)?,
            document
                .runs()
                .iter()
                .filter(|run| run.end > split)
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
            true,
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
        Some((plan, lines, reused, old.shaped_fragments.min(reused)))
    }

    fn take(&mut self, key: &LayoutKey) -> Option<CachedLayout> {
        let cached = self.entries.remove(key)?;
        self.bytes -= cached.bytes;
        self.order.retain(|previous| previous != key);
        Some(cached)
    }

    fn insert(&mut self, key: LayoutKey, mut cached: CachedLayout) {
        if cached.bytes == 0 {
            cached.bytes = document_bytes(&cached.document)
                .saturating_add(cached.plan.allocated_bytes())
                .saturating_add(
                    cached
                        .lines
                        .as_deref()
                        .map_or(0, StyledTextLineIndex::allocated_bytes),
                );
        }
        let bytes = cached.bytes;
        if bytes > MAX_CACHE_BYTES {
            return;
        }
        while self.entries.len() >= MAX_LAYOUTS || self.bytes + bytes > MAX_CACHE_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes -= entry.bytes;
            }
        }
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.entries.insert(key, cached);
    }
}

fn layout(
    system: &TextSystem,
    node: &Node,
    document: &StyledText,
    available: Size,
    paints: &mut HashMap<TextStyle, Paint>,
    indexed: bool,
) -> (StyledTextLayout, Option<StyledTextLineIndex>) {
    let advance = |text: &str, style| {
        let paint = paints
            .entry(style)
            .or_insert_with(|| system.paint_styled(node, femtovg::Color::white(), style));
        system.advance_painted(text, paint)
    };
    let size = system.font_size(node);
    if indexed {
        let (plan, lines) = StyledTextLayout::indexed(node, document, available, size, advance);
        (plan, Some(lines))
    } else {
        (
            StyledTextLayout::new(node, document, available, size, advance),
            None,
        )
    }
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

pub(crate) fn draw(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    node: &Node,
    layout: &LayoutNode,
    frame: PaintFrame<'_>,
    focused: Option<usize>,
    selection: Range<usize>,
) -> Result<(), String> {
    let (Some(document), Some(plan)) = (node.styled_text("text"), &layout.styled_text) else {
        return Ok(());
    };
    let PaintFrame { theme, scale, .. } = frame;
    let bounds = layout.bounds;
    let tint = node.color("color").unwrap_or(theme.text);
    let link_tint = node.color("link-color").unwrap_or(theme.focus);
    let highlight = Paint::color(color(theme.text_selection(node)));
    let selected_color = node.color("selected-text-color").map(color);
    let mut paints = HashMap::new();
    let base = system.paint_styled(node, color(tint), TextStyle::default());
    let baseline = system
        .context()
        .measure_font(&base)
        .map_or_else(|_| system.font_size(node), |metrics| metrics.ascender());
    let visible = if scale.is_finite() && scale > 0.0 {
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
    let fragments = visible.map_or(plan.fragments.as_slice(), |range| &plan.fragments[range]);
    for fragment in fragments {
        let spans = selection_spans(fragment, &selection);
        let x = bounds.x + fragment.bounds.x;
        let y = bounds.y + fragment.bounds.y;
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
