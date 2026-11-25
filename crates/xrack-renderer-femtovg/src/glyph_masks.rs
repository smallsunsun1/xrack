use crate::{Canvas, Paint, TextSystem, WGPURenderer, label_glyphs::FontRun};
use femtovg::{
    Atlas, Color, DrawCommand, FontId, GlyphDrawCommands, ImageFlags, ImageId, PixelFormat,
    PositionedGlyph, Quad, TextMetrics, renderer::RenderTarget,
};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::{Rc, Weak},
};

const SIDE: usize = 1024;
const REGION_SIDE: usize = SIDE / 2;
const REGION_COUNT: usize = 4;
const DENSE_GLYPH_IDS: u16 = 512;
// The Bazel vector rasterizer shares its coverage across pen phases.
const SUBPIXEL_PHASES: usize = if cfg!(xrack_femtovg_glyph_samples) {
    1
} else {
    11
};
const MAX_FONT_PAGES: usize = 8;
const DENSE_SLOTS: usize = DENSE_GLYPH_IDS as usize * SUBPIXEL_PHASES;
const POSITIVE_SLOTS: usize = (u16::MAX as usize + 1) * SUBPIXEL_PHASES;
const NEGATIVE_ORIGIN: f32 = 2.0 * SIDE as f32;
const MAX_MASKS_PER_FRAME: usize = 64;
const MAX_FALLBACK_RUNS_PER_FRAME: usize = 32;

#[derive(Default)]
pub(super) struct GlyphMasks {
    texture: Option<MaskTexture>,
    pages: Vec<Page>,
    pending: usize,
    remaining: usize,
    pub(super) defer_preparation: bool,
    fallback_runs: usize,
    retained: Option<RetainedQuads>,
    #[cfg(not(xrack_femtovg_glyph_samples))]
    pipeline_warmup: AtlasPipelineWarmup,
}

#[cfg(not(xrack_femtovg_glyph_samples))]
#[derive(Default, PartialEq, Eq)]
enum AtlasPipelineWarmup {
    #[default]
    Unrequested,
    Pending,
    Finished,
}

struct RetainedQuads {
    positions: Weak<Vec<PositionedGlyph>>,
    image: ImageId,
    font: FontId,
    size: f32,
    scale: f32,
    regions: u8,
    quads: Vec<Quad>,
}

struct MaskTexture {
    image: ImageId,
    regions: [MaskRegion; REGION_COUNT],
    frame: u64,
    needs_reclaim: bool,
}

struct MaskRegion {
    packing: Atlas,
    slots: Vec<(usize, usize)>,
    last_used: Cell<u64>,
    last_allocated: u64,
    clear: bool,
}

pub(super) struct MaskRun<'a> {
    pub(super) font: FontId,
    pub(super) positions: MaskPositions<'a>,
    pub(super) paint: &'a Paint,
    pub(super) scale: f32,
    pub(super) cache_short_run: bool,
}

pub(super) enum MaskPositions<'a> {
    Transient(&'a [PositionedGlyph]),
    Retained(&'a Rc<Vec<PositionedGlyph>>),
}

pub(super) enum OutlineRuns {
    Drawn,
    Fallback,
    Unsupported,
}

pub(super) struct OutlineBatch<'a> {
    pub(super) runs: &'a [FontRun],
    pub(super) glyphs: &'a [PositionedGlyph],
    pub(super) paint: &'a Paint,
    pub(super) scale: f32,
    pub(super) allow_partial: bool,
}

enum OutlineSpan {
    Mask(usize),
    Native(usize),
}

struct Page {
    index: usize,
    font: FontId,
    size: f32,
    scale: f32,
    tiles: Vec<Option<Tile>>,
    // Rare glyph IDs and negative phases share the atlas without
    // allocating empty slots up to the highest glyph ID.
    sparse: HashMap<usize, Tile>,
    pending: Vec<(usize, PositionedGlyph)>,
}

