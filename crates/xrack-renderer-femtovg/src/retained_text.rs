use crate::{Canvas, Paint, Path, Point, Rect, TextSystem, WGPURenderer};
use femtovg::{Color, ErrorKind, FontId, PositionedGlyph};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, GenericFamily,
    GlyphRun, Layout, LayoutContext, LineHeight, OverflowWrap, PositionedLayoutItem, StyleProperty,
};
use std::{
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
};
use xrack_layout::{
    LayoutNode, Size, StyledFragment, StyledFragmentBlock, StyledFragmentSource, StyledFragments,
    StyledGlyph, StyledGlyphFont, StyledGlyphRun, StyledTextLayout, TextMeasurer, VisualGrapheme,
    VisualTextLine,
};

pub(crate) fn clip_range(canvas: &Canvas<WGPURenderer>, layout: &LayoutNode) -> Option<(f32, f32)> {
    let clip = layout.clip.intersect(layout.bounds);
    if clip.height >= layout.bounds.height {
        return None;
    }
    let [_, y_from_x, _, y_from_y, _, _] =
        crate::canvas_transform(canvas).inverse()?.coefficients();
    let fringe = 4.0 * (y_from_x.abs() + y_from_y.abs()).max(1.0);
    fringe.is_finite().then_some((
        clip.y - layout.bounds.y - fringe,
        clip.y + clip.height - layout.bounds.y + fringe,
    ))
}
use xrack_runtime::{
    Node, StyledText, TextHorizontalAlignment, TextOverflow, TextStyle, TextVerticalAlignment,
    TextWrap, Value,
};

#[derive(Clone, PartialEq)]
struct DocumentKey {
    identity: [(usize, usize); 3],
    family: String,
    size: u32,
    weight: u16,
    italic: bool,
    selectable: bool,
}

impl DocumentKey {
    fn same_typography(&self, other: &Self) -> bool {
        self.family == other.family
            && self.size == other.size
            && self.weight == other.weight
            && self.italic == other.italic
            && self.selectable == other.selectable
    }
}

struct InlineSpan {
    source: Range<usize>,
    style: TextStyle,
    link: Option<usize>,
}

#[derive(Eq, PartialEq)]
struct RunKey {
    source: Range<usize>,
    font: (u64, u32),
    advance: u32,
    rtl: bool,
    baseline: u32,
}

struct GlyphCache {
    start: usize,
    by_source: Vec<u32>,
    entries: Vec<(RunKey, Arc<StyledGlyphRun>)>,
}

impl GlyphCache {
    fn new(bytes: Range<usize>) -> Self {
        Self {
            start: bytes.start,
            by_source: vec![0; bytes.len()],
            entries: Vec::new(),
        }
    }

    fn get(&self, key: &RunKey) -> Option<&Arc<StyledGlyphRun>> {
        let offset = key.source.start.checked_sub(self.start)?;
        let index = self.by_source.get(offset)?.checked_sub(1)? as usize;
        let (stored, glyphs) = &self.entries[index];
        (stored == key).then_some(glyphs)
    }

    fn insert(&mut self, key: RunKey, glyphs: Arc<StyledGlyphRun>) {
        if key.source.len() > 512 {
            return;
        }
        let Some(slot) = key
            .source
            .start
            .checked_sub(self.start)
            .and_then(|offset| self.by_source.get_mut(offset))
        else {
            return;
        };
        if let Some(index) = slot.checked_sub(1) {
            // A wrap can change the run's end at the same source start.
            // Existing frames keep their own immutable glyph data.
            self.entries[index as usize] = (key, glyphs);
        } else if self.entries.len() < 131_072 {
            self.entries.push((key, glyphs));
            *slot = self.entries.len() as u32;
        }
    }
}

struct Paragraph {
    index: usize,
    end: usize,
    ink_from_baseline: Option<[f32; 2]>,
    placement: Option<ParagraphPlacement>,
}

struct ParagraphShape {
    start: usize,
    layout: Layout<usize>,
    single_line_limit: Option<f32>,
    wrapped: Option<(u32, Alignment)>,
}

