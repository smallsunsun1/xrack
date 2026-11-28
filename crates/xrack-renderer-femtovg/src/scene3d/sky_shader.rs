//! The cube and screen sky passes share uploaded images, samplers and shader layouts.
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use xrack_runtime::{
    material_shader::MAX_TEXTURES,
    scene_effect::{BufferSampling, ExternalTexture},
    scene3d::{
        SKY_PROGRAM_ENTRY, SKY_RAY_WGSL, SKY_SCREEN_ENTRY, SkyProgram, SkyShader, SkyTextureKind,
        TextureFilter, TextureWrap, sky_texture_interface,
    },
};

use super::{HDR_FORMAT, buffer, material::SharedImages};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SkyTarget {
    Cube,
    Screen,
}

type TextureKinds = [Option<SkyTextureKind>; MAX_TEXTURES];

struct ProgramPipeline {
    shader: Arc<SkyShader>,
    target: SkyTarget,
    kinds: TextureKinds,
    pipeline: wgpu::ComputePipeline,
}

type UniformWords = Arc<[[u32; 4]]>;

pub(super) struct SkyBindings {
    pipeline: wgpu::ComputePipeline,
    textures: Option<wgpu::BindGroup>,
    uniforms: Option<wgpu::Buffer>,
}

impl SkyBindings {
    pub fn output(
        &self,
        device: &wgpu::Device,
        params: &wgpu::Buffer,
        target: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(target),
            },
        ];
        if let Some(uniforms) = &self.uniforms {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: uniforms.as_entire_binding(),
            });
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack sky output bindings"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        })
    }

    pub fn dispatch(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        output: &wgpu::BindGroup,
        [x, y, z]: [u32; 3],
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, output, &[]);
        if let Some(textures) = &self.textures {
            pass.set_bind_group(1, textures, &[]);
        }
        pass.dispatch_workgroups(x, y, z);
    }
}

pub(super) struct SkyShaders {
    images: SharedImages,
    samplers: HashMap<BufferSampling, wgpu::Sampler>,
    pipelines: VecDeque<ProgramPipeline>,
    uniforms: VecDeque<(UniformWords, wgpu::Buffer)>,
    native_anisotropy: bool,
    pub pipeline_builds: u64,
    pub uniform_uploads: u64,
}

impl SkyShaders {
    pub fn new(images: SharedImages, native_anisotropy: bool) -> Self {
        Self {
            images,
            samplers: HashMap::new(),
            pipelines: VecDeque::new(),
            uniforms: VecDeque::new(),
            native_anisotropy,
            pipeline_builds: 0,
            uniform_uploads: 0,
        }
    }