#[derive(Clone, Copy)]
struct Tile {
    uv: [f32; 4],
    extent: [f32; 4],
    region: u8,
    ready: bool,
}

fn placed_edges([left, top, right, bottom]: [f32; 4], x: f32, y: f32) -> [f32; 4] {
    let x = x.trunc();
    let y = y.round();
    [x + left, y + top, x + right, y + bottom]
}

pub(super) fn subpixel_phase(fract: f32) -> i8 {
    ((fract.abs() / 0.1 + 0.5).trunc() * 0.1 * 10.0).copysign(fract) as i8
}

#[cfg(xrack_femtovg_glyph_samples)]
fn mask_phase(_: usize) -> i8 {
    0
}

#[cfg(not(xrack_femtovg_glyph_samples))]
fn mask_phase(index: usize) -> i8 {
    let phase = (index % SUBPIXEL_PHASES) as i8;
    if index < POSITIVE_SLOTS {
        phase
    } else {
        -phase
    }
}

fn mask_slot(glyph: u16, x: f32, y: f32) -> Option<usize> {
    const MAX_COORDINATE: f32 = 1_048_576.0;
    if !(x.abs() <= MAX_COORDINATE && y.abs() <= MAX_COORDINATE) {
        return None;
    }
    let phase = if SUBPIXEL_PHASES == 1 {
        0
    } else {
        subpixel_phase(x.fract())
    };
    let base = if phase < 0 { POSITIVE_SLOTS } else { 0 };
    Some(base + usize::from(glyph) * SUBPIXEL_PHASES + phase.unsigned_abs() as usize)
}

fn quad(edges: [f32; 4], uv: [f32; 4], inverse: f32) -> Quad {
    let [x0, y0, x1, y1] = edges.map(|edge| edge * inverse);
    let [s0, t0, s1, t1] = uv;
    Quad {
        x0,
        y0,
        x1,
        y1,
        s0,
        t0,
        s1,
        t1,
    }
}

pub(super) fn draw_mask_quads(
    canvas: &mut Canvas<WGPURenderer>,
    image: ImageId,
    quads: Vec<Quad>,
    paint: &Paint,
) {
    canvas.draw_glyph_commands(
        GlyphDrawCommands {
            alpha_glyphs: vec![DrawCommand {
                image_id: image,
                quads,
            }],
            ..Default::default()
        },
        paint,
    );
}