struct ParagraphPlacement {
    lines: Vec<(Range<usize>, parley::LineMetrics)>,
    fragments: Arc<ParagraphSource>,
    size: Size,
    ink: Option<[f32; 2]>,
}

struct DocumentGeometry {
    shapes: Vec<ParagraphShape>,
    glyphs: DocumentGlyphs,
}

struct DocumentGlyphs {
    spans: Vec<InlineSpan>,
    glyphs: GlyphCache,
    faces: HashMap<(u64, u32), Arc<StyledGlyphFont>>,
    selectable: bool,
}

struct ParagraphSource {
    geometry: Arc<Mutex<DocumentGeometry>>,
    index: usize,
    width: f32,
    alignment: Alignment,
    fragments: OnceLock<Vec<StyledFragment>>,
}

impl StyledFragmentSource for ParagraphSource {
    fn fragments(&self) -> &[StyledFragment] {
        self.fragments.get_or_init(|| {
            let mut geometry = self
                .geometry
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let DocumentGeometry { shapes, glyphs } = &mut *geometry;
            let shape = &mut shapes[self.index];
            // An older snapshot can be painted after the paragraph was
            // reflowed. Its width and alignment remain part of its source.
            shape.reflow(self.width, self.alignment);
            let fragments = materialize(shape, glyphs);
            // The unwrapped shape already checked all style edges and
            // reflow can only cut it at shaped cluster boundaries.
            debug_assert!(fragments.is_some());
            fragments.unwrap_or_default()
        })
    }
}

impl ParagraphShape {
    fn reflow(&mut self, width: f32, alignment: Alignment) {
        let wrapped = (width.to_bits(), alignment);
        if self.wrapped == Some(wrapped) {
            return;
        }
        let previous = self.wrapped.replace(wrapped);
        let left_aligned = alignment == Alignment::Left
            || (alignment == Alignment::Start && !self.layout.is_rtl())
            || (alignment == Alignment::End && self.layout.is_rtl());
        if left_aligned
            && self.layout.len() == 1
            && let Some((old_width, old_alignment)) = previous
            && old_alignment == alignment
            && let Some(limit) = self.single_line_limit
            && width >= limit
            && f32::from_bits(old_width) >= limit
        {
            return;
        }
        self.layout
            .break_all_lines(width.is_finite().then_some(width));
        self.layout.align(alignment, AlignmentOptions::default());
        if self.single_line_limit.is_none()
            && self.layout.len() == 1
            && let Some(line) = self.layout.get(0)
        {
            // Every prefix fits below the sum of the positive advances.
            // Include hanging space and guard against rounding when summing.
            let mut advance = 0.0_f64;
            let mut clusters = 0;
            for cluster in line.runs().flat_map(|run| run.clusters()) {
                advance += f64::from(cluster.advance().max(0.0));
                clusters += 1;
            }
            let rounding = 2.0 * f64::from(f32::EPSILON) * f64::from(clusters);
            let limit = ((advance * (1.0 + rounding)) as f32)
                .max(line.metrics().advance)
                .next_up();
            self.single_line_limit = limit.is_finite().then_some(limit);
        }
    }
}

impl ParagraphPlacement {
    fn matches(&self, layout: &Layout<usize>) -> bool {
        self.lines.len() == layout.len()
            && layout
                .lines()
                .zip(&self.lines)
                .all(|(line, (range, metrics))| {
                    // The inline box can widen without moving any glyphs.
                    let current = line.metrics();
                    line.text_range() == *range
                        && current.offset == metrics.offset
                        && current.baseline == metrics.baseline
                        && current.line_height == metrics.line_height
                        && current.block_min_coord == metrics.block_min_coord
                        && current.advance == metrics.advance
                        && current.hanging_advance == metrics.hanging_advance
                })
    }
}

