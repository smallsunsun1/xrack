use xrack_runtime::scene3d::SceneCamera;

use super::append_floats;

pub(super) const SHADER: &str = include_str!("screen_depth.wgsl");
pub(super) const UNIFORM_BYTES: usize = 192;
pub(super) const MODES: usize = 6;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Flat,
    Oblique,
    Matrix,
    FlatReverse,
    ObliqueReverse,
    MatrixReverse,
}

pub(super) fn mode(camera: SceneCamera) -> Mode {
    match (
        camera.has_custom_projection(),
        camera.has_oblique_depth(),
        camera.reverse_depth(),
    ) {
        (true, _, true) => Mode::MatrixReverse,
        (true, _, false) => Mode::Matrix,
        (_, true, true) => Mode::ObliqueReverse,
        (_, true, false) => Mode::Oblique,
        (_, _, true) => Mode::FlatReverse,
        _ => Mode::Flat,
    }
}

pub(super) fn compilation(mode: Mode) -> wgpu::PipelineCompilationOptions<'static> {
    wgpu::PipelineCompilationOptions {
        constants: match mode {
            Mode::Flat => &[],
            Mode::Oblique => &[("OBLIQUE_CLIP_DEPTH", 1.0)],
            Mode::Matrix => &[("MATRIX_CLIP_PROJECTION", 1.0)],
            Mode::FlatReverse => &[("REVERSE_DEVICE_DEPTH", 1.0)],
            Mode::ObliqueReverse => &[("OBLIQUE_CLIP_DEPTH", 1.0), ("REVERSE_DEVICE_DEPTH", 1.0)],
            Mode::MatrixReverse => &[
                ("MATRIX_CLIP_PROJECTION", 1.0),
                ("REVERSE_DEVICE_DEPTH", 1.0),
            ],
        },
        ..Default::default()
    }
}

/// Serialize the raster camera's depth mapping shared by screen-space effects.
pub(super) fn write(bytes: &mut Vec<u8>, camera: SceneCamera) {
    let [near, far, x, y] = camera.depth_projection();
    let [start, end] = camera.depth_limits();
    let finite_far = |far: f32| if far.is_finite() { far } else { 0.0 };
    append_floats(bytes, camera.clip_lens());
    append_floats(
        bytes,
        [
            near,
            finite_far(far),
            f32::from(u8::from(camera.is_orthographic())),
            near / far,
        ],
    );
    append_floats(bytes, [x, y, start, finite_far(end)]);
    for rows in [
        camera.custom_view_clip_rows(),
        camera.custom_inverse_view_clip_rows(),
    ] {
        append_floats(bytes, rows.unwrap_or([[0.0; 4]; 4]).into_iter().flatten());
    }
    append_floats(bytes, camera.custom_viewer().unwrap_or([0.0; 4]));
}