impl GlyphMasks {
    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) fn request_atlas_pipeline_warmup(&mut self) {
        if self.pipeline_warmup == AtlasPipelineWarmup::Unrequested {
            self.pipeline_warmup = AtlasPipelineWarmup::Pending;
        }
    }

    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) fn atlas_pipeline_warmup_pending(&self) -> bool {
        self.pipeline_warmup == AtlasPipelineWarmup::Pending
    }

    #[cfg(not(xrack_femtovg_glyph_samples))]
    pub(super) fn warm_atlas_pipelines(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        use femtovg::{BlendFactor, CompositeOperation, Path};

        self.pipeline_warmup = AtlasPipelineWarmup::Finished;
        // The native atlas normally uses stencil while seeding complex
        // outlines. A later page with only convex outlines uses these two
        // variants without stencil. Prepare them in the same canvas cache.
        let Ok(image) = canvas.create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::NEAREST)
        else {
            return;
        };
        canvas.save();
        canvas.with_render_target(RenderTarget::Image(image), |canvas| {
            canvas.reset_transform();
            canvas.reset_scissor();
            canvas.set_global_alpha(1.0);
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 0));
            canvas.global_composite_operation(CompositeOperation::SourceOver);
            canvas.clear_rect(1, 1, 2, 2, Color::black());
            canvas.global_composite_blend_func(BlendFactor::SrcAlpha, BlendFactor::One);
            let mut path = Path::new();
            path.rect(2.0, 2.0, 2.0, 2.0);
            let mask = Paint::color(Color::rgbf(0.125, 0.125, 0.125)).with_anti_alias(false);
            canvas.fill_path(&path, &mask);
        });
        canvas.restore();
        canvas.delete_image(image);
    }

    pub(super) fn begin_frame(&mut self) {
        self.fallback_runs = MAX_FALLBACK_RUNS_PER_FRAME;
        self.remaining = MAX_MASKS_PER_FRAME;
        if let Some(slots) = self.texture.as_mut().and_then(MaskTexture::begin_frame) {
            self.retained = None;
            for (page, slot) in slots.drain(..) {
                self.pages[page].remove(slot);
            }
            self.pending = 0;
            for page in &mut self.pages {
                page.remove_evicted_pending();
                self.pending += page.pending.len();
            }
        }
    }

    pub(super) fn can_prepare_during_paint(&self, portable_cell: bool) -> bool {
        (cfg!(xrack_femtovg_glyph_samples) || portable_cell) && !self.defer_preparation
    }

    pub(super) fn has_pending(&self) -> bool {
        self.pending != 0
    }

    pub(super) fn prepare(&mut self, canvas: &mut Canvas<WGPURenderer>) -> bool {
        let Some(texture) = self.texture.as_mut() else {
            return false;
        };
        canvas.save();
        let success = canvas.with_render_target(RenderTarget::Image(texture.image), |canvas| {
            canvas.reset_scissor();
            canvas.set_global_alpha(1.0);
            canvas.global_composite_operation(femtovg::CompositeOperation::SourceOver);
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 0));
            texture.clear_reclaimed(canvas);
            self.pages.iter_mut().all(|page| page.prepare(canvas))
        });
        canvas.restore();
        if success {
            self.pending = 0;
        } else {
            self.discard_texture(canvas);
        }
        success
    }

    fn discard_texture(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(texture) = self.texture.take() {
            canvas.delete_image(texture.image);
        }
        self.pages.clear();
        self.pending = 0;
        self.retained = None;
    }

    pub(super) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        text: &TextSystem,
        run: MaskRun<'_>,
    ) -> bool {
        let MaskRun {
            font,
            positions,
            paint,
            scale,
            cache_short_run,
        } = run;
        let size = paint.font_size();
        let (positions, source) = match positions {
            MaskPositions::Transient(positions) => (positions, None),
            MaskPositions::Retained(positions) => (positions.as_slice(), Some(positions)),
        };
        if (!cache_short_run && positions.len() < 12) || !(size > 0.0 && size * scale <= 40.0) {
            return false;
        }
        if let (Some(source), Some(retained), Some(texture)) =
            (source, &self.retained, &self.texture)
            && std::ptr::eq(retained.positions.as_ptr(), Rc::as_ptr(source))
            && retained.image == texture.image
            && retained.font == font
            && retained.size == size
            && retained.scale == scale
        {
            texture.touch(retained.regions);
            draw_mask_quads(canvas, texture.image, retained.quads.clone(), paint);
            return true;
        }
        let Some(page) = page(&mut self.pages, font, size, scale, || true) else {
            return false;
        };
        let texture = match &mut self.texture {
            Some(value) => value,
            slot => {
                let Some(texture) = MaskTexture::new(canvas) else {
                    return false;
                };
                slot.insert(texture)
            }
        };
        let mut ready = true;
        let mut used = 0;
        let mut empty_glyph = None;
        let mut quads = Vec::with_capacity(positions.len());
        let inverse = 1.0 / scale;
        for glyph in positions {
            let x = glyph.x * scale;
            let y = glyph.y * scale;
            let Some(index) = mask_slot(glyph.glyph_id, x, y) else {
                return false;
            };
            if let Some(tile) = page.tile(index) {
                if !tile.ready {
                    ready = false;
                    continue;
                }
                used |= 1 << tile.region;
                quads.push(quad(placed_edges(tile.extent, x, y), tile.uv, inverse));
            } else {
                if source.is_some()
                    && (empty_glyph == Some(glyph.glyph_id)
                        || text
                            .outline_glyph(font, glyph.glyph_id)
                            .is_some_and(|outline| outline.bounds.is_none()))
                {
                    empty_glyph = Some(glyph.glyph_id);
                    continue;
                }
                ready = false;
                if self.remaining != 0
                    && page.schedule(text, texture, index, glyph.glyph_id, mask_phase(index))
                {
                    self.pending += 1;
                    self.remaining -= 1;
                }
            }
        }
        if ready {
            texture.touch(used);
            if let Some(source) = source {
                self.retained = Some(RetainedQuads {
                    positions: Rc::downgrade(source),
                    image: texture.image,
                    font,
                    size,
                    scale,
                    regions: used,
                    quads: quads.clone(),
                });
            }
            draw_mask_quads(canvas, texture.image, quads, paint);
        }
        ready
    }

    pub(super) fn draw_outline_runs(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        text: &TextSystem,
        batch: OutlineBatch<'_>,
    ) -> Result<OutlineRuns, String> {
        let OutlineBatch {
            runs,
            glyphs,
            paint,
            scale,
            ..
        } = batch;
        let size = paint.font_size();
        if runs.len() < 4 || !(size > 0.0 && size * scale <= 40.0) {
            return Ok(OutlineRuns::Fallback);
        }
        let positions = runs.iter().flat_map(|run| {
            glyphs[run.glyphs.clone()]
                .iter()
                .map(|glyph| (run.font, glyph.glyph_id, glyph.x, glyph.y, true))
        });
        let masked = self
            .clipped_batch(paint, scale, false)
            .and_then(|mut masks| {
                let spans = masks.outline_spans(&batch, text)?;
                Some((masks.finish(), spans))
            });
        if let Some(((image, quads), spans)) = masked {
            if spans.is_empty() {
                if !quads.is_empty() {
                    draw_mask_quads(canvas, image, quads, paint);
                }
            } else {
                let mut quads = quads.into_iter();
                for span in spans {
                    match span {
                        OutlineSpan::Mask(count) => draw_mask_quads(
                            canvas,
                            image,
                            quads.by_ref().take(count).collect(),
                            paint,
                        ),
                        OutlineSpan::Native(index) => {
                            let run = &runs[index];
                            canvas
                                .fill_glyph_run(
                                    run.font,
                                    &[],
                                    glyphs[run.glyphs.clone()].iter().cloned(),
                                    paint,
                                )
                                .map_err(|error| format!("text drawing failed: {error}"))?;
                        }
                    }
                }
                self.prime_positioned(canvas, text, positions, paint, scale);
            }
            return Ok(OutlineRuns::Drawn);
        }
        // Reject bitmap, color and variable glyphs before spending the
        // preparation budget on a batch that cannot be replayed together.
        if positions
            .clone()
            .any(|(font, glyph, _, _, _)| text.outline_glyph(font, glyph).is_none())
        {
            return Ok(OutlineRuns::Unsupported);
        }
        self.prime_positioned(canvas, text, positions, paint, scale);
        Ok(OutlineRuns::Fallback)
    }
}