impl Paragraph {
    fn place(
        &mut self,
        shape: &mut ParagraphShape,
        width: f32,
        alignment: Alignment,
        geometry: &Arc<Mutex<DocumentGeometry>>,
    ) -> (Size, bool) {
        shape.reflow(width, alignment);
        let size = Size::new(shape.layout.width(), shape.layout.height());
        if let Some(placement) = &self.placement
            && placement.size == size
            && placement.matches(&shape.layout)
        {
            return (size, false);
        }

        let mut lines = self
            .placement
            .take()
            .map(|placement| placement.lines)
            .unwrap_or_default();
        lines.clear();
        lines.extend(
            shape
                .layout
                .lines()
                .map(|line| (line.text_range(), *line.metrics())),
        );
        let ink = self.ink_from_baseline.map(|[top, bottom]| {
            let mut bound = [0.0_f32, size.height];
            for (_, metrics) in &lines {
                bound[0] = bound[0].min(metrics.baseline + top);
                bound[1] = bound[1].max(metrics.baseline + bottom);
            }
            bound
        });
        self.placement = Some(ParagraphPlacement {
            lines,
            size,
            ink,
            fragments: Arc::new(ParagraphSource {
                geometry: geometry.clone(),
                index: self.index,
                width,
                alignment,
                fragments: OnceLock::new(),
            }),
        });
        (size, true)
    }
}

type FontFaces = HashMap<(u64, u32), Arc<StyledGlyphFont>>;

struct Document {
    key: DocumentKey,
    source: StyledText,
    chunks: Vec<ParagraphChunk>,
    faces: FontFaces,
    wrapped: Option<(u32, Alignment)>,
    size: Size,
    plan: Option<([u32; 2], Rc<StyledTextLayout>)>,
}

struct ParagraphChunk {
    storage: Range<usize>,
    paragraphs: Vec<Paragraph>,
    geometry: Arc<Mutex<DocumentGeometry>>,
    wrapped: Option<(u32, Alignment)>,
}

#[derive(Default)]
struct RetainedPrefix {
    chunks: usize,
    paragraphs: usize,
    bytes: usize,
}

impl ParagraphChunk {
    fn reflow(&mut self, width: f32, alignment: Alignment) -> bool {
        let wrapped = (width.to_bits(), alignment);
        if self.wrapped == Some(wrapped) {
            return false;
        }
        let mut geometry = self
            .geometry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut changed = false;
        for (paragraph, shape) in self.paragraphs.iter_mut().zip(&mut geometry.shapes) {
            let (_, updated) = paragraph.place(shape, width, alignment, &self.geometry);
            changed |= updated;
        }
        self.wrapped = Some(wrapped);
        changed
    }
}

