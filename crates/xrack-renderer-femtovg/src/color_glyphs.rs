use femtovg::{
    Canvas, DrawCommand, FontId, GlyphDrawCommands, ImageFlags, ImageId, ImageSource, Paint, Quad,
    TextMetrics, renderer::WGPURenderer,
};
use rgb::FromSlice;
use std::collections::HashMap;

/// A system glyph bitmap, with an origin relative to its alphabetic baseline.
#[doc(hidden)]
pub struct GlyphBitmap {
    pub offset: [i32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct ColorGlyphs {
    textures: HashMap<(u16, u16), Texture>,
}

struct Texture {
    id: ImageId,
    offset: [i32; 2],
    size: [u32; 2],
    used: bool,
}

impl ColorGlyphs {
    pub(crate) fn begin_frame(&mut self, canvas_changed: bool) {
        if canvas_changed {
            self.textures.clear();
        }
        for texture in self.textures.values_mut() {
            texture.used = false;
        }
    }

    pub(crate) fn end_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.textures.retain(|_, texture| {
            if !texture.used {
                canvas.delete_image(texture.id);
            }
            texture.used
        });
    }

    pub(crate) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        metrics: &TextMetrics,
        paint: &Paint,
        font: FontId,
        rasterize: impl Fn(u16, u16) -> Option<GlyphBitmap>,
    ) {
        let font_size = paint.font_size();
        let pixels_per_em = (font_size * crate::transform::glyph_raster_scale(canvas))
            .ceil()
            .clamp(1.0, 256.0) as u16;
        let scale = font_size / f32::from(pixels_per_em);
        let mut commands = GlyphDrawCommands::default();
        for glyph in metrics.glyphs.iter().filter(|glyph| {
            glyph.font_id == font && glyph.glyph_id != 0 && !glyph.c.is_whitespace()
        }) {
            let key = (glyph.glyph_id, pixels_per_em);
            let texture = match self.textures.entry(key) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let Some(bitmap) = rasterize(glyph.glyph_id, pixels_per_em) else {
                        continue;
                    };
                    let [width, height] = bitmap.size.map(|size| size as usize);
                    if width == 0
                        || height == 0
                        || width
                            .checked_mul(height)
                            .and_then(|area| area.checked_mul(4))
                            != Some(bitmap.rgba.len())
                    {
                        continue;
                    }
                    let pixels =
                        ImageSource::Rgba(imgref::Img::new(bitmap.rgba.as_rgba(), width, height));
                    let Ok(id) = canvas.create_image(pixels, ImageFlags::empty()) else {
                        continue;
                    };
                    entry.insert(Texture {
                        id,
                        offset: bitmap.offset,
                        size: bitmap.size,
                        used: true,
                    })
                }
            };
            texture.used = true;
            let x = glyph.x + texture.offset[0] as f32 * scale;
            let y = glyph.y + texture.offset[1] as f32 * scale;
            commands.color_glyphs.push(DrawCommand {
                image_id: texture.id,
                quads: vec![Quad {
                    x0: x,
                    y0: y,
                    s0: 0.0,
                    t0: 0.0,
                    x1: x + texture.size[0] as f32 * scale,
                    y1: y + texture.size[1] as f32 * scale,
                    s1: 1.0,
                    t1: 1.0,
                }],
            });
        }
        if !commands.color_glyphs.is_empty() {
            canvas.draw_glyph_commands(commands, paint);
        }
    }
}
