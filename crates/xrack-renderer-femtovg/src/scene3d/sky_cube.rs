use std::ops::Range;

use xrack_runtime::scene3d::{SkyProgram, SkySettings};

use super::{
    HDR_FORMAT, append_floats, buffer,
    sky_shader::{SkyShaders, SkyTarget},
};

pub(super) struct SkyPrefilter {
    sums: wgpu::Buffer,
    source: wgpu::TextureView,
    horizon: f32,
    samples: u32,
}

impl SkyPrefilter {
    pub fn new(
        device: &wgpu::Device,
        source: wgpu::TextureView,
        horizon: f32,
        samples: u32,
    ) -> Self {
        let size = source.texture().width();
        let texels: u32 = (1..=size.ilog2()).map(|mip| 6 * (size >> mip).pow(2)).sum();
        let sums = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("XRack sky radiance sums"),
            size: u64::from(texels) * 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            sums,
            source,
            horizon,
            samples,
        }
    }

    fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        sampler: &wgpu::Sampler,
        pipeline: &wgpu::ComputePipeline,
        range: Range<u32>,
        target: Option<&wgpu::Texture>,
    ) {
        let size = self.source.texture().width();
        let last_mip = size.ilog2();
        let layout = pipeline.get_bind_group_layout(0);
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some(if target.is_some() {
                    "XRack resolves filtered sky"
                } else {
                    "XRack accumulates sky samples"
                }),
                timestamp_writes: None,
            },
        );
        pass.set_pipeline(pipeline);
        let mut settings = [[0.0; 4]; 7];
        settings[6][2] = self.horizon;
        let mut offset = 0;
        for mip in u32::from(target.is_none())..=last_mip {
            let side = size >> mip;
            let uniform = sky_uniform(
                device,
                [side, mip, last_mip, self.samples],
                settings,
                [range.start, range.end, offset, 0],
            );
            let view = target.map(|texture| layer_view(texture, mip));
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.sums.as_entire_binding(),
                },
            ];
            if let Some(view) = &view {
                entries.push(wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                });
            }
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack sliced sky level"),
                layout: &layout,
                entries: &entries,
            });
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(side.div_ceil(8), side.div_ceil(8), 6);
            if mip != 0 {
                offset += 6 * side * side;
            }
        }
    }
}

pub(super) struct SkyGenerator {
    shader: wgpu::ShaderModule,
    sky: Option<wgpu::ComputePipeline>,
    masked: Option<wgpu::ComputePipeline>,
    accumulate: Option<wgpu::ComputePipeline>,
    resolve: Option<wgpu::ComputePipeline>,
    sh: Option<wgpu::ComputePipeline>,
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
            accumulate: None,
            resolve: None,
            sh: None,
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
            (sky.resolution(), settings, 0),
            None,
        )
    }

    pub fn diffuse(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        horizon: f32,
    ) -> wgpu::Buffer {
        let pipeline = self
            .sh
            .get_or_insert_with(|| pipeline(device, &self.shader, "irradiance"));
        let coefficients = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("XRack sky irradiance coefficients"),
            size: 9 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut settings = [[0.0; 4]; 7];
        settings[6][2] = horizon;
        let params = sky_uniform(device, [0; 4], settings, [0; 4]);
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack sky irradiance bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: coefficients.as_entire_binding(),
                },
            ],
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack projects sky irradiance"),
                timestamp_writes: None,
            },
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(9, 1, 1);
        coefficients
    }

    pub fn accumulate(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        sampler: &wgpu::Sampler,
        work: &SkyPrefilter,
        range: Range<u32>,
    ) {
        let pipeline = self
            .accumulate
            .get_or_insert_with(|| pipeline(device, &self.shader, "accumulate_panorama"));
        work.encode(device, encoder, sampler, pipeline, range, None);
    }

    pub fn publish(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        sampler: &wgpu::Sampler,
        work: &SkyPrefilter,
    ) -> (wgpu::TextureView, wgpu::Buffer) {
        let pipeline = self
            .resolve
            .get_or_insert_with(|| pipeline(device, &self.shader, "resolve_panorama"));
        let size = work.source.texture().width();
        let texture = sky_texture(device, size, size.ilog2() + 1);
        work.encode(device, encoder, sampler, pipeline, 0..0, Some(&texture));
        let diffuse = self.diffuse(device, encoder, &work.source, sampler, work.horizon);
        (as_cube(&texture), diffuse)
    }

    pub fn attenuate(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        (horizon, samples): (f32, u32),
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
            (source.texture().width(), settings, samples),
            Some((source, sampler)),
        )
    }
}

pub(super) fn program_cube(
    shaders: &mut SkyShaders,
    (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
    input: &SkyProgram,
) -> wgpu::TextureView {
    let bindings = shaders.bindings(device, queue, input, SkyTarget::Cube);
    let size = input.settings.resolution;
    let texture = sky_texture(device, size, 1);
    let target = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let [x, y, z] = input.settings.parameters;
    let mut bytes = Vec::with_capacity(32);
    bytes.extend([size, 0, 0, 0].into_iter().flat_map(u32::to_ne_bytes));
    append_floats(&mut bytes, [x, y, z, 0.0]);
    let params = buffer(
        device,
        "XRack sky program parameters",
        &bytes,
        wgpu::BufferUsages::UNIFORM,
    );
    let output = bindings.output(device, &params, &target);
    {
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack evaluates sky shader"),
                timestamp_writes: None,
            },
        );
        bindings.dispatch(&mut pass, &output, [size.div_ceil(8), size.div_ceil(8), 6]);
    }

    as_cube(&texture)
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
    (size, settings, samples): (u32, [[f32; 4]; 7], u32),
    source: Option<(&wgpu::TextureView, &wgpu::Sampler)>,
) -> wgpu::TextureView {
    let last_mip = size.ilog2();
    let texture = sky_texture(device, size, last_mip + 1);
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
        let target = layer_view(&texture, level);
        let uniform = sky_uniform(device, [side, level, last_mip, samples], settings, [0; 4]);
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
    as_cube(&texture)
}

fn sky_uniform(
    device: &wgpu::Device,
    level: [u32; 4],
    settings: [[f32; 4]; 7],
    slice: [u32; 4],
) -> wgpu::Buffer {
    let mut bytes = Vec::with_capacity(144);
    bytes.extend(level.into_iter().flat_map(u32::to_ne_bytes));
    append_floats(&mut bytes, settings.into_iter().flatten());
    bytes.extend(slice.into_iter().flat_map(u32::to_ne_bytes));
    buffer(
        device,
        "XRack sky parameters",
        &bytes,
        wgpu::BufferUsages::UNIFORM,
    )
}

fn layer_view(texture: &wgpu::Texture, level: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        base_mip_level: level,
        mip_level_count: Some(1),
        ..Default::default()
    })
}

fn sky_texture(device: &wgpu::Device, size: u32, levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack filtered environment cubemap"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn as_cube(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}