impl Document {
    fn paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.chunks.iter().flat_map(|chunk| chunk.paragraphs.iter())
    }

    fn retained_prefix(&self, next: &StyledText) -> RetainedPrefix {
        let split = crate::styled_text::unchanged_paragraph_prefix(&self.source, next);
        if split == 0 {
            return RetainedPrefix::default();
        }
        // The separator byte is in the proven prefix. A final empty or
        // unfinished paragraph must be shaped again with the new tail.
        let chunks = self.chunks.partition_point(|chunk| {
            chunk
                .paragraphs
                .last()
                .is_some_and(|paragraph| paragraph.end < split)
        });
        let Some(partial) = self.chunks.get(chunks) else {
            return RetainedPrefix {
                chunks,
                paragraphs: 0,
                bytes: split,
            };
        };
        let paragraphs = partial
            .paragraphs
            .partition_point(|paragraph| paragraph.end < split);
        if paragraphs > 0
            && split.saturating_sub(partial.storage.start) >= partial.storage.len() / 2
        {
            RetainedPrefix {
                chunks,
                paragraphs,
                bytes: split,
            }
        } else {
            // A retained chunk should not hold a large discarded suffix alive.
            RetainedPrefix {
                chunks,
                paragraphs: 0,
                bytes: partial.storage.start.min(split),
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Cache {
    fonts: FontContext,
    shaper: LayoutContext<usize>,
    document: Option<Document>,
    rejected: Option<(DocumentKey, StyledText)>,
    registered: HashMap<(u64, u32), FontId>,
    registered_context: Option<Rc<()>>,
}

impl Cache {
    pub(crate) fn measure(&mut self, system: &TextSystem, node: &Node, width: f32) -> Option<Size> {
        self.prepare(system, node, width)
            .map(|document| document.size)
    }

    pub(crate) fn layout(
        &mut self,
        system: &TextSystem,
        node: &Node,
        available: Size,
    ) -> Option<Rc<StyledTextLayout>> {
        let document = self.prepare(system, node, available.width)?;
        let dimensions = [
            document.size.width,
            available.height.min(document.size.height),
        ]
        .map(f32::to_bits);
        if let Some((previous, plan)) = &document.plan
            && *previous == dimensions
        {
            return Some(plan.clone());
        }
        let mut y = 0.0;
        let mut bottom = 0.0_f32;
        let mut width = 0.0_f32;
        let mut blocks = Vec::with_capacity(
            document
                .chunks
                .iter()
                .map(|chunk| chunk.paragraphs.len())
                .sum(),
        );
        for paragraph in document.paragraphs() {
            if y >= available.height {
                break;
            }
            let placement = paragraph.placement.as_ref()?;
            let height = if y + placement.size.height <= available.height {
                bottom = y + placement.size.height;
                width = width.max(placement.size.width);
                f32::INFINITY
            } else {
                for (_, metrics) in &placement.lines {
                    let top = y + metrics.block_min_coord;
                    if top >= available.height {
                        break;
                    }
                    bottom = bottom.max(top + metrics.line_height);
                    width = width.max(metrics.advance - metrics.hanging_advance);
                }
                available.height
            };
            blocks.push(StyledFragmentBlock::deferred(
                placement.fragments.clone(),
                y,
                height,
                placement.ink,
            ));
            y += placement.size.height;
        }
        if blocks.is_empty() {
            return None;
        }
        let plan = Rc::new(StyledTextLayout {
            size: Size::new(width.min(available.width), bottom),
            fragments: StyledFragments::from_blocks(blocks),
        });
        document.plan = Some((dimensions, plan.clone()));
        Some(plan)
    }

    fn prepare(&mut self, system: &TextSystem, node: &Node, width: f32) -> Option<&mut Document> {
        if node.kind != "StyledText"
            || width <= 0.0
            || width.is_nan()
            || node.int("max-lines").is_some_and(|count| count > 0)
            || node.properties.get("overflow") == Some(&Value::TextOverflow(TextOverflow::Elide))
            || node
                .properties
                .get("wrap")
                .is_some_and(|wrap| *wrap != Value::TextWrap(TextWrap::WordWrap))
            || node
                .properties
                .get("vertical-alignment")
                .is_some_and(|alignment| {
                    *alignment != Value::TextVerticalAlignment(TextVerticalAlignment::Top)
                })
        {
            return None;
        }
        let text = node.styled_text("text")?;
        if text.text().is_empty() || text.text().len() > 1024 * 1024 || text.text().contains('\r') {
            return None;
        }
        let (family, weight, italic) = system.retained_text_font(node);
        let size = system.font_size(node);
        let key = DocumentKey {
            identity: crate::styled_text::document_identity(text),
            family: family.into(),
            size: size.to_bits(),
            weight,
            italic,
            selectable: node.selectable_styled_text(),
        };
        if self
            .document
            .as_ref()
            .is_none_or(|document| document.key != key)
        {
            if self
                .rejected
                .as_ref()
                .is_some_and(|(rejected, _)| rejected == &key)
            {
                return None;
            }
            if self.update_document(text, &key, size).is_none() {
                self.rejected = Some((key, text.clone()));
                return None;
            }
            self.rejected = None;
        }
        let document = self.document.as_mut()?;
        let alignment = match node.properties.get("horizontal-alignment") {
            Some(Value::TextHorizontalAlignment(TextHorizontalAlignment::Left)) => Alignment::Left,
            Some(Value::TextHorizontalAlignment(TextHorizontalAlignment::Right)) => {
                Alignment::Right
            }
            Some(Value::TextHorizontalAlignment(TextHorizontalAlignment::Center)) => {
                Alignment::Center
            }
            Some(Value::TextHorizontalAlignment(TextHorizontalAlignment::End)) => Alignment::End,
            _ => Alignment::Start,
        };
        let wrapped = (width.to_bits(), alignment);
        if document.wrapped != Some(wrapped) {
            let mut height = 0.0;
            let mut max_width = 0.0_f32;
            let mut changed = false;
            for chunk in &mut document.chunks {
                changed |= chunk.reflow(width, alignment);
                for paragraph in &chunk.paragraphs {
                    let size = paragraph.placement.as_ref()?.size;
                    height += size.height;
                    max_width = max_width.max(size.width);
                }
            }
            if changed {
                document.plan = None;
            }
            document.size = Size::new(max_width.min(width), height);
            document.wrapped = Some(wrapped);
        }
        Some(document)
    }

    fn update_document(&mut self, input: &StyledText, key: &DocumentKey, size: f32) -> Option<()> {
        let previous = self
            .document
            .as_ref()
            .filter(|document| document.key.same_typography(key));
        let prefix = previous.map_or_else(RetainedPrefix::default, |document| {
            document.retained_prefix(input)
        });
        let mut faces = previous.map_or_else(FontFaces::new, |document| document.faces.clone());
        let mut start = prefix.bytes;
        let ranges: Vec<_> = input.text()[start..]
            .split('\n')
            .map(|paragraph| {
                let end = start + paragraph.len();
                let range = start..end;
                start = end + 1;
                range
            })
            .collect();
        // Bound the storage kept by a saved frame when the document suffix
        // is replaced. A failed tail shape leaves the current cache intact.
        const PARAGRAPHS_PER_CHUNK: usize = 128;
        let tail = ranges
            .chunks(PARAGRAPHS_PER_CHUNK)
            .map(|ranges| self.shape_chunk(input, key, size, ranges, &mut faces))
            .collect::<Option<Vec<_>>>()?;

        let mut chunks = self
            .document
            .take()
            .map_or_else(Vec::new, |document| document.chunks);
        chunks.truncate(prefix.chunks + usize::from(prefix.paragraphs > 0));
        if prefix.paragraphs > 0
            && let Some(partial) = chunks.last_mut()
        {
            partial.paragraphs.truncate(prefix.paragraphs);
        }
        chunks.extend(tail);
        self.document = Some(Document {
            key: key.clone(),
            source: input.clone(),
            chunks,
            faces,
            wrapped: None,
            size: Size::default(),
            plan: None,
        });
        Some(())
    }

    fn shape_chunk(
        &mut self,
        input: &StyledText,
        key: &DocumentKey,
        size: f32,
        ranges: &[Range<usize>],
        faces: &mut FontFaces,
    ) -> Option<ParagraphChunk> {
        let bytes = ranges.first()?.start..ranges.last()?.end;
        let mut glyphs = DocumentGlyphs {
            spans: inline_spans(input, bytes.clone()),
            glyphs: GlyphCache::new(bytes.clone()),
            faces: std::mem::take(faces),
            selectable: key.selectable,
        };
        let mut shapes = Vec::with_capacity(ranges.len());
        let paragraphs = ranges
            .iter()
            .map(|range| {
                let start = range.start;
                let end = range.end;
                let text = &input.text()[range.clone()];
                let mut builder = self.shaper.ranged_builder(&mut self.fonts, text, 1.0, true);
                let family = if key.family.is_empty() {
                    FontFamily::from(GenericFamily::SansSerif)
                } else {
                    FontFamily::named(&key.family)
                };
                builder.push_default(StyleProperty::FontFamily(family));
                builder.push_default(StyleProperty::FontSize(size));
                builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
                    key.weight,
                ))));
                if key.italic {
                    builder.push_default(StyleProperty::FontStyle(FontStyle::Italic));
                }
                builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.4)));
                builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
                let first = glyphs
                    .spans
                    .partition_point(|span| span.source.end <= start);
                for (index, span) in glyphs
                    .spans
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take_while(|(_, span)| span.source.start < end)
                {
                    let range =
                        span.source.start.max(start) - start..span.source.end.min(end) - start;
                    builder.push(StyleProperty::Brush(index), range.clone());
                    if span.style.monospace {
                        builder.push(
                            StyleProperty::FontFamily(FontFamily::from(GenericFamily::Monospace)),
                            range.clone(),
                        );
                    }
                    if span.style.bold {
                        builder.push(
                            StyleProperty::FontWeight(FontWeight::new(f32::from(
                                key.weight.max(700),
                            ))),
                            range.clone(),
                        );
                    }
                    if span.style.italic {
                        builder.push(StyleProperty::FontStyle(FontStyle::Italic), range);
                    }
                }
                let mut shape = ParagraphShape {
                    start,
                    layout: builder.build(text),
                    single_line_limit: None,
                    wrapped: None,
                };
                shape.reflow(f32::INFINITY, Alignment::Start);
                let ink_from_baseline = paragraph_ink(&shape, &mut glyphs)?;
                let index = shapes.len();
                shapes.push(shape);
                Some(Paragraph {
                    index,
                    end,
                    ink_from_baseline,
                    placement: None,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        *faces = glyphs.faces.clone();
        Some(ParagraphChunk {
            storage: bytes,
            paragraphs,
            geometry: Arc::new(Mutex::new(DocumentGeometry { shapes, glyphs })),
            wrapped: None,
        })
    }

    pub(crate) fn register_font(
        &mut self,
        system: &TextSystem,
        font: &StyledGlyphFont,
    ) -> Result<FontId, ErrorKind> {
        let context = system.context_key();
        if self
            .registered_context
            .as_ref()
            .is_none_or(|old| !Rc::ptr_eq(old, context))
        {
            self.registered.clear();
            self.registered_context = Some(context.clone());
        }
        if let Some(id) = self.registered.get(&font.key) {
            return Ok(*id);
        }
        let id = system
            .context()
            .add_shared_font_with_index(font.clone(), font.key.1)?;
        self.registered.insert(font.key, id);
        Ok(id)
    }
}