impl GlyphMasks {
    pub(super) fn prime_fallback(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        text: &TextSystem,
        metrics: &TextMetrics,
        paint: &Paint,
        scale: f32,
    ) {
        self.prime_positioned(
            canvas,
            text,
            metrics.glyphs.iter().map(|glyph| {
                (
                    glyph.font_id,
                    glyph.glyph_id,
                    glyph.x,
                    glyph.y,
                    !glyph.c.is_control(),
                )
            }),
            paint,
            scale,
        );
    }

    pub(super) fn prime_positioned(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        text: &TextSystem,
        glyphs: impl Iterator<Item = (FontId, u16, f32, f32, bool)> + Clone,
        paint: &Paint,
        scale: f32,
    ) {
        let size = paint.font_size();
        if self.fallback_runs == 0 || self.remaining == 0 || !(size > 0.0 && size * scale <= 40.0) {
            return;
        }
        let Some((first, _, _, _, _)) = glyphs.clone().next() else {
            return;
        };
        self.fallback_runs -= 1;
        if !glyphs
            .clone()
            .any(|(font, glyph, _, _, _)| font != first || glyph >= DENSE_GLYPH_IDS)
        {
            return;
        }
        // Ordinary text has already drawn these glyphs. Use the same
        // font IDs and positions to request masks for a later frame.
        for (font, glyph, x, y, drawable) in glyphs {
            if self.remaining == 0 {
                break;
            }
            if !drawable {
                continue;
            }
            let Some(index) = mask_slot(glyph, x * scale, y * scale) else {
                continue;
            };
            let Some(page) = page(&mut self.pages, font, size, scale, || {
                text.outline_glyph(font, glyph)
                    .is_some_and(|outline| outline.bounds.is_some())
            }) else {
                continue;
            };
            if page.tile(index).is_some() {
                continue;
            }
            let texture = match &mut self.texture {
                Some(value) => value,
                slot => {
                    let Some(texture) = MaskTexture::new(canvas) else {
                        return;
                    };
                    slot.insert(texture)
                }
            };
            if page.schedule(text, texture, index, glyph, mask_phase(index)) {
                self.pending += 1;
                self.remaining -= 1;
            }
        }
    }
}

