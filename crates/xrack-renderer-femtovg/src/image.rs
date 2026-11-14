pub(crate) use crate::image_colorize::Colorize;
use crate::{
    image_colorize::{ColorizeGeometry, GradientTexture},
    nine_slice,
};
use femtovg::{
    Canvas, ImageFlags, ImageId, ImageSource, Paint, Path, Transform2D, renderer::WGPURenderer,
};
use rgb::{FromSlice, Gray};
use std::collections::{HashMap, hash_map::Entry};
use xrack_assets::{MAX_IMAGE_DIMENSION, Raster, decode_png, rasterize_svg_region};
use xrack_layout::{ImagePresentation, ImageRegion, Rect, Size};
use xrack_runtime::{
    ImageAsset, ImageFit, ImageFormat, ImageIdentity, ImagePixels, ImageRendering, MediaFrame,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RasterKey {
    identity: ImageIdentity,
    len: usize,
    format: ImageFormat,
    size: [u32; 2],
    svg_region: Option<[u32; 4]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    raster: RasterKey,
    region: Option<[u32; 4]>,
    repeat: [bool; 2],
    nearest: bool,
    alpha_mask: bool,
}

impl Key {
    fn sampling(self) -> ImageFlags {
        let mut flags = ImageFlags::empty();
        flags.set(ImageFlags::NEAREST, self.nearest);
        flags.set(ImageFlags::REPEAT_X, self.repeat[0]);
        flags.set(ImageFlags::REPEAT_Y, self.repeat[1]);
        flags
    }

    fn colorize_geometry(self, source: [u32; 2]) -> ColorizeGeometry {
        let [source_x, source_y, source_w, source_h] = self
            .raster
            .svg_region
            .unwrap_or([0, 0, source[0], source[1]]);
        let [raster_w, raster_h] = self.raster.size;
        let [region_x, region_y, _, _] = self.region.unwrap_or([0, 0, raster_w, raster_h]);
        let sx = raster_w as f32 / source_w as f32;
        let sy = raster_h as f32 / source_h as f32;
        ColorizeGeometry {
            source,
            transform: Transform2D::new(
                sx,
                0.0,
                0.0,
                sy,
                -(source_x as f32 * sx + region_x as f32),
                -(source_y as f32 * sy + region_y as f32),
            ),
        }
    }
}

struct Texture {
    id: ImageId,
    size: [u32; 2],
    used: bool,
    gradient: Option<GradientTexture>,
}

struct Atlas {
    raster: Raster,
    last_used: u64,
}

/// Keep source pixels for changing PNG atlas cells. GPU textures still retain
/// only the regions used this frame, and large sources use a draw-local raster.
#[derive(Default)]
struct AtlasCache {
    entries: HashMap<RasterKey, Atlas>,
    bytes: usize,
    clock: u64,
}

impl AtlasCache {
    fn raster<'a>(
        &'a mut self,
        key: RasterKey,
        bytes: &[u8],
        scratch: &'a mut Option<Raster>,
    ) -> Result<&'a Raster, String> {
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        const MAX_ATLASES: usize = 128;
        if let Some(raster) = scratch {
            return Ok(raster);
        }
        self.clock = self.clock.wrapping_add(1);
        if !self.entries.contains_key(&key) {
            let raster = decode_png(bytes)?;
            if raster.rgba.len() > MAX_BYTES {
                return Ok(scratch.insert(raster));
            }
            while self.bytes + raster.rgba.len() > MAX_BYTES || self.entries.len() >= MAX_ATLASES {
                let oldest = *self
                    .entries
                    .iter()
                    .min_by_key(|(_, atlas)| atlas.last_used)
                    .expect("cache contains an atlas to evict")
                    .0;
                self.bytes -= self.entries.remove(&oldest).unwrap().raster.rgba.len();
            }
            self.bytes += raster.rgba.len();
            self.entries.insert(
                key,
                Atlas {
                    raster,
                    last_used: self.clock,
                },
            );
        }
        let atlas = self.entries.get_mut(&key).unwrap();
        atlas.last_used = self.clock;
        Ok(&atlas.raster)
    }
}

struct VideoTexture {
    id: ImageId,
    frame: MediaFrame,
    used: bool,
}

#[derive(Default)]
pub(crate) struct Images {
    textures: HashMap<Key, Texture>,
    atlases: AtlasCache,
    video: HashMap<usize, VideoTexture>,
}