fn source_span<'a>(
    start: usize,
    run: &GlyphRun<'_, usize>,
    spans: &'a [InlineSpan],
) -> Option<(&'a InlineSpan, Range<usize>)> {
    let span = spans.get(run.style().brush)?;
    let local = run.run().text_range();
    let source =
        (start + local.start).max(span.source.start)..(start + local.end).min(span.source.end);
    Some((span, source))
}

fn glyph_font<'a>(
    font: &parley::FontData,
    faces: &'a mut HashMap<(u64, u32), Arc<StyledGlyphFont>>,
) -> &'a Arc<StyledGlyphFont> {
    let key = (font.data.id(), font.index);
    faces.entry(key).or_insert_with(|| {
        Arc::new(StyledGlyphFont {
            key,
            bytes: Arc::new(font.data.clone()),
            ink: font_ink(font.data.as_ref(), font.index),
        })
    })
}

fn paragraph_ink(
    paragraph: &ParagraphShape,
    geometry: &mut DocumentGlyphs,
) -> Option<Option<[f32; 2]>> {
    let mut ink = Some([0.0_f32, 0.0_f32]);
    for line in paragraph.layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let (_, source) = source_span(paragraph.start, &run, &geometry.spans)?;
            if source.is_empty() {
                continue;
            }
            let local = source.start - paragraph.start..source.end - paragraph.start;
            if geometry.selectable
                && local != run.run().text_range()
                && run.run().clusters().any(|cluster| {
                    let range = cluster.text_range();
                    range.end > local.start
                        && range.start < local.end
                        && (range.start < local.start || range.end > local.end)
                })
            {
                return None;
            }
            if let Some([top, bottom]) = ink {
                let font = glyph_font(run.run().font(), &mut geometry.faces);
                let extent = run
                    .glyphs()
                    .fold([f32::INFINITY, f32::NEG_INFINITY], |[min, max], glyph| {
                        [min.min(glyph.y), max.max(glyph.y)]
                    });
                ink = run_ink(&run, font, extent)
                    .map(|[first, last]| [top.min(first), bottom.max(last)]);
            }
        }
    }
    Some(ink)
}