fn page(
    pages: &mut Vec<Page>,
    font: FontId,
    size: f32,
    scale: f32,
    admit: impl FnOnce() -> bool,
) -> Option<&mut Page> {
    let index = pages
        .iter()
        .position(|page| page.font == font && page.size == size && page.scale == scale);
    if let Some(index) = index {
        return pages.get_mut(index);
    }
    if pages.len() == MAX_FONT_PAGES || !admit() {
        return None;
    }
    pages.push(Page {
        index: pages.len(),
        font,
        size,
        scale,
        tiles: Vec::new(),
        sparse: HashMap::new(),
        pending: Vec::new(),
    });
    pages.last_mut()
}

impl MaskTexture {
    fn new(canvas: &mut Canvas<WGPURenderer>) -> Option<Self> {
        // Coverage uses one channel and is independent of the window's color format.
        let image = canvas
            .create_image_empty(SIDE, SIDE, PixelFormat::Gray8, ImageFlags::NEAREST)
            .ok()?;
        Some(Self {
            image,
            regions: std::array::from_fn(|_| MaskRegion {
                packing: Atlas::new(REGION_SIDE, REGION_SIDE),
                slots: Vec::new(),
                last_used: Cell::new(0),
                last_allocated: 0,
                clear: false,
            }),
            frame: 1,
            needs_reclaim: false,
        })
    }

    fn begin_frame(&mut self) -> Option<&mut Vec<(usize, usize)>> {
        self.frame += 1;
        if !std::mem::take(&mut self.needs_reclaim) {
            return None;
        }
        // Earlier frames have already been submitted to the same queue.
        // Clear the reclaimed region before this frame samples a new slot.
        // Break ties in render use by protecting more recent allocations.
        let region = self
            .regions
            .iter_mut()
            .filter(|region| !region.slots.is_empty())
            .min_by_key(|region| (region.last_used.get(), region.last_allocated))?;
        region.packing.reset(REGION_SIDE, REGION_SIDE);
        region.clear = true;
        Some(&mut region.slots)
    }

    fn touch(&self, mut used: u8) {
        while used != 0 {
            let index = used.trailing_zeros() as usize;
            self.regions[index].last_used.set(self.frame);
            used &= used - 1;
        }
    }

