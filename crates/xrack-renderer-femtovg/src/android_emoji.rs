use crate::color_glyphs::{ColorGlyphs, GlyphBitmap};
use femtovg::{Canvas, FontId, Paint, TextMetrics, renderer::WGPURenderer};
use std::path::{Path, PathBuf};

type Rasterizer = dyn Fn(&Path, u32, u16, u16) -> Option<GlyphBitmap>;

pub(crate) struct Emoji {
    rasterize: Box<Rasterizer>,
    textures: ColorGlyphs,
}

impl Emoji {
    pub(crate) fn new(rasterize: Box<Rasterizer>) -> Self {
        Self {
            rasterize,
            textures: ColorGlyphs::default(),
        }
    }

    pub(crate) fn begin_frame(&mut self, canvas_changed: bool) {
        self.textures.begin_frame(canvas_changed);
    }

    pub(crate) fn end_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.textures.end_frame(canvas);
    }

    pub(crate) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        metrics: &TextMetrics,
        paint: &Paint,
        (id, path, index): &(FontId, PathBuf, u32),
    ) {
        self.textures
            .draw(canvas, metrics, paint, *id, |glyph, size| {
                (self.rasterize)(path, *index, glyph, size)
            });
    }
}