fn materialize(
    paragraph: &ParagraphShape,
    geometry: &mut DocumentGlyphs,
) -> Option<Vec<StyledFragment>> {
    let mut fragments = Vec::new();
    for line in paragraph.layout.lines() {
        let metrics = line.metrics();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let (span, source) = source_span(paragraph.start, &run, &geometry.spans)?;
            if source.is_empty() {
                continue;
            }
            let font = run.run().font();
            let id = (font.data.id(), font.index);
            let baseline = run.baseline() - metrics.block_min_coord;
            let key = RunKey {
                source: source.clone(),
                font: id,
                advance: run.advance().to_bits(),
                rtl: run.run().is_rtl(),
                baseline: baseline.to_bits(),
            };
            let glyphs = if let Some(glyphs) = geometry.glyphs.get(&key) {
                glyphs.clone()
            } else {
                let face = glyph_font(font, &mut geometry.faces);
                let local = source.start - paragraph.start..source.end - paragraph.start;
                let glyphs = Arc::new(glyph_run(
                    &run,
                    local,
                    face.clone(),
                    baseline,
                    geometry.selectable,
                )?);
                geometry.glyphs.insert(key, glyphs.clone());
                glyphs
            };
            let bounds = Rect::new(
                run.offset(),
                metrics.block_min_coord,
                run.advance(),
                metrics.line_height,
            );
            fragments.push(StyledFragment::from_shaped_run(
                source, span.style, span.link, bounds, glyphs,
            ));
        }
    }
    Some(fragments)
}

