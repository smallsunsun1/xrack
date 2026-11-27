use std::collections::{HashMap, VecDeque};

use xrack_runtime::{Color, Node, scene3d::SceneCamera};

use super::{
    DEPTH_FORMAT, append_floats, buffer, linear_rgb, pipeline::ColorOutput, screen_lighting,
};

const UNIFORM_CACHE_SIZE: usize = 4;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct GridSettings {
    height: f32,
    frequency: f32,
    color: Color,
    axes: bool,
}

impl GridSettings {
    pub(super) fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("scene-grid", false) {
            return None;
        }
        let spacing = node.number("scene-grid-spacing").unwrap_or(1.0);
        let height = node.number("scene-grid-height").unwrap_or(0.0) as f32;
        let color = node.color("scene-grid-color").unwrap_or(Color(0x91a6c0a0));
        (spacing > 0.0 && spacing.is_finite() && height.is_finite() && color.0 as u8 != 0).then(
            || Self {
                height,
                frequency: spacing.recip().clamp(1.0e-6, 1.0e4) as f32,
                color,
                axes: node.boolean("scene-grid-axes", true),
            },
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    format: wgpu::TextureFormat,
    samples: u32,
    reverse_depth: bool,
    screen_data: bool,
    indirect: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct UniformKey {
    clip: [[f32; 4]; 4],
    depth: [f32; 4],
    far: f32,
    style: GridSettings,
    encode_srgb: bool,
}

pub(super) struct GridPass {
    shader: wgpu::ShaderModule,
    inputs: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    uniforms: VecDeque<(UniformKey, wgpu::BindGroup)>,
}

impl GridPass {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack ground grid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let inputs = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack ground grid inputs"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("XRack ground grid pipeline layout"),
            bind_group_layouts: &[Some(&inputs)],
            immediate_size: 0,
        });
        Self {
            shader,
            inputs,
            layout,
            pipelines: HashMap::new(),
            uniforms: VecDeque::new(),
        }
    }

    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        camera: SceneCamera,
        output: ColorOutput,
        style: GridSettings,
    ) -> bool {
        let key = UniformKey {
            clip: camera.clip_rows(),
            depth: camera.depth_row(),
            far: camera.depth_limits()[1],
            style,
            encode_srgb: output.encode_srgb,
        };
        let Some(inputs) = self.bindings(device, camera, key) else {
            return false;
        };
        let key = PipelineKey {
            format: output.format(),
            samples: output.samples(),
            reverse_depth: output.reverse_depth,
            screen_data: output.screen_reflections || output.screen_indirect_light,
            indirect: output.screen_indirect_light,
        };
        let pipeline = self
            .pipelines
            .entry(key)
            .or_insert_with(|| grid_pipeline(device, &self.shader, &self.layout, key));
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &inputs, &[]);
        pass.draw(0..3, 0..1);
        true
    }

    fn bindings(
        &mut self,
        device: &wgpu::Device,
        camera: SceneCamera,
        key: UniformKey,
    ) -> Option<wgpu::BindGroup> {
        if let Some(index) = self.uniforms.iter().position(|(old, _)| *old == key)
            && let Some(entry) = self.uniforms.remove(index)
        {
            let inputs = entry.1.clone();
            self.uniforms.push_front(entry);
            return Some(inputs);
        }
        let inverse = camera.inverse_clip_rows()?;
        let style = key.style;
        let mut bytes = Vec::with_capacity(144);
        append_floats(&mut bytes, inverse.into_iter().flatten());
        append_floats(&mut bytes, key.depth);
        append_floats(
            &mut bytes,
            [
                style.height,
                style.frequency,
                if key.far.is_finite() { key.far } else { 0.0 },
                f32::from(style.axes),
            ],
        );
        for color in [style.color, Color(0xf05d60ff), Color(0x5d9cf0ff)] {
            let [r, g, b, a] = color.0.to_be_bytes().map(|value| f32::from(value) / 255.0);
            let [r, g, b] = if key.encode_srgb {
                [r, g, b]
            } else {
                linear_rgb(color).map(|value| value as f32)
            };
            append_floats(&mut bytes, [r, g, b, a]);
        }
        let uniform = buffer(
            device,
            "XRack ground grid settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let inputs = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack ground grid settings"),
            layout: &self.inputs,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        self.uniforms.truncate(UNIFORM_CACHE_SIZE - 1);
        self.uniforms.push_front((key, inputs.clone()));
        Some(inputs)
    }
}

fn grid_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    key: PipelineKey,
) -> wgpu::RenderPipeline {
    let color = Some(wgpu::ColorTargetState {
        format: key.format,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    });
    let single = [color.clone()];
    let mut surface = screen_lighting::surface_targets(key.samples > 1, key.indirect);
    surface[0] = color;
    for target in surface[1..].iter_mut().flatten() {
        target.write_mask = wgpu::ColorWrites::empty();
    }
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack ground grid"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("grid_vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("grid_fragment"),
            compilation_options: Default::default(),
            targets: if key.screen_data { &surface } else { &single },
        }),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(if key.reverse_depth {
                wgpu::CompareFunction::GreaterEqual
            } else {
                wgpu::CompareFunction::LessEqual
            }),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: key.samples,
            ..Default::default()
        },
        primitive: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
