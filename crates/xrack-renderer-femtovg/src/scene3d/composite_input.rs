use std::collections::{HashMap, VecDeque};

use xrack_runtime::{
    Node,
    scene_effect::{CAPTURE_DEPTH_READ, CaptureInputs},
    scene3d::SceneCamera,
};

use super::{
    buffer,
    color_capture::CaptureViews,
    pipeline::{ColorOutput, GeometryOutput, orient_depth_compare},
};

pub(super) fn capture_reads(node: &Node) -> CaptureInputs {
    CaptureInputs::new(
        node.text("scene-color-capture").unwrap_or_default(),
        node.text("scene-depth-capture").unwrap_or_default(),
        "",
    )
}

#[derive(Clone, Copy)]
pub(super) struct CompositeInput<'a> {
    color: Option<&'a wgpu::TextureView>,
    depth: Option<&'a wgpu::TextureView>,
}

impl<'a> CompositeInput<'a> {
    pub fn from_node(node: &Node, camera: SceneCamera, captures: &'a CaptureViews) -> Option<Self> {
        Self::from_reads(&capture_reads(node), camera, captures)
    }

    pub fn from_reads(
        reads: &CaptureInputs,
        camera: SceneCamera,
        captures: &'a CaptureViews,
    ) -> Option<Self> {
        let color = reads
            .color
            .as_ref()
            .and_then(|read| captures.get(read))
            .map(|capture| &capture.view);
        let depth = reads
            .depth
            .as_ref()
            .and_then(|read| captures.get(read))
            .filter(|capture| {
                capture
                    .projection
                    .as_ref()
                    .is_some_and(|projection| Some(projection.rows) == camera.inverse_clip_rows())
            })
            .map(|capture| &capture.view);
        (color.is_some() || depth.is_some()).then_some(Self { color, depth })
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct PipelineKey {
    color: bool,
    depth: bool,
    stenciled: bool,
    format: wgpu::TextureFormat,
    msaa: bool,
    surfaces: bool,
    indirect: bool,
    geometry: Option<GeometryOutput>,
    reverse_depth: bool,
}

#[derive(Default)]
pub(super) struct CompositeInputs {
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    bindings: VecDeque<Binding>,
    layout: Option<wgpu::BindGroupLayout>,
    shader: Option<wgpu::ShaderModule>,
}

struct Binding {
    views: [wgpu::TextureView; 2],
    offset: [f32; 2],
    group: wgpu::BindGroup,
}

impl CompositeInputs {
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        source: CompositeInput<'_>,
        output: ColorOutput,
        clip_offset: [f32; 2],
    ) {
        let key = PipelineKey {
            color: source.color.is_some(),
            depth: source.depth.is_some(),
            stenciled: output.stenciled,
            format: output.format(),
            msaa: output.multisampling,
            surfaces: output.screen_reflections || output.screen_indirect_light,
            indirect: output.screen_indirect_light,
            geometry: output
                .capture_geometry
                .filter(|geometry| geometry.combined()),
            reverse_depth: output.reverse_depth,
        };
        let Some(fallback) = source.color.or(source.depth) else {
            return;
        };
        let views = [
            source.color.unwrap_or(fallback),
            source.depth.unwrap_or(fallback),
        ];
        let offset = [-clip_offset[0] * 0.5, clip_offset[1] * 0.5];
        let layout = self.layout.get_or_insert_with(|| {
            let texture = |binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("XRack scene capture inputs"),
                entries: &[
                    texture(0),
                    texture(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            })
        });
        let pipeline = self.pipelines.entry(key).or_insert_with(|| {
            let shader = self.shader.get_or_insert_with(|| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("XRack capture composite"),
                    source: wgpu::ShaderSource::Wgsl(
                        format!(
                            "{CAPTURE_DEPTH_READ}\n{}",
                            include_str!("composite_input.wgsl")
                        )
                        .into(),
                    ),
                })
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack scene capture composite"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            let mut targets = super::screen_lighting::surface_targets(key.msaa, key.indirect);
            let color = wgpu::ColorTargetState {
                format: key.format,
                blend: key
                    .color
                    .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: if key.color {
                    wgpu::ColorWrites::ALL
                } else {
                    wgpu::ColorWrites::empty()
                },
            };
            let combined_targets = key.geometry.and_then(|geometry| {
                geometry.combined_targets(color.clone(), wgpu::ColorWrites::empty())
            });
            targets[0] = Some(color);
            for target in targets[1..].iter_mut().flatten() {
                target.write_mask = wgpu::ColorWrites::empty();
            }
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack scene capture composite"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(if key.geometry.is_some() {
                        "composite_named_color"
                    } else {
                        "composite"
                    }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("HAS_COLOR", f64::from(key.color)),
                            ("HAS_DEPTH", f64::from(key.depth)),
                            ("REVERSE_DEVICE_DEPTH", f64::from(key.reverse_depth)),
                        ],
                        ..Default::default()
                    },
                    targets: if let Some((targets, len)) = &combined_targets {
                        &targets[..*len]
                    } else if key.surfaces {
                        &targets
                    } else {
                        &targets[..1]
                    },
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: output.depth_format(),
                    depth_write_enabled: Some(key.depth),
                    depth_compare: Some(if key.depth {
                        orient_depth_compare(wgpu::CompareFunction::LessEqual, key.reverse_depth)
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: output.samples(),
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        });
        let index = self.bindings.iter().position(|entry| {
            entry.offset == offset && [&entry.views[0], &entry.views[1]] == views
        });
        let binding = index
            .and_then(|index| self.bindings.remove(index))
            .unwrap_or_else(|| {
                let bytes = offset.map(f32::to_ne_bytes).concat();
                let uniform = buffer(
                    device,
                    "XRack scene capture reprojection offset",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                );
                Binding {
                    views: views.map(Clone::clone),
                    offset,
                    group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("XRack scene capture inputs"),
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(views[0]),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(views[1]),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: uniform.as_entire_binding(),
                            },
                        ],
                    }),
                }
            });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &binding.group, &[]);
        pass.draw(0..3, 0..1);
        self.bindings.push_front(binding);
        self.bindings.truncate(8);
    }
}