fn font_ink(bytes: &[u8], index: u32) -> Option<[f32; 2]> {
    let face = ttf_parser::Face::parse(bytes, index).ok()?;
    let tables = face.tables();
    if tables.bdat.is_some()
        || tables.cbdt.is_some()
        || tables.colr.is_some()
        || tables.svg.is_some()
    {
        return None;
    }
    let bounds = face.global_bounding_box();
    let units = f32::from(face.units_per_em());
    let mut ink = [
        -f32::from(bounds.y_max) / units,
        -f32::from(bounds.y_min) / units,
    ];
    if let Some(sbix) = tables.sbix {
        for strike in sbix.strikes {
            for id in 0..strike.len() {
                if let Some(image) = strike.get(ttf_parser::GlyphId(id)) {
                    let units = if image.pixels_per_em == 0 {
                        units
                    } else {
                        f32::from(image.pixels_per_em)
                    };
                    let bottom = -f32::from(image.y) / units;
                    ink[0] = ink[0].min(bottom - f32::from(image.height) / units);
                    ink[1] = ink[1].max(bottom);
                }
            }
        }
    }
    Some(ink)
}

fn inline_spans(input: &StyledText, bytes: Range<usize>) -> Vec<InlineSpan> {
    if bytes.is_empty() {
        return vec![InlineSpan {
            source: bytes,
            style: TextStyle::default(),
            link: None,
        }];
    }
    let run_start = input.runs().partition_point(|run| run.end <= bytes.start);
    let link_start = input
        .links()
        .partition_point(|link| link.end <= bytes.start);
    let runs = input.runs()[run_start..]
        .iter()
        .take_while(|run| run.start < bytes.end);
    let links = input
        .links()
        .iter()
        .enumerate()
        .skip(link_start)
        .take_while(|(_, link)| link.start < bytes.end);
    let mut cuts = vec![bytes.start, bytes.end];
    let restrict = |start: usize, end: usize| [start.max(bytes.start), end.min(bytes.end)];
    cuts.extend(runs.clone().flat_map(|run| restrict(run.start, run.end)));
    cuts.extend(
        links
            .clone()
            .flat_map(|(_, link)| restrict(link.start, link.end)),
    );
    cuts.sort_unstable();
    cuts.dedup();
    let mut styles = runs.peekable();
    let mut links = links.peekable();
    cuts.windows(2)
        .map(|pair| {
            let source = pair[0]..pair[1];
            while styles.peek().is_some_and(|run| run.end <= source.start) {
                styles.next();
            }
            while links
                .peek()
                .is_some_and(|(_, link)| link.end <= source.start)
            {
                links.next();
            }
            let style = styles
                .peek()
                .filter(|run| run.start <= source.start)
                .map_or_else(TextStyle::default, |run| run.style);
            let link = links
                .peek()
                .filter(|(_, link)| link.start <= source.start)
                .map(|(index, _)| *index);
            InlineSpan {
                source,
                style,
                link,
            }
        })
        .collect()
}

