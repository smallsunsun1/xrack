use femtovg::{Canvas, Transform2D as FemtoTransform, renderer::WGPURenderer};
use xrack_layout::{LayoutSpace, Transform2D};

/// Pixels in the current canvas target, including offscreen targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TargetPixels;

pub(super) type CanvasTransform = Transform2D<LayoutSpace, TargetPixels>;

pub(super) fn canvas_transform(canvas: &Canvas<WGPURenderer>) -> CanvasTransform {
    CanvasTransform::from_coefficients(canvas.transform().0)
}

pub(super) fn to_femtovg<From, To>(transform: Transform2D<From, To>) -> FemtoTransform {
    FemtoTransform(transform.coefficients())
}

pub(super) fn set_canvas_transform(canvas: &mut Canvas<WGPURenderer>, transform: CanvasTransform) {
    canvas.reset_transform();
    canvas.set_transform(&to_femtovg(transform));
}

pub(super) fn has_device_scale(transform: CanvasTransform, scale: f32) -> bool {
    transform.axis_scales() == Some([scale, scale]) && transform.translation() == [0.0, 0.0]
}

pub(super) fn has_pixel_aligned_scale(transform: CanvasTransform, scale: f32) -> bool {
    transform.axis_scales() == Some([scale, scale])
        && transform
            .translation()
            .into_iter()
            .all(|offset| offset.fract() == 0.0)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(super) fn glyph_raster_scale(canvas: &Canvas<WGPURenderer>) -> f32 {
    // Match FemtoVG's font size selection for its fallback glyphs.
    canvas.transform().average_scale()
}
