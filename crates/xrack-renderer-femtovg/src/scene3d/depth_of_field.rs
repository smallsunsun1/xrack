//! Depth-guided half-resolution blur and full-resolution composition.
use std::collections::VecDeque;

use xrack_runtime::{Node, scene3d::SceneCamera};

use crate::GpuPaintContext;

use super::{
    COLOR_FORMAT, HDR_FORMAT, append_floats, buffer,
    postprocess::{DisplayLeases, RETAINED_VIEW_SIZES, color_target},
    screen_depth,
};

#[derive(Clone, Copy)]
pub(super) struct FocusSettings {
    distance: f32,
    half_range: f32,
    radius: f32,
}

impl FocusSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("depth-of-field", false) {
            return None;
        }
        let radius = node
            .length("dof-blur-radius")
            .unwrap_or(12.0)
            .clamp(0.0, 32.0);
        (radius > 0.0).then_some(Self {
            radius,
            distance: node.float("dof-focus-distance").unwrap_or(10.0).max(0.01),
            half_range: node.float("dof-focus-range").unwrap_or(2.0).max(0.0) * 0.5,
        })
    }
}

pub(super) struct FocusDepth {
    pub view: wgpu::TextureView,
    pub camera: SceneCamera,
    pub multisampled: bool,
}

pub(super) struct FocusOutput {
    pub color: Option<wgpu::TextureView>,
    pub filtered_pixels: u64,
}

struct Targets {
    size: [u32; 2],
    layers: [wgpu::TextureView; 4],
    coverage: Option<wgpu::TextureView>,
    resolve: [Option<wgpu::TextureView>; 2],
}

pub(super) struct DepthOfField {
    shader: wgpu::ShaderModule,
    reduce: [[Option<Pass>; 4]; screen_depth::MODES],
    gather: Pass,
    sampler: wgpu::Sampler,
    composite: [[[Option<Pass>; 2]; 2]; screen_depth::MODES],
    targets: VecDeque<Targets>,
}

