//! Shared opaque data, tracing and composition for screen-space lighting.
use xrack_runtime::{
    Node,
    scene3d::{Mesh3D, MeshDepthDraw, SceneCamera},
};

use super::{Attachment, COLOR_FORMAT, HDR_FORMAT, append_floats, buffer, screen_depth};

#[derive(Clone, Copy)]
pub(super) struct ReflectionSettings {
    strength: f32,
    distance: f32,
    thickness: f32,
    steps: u32,
    roughness: f32,
    scale: f32,
}

impl ReflectionSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("screen-space-reflections", false) {
            return None;
        }
        let read = |name, default, min, max| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
                .clamp(min, max)
        };
        let strength = read("ssr-strength", 1.0, 0.0, 1.0);
        (strength > 0.0).then(|| Self {
            strength,
            distance: read("ssr-distance", 24.0, 0.1, 1000.0),
            thickness: read("ssr-thickness", 0.2, 0.005, 20.0),
            steps: node.int("ssr-steps").unwrap_or(64).clamp(8, 256) as u32,
            roughness: read("ssr-roughness-cutoff", 0.65, 0.05, 1.0),
            scale: read("ssr-scale", 0.5, 0.25, 1.0),
        })
    }
    pub fn receives(self, mesh: &Mesh3D) -> bool {
        mesh.has_opaque_parts()
            && mesh.sections().any(|(_, material)| {
                material.is_some_and(|material| {
                    !material.is_blended()
                        && material.depth_draw_mode != MeshDepthDraw::Never
                        && material.pbr.as_ref().is_some_and(|pbr| {
                            material.transmission_blur().is_none()
                                && (pbr.roughness < self.roughness
                                    || pbr.surface_texture.is_some()
                                    || material.vertex_masks.masks_roughness()
                                    || pbr.clearcoat.as_ref().is_some_and(|coat| {
                                        coat.amount > 0.0
                                            && (coat.roughness < self.roughness
                                                || coat.roughness_texture.is_some()
                                                || material
                                                    .vertex_masks
                                                    .masks_clearcoat_roughness())
                                    }))
                        })
                })
            })
    }
}

#[derive(Clone, Copy)]
pub(super) struct IndirectSettings {
    radius: f32,
    thickness: f32,
    strength: f32,
    pub occlusion: f32,
    scale: f32,
    rays: u32,
    steps: u32,
}

impl IndirectSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("screen-space-global-illumination", false) {
            return None;
        }
        let read = |name, default, low, high| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
                .clamp(low, high)
        };
        let strength = read("ssgi-strength", 1.0, 0.0, 8.0);
        let occlusion = read("ssgi-occlusion", 0.5, 0.0, 1.0);
        (strength > 0.0 || occlusion > 0.0).then(|| Self {
            radius: read("ssgi-radius", 3.0, 0.1, 50.0),
            thickness: read("ssgi-thickness", 0.22, 0.005, 5.0),
            scale: read("ssgi-scale", 0.5, 0.25, 1.0),
            rays: node.int("ssgi-rays").unwrap_or(8).clamp(2, 32) as u32,
            steps: node.int("ssgi-steps").unwrap_or(24).clamp(4, 64) as u32,
            strength,
            occlusion,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct ScreenSettings {
    pub reflection: Option<ReflectionSettings>,
    pub indirect: Option<IndirectSettings>,
}

impl ScreenSettings {
    pub fn active(self) -> bool {
        self.reflection.is_some() || self.indirect.is_some()
    }
}

#[derive(Default)]
pub(super) struct ScreenLighting {
    radiance: Option<Attachment>,
    data: [Option<Attachment>; 3],
    layout: (bool, bool),
    reflection: [Option<Attachment>; 2],
    indirect: [Option<Attachment>; 2],
    filtered: Option<Attachment>,
    shaders: [Option<wgpu::ShaderModule>; 4],
    trace: [[Option<PassPipeline>; 4]; screen_depth::MODES],
    indirect_trace: [[Option<PassPipeline>; 2]; screen_depth::MODES],
    denoise: Option<PassPipeline>,
    composite: [[Option<PassPipeline>; 6]; screen_depth::MODES],
}

pub(super) struct OpaqueSurfaces {
    radiance: wgpu::TextureView,
    data: [wgpu::TextureView; 3],
}

impl OpaqueSurfaces {
    pub fn begin<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        clear: wgpu::Color,
        multisampled_color: Option<&wgpu::TextureView>,
        depth_load: wgpu::LoadOp<f32>,
    ) -> wgpu::RenderPass<'a> {
        let colors = [
            Some(wgpu::RenderPassColorAttachment {
                resolve_target: multisampled_color.map(|_| &self.radiance),
                ..color_attachment(multisampled_color.unwrap_or(&self.radiance), clear)
            }),
            Some(color_attachment(&self.data[0], wgpu::Color::TRANSPARENT)),
            Some(color_attachment(&self.data[1], wgpu::Color::TRANSPARENT)),
            Some(color_attachment(&self.data[2], wgpu::Color::TRANSPARENT)),
        ];
        crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack opaque radiance and screen lighting surfaces"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            },
        )
    }
}

