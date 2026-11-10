use crate::{color, paint_brush};
use femtovg::{
    Canvas, Color, CompositeOperation, ImageFlags, ImageId, Paint, Path, PixelFormat, Transform2D,
    renderer::{RenderTarget, WGPURenderer},
};
use xrack_layout::Rect;
use xrack_runtime::{Brush, Value};

#[derive(Clone, Copy)]
pub(crate) enum Colorize<'a> {
    Original,
    Solid(Color),
    Gradient(&'a Brush),
}

impl<'a> Colorize<'a> {
    pub(crate) fn from_value(value: Option<&'a Value>) -> Self {
        match value {
            Some(Value::Color(tint) | Value::Brush(Brush::Solid(tint))) if tint.0 & 0xff != 0 => {
                Self::Solid(color(*tint))
            }
            Some(Value::Brush(brush @ (Brush::Linear { stops, .. } | Brush::Radial { stops })))
                if stops.iter().any(|stop| stop.color.0 & 0xff != 0) =>
            {
                Self::Gradient(brush)
            }
            _ => Self::Original,
        }
    }

    pub(crate) fn uses_alpha(self) -> bool {
        !matches!(self, Self::Original)
    }

    pub(crate) fn paint(self, texture: ImageId, pattern: Rect) -> Paint {
        let tint = match self {
            Self::Solid(tint) => tint,
            _ => Color::white(),
        };
        Paint::image_tint(
            texture,
            pattern.x,
            pattern.y,
            pattern.width,
            pattern.height,
            0.0,
            tint,
        )
    }
}

/// Map the full intrinsic source to one raster region without moving its
/// coverage path. The same paint therefore spans the cuts of a nine-slice.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ColorizeGeometry {
    pub(crate) source: [u32; 2],
    pub(crate) transform: Transform2D,
}

pub(crate) struct GradientTexture {
    pub(crate) id: ImageId,
    pub(crate) used: bool,
    paint: Option<(Brush, ColorizeGeometry)>,
}

impl GradientTexture {
    pub(crate) fn new(
        canvas: &mut Canvas<WGPURenderer>,
        size: [u32; 2],
        sampling: ImageFlags,
    ) -> Result<Self, String> {
        let [width, height] = size;
        // Canvas image targets hold rows bottom-up.
        let flags = sampling | ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y;
        let id = canvas
            .create_image_empty(width as usize, height as usize, PixelFormat::Rgba8, flags)
            .map_err(|error| format!("image colorize texture allocation failed: {error}"))?;
        Ok(Self {
            id,
            used: false,
            paint: None,
        })
    }

    pub(crate) fn paint(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        mask: ImageId,
        size: [u32; 2],
        geometry: ColorizeGeometry,
        brush: &Brush,
    ) -> ImageId {
        self.used = true;
        if self
            .paint
            .as_ref()
            .is_some_and(|(old, region)| old == brush && *region == geometry)
        {
            return self.id;
        }
        let [width, height] = size;
        let [source_w, source_h] = geometry.source;
        let mut gradient = paint_brush(
            brush,
            Rect::new(0.0, 0.0, source_w as f32, source_h as f32),
            0.0,
        )
        .with_gradient_transform(geometry.transform);
        gradient.set_anti_alias(false);
        let mut coverage = Paint::image(mask, 0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        coverage.set_anti_alias(false);
        let mut rect = Path::new();
        rect.rect(0.0, 0.0, width as f32, height as f32);

        // The render-target switches are ordered with the image draws. Different
        // paints sharing a source can reuse this store without aliasing a draw.
        canvas.save();
        canvas.reset_transform();
        canvas.reset_scissor();
        canvas.set_global_alpha(1.0);
        canvas.with_render_target(RenderTarget::Image(self.id), |canvas| {
            canvas.clear_rect(0, 0, width, height, Color::rgba(0, 0, 0, 0));
            canvas.global_composite_operation(CompositeOperation::SourceOver);
            canvas.fill_path(&rect, &coverage);
            canvas.global_composite_operation(CompositeOperation::SourceIn);
            canvas.fill_path(&rect, &gradient);
        });
        canvas.restore();
        self.paint = Some((brush.clone(), geometry));
        self.id
    }
}
