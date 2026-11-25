//! Resolved opaque radiance shared by the thin transmission draws of a viewport.
use std::collections::VecDeque;

use super::{pipeline::ColorOutput, supersampling::Supersampling};

const RETAINED_CAPTURES: usize = 2;

#[derive(Default)]
pub(super) struct Transmission {
    captures: VecDeque<Capture>,
    filter: Option<Supersampling>,
}

pub(super) struct Capture {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    pub sampled: wgpu::TextureView,
    pub levels: Vec<wgpu::TextureView>,
}

impl Transmission {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        output: ColorOutput,
        blur: f32,
    ) -> (&Capture, &mut Supersampling, usize) {
        let [w, h] = size;
        let max_mip = w.max(h).ilog2() as f32;
        let levels = 1 + (max_mip * blur).ceil() as u32;
        let reusable = self.captures.iter().position(|capture| {
            capture.size == size
                && capture.format == output.format()
                && capture.levels.len() >= levels as usize
        });
        let capture = reusable
            .and_then(|index| self.captures.remove(index))
            .unwrap_or_else(|| Capture::new(device, size, output, levels));
        self.captures.truncate(RETAINED_CAPTURES - 1);
        self.captures.push_front(capture);
        let filter = self
            .filter
            .get_or_insert_with(|| Supersampling::new(device));
        (&self.captures[0], filter, levels as usize)
    }
}

impl Capture {
    fn new(
        device: &wgpu::Device,
        size: [u32; 2],
        output: ColorOutput,
        mip_level_count: u32,
    ) -> Self {
        let [width, height] = size;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack 3D transmission capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: output.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            size,
            format: output.format(),
            sampled: texture.create_view(&Default::default()),
            levels: (0..mip_level_count)
                .map(|base_mip_level| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        base_mip_level,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect(),
        }
    }

    pub fn filter(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        area: &mut Supersampling,
        levels: usize,
        seed: Option<&wgpu::TextureView>,
    ) {
        let mut size = self.size;
        for pair in self.levels[..levels].windows(2) {
            size = size.map(|side| (side / 2).max(1));
            area.resolve(device, encoder, (&pair[0], &pair[1]), size, self.format);
        }
        if let Some(target) = seed {
            area.resolve(
                device,
                encoder,
                (&self.levels[0], target),
                self.size,
                self.format,
            );
        }
    }
}

/// Nearest opaque depth shared by projected refraction and multisampled
/// screen-space ray traversal.
#[derive(Default)]
pub(super) struct OpaqueDepth {
    buffer: Option<(u64, wgpu::Buffer)>,
    shader: Option<wgpu::ShaderModule>,
    pipelines: [Option<wgpu::ComputePipeline>; 3],
}

impl OpaqueDepth {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        [width, height]: [u32; 2],
    ) -> Option<wgpu::Buffer> {
        let bytes = u64::from(width) * u64::from(height) * 4;
        let limits = device.limits();
        if bytes > limits.max_storage_buffer_binding_size || bytes > limits.max_buffer_size {
            return None;
        }
        if self.buffer.as_ref().is_none_or(|(size, _)| *size != bytes) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack 3D opaque depth snapshot"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            self.buffer = Some((bytes, buffer));
        }
        self.buffer.as_ref().map(|(_, buffer)| buffer.clone())
    }

    pub fn capture(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (depth, output): (&wgpu::TextureView, &wgpu::Buffer),
        [width, height]: [u32; 2],
        (multisampled, reverse_depth): (bool, bool),
    ) {
        let shader = self.shader.get_or_insert_with(|| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack 3D opaque depth sampling"),
                source: wgpu::ShaderSource::Wgsl(include_str!("transmission_depth.wgsl").into()),
            })
        });
        let slot = usize::from(multisampled) + usize::from(multisampled && reverse_depth);
        let pipeline = self.pipelines[slot].get_or_insert_with(|| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack 3D opaque depth capture"),
                layout: None,
                module: shader,
                entry_point: Some(if multisampled {
                    "multisampled"
                } else {
                    "single"
                }),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[(
                        "REVERSE_DEVICE_DEPTH",
                        f64::from(multisampled && reverse_depth),
                    )],
                    ..Default::default()
                },
                cache: None,
            })
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D opaque depth capture"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: u32::from(multisampled),
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("XRack 3D opaque depth capture"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }
}