pub(super) enum ScreenTarget<'a> {
    Single {
        color: &'a wgpu::TextureView,
        hdr: bool,
    },
    Multisampled {
        color: &'a wgpu::TextureView,
        resolve: Option<&'a wgpu::TextureView>,
        keep_samples: bool,
        depth_snapshot: &'a wgpu::Buffer,
    },
}

impl ScreenLighting {
    pub fn surfaces(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        multisampled: bool,
        indirect: bool,
    ) -> OpaqueSurfaces {
        let layout = (multisampled, indirect);
        if self.layout != layout {
            self.layout = layout;
            self.data = Default::default();
        }
        let formats = surface_formats(multisampled, indirect);
        let samples = if multisampled { 4 } else { 1 };
        OpaqueSurfaces {
            radiance: Attachment::get(&mut self.radiance, device, size, HDR_FORMAT, 1, true)
                .clone(),
            data: std::array::from_fn(|index| {
                Attachment::get(
                    &mut self.data[index],
                    device,
                    size,
                    formats[index + 1],
                    samples,
                    true,
                )
                .clone()
            }),
        }
    }

    pub fn resolve(
        &mut self,
        (device, encoder): (&wgpu::Device, &mut wgpu::CommandEncoder),
        surfaces: &OpaqueSurfaces,
        (target, depth): (ScreenTarget<'_>, &wgpu::TextureView),
        size: [u32; 2],
        (camera, settings): (SceneCamera, ScreenSettings),
    ) -> [u64; 2] {
        let extent = |scale: f32| size.map(|n| ((n as f32 * scale).ceil() as u32).max(1));
        let reflection_size = settings.reflection.map(|settings| extent(settings.scale));
        let indirect_size = settings.indirect.map(|settings| extent(settings.scale));
        let [rw, rh] = reflection_size.unwrap_or([0; 2]).map(|axis| axis as f32);
        let [iw, ih] = indirect_size.unwrap_or([0; 2]).map(|axis| axis as f32);
        let mut bytes = Vec::with_capacity(screen_depth::UNIFORM_BYTES + 80);
        screen_depth::write(&mut bytes, camera);
        append_floats(
            &mut bytes,
            [
                settings
                    .reflection
                    .map_or(0.0, |settings| settings.strength),
                0.0,
                0.0,
                0.0,
            ],
        );
        append_floats(
            &mut bytes,
            settings.reflection.map_or([0.0; 4], |settings| {
                [
                    settings.distance,
                    settings.thickness,
                    settings.roughness,
                    settings.steps as f32,
                ]
            }),
        );
        append_floats(&mut bytes, [size[0] as f32, size[1] as f32, rw, rh]);
        append_floats(
            &mut bytes,
            settings.indirect.map_or([0.0; 4], |settings| {
                [
                    settings.radius,
                    settings.thickness,
                    settings.strength,
                    settings.occlusion,
                ]
            }),
        );
        append_floats(
            &mut bytes,
            settings.indirect.map_or([0.0; 4], |settings| {
                [iw, ih, settings.rays as f32, settings.steps as f32]
            }),
        );
        let params = buffer(
            device,
            "XRack screen lighting parameters",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let projection = screen_depth::mode(camera);
        let depth_mode = projection as usize;
        let multisampled = matches!(target, ScreenTarget::Multisampled { .. });
        let packed = indirect_size.is_some();
        let shader_index = 2 * usize::from(packed) + usize::from(multisampled);
        let shader = self.shaders[shader_index]
            .get_or_insert_with(|| make_shader(device, multisampled, packed));
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let uniform = wgpu::BindGroupEntry {
            binding: 0,
            resource: params.as_entire_binding(),
        };
        let mut ray_inputs = vec![
            uniform.clone(),
            texture(1, &surfaces.radiance),
            texture(2, depth),
            texture(3, &surfaces.data[0]),
        ];
        if let ScreenTarget::Multisampled { depth_snapshot, .. } = target {
            ray_inputs.push(wgpu::BindGroupEntry {
                binding: 7,
                resource: depth_snapshot.as_entire_binding(),
            });
        }
        let reflected = reflection_size.map(|size| {
            let views = trace_targets(&mut self.reflection, device, size);
            let trace = self.trace[depth_mode][shader_index].get_or_insert_with(|| {
                PassPipeline::new(device, shader, "trace", HDR_FORMAT, false, projection)
            });
            trace.draw(device, encoder, &ray_inputs, &effect_attachments(views));
            views
        });
        let indirect = indirect_size.map(|size| {
            let views = trace_targets(&mut self.indirect, device, size);
            ray_inputs.push(texture(4, &surfaces.data[1]));
            let trace = self.indirect_trace[depth_mode][usize::from(multisampled)]
                .get_or_insert_with(|| {
                    PassPipeline::new(
                        device,
                        shader,
                        "trace_indirect",
                        HDR_FORMAT,
                        false,
                        projection,
                    )
                });
            trace.draw(device, encoder, &ray_inputs, &effect_attachments(views));
            let filtered = Attachment::get(&mut self.filtered, device, size, HDR_FORMAT, 1, true);
            let filter = self.denoise.get_or_insert_with(|| {
                PassPipeline::new(
                    device,
                    shader,
                    "denoise_indirect",
                    HDR_FORMAT,
                    false,
                    screen_depth::Mode::Flat,
                )
            });
            filter.draw(
                device,
                encoder,
                &[uniform.clone(), texture(9, views[0]), texture(10, views[1])],
                &[Some(color_attachment(filtered, wgpu::Color::TRANSPARENT))],
            );
            [filtered, views[1]]
        });
        let Some([reflected, reflection_guide]) = reflected.or(indirect) else {
            return [0; 2];
        };
        let mut composite_inputs = vec![
            uniform,
            texture(2, depth),
            texture(3, &surfaces.data[0]),
            texture(4, &surfaces.data[1]),
            texture(5, &surfaces.data[2]),
            texture(6, reflected),
            texture(8, reflection_guide),
        ];
        if let Some([light, guide]) = indirect {
            composite_inputs.extend([texture(9, light), texture(10, guide)]);
        }
        let (mode, fragment, format, attachment) = match target {
            ScreenTarget::Single { color, hdr } => {
                composite_inputs.push(texture(1, &surfaces.radiance));
                (
                    usize::from(hdr),
                    "composite",
                    if hdr { HDR_FORMAT } else { COLOR_FORMAT },
                    color_attachment(color, wgpu::Color::TRANSPARENT),
                )
            }
            ScreenTarget::Multisampled {
                color,
                resolve,
                keep_samples,
                ..
            } => (
                2,
                "composite_samples",
                HDR_FORMAT,
                wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: if keep_samples {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    },
                },
            ),
        };
        let composite = self.composite[depth_mode][3 * usize::from(packed) + mode]
            .get_or_insert_with(|| {
                PassPipeline::new(device, shader, fragment, format, multisampled, projection)
            });
        composite.draw(device, encoder, &composite_inputs, &[Some(attachment)]);
        [reflection_size, indirect_size]
            .map(|size| size.map_or(0, |size| size.into_iter().map(u64::from).product()))
    }
}

fn make_shader(device: &wgpu::Device, multisampled: bool, packed: bool) -> wgpu::ShaderModule {
    let (depth, texture) = if multisampled {
        (
            include_str!("screen_reflection_msaa.wgsl"),
            "texture_multisampled_2d",
        )
    } else {
        (include_str!("screen_reflection_single.wgsl"), "texture_2d")
    };
    let (data, indirect) = if packed {
        (
            include_str!("screen_data_packed.wgsl"),
            include_str!("screen_indirect.wgsl"),
        )
    } else {
        (include_str!("screen_data_float.wgsl"), "")
    };
    let source = [
        screen_depth::SHADER,
        include_str!("screen_packing.wgsl"),
        depth,
        &data.replace("XRACK_SCREEN_TEXTURE", texture),
        include_str!("screen_reflection.wgsl"),
        indirect,
    ]
    .join("\n");
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack screen-space lighting"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn trace_targets<'a>(
    pool: &'a mut [Option<Attachment>; 2],
    device: &wgpu::Device,
    size: [u32; 2],
) -> [&'a wgpu::TextureView; 2] {
    pool.each_mut()
        .map(|slot| Attachment::get(slot, device, size, HDR_FORMAT, 1, true))
}

fn effect_attachments(
    views: [&wgpu::TextureView; 2],
) -> [Option<wgpu::RenderPassColorAttachment<'_>>; 2] {
    views.map(|view| Some(color_attachment(view, wgpu::Color::TRANSPARENT)))
}

fn color_attachment(
    view: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(clear),
            store: wgpu::StoreOp::Store,
        },
    }
}