impl Images {
    pub(crate) fn draw_icon(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        image: &ImageAsset,
        bounds: Rect,
        scale: f32,
        enabled: bool,
    ) -> Result<(), String> {
        canvas.save();
        if !enabled {
            canvas.set_global_alpha(0.45);
        }
        let result = self.draw(
            canvas,
            image,
            bounds,
            scale,
            ImagePresentation::whole(image.size(), ImageFit::Contain),
            Colorize::Original,
        );
        canvas.restore();
        result
    }

    pub(crate) fn begin_frame(&mut self) {
        for texture in self.video.values_mut() {
            texture.used = false;
        }
        for texture in self.textures.values_mut() {
            texture.used = false;
            if let Some(gradient) = &mut texture.gradient {
                gradient.used = false;
            }
        }
    }

    pub(crate) fn end_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.video.retain(|_, texture| {
            if !texture.used {
                canvas.delete_image(texture.id);
            }
            texture.used
        });
        self.textures.retain(|_, texture| {
            if let Some(gradient) = texture.gradient.take_if(|gradient| !gradient.used) {
                canvas.delete_image(gradient.id);
            }
            if !texture.used {
                canvas.delete_image(texture.id);
            }
            texture.used
        });
    }

    pub(crate) fn draw_video(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        key: usize,
        frame: &MediaFrame,
        bounds: Rect,
        presentation: ImagePresentation,
    ) -> Result<(), String> {
        let placement = presentation.placement(bounds);
        if placement.visible.width <= 0.0 || placement.visible.height <= 0.0 {
            return Ok(());
        }
        let [width, height] = frame.size();
        let pixels = ImageSource::Rgba(imgref::Img::new(
            frame.rgba().as_rgba(),
            width as usize,
            height as usize,
        ));
        if self
            .video
            .get(&key)
            .is_some_and(|texture| texture.frame.size() != frame.size())
        {
            canvas.delete_image(self.video.remove(&key).unwrap().id);
        }
        let texture = match self.video.entry(key) {
            Entry::Occupied(entry) => {
                let texture = entry.into_mut();
                if &texture.frame != frame {
                    canvas
                        .update_image(texture.id, pixels, 0, 0)
                        .map_err(|error| error.to_string())?;
                    texture.frame = frame.clone();
                }
                texture
            }
            Entry::Vacant(entry) => entry.insert(VideoTexture {
                id: canvas
                    .create_image(pixels, ImageFlags::empty())
                    .map_err(|error| error.to_string())?,
                frame: frame.clone(),
                used: true,
            }),
        };
        texture.used = true;
        let paint = Colorize::Original.paint(texture.id, placement.pattern);
        fill(canvas, placement.visible, &paint);
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        image: &ImageAsset,
        bounds: Rect,
        scale: f32,
        presentation: ImagePresentation,
        colorize: Colorize<'_>,
    ) -> Result<(), String> {
        let image = image.frame(0);
        let Some(ImagePixels {
            identity,
            format,
            bytes,
            size: source,
            edges,
        }) = image.pixels()
        else {
            return Ok(());
        };
        if edges.is_some_and(|edges| !edges.fits(source)) {
            return Err(xrack_runtime::InvalidNineSlice.to_string());
        }
        let clip = presentation.region;
        if clip.width == 0 || clip.height == 0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Ok(());
        }
        let placement = presentation.placement(bounds);
        if edges.is_none() && (placement.visible.width <= 0.0 || placement.visible.height <= 0.0) {
            return Ok(());
        }
        let region = (clip != ImageRegion::full(source)).then_some([
            clip.x,
            clip.y,
            clip.width,
            clip.height,
        ]);
        let nearest = presentation.rendering == ImageRendering::Pixelated;
        let raster_size = match format {
            ImageFormat::Png => source,
            ImageFormat::Svg => {
                // Nine-slice SVGs keep their source density across resizes. The
                // rest rasterize just the selected region at the pattern scale.
                let (size, scale) = if nearest {
                    (clip.logical_size(), 1.0)
                } else if edges.is_some() {
                    (clip.logical_size(), scale.max(1.0))
                } else {
                    (
                        Size::new(placement.pattern.width, placement.pattern.height),
                        scale,
                    )
                };
                let raster_scale =
                    scale.min(MAX_IMAGE_DIMENSION as f32 / size.width.max(size.height));
                [size.width, size.height].map(|n| {
                    (n * raster_scale)
                        .ceil()
                        .clamp(1.0, MAX_IMAGE_DIMENSION as f32) as u32
                })
            }
        };
        let is_png = format == ImageFormat::Png;
        let key = Key {
            raster: RasterKey {
                identity,
                len: bytes.len(),
                format,
                size: raster_size,
                svg_region: if is_png { None } else { region },
            },
            region: if is_png { region } else { None },
            repeat: placement.repeat,
            nearest,
            alpha_mask: colorize.uses_alpha(),
        };
        let mut scratch = None;
        if let Some(edges) = edges {
            let edges = clip.clipped_edges(source, edges);
            let grid_size = if is_png { clip.size() } else { raster_size };
            for patch in
                nine_slice::patches(bounds, clip.size(), grid_size, edges, presentation.modes)
            {
                let [x, y, width, height] = patch.source;
                let region = if is_png {
                    [clip.x + x, clip.y + y, width, height]
                } else {
                    patch.source
                };
                let key = Key {
                    region: Some(region),
                    repeat: patch.repeat,
                    ..key
                };
                let texture = self.texture(canvas, key, bytes, &mut scratch, source, colorize)?;
                let mut paint = colorize.paint(
                    texture,
                    Rect::new(
                        patch.destination.x,
                        patch.destination.y,
                        patch.tile.width,
                        patch.tile.height,
                    ),
                );
                // Independent anti-aliased edges would blend at shared cuts.
                paint.set_anti_alias(false);
                fill(canvas, patch.destination, &paint);
            }
        } else {
            let texture = self.texture(canvas, key, bytes, &mut scratch, source, colorize)?;
            let paint = colorize.paint(texture, placement.pattern);
            fill(canvas, placement.visible, &paint);
        }
        Ok(())
    }

    fn texture(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        key: Key,
        bytes: &[u8],
        scratch: &mut Option<Raster>,
        source: [u32; 2],
        colorize: Colorize<'_>,
    ) -> Result<ImageId, String> {
        let texture = match self.textures.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let RasterKey {
                    format,
                    size,
                    svg_region,
                    ..
                } = key.raster;
                let raster = if format == ImageFormat::Png && key.region.is_some() {
                    self.atlases.raster(key.raster, bytes, scratch)?
                } else {
                    match scratch {
                        Some(raster) => raster,
                        empty => empty.insert(match format {
                            ImageFormat::Png => decode_png(bytes)?,
                            ImageFormat::Svg => rasterize_svg_region(bytes, size, svg_region)?,
                        }),
                    }
                };
                let [width, height] = raster.size;
                let mut pixels =
                    imgref::Img::new(raster.rgba.as_rgba(), width as usize, height as usize);
                if let Some([x, y, w, h]) = key.region {
                    if x.saturating_add(w) > width || y.saturating_add(h) > height {
                        return Err("image clip is outside the decoded raster".into());
                    }
                    pixels = pixels.sub_image(x as usize, y as usize, w as usize, h as usize);
                }
                let size = [pixels.width() as u32, pixels.height() as u32];
                let mut flags = key.sampling();
                let alpha;
                let packed;
                let image = if key.alpha_mask {
                    // Gray8 samples as (a, a, a, a) in FemtoVG's image shader.
                    alpha = pixels
                        .pixels()
                        .map(|pixel| Gray(pixel.a))
                        .collect::<Vec<_>>();
                    ImageSource::Gray(imgref::Img::new(
                        alpha.as_slice(),
                        pixels.width(),
                        pixels.height(),
                    ))
                } else {
                    flags.set(ImageFlags::PREMULTIPLIED, format == ImageFormat::Svg);
                    if pixels.stride() != pixels.width() {
                        // WGPU's upload reads tightly packed rows.
                        packed = pixels.pixels().collect::<Vec<_>>();
                        pixels =
                            imgref::Img::new(packed.as_slice(), pixels.width(), pixels.height());
                    }
                    ImageSource::Rgba(pixels)
                };
                let id = canvas
                    .create_image(image, flags)
                    .map_err(|error| format!("image texture upload failed: {error}"))?;
                entry.insert(Texture {
                    id,
                    size,
                    used: true,
                    gradient: None,
                })
            }
        };
        texture.used = true;
        if let Colorize::Gradient(brush) = colorize {
            let gradient = match &mut texture.gradient {
                Some(gradient) => gradient,
                slot => slot.insert(GradientTexture::new(canvas, texture.size, key.sampling())?),
            };
            return Ok(gradient.paint(
                canvas,
                texture.id,
                texture.size,
                key.colorize_geometry(source),
                brush,
            ));
        }
        Ok(texture.id)
    }
}

fn fill(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, paint: &Paint) {
    let mut path = Path::new();
    path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    canvas.fill_path(&path, paint);
}