impl DepthOfField {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D depth of field"),
            source: wgpu::ShaderSource::Wgsl(
                [screen_depth::SHADER, include_str!("depth_of_field.wgsl")]
                    .join("\n")
                    .into(),
            ),
        });
        Self {
            gather: Pass::new(
                device,
                &shader,
                "gather",
                &[HDR_FORMAT, HDR_FORMAT, wgpu::TextureFormat::R32Float],
                screen_depth::Mode::Flat,
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D depth of field sampler"),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            shader,
            reduce: [const { [const { None }; 4] }; screen_depth::MODES],
            composite: [const { [const { [None, None] }; 2] }; screen_depth::MODES],
            targets: VecDeque::new(),
        }
    }

    pub fn render(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        color: &wgpu::TextureView,
        depth: &FocusDepth,
        settings: FocusSettings,
        (resolve, composite_alpha): (Option<wgpu::TextureFormat>, bool),
        leases: &DisplayLeases,
    ) -> FocusOutput {
        let device = ctx.device;
        let size = ctx.size;
        let half = size.map(|side| side.div_ceil(2));
        let index = self.targets.iter().position(|target| target.size == size);
        let target = index
            .and_then(|index| self.targets.remove(index))
            .unwrap_or_else(|| Targets {
                size,
                layers: [
                    HDR_FORMAT,
                    HDR_FORMAT,
                    HDR_FORMAT,
                    wgpu::TextureFormat::R32Float,
                ]
                .map(|format| image(device, half, format)),
                coverage: None,
                resolve: [None, None],
            });
        self.targets.truncate(RETAINED_VIEW_SIZES - 1);
        self.targets.push_front(target);
        let Some(target) = self.targets.front_mut() else {
            return FocusOutput {
                color: None,
                filtered_pixels: 0,
            };
        };
        let format = resolve.unwrap_or(COLOR_FORMAT);
        let format_index = usize::from(format == HDR_FORMAT);
        let resolved = resolve.map(|format| {
            let slot = &mut target.resolve[format_index];
            if slot.as_ref().is_some_and(|view| leases.contains(view)) {
                *slot = None;
            }
            slot.get_or_insert_with(|| image(device, size, format))
                .clone()
        });
        let [reduced, blurred, foreground, foreground_depth] = &target.layers;
        let coverage = composite_alpha.then(|| {
            target
                .coverage
                .get_or_insert_with(|| image(device, half, wgpu::TextureFormat::R16Float))
        });
        let output = resolved.as_ref().unwrap_or(ctx.target);

        let logical = [ctx.paint.bounds.width, ctx.paint.bounds.height];
        let [radius_x, radius_y]: [f32; 2] = std::array::from_fn(|axis| {
            let scale = size[axis] as f32 / logical[axis].max(1.0);
            (settings.radius * scale).clamp(0.0, 64.0)
        });
        let mut bytes = Vec::with_capacity(screen_depth::UNIFORM_BYTES + 32);
        screen_depth::write(&mut bytes, depth.camera);
        append_floats(
            &mut bytes,
            [settings.distance, settings.half_range, radius_x, radius_y],
        );
        append_floats(
            &mut bytes,
            [f32::from(u8::from(composite_alpha)), 0.0, 0.0, 0.0],
        );
        let uniform = buffer(
            device,
            "XRack 3D focus settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let sample = usize::from(depth.multisampled);
        let projection = screen_depth::mode(depth.camera);
        let depth_mode = projection as usize;
        let depth_input = (2 + sample as u32, &depth.view);

        let reduced_targets = 1 + usize::from(composite_alpha);
        let reduce_slot = sample + 2 * usize::from(composite_alpha);
        let reduce = self.reduce[depth_mode][reduce_slot].get_or_insert_with(|| {
            Pass::new(
                device,
                &self.shader,
                ["reduce", "reduce_msaa", "reduce_alpha", "reduce_alpha_msaa"][reduce_slot],
                &[HDR_FORMAT, wgpu::TextureFormat::R16Float][..reduced_targets],
                projection,
            )
        });
        let coverage = coverage.as_deref().unwrap_or(reduced);
        reduce.draw(
            device,
            ctx.encoder,
            &[reduced, coverage][..reduced_targets],
            &uniform,
            &[(0, color), depth_input],
            None,
        );
        self.gather.draw(
            device,
            ctx.encoder,
            &[blurred, foreground, foreground_depth],
            &uniform,
            &[(4, reduced), (9, coverage)],
            Some(&self.sampler),
        );
        let composite = self.composite[depth_mode][sample][format_index].get_or_insert_with(|| {
            Pass::new(
                device,
                &self.shader,
                if depth.multisampled {
                    "composite_msaa"
                } else {
                    "composite"
                },
                &[format],
                projection,
            )
        });
        composite.draw(
            device,
            ctx.encoder,
            &[output],
            &uniform,
            &[
                (0, color),
                depth_input,
                (4, reduced),
                (5, blurred),
                (6, foreground),
                (7, foreground_depth),
            ],
            None,
        );
        FocusOutput {
            color: resolved,
            filtered_pixels: 2 * half.into_iter().map(u64::from).product::<u64>(),
        }
    }
}

fn image(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::TextureView {
    color_target(device, size, format, "XRack 3D depth of field target")
}

struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Pass {
    fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        entry: &str,
        formats: &[wgpu::TextureFormat],
        projection: screen_depth::Mode,
    ) -> Self {
        let targets: Vec<_> = formats
            .iter()
            .map(|&format| {
                Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect();
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack 3D depth of field pass"),
            layout: None,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry),
                compilation_options: screen_depth::compilation(projection),
                targets: &targets,
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
        }
    }

    fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        targets: &[&wgpu::TextureView],
        uniform: &wgpu::Buffer,
        inputs: &[(u32, &wgpu::TextureView)],
        sampler: Option<&wgpu::Sampler>,
    ) {
        let mut entries = Vec::with_capacity(inputs.len() + 1);
        entries.push(wgpu::BindGroupEntry {
            binding: 1,
            resource: uniform.as_entire_binding(),
        });
        entries.extend(inputs.iter().map(|&(binding, view)| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        }));
        if let Some(sampler) = sampler {
            entries.push(wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D depth of field sources"),
            layout: &self.layout,
            entries: &entries,
        });
        let colors: Vec<_> = targets
            .iter()
            .map(|&view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect();
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack 3D depth of field"),
                color_attachments: &colors,
                ..Default::default()
            },
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