fn glyph_run(
    run: &GlyphRun<'_, usize>,
    range: Range<usize>,
    font: Arc<StyledGlyphFont>,
    baseline: f32,
    selectable: bool,
) -> Option<StyledGlyphRun> {
    let rtl = run.run().is_rtl();
    let mut visual = None;
    if selectable {
        let mut cells = Vec::new();
        let mut pen = 0.0;
        for cluster in run.run().visual_clusters() {
            let source = cluster.text_range();
            if source.end <= range.start || source.start >= range.end {
                continue;
            }
            if source.start < range.start || source.end > range.end {
                return None;
            }
            let next = pen + cluster.advance();
            let (leading, trailing) = if rtl { (next, pen) } else { (pen, next) };
            cells.push(VisualGrapheme {
                bytes: source.start - range.start..source.end - range.start,
                leading,
                trailing,
                rtl,
            });
            pen = next;
        }
        visual = Some(Arc::new(VisualTextLine::from_visual_graphemes(
            cells,
            run.advance(),
            rtl,
        )));
    }
    let size = run.run().font_size();
    let mut extent = [f32::INFINITY, f32::NEG_INFINITY];
    let mut x = 0.0;
    let glyphs = run
        .glyphs()
        .map(|glyph| {
            let positioned = StyledGlyph {
                id: glyph.id as u16,
                x: x + glyph.x,
                y: glyph.y,
            };
            extent[0] = extent[0].min(glyph.y);
            extent[1] = extent[1].max(glyph.y);
            x += glyph.advance;
            positioned
        })
        .collect();
    let metrics = run.run().font_metrics();
    let ink = run_ink(run, &font, extent).map(|bounds| bounds.map(|y| y + baseline));
    Some(StyledGlyphRun {
        font,
        font_size: size,
        baseline,
        ink,
        variations: run
            .run()
            .normalized_coords()
            .iter()
            .map(|value| value.to_bits())
            .collect(),
        glyphs,
        underline: [metrics.underline_offset, metrics.underline_size],
        strikethrough: [metrics.strikethrough_offset, metrics.strikethrough_size],
        visual,
    })
}

fn run_ink(
    run: &GlyphRun<'_, usize>,
    font: &StyledGlyphFont,
    extent: [f32; 2],
) -> Option<[f32; 2]> {
    let [top, bottom] = font.ink?;
    if run
        .run()
        .normalized_coords()
        .iter()
        .any(|value| value.to_bits() != 0)
    {
        return None;
    }
    let size = run.run().font_size();
    let metrics = run.run().font_metrics();
    Some([
        (top * size + extent[0])
            .min(-metrics.underline_offset)
            .min(-metrics.strikethrough_offset),
        (bottom * size + extent[1])
            .max(-metrics.underline_offset + metrics.underline_size.max(1.0))
            .max(-metrics.strikethrough_offset + metrics.strikethrough_size.max(1.0)),
    ])
}

pub(crate) fn paint_run(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    fragment: &StyledFragment,
    clip: Rect,
    origin: Point,
    tint: Color,
    selection: &[Range<f32>],
    selected_tint: Option<Color>,
) -> Result<(), ErrorKind> {
    let Some(run) = fragment.shaped_run() else {
        return Ok(());
    };
    let font = system.retained_glyph_font(&run.font)?;
    let style = fragment.style();
    let baseline = origin.y + run.baseline;
    let mut paint = Paint::color(tint);
    paint.set_font_size(run.font_size);
    crate::text::fill_with_selection(
        canvas,
        clip,
        origin.x,
        &paint,
        selection,
        selected_tint,
        |canvas, paint| {
            canvas.fill_glyph_run(
                font,
                &run.variations,
                run.glyphs.iter().map(|glyph| PositionedGlyph {
                    glyph_id: glyph.id,
                    x: origin.x + glyph.x,
                    y: baseline + glyph.y,
                }),
                paint,
            )?;
            for (enabled, [offset, thickness]) in [
                (style.underline || fragment.link().is_some(), run.underline),
                (style.strikethrough, run.strikethrough),
            ] {
                if enabled {
                    let mut path = Path::new();
                    path.rect(
                        origin.x,
                        baseline - offset,
                        fragment.bounds.width,
                        thickness.max(1.0),
                    );
                    canvas.fill_path(&path, paint);
                }
            }
            Ok(())
        },
    )
}
