//! Raster-space evaluation of a programmable sky, independently of its IBL cube.
use std::sync::Arc;

use xrack_runtime::scene3d::{SceneCamera, SkyProgram};

use super::{
    HDR_FORMAT, append_floats, buffer,
    sky_shader::{SkyShaders, SkyTarget},
};

#[derive(Clone, Copy)]
pub(super) struct ScreenSkyInput<'a> {
    pub program: &'a Arc<SkyProgram>,
    pub basis: [[f32; 4]; 3],
    pub divisor: u32,
}

struct ScreenImage {
    texture: wgpu::TextureView,
    program: Arc<SkyProgram>,
    settings: Vec<u8>,
}

#[derive(Default)]
pub(super) struct ScreenSky {
    image: Option<ScreenImage>,
    recorded: bool,
    pub dispatches: u64,
    pub texels: u64,
}

impl ScreenSky {
    pub fn prepare(
        &mut self,
        shaders: &mut SkyShaders,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        size: [u32; 2],
        camera: SceneCamera,
        input: ScreenSkyInput<'_>,
    ) -> wgpu::TextureView {
        let [width, height] = size.map(|extent| extent.div_ceil(input.divisor).max(1));
        let settings = screen_uniforms(width, height, camera, input);
        if let Some(image) = &self.image
            && image.program.shader == input.program.shader
            && image.program.uniforms == input.program.uniforms
            && image.program.textures == input.program.textures
            && image.settings == settings
        {
            return image.texture.clone();
        }
        let bindings = shaders.bindings(device, queue, input.program, SkyTarget::Screen);
        let texture = self
            .image
            .take()
            .filter(|entry| {
                entry.texture.texture().width() == width
                    && entry.texture.texture().height() == height
            })
            .map_or_else(|| sky_texture(device, width, height), |entry| entry.texture);
        let uniform = buffer(
            device,
            "XRack screen sky parameters",
            &settings,
            wgpu::BufferUsages::UNIFORM,
        );
        let output = bindings.output(device, &uniform, &texture);
        {
            let mut pass = crate::viewport_gpu_timing::compute_pass(
                encoder,
                &wgpu::ComputePassDescriptor {
                    label: Some("XRack evaluates screen sky"),
                    timestamp_writes: None,
                },
            );
            bindings.dispatch(
                &mut pass,
                &output,
                [width.div_ceil(8), height.div_ceil(8), 1],
            );
        }
        self.dispatches += 1;
        self.texels += u64::from(width) * u64::from(height);
        self.recorded = true;
        self.image = Some(ScreenImage {
            texture: texture.clone(),
            program: Arc::clone(input.program),
            settings,
        });
        texture
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        if std::mem::take(&mut self.recorded) && !submitted {
            self.image = None;
        }
    }
}

fn screen_uniforms(
    width: u32,
    height: u32,
    camera: SceneCamera,
    input: ScreenSkyInput<'_>,
) -> Vec<u8> {
    let custom = camera.has_custom_projection();
    let mut bytes = Vec::with_capacity(240);
    bytes.extend(
        [width, height, u32::from(custom), 0]
            .into_iter()
            .flat_map(u32::to_ne_bytes),
    );
    let [x, y, z] = input.program.settings.parameters;
    append_floats(&mut bytes, [x, y, z, 0.0]);
    append_floats(
        &mut bytes,
        input
            .basis
            .into_iter()
            .flat_map(|[x, y, z, _]| [x, y, z, 0.0]),
    );
    // Sky rays ignore translation under ordinary projection. Avoid
    // invalidating the image when only the camera's position changes.
    let [clip0, clip1, ..] = camera.clip_rows();
    let [x, y, z, _] = camera.depth_row();
    append_floats(
        &mut bytes,
        [clip0, clip1]
            .into_iter()
            .flat_map(|[x, y, z, _]| [x, y, z, 0.0])
            .chain([x, y, z, 0.0])
            .chain(camera.clip_lens())
            .chain([0.0, 0.0, 0.0, f32::from(camera.is_orthographic())])
            .chain(camera.custom_world_viewer().unwrap_or([0.0; 4])),
    );
    let inverse = custom
        .then(|| camera.inverse_clip_rows())
        .flatten()
        .unwrap_or([[0.0; 4]; 4]);
    append_floats(&mut bytes, inverse.into_iter().flatten());
    bytes
}

fn sky_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack screen sky image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
