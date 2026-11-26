use xrack_runtime::scene3d::SkySettings;

use super::{HDR_FORMAT, append_floats, buffer};

pub(super) struct SkyGenerator {
    shader: wgpu::ShaderModule,
    sky: Option<wgpu::ComputePipeline>,
    masked: Option<wgpu::ComputePipeline>,
}

impl SkyGenerator {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack procedural sky"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("sky_radiance.wgsl"),
                    "\n",
                    include_str!("cube_direction.wgsl"),
                    "\n",
                    include_str!("sky_cube.wgsl")
                )
                .into(),
            ),
        });
        Self {
            shader,
            sky: None,
            masked: None,
        }
    }

    pub fn generate(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        sky: SkySettings,
        horizon: f32,
    ) -> wgpu::TextureView {
        let pipeline = self
            .sky
            .get_or_insert_with(|| pipeline(device, &self.shader, "generate"));
        let mut settings = sky.uniforms();
        settings[6][2] = horizon;
        write_cube(
            device,
            encoder,
            pipeline,
            (sky.resolution(), settings),
            None,
        )
    }

    pub fn attenuate(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        horizon: f32,
    ) -> wgpu::TextureView {
        let pipeline = self
            .masked
            .get_or_insert_with(|| pipeline(device, &self.shader, "attenuate_panorama"));
        let mut settings = [[0.0; 4]; 7];
        settings[6][2] = horizon;
        write_cube(
            device,
            encoder,
            pipeline,
            (source.texture().width(), settings),
            Some((source, sampler)),
        )
    }
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("XRack environment filtering"),
        layout: None,
        module: shader,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn write_cube(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    (size, settings): (u32, [[f32; 4]; 7]),
    source: Option<(&wgpu::TextureView, &wgpu::Sampler)>,
) -> wgpu::TextureView {
    let last_mip = size.ilog2();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack filtered environment cubemap"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: last_mip + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut pass = crate::viewport_gpu_timing::compute_pass(
        encoder,
        &wgpu::ComputePassDescriptor {
            label: Some("XRack environment filtering"),
            timestamp_writes: None,
        },
    );
    pass.set_pipeline(pipeline);
    let layout = pipeline.get_bind_group_layout(0);
    for level in 0..=last_mip {
        let side = size >> level;
        let target = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let mut bytes = Vec::with_capacity(128);
        bytes.extend(
            [side, level, last_mip, 0]
                .into_iter()
                .flat_map(u32::to_ne_bytes),
        );
        append_floats(&mut bytes, settings.into_iter().flatten());
        let uniform = buffer(
            device,
            "XRack procedural sky parameters",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&target),
            },
        ];
        if let Some((view, sampler)) = source {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ]);
        }
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack filtered environment level"),
            layout: &layout,
            entries: &entries,
        });
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), 6);
    }
    drop(pass);
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}