pub(super) fn surface_targets(
    multisampled: bool,
    indirect: bool,
) -> [Option<wgpu::ColorTargetState>; 4] {
    surface_formats(multisampled, indirect).map(|format| {
        Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })
    })
}

fn surface_formats(multisampled: bool, indirect: bool) -> [wgpu::TextureFormat; 4] {
    let metadata = if indirect {
        wgpu::TextureFormat::Rgba16Uint
    } else if multisampled {
        wgpu::TextureFormat::Rgba8Unorm
    } else {
        HDR_FORMAT
    };
    [HDR_FORMAT, metadata, metadata, HDR_FORMAT]
}

struct PassPipeline {
    render: wgpu::RenderPipeline,
    inputs: wgpu::BindGroupLayout,
}

impl PassPipeline {
    fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        fragment: &str,
        format: wgpu::TextureFormat,
        apply_to_samples: bool,
        projection: screen_depth::Mode,
    ) -> Self {
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let targets = [
            Some(wgpu::ColorTargetState {
                format,
                blend: apply_to_samples.then_some(wgpu::BlendState {
                    color: add,
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: if apply_to_samples {
                    wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE
                } else {
                    wgpu::ColorWrites::ALL
                },
            }),
            Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ];
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack screen-space reflection pass"),
            layout: None,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(fragment),
                compilation_options: screen_depth::compilation(projection),
                targets: &targets[..if fragment.starts_with("trace") { 2 } else { 1 }],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: if apply_to_samples { 4 } else { 1 },
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let inputs = render.get_bind_group_layout(0);
        Self { render, inputs }
    }

    fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &[wgpu::BindGroupEntry<'_>],
        attachments: &[Option<wgpu::RenderPassColorAttachment<'_>>],
    ) {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack screen reflection attachments"),
            layout: &self.inputs,
            entries: inputs,
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack screen reflection resolve"),
                color_attachments: attachments,
                ..Default::default()
            },
        );
        pass.set_pipeline(&self.render);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