    fn allocate(&mut self, width: usize, height: usize) -> Option<(usize, usize, usize)> {
        for (index, region) in self.regions.iter_mut().enumerate() {
            if let Some((x, y)) = region.packing.add_rect(width, height) {
                region.last_used.set(self.frame);
                region.last_allocated = self.frame;
                let (origin_x, origin_y) = region_origin(index);
                return Some((index, origin_x + x, origin_y + y));
            }
        }
        self.needs_reclaim = true;
        None
    }

    fn clear_reclaimed(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        for (index, region) in self.regions.iter_mut().enumerate() {
            if std::mem::take(&mut region.clear) {
                let (x, y) = region_origin(index);
                canvas.clear_rect(
                    x as u32,
                    y as u32,
                    REGION_SIDE as u32,
                    REGION_SIDE as u32,
                    Color::rgba(0, 0, 0, 0),
                );
            }
        }
    }
}

fn region_origin(index: usize) -> (usize, usize) {
    ((index % 2) * REGION_SIDE, (index / 2) * REGION_SIDE)
}

impl Page {
    fn tile(&self, index: usize) -> Option<Tile> {
        if index < DENSE_SLOTS {
            self.tiles.get(index).copied().flatten()
        } else {
            self.sparse.get(&index).copied()
        }
    }

    fn remove(&mut self, index: usize) {
        if index < DENSE_SLOTS {
            self.tiles[index] = None;
        } else {
            self.sparse.remove(&index);
        }
    }

    fn remove_evicted_pending(&mut self) {
        let tiles = &self.tiles;
        let sparse = &self.sparse;
        self.pending.retain(|(index, _)| {
            if *index < DENSE_SLOTS {
                tiles[*index].is_some()
            } else {
                sparse.contains_key(index)
            }
        });
    }

    fn schedule(
        &mut self,
        text: &TextSystem,
        texture: &mut MaskTexture,
        index: usize,
        glyph: u16,
        phase: i8,
    ) -> bool {
        let Some(outline) = text.outline_glyph(self.font, glyph) else {
            return false;
        };
        let Some(extent) = outline
            .atlas_extents(crate::text::atlas_size_range(self.size, self.scale))
            .map(crate::text::GlyphExtents::padded_edges)
        else {
            return false;
        };
        self.schedule_extent(texture, index, glyph, phase, extent)
    }

    fn schedule_extent(
        &mut self,
        texture: &mut MaskTexture,
        index: usize,
        glyph: u16,
        phase: i8,
        extent: [f32; 4],
    ) -> bool {
        let [left, top, right, bottom] = extent;
        let width = (right - left) as usize;
        let height = (bottom - top) as usize;
        if width == 0 || width > 128 || height == 0 || height > 128 {
            return false;
        }
        // Leave room for a positive bearing so the atlas pen position
        // remains nonnegative and uses the same subpixel bin.
        let leading = left.max(0.0) as usize;
        if leading > 128 {
            return false;
        }
        let Some((region, x, y)) = texture.allocate(width + leading, height) else {
            return false;
        };
        let x = x + leading;
        let fraction = if phase.abs() == 10 {
            0.98 * f32::from(phase.signum())
        } else {
            f32::from(phase) / 10.0
        };
        // FemtoVG uses the signed fraction of the pen position. Keep
        // negative pens negative when preparing their masks and translate
        // the canvas to place the result inside the texture.
        let origin = x as f32 - left - if phase < 0 { NEGATIVE_ORIGIN } else { 0.0 };
        let sample = origin + fraction;
        if sample.trunc() != origin || subpixel_phase(sample.fract()) != phase {
            return false;
        }
        // FemtoVG reverses the Y axis for image targets.
        let (top_uv, bottom_uv) = (SIDE - y, SIDE - (y + height));
        let uv = [x, top_uv, x + width, bottom_uv].map(|edge| edge as f32 / SIDE as f32);
        let tile = Tile {
            uv,
            extent,
            region: region as u8,
            ready: false,
        };
        texture.regions[region].slots.push((self.index, index));
        if index < DENSE_SLOTS {
            if self.tiles.len() <= index {
                self.tiles.resize(index + 1, None);
            }
            self.tiles[index] = Some(tile);
        } else {
            self.sparse.insert(index, tile);
        }
        self.pending.push((
            index,
            PositionedGlyph {
                x: sample / self.scale,
                y: (y as f32 - top) / self.scale,
                glyph_id: glyph,
            },
        ));
        true
    }

