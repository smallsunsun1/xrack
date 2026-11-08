use femtovg::{Canvas, ImageFlags, ImageId, ImageSource, Paint, Path, renderer::WGPURenderer};
use rgb::FromSlice;
use std::collections::HashMap;
use xrack_assets::{MAX_IMAGE_DIMENSION, decode_png, rasterize_svg};
use xrack_layout::{Rect, Size};
use xrack_runtime::{Image, ImageFit};

pub(crate) fn fitted_rect(bounds: Rect, source: Size, fit: ImageFit) -> Rect {
    let (width, height) = match fit {
        ImageFit::Fill => return bounds,
        ImageFit::Preserve => (source.width, source.height),
        ImageFit::Contain | ImageFit::Cover => {
            let x = bounds.width / source.width;
            let y = bounds.height / source.height;
            let scale = if fit == ImageFit::Contain {
                x.min(y)
            } else {
                x.max(y)
            };
            (source.width * scale, source.height * scale)
        }
    };
    Rect::new(
        bounds.x + (bounds.width - width) * 0.5,
        bounds.y + (bounds.height - height) * 0.5,
        width,
        height,
    )
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    address: usize,
    len: usize,
    format: xrack_runtime::ImageFormat,
    size: [u32; 2],
}

struct Texture {
    id: ImageId,
    used: bool,
}

#[derive(Default)]
pub(crate) struct Images {
    textures: HashMap<Key, Texture>,
}

impl Images {
    pub(crate) fn begin_frame(&mut self) {
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
        image: Image,
        bounds: Rect,
        fit: ImageFit,
        scale: f32,
    ) -> Result<(), String> {
        use xrack_runtime::ImageFormat;

        let Image::Embedded {
            format,
            bytes,
            size: [width, height],
        } = image
        else {
            return Ok(());
        };
        if width == 0 || height == 0 {
            return Ok(());
        }
        let destination = fitted_rect(bounds, Size::new(width as f32, height as f32), fit);
        let visible = destination.intersect(bounds);
        if visible.width <= 0.0 || visible.height <= 0.0 {
            return Ok(());
        }

        let size = match format {
            ImageFormat::Png => [width, height],
            ImageFormat::Svg => {
                // Bound texture size even if a narrow source is used with cover in a wide box.
                let raster_scale = scale
                    .min(MAX_IMAGE_DIMENSION as f32 / destination.width.max(destination.height));
                [destination.width, destination.height].map(|n| {
                    (n * raster_scale)
                        .ceil()
                        .clamp(1.0, MAX_IMAGE_DIMENSION as f32) as u32
                })
            }
        };
        let key = Key {
            address: bytes.as_ptr() as usize,
            len: bytes.len(),
            format,
            size,
        };
        let texture = match self.textures.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let (raster, flags) = match format {
                    ImageFormat::Png => (decode_png(bytes)?, ImageFlags::empty()),
                    ImageFormat::Svg => (rasterize_svg(bytes, size)?, ImageFlags::PREMULTIPLIED),
                };
                let [width, height] = raster.size;
                let pixels =
                    imgref::Img::new(raster.rgba.as_rgba(), width as usize, height as usize);
                let id = canvas
                    .create_image(ImageSource::Rgba(pixels), flags)
                    .map_err(|error| format!("image texture upload failed: {error}"))?;
                entry.insert(Texture { id, used: true })
            }
        };
        texture.used = true;

        let mut path = Path::new();
        path.rect(visible.x, visible.y, visible.width, visible.height);
        canvas.fill_path(
            &path,
            &Paint::image(
                texture.id,
                destination.x,
                destination.y,
                destination.width,
                destination.height,
                0.0,
                1.0,
            ),
        );
        Ok(())
    }
}