    pub fn bindings(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        program: &SkyProgram,
        target: SkyTarget,
    ) -> SkyBindings {
        let mut kinds = [None; MAX_TEXTURES];
        let mut textures = Vec::new();
        if !program.textures.is_empty() {
            let mut images = self.images.borrow_mut();
            for (index, input) in program.textures.iter().take(MAX_TEXTURES).enumerate() {
                let (kind, view) = match input.external_texture() {
                    Some(ExternalTexture::Image(image)) => (
                        SkyTextureKind::Image,
                        Some(images.sampled_input_view(
                            device,
                            queue,
                            &image.image,
                            image.color,
                            input.sampling,
                        )),
                    ),
                    Some(ExternalTexture::Cubemap(cube)) => (
                        SkyTextureKind::Cubemap,
                        images.cubemap_view(device, queue, cube),
                    ),
                    Some(ExternalTexture::Volume(volume)) => (
                        SkyTextureKind::Volume,
                        images.volume_view(device, queue, volume),
                    ),
                    _ => continue,
                };
                if let Some(view) = view {
                    kinds[index] = Some(kind);
                    let sampling = input.sampling;
                    let sampler = self.samplers.entry(sampling).or_insert_with(|| {
                        create_sampler(device, sampling, self.native_anisotropy)
                    });
                    textures.push((index as u32, view, sampler.clone()));
                }
            }
        }
        let index = self.pipelines.iter().position(|entry| {
            entry.shader == program.shader && entry.target == target && entry.kinds == kinds
        });
        let entry = index
            .and_then(|index| self.pipelines.remove(index))
            .unwrap_or_else(|| {
                self.pipeline_builds += 1;
                ProgramPipeline {
                    shader: Arc::clone(&program.shader),
                    target,
                    kinds,
                    pipeline: create_pipeline(device, &program.shader, target, kinds),
                }
            });
        let bindings = (!textures.is_empty()).then(|| {
            let entries: Vec<_> = textures
                .iter()
                .flat_map(|(index, view, sampler)| {
                    [
                        wgpu::BindGroupEntry {
                            binding: 2 * index,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2 * index + 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ]
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack sky shader textures"),
                layout: &entry.pipeline.get_bind_group_layout(1),
                entries: &entries,
            })
        });
        let pipeline = entry.pipeline.clone();
        self.pipelines.truncate(15);
        self.pipelines.push_front(entry);
        let uniforms =
            (!program.uniforms.is_empty()).then(|| self.uniform_buffer(device, &program.uniforms));
        SkyBindings {
            pipeline,
            textures: bindings,
            uniforms,
        }
    }

    fn uniform_buffer(&mut self, device: &wgpu::Device, words: &UniformWords) -> wgpu::Buffer {
        let index = self.uniforms.iter().position(|(cached, _)| cached == words);
        let entry = index
            .and_then(|index| self.uniforms.remove(index))
            .unwrap_or_else(|| {
                self.uniform_uploads += 1;
                let bytes: Vec<_> = words
                    .iter()
                    .flatten()
                    .copied()
                    .flat_map(u32::to_ne_bytes)
                    .collect();
                (
                    Arc::clone(words),
                    buffer(
                        device,
                        "XRack sky uniform data",
                        &bytes,
                        wgpu::BufferUsages::UNIFORM,
                    ),
                )
            });
        let result = entry.1.clone();
        self.uniforms.truncate(7);
        self.uniforms.push_front(entry);
        result
    }
}

fn create_sampler(
    device: &wgpu::Device,
    sampling: BufferSampling,
    native_anisotropy: bool,
) -> wgpu::Sampler {
    let filter = |filter| match filter {
        TextureFilter::Nearest => wgpu::FilterMode::Nearest,
        TextureFilter::Linear => wgpu::FilterMode::Linear,
    };
    let wrap = |wrap| match wrap {
        TextureWrap::Clamp => wgpu::AddressMode::ClampToEdge,
        TextureWrap::Repeat => wgpu::AddressMode::Repeat,
        TextureWrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    };
    let anisotropy = native_anisotropy
        && sampling.min_filter == TextureFilter::Linear
        && sampling.mag_filter == TextureFilter::Linear
        && sampling.mip_filter == Some(TextureFilter::Linear);
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("XRack sky shader sampler"),
        address_mode_u: wrap(sampling.wrap[0]),
        address_mode_v: wrap(sampling.wrap[1]),
        min_filter: filter(sampling.min_filter),
        mag_filter: filter(sampling.mag_filter),
        mipmap_filter: match sampling.mip_filter {
            Some(TextureFilter::Linear) => wgpu::MipmapFilterMode::Linear,
            _ => wgpu::MipmapFilterMode::Nearest,
        },
        lod_max_clamp: if sampling.mip_filter.is_some() {
            32.0
        } else {
            0.0
        },
        anisotropy_clamp: if anisotropy {
            u16::from(sampling.max_anisotropy.clamp(1, 16))
        } else {
            1
        },
        ..Default::default()
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    asset: &SkyShader,
    target: SkyTarget,
    kinds: TextureKinds,
) -> wgpu::ComputePipeline {
    let (direction, interface, main, dimension) = match target {
        SkyTarget::Cube => (
            include_str!("cube_direction.wgsl"),
            SKY_PROGRAM_ENTRY,
            "xr_generate_sky",
            wgpu::TextureViewDimension::D2Array,
        ),
        SkyTarget::Screen => (
            SKY_RAY_WGSL,
            SKY_SCREEN_ENTRY,
            "xr_generate_screen_sky",
            wgpu::TextureViewDimension::D2,
        ),
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack programmable sky"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{direction}\n{interface}\n{}\n{}",
                asset.source,
                sky_texture_interface(kinds),
                asset.uniform_interface,
            )
            .into(),
        ),
    });
    let mut output = Vec::from([
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: HDR_FORMAT,
                view_dimension: dimension,
            },
            count: None,
        },
    ]);
    if !asset.fields.is_empty() {
        output.push(wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    let output_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("XRack sky shader output"),
        entries: &output,
    });
    let entries: Vec<_> = kinds
        .into_iter()
        .enumerate()
        .filter_map(|(slot, kind)| {
            let dimension = match kind? {
                SkyTextureKind::Image => wgpu::TextureViewDimension::D2,
                SkyTextureKind::Cubemap => wgpu::TextureViewDimension::Cube,
                SkyTextureKind::Volume => wgpu::TextureViewDimension::D3,
            };
            Some([
                wgpu::BindGroupLayoutEntry {
                    binding: 2 * slot as u32,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: dimension,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2 * slot as u32 + 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ])
        })
        .flatten()
        .collect();
    let textures = (!entries.is_empty()).then(|| {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack sky shader inputs"),
            entries: &entries,
        })
    });
    let mut groups = vec![Some(&output_layout)];
    if let Some(layout) = &textures {
        groups.push(Some(layout));
    }
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("XRack sky shader pipeline layout"),
        bind_group_layouts: &groups,
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("XRack sky shader pipeline"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some(main),
        compilation_options: Default::default(),
        cache: None,
    })
}