    fn prepare(&mut self, canvas: &mut Canvas<WGPURenderer>) -> bool {
        if self.pending.is_empty() {
            return true;
        }
        for negative in [false, true] {
            let mut glyphs = self
                .pending
                .iter()
                .filter(|(index, _)| (*index >= POSITIVE_SLOTS) == negative)
                .map(|(_, glyph)| glyph.clone())
                .peekable();
            if glyphs.peek().is_none() {
                continue;
            }
            let translation = if negative { NEGATIVE_ORIGIN } else { 0.0 };
            crate::transform::set_canvas_transform(
                canvas,
                crate::CanvasTransform::from_coefficients([
                    self.scale,
                    0.0,
                    0.0,
                    self.scale,
                    translation,
                    0.0,
                ]),
            );
            if canvas
                .fill_glyph_run(
                    self.font,
                    &[],
                    glyphs,
                    &Paint::color(Color::white()).with_font_size(self.size),
                )
                .is_err()
            {
                return false;
            }
        }
        for (index, _) in self.pending.drain(..) {
            let tile = if index < DENSE_SLOTS {
                self.tiles[index].as_mut()
            } else {
                self.sparse.get_mut(&index)
            };
            if let Some(tile) = tile {
                tile.ready = true;
            }
        }
        true
    }
}

pub(super) struct MaskBatch<'a> {
    texture: &'a mut MaskTexture,
    used: u8,
    pages: &'a mut Vec<Page>,
    page: Option<usize>,
    pending: &'a mut usize,
    remaining: &'a mut usize,
    allow_preparation: bool,
    size: f32,
    scale: f32,
    inverse: f32,
    empty_glyph: Option<(FontId, u16)>,
    pub(super) quads: Vec<Quad>,
    pub(super) clipped: bool,
}

impl GlyphMasks {
    pub(super) fn clipped_batch(
        &mut self,
        paint: &Paint,
        scale: f32,
        portable_cell: bool,
    ) -> Option<MaskBatch<'_>> {
        let size = paint.font_size();
        let allow_preparation = self.can_prepare_during_paint(portable_cell);
        let texture = self.texture.as_mut()?;
        if !self.pages.iter().any(|page| {
            page.size == size
                && page.scale == scale
                && (!page.tiles.is_empty() || !page.sparse.is_empty())
        }) {
            return None;
        }
        Some(MaskBatch {
            texture,
            used: 0,
            pages: &mut self.pages,
            pending: &mut self.pending,
            remaining: &mut self.remaining,
            allow_preparation,
            page: None,
            size,
            scale,
            inverse: 1.0 / scale,
            empty_glyph: None,
            quads: Vec::new(),
            clipped: false,
        })
    }
}

impl MaskBatch<'_> {
    fn outline_spans(
        &mut self,
        batch: &OutlineBatch<'_>,
        text: &TextSystem,
    ) -> Option<Vec<OutlineSpan>> {
        // The label's scissor clips the draw. Keep native runs intact and
        // combine only adjacent completed mask runs to preserve paint order.
        let edges = [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ];
        let mut spans = Vec::new();
        let mut covered = 0;
        for (index, run) in batch.runs.iter().enumerate() {
            let start = self.quads.len();
            if batch.glyphs[run.glyphs.clone()]
                .iter()
                .all(|glyph| self.append(run.font, glyph.glyph_id, [glyph.x, glyph.y], edges, text))
            {
                continue;
            }
            if !batch.allow_partial {
                return None;
            }
            self.quads.truncate(start);
            if start > covered {
                spans.push(OutlineSpan::Mask(start - covered));
                covered = start;
            }
            spans.push(OutlineSpan::Native(index));
        }
        if !spans.is_empty() {
            if self.quads.is_empty() {
                return None;
            }
            if self.quads.len() > covered {
                spans.push(OutlineSpan::Mask(self.quads.len() - covered));
            }
        }
        Some(spans)
    }

    pub(super) fn append(
        &mut self,
        font: FontId,
        glyph: u16,
        origin: [f32; 2],
        edge: [f32; 4],
        text: &TextSystem,
    ) -> bool {
        let page_index = self
            .page
            .filter(|&page| self.pages[page].font == font)
            .or_else(|| {
                self.pages.iter().position(|page| {
                    page.font == font && page.size == self.size && page.scale == self.scale
                })
            });
        self.page = page_index;
        let [x, y] = origin.map(|position| position * self.scale);
        let Some(index) = mask_slot(glyph, x, y) else {
            return false;
        };
        let outside = |[left, top, right, bottom]: [f32; 4]| {
            right <= edge[0] || bottom <= edge[1] || left >= edge[2] || top >= edge[3]
        };
        let tile = if let Some(tile) = page_index.and_then(|page| self.pages[page].tile(index)) {
            tile
        } else {
            if self.empty_glyph == Some((font, glyph)) {
                return true;
            }
            let Some(outline) = text.outline_glyph(font, glyph) else {
                return false;
            };
            if outline.bounds.is_none() {
                self.empty_glyph = Some((font, glyph));
                return true;
            }
            let Some(extent) = outline
                .atlas_extents(crate::text::atlas_size_range(self.size, self.scale))
                .map(crate::text::GlyphExtents::padded_edges)
            else {
                return false;
            };
            // A hidden glyph neither requires a tile nor consumes a slot in
            // the per-frame preparation budget for visible glyphs.
            if outside(placed_edges(extent, x, y)) {
                self.clipped = true;
                return true;
            }
            if !self.allow_preparation || *self.remaining == 0 {
                return false;
            }
            let page = if let Some(index) = page_index {
                &mut self.pages[index]
            } else {
                let Some(page) = page(self.pages, font, self.size, self.scale, || true) else {
                    return false;
                };
                self.page = Some(page.index);
                page
            };
            if !page.schedule_extent(self.texture, index, glyph, mask_phase(index), extent) {
                return false;
            }
            *self.pending += 1;
            *self.remaining -= 1;
            let Some(tile) = page.tile(index) else {
                return false;
            };
            tile
        };
        let mut edges = placed_edges(tile.extent, x, y);
        let mut uv = tile.uv;
        if edges[0] < edge[0] || edges[1] < edge[1] || edges[2] > edge[2] || edges[3] > edge[3] {
            let clipped = [
                edges[0].max(edge[0]),
                edges[1].max(edge[1]),
                edges[2].min(edge[2]),
                edges[3].min(edge[3]),
            ];
            if clipped[0] >= clipped[2] || clipped[1] >= clipped[3] {
                self.clipped = true;
                return true;
            }
            let dy = if uv[3] > uv[1] { 1.0 } else { -1.0 };
            for (i, (value, sign)) in uv.iter_mut().zip([1.0, dy, 1.0, dy]).enumerate() {
                *value += (clipped[i] - edges[i]) * sign / SIDE as f32;
            }
            edges = clipped;
            self.clipped = true;
        }
        if !tile.ready && !self.allow_preparation {
            return false;
        }
        self.used |= 1 << tile.region;
        self.quads.push(quad(edges, uv, self.inverse));
        true
    }

    pub(super) fn finish(self) -> (ImageId, Vec<Quad>) {
        self.texture.touch(self.used);
        (self.texture.image, self.quads)
    }
}
