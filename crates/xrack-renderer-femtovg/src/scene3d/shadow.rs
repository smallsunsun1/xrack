use std::{collections::VecDeque, ops::Range, sync::Arc};

use xrack_runtime::{
    Node, Value,
    scene3d::{
        DirectionalShadowSpace, MAX_SCENE_LIGHTS, Mesh3D, PointShadowProjection, SceneCamera,
        SceneLight, SceneObject, SceneVisibility, ShadowLight,
    },
};

use crate::GpuPaintContext;

use super::{
    DEPTH_FORMAT, MeshCache, append_floats, buffer,
    cascade::{CascadeRequest, CascadeSplits, MAX_CASCADES},
    deform::{DeformationLayouts, GeometryBindings},
    depth::{DepthPass, DepthPipelines},
    particles::ModelParticleBatch,
};

const MAX_SHADOW_MAPS: usize = 4;
const CUBE_FACES: usize = PointShadowProjection::FACE_COUNT;
const MAX_SHADOW_VIEWS: usize = MAX_SHADOW_MAPS * CUBE_FACES;
// Limit each cached Depth32Float atlas to 64 MiB.
const MAX_SHADOW_TEXELS: u32 = 16 * 1024 * 1024;
type LightSlots = [Option<(usize, ShadowLight)>; MAX_SHADOW_MAPS];
type ClipRows = [[f32; 4]; 4];

pub(super) struct ShadowAtlas {
    pub matrices: wgpu::Buffer,
    pub depth: wgpu::TextureView,
    pub slots: [u8; MAX_SCENE_LIGHTS],
}

struct CachedAtlas {
    path: Vec<usize>,
    resolution: u32,
    lights: LightSlots,
    objects: Option<Arc<[SceneObject]>>,
    particles: Option<ParticleShadowKey>,
    layer_filter: Option<Arc<SceneVisibility>>,
    cascades: Option<CascadeRequest>,
    fog_receiver: Option<[[f32; 3]; 8]>,
    gpu: Option<RenderedAtlas>,
}

struct RenderedAtlas {
    bindings: ShadowAtlas,
    texture: wgpu::Texture,
    layers: Vec<Option<LayerContents>>,
}

#[derive(PartialEq)]
struct LayerContents {
    slot: usize,
    face: usize,
    rows: ClipRows,
    instances: Vec<Range<u32>>,
    particle_batches: Vec<usize>,
}

pub(super) struct Shadows {
    materials: wgpu::BindGroupLayout,
    deformations: DeformationLayouts,
    pipeline: Option<DepthPipelines>,
    empty: ShadowAtlas,
    cache: VecDeque<CachedAtlas>,
    pub sampler: wgpu::Sampler,
    pub passes: u64,
    particle_draws: u64,
}

pub(super) struct ShadowRequest<'a> {
    path: &'a [usize],
    resolution: u32,
    lights: LightSlots,
    objects: Option<&'a Arc<[SceneObject]>>,
    particles: Option<ParticleShadowKey>,
    model_batches: &'a [ModelParticleBatch],
    layer_filter: Option<&'a Arc<SceneVisibility>>,
    cascades: Option<CascadeRequest>,
    fog_receiver: Option<[[f32; 3]; 8]>,
}

#[derive(PartialEq)]
struct ParticleShadowKey {
    emitters: Option<Value>,
    affectors: Option<Value>,
    time: f64,
    layers: u64,
}

impl<'a> ShadowRequest<'a> {
    pub fn for_view(
        node: &Node,
        (objects, model_batches): (Option<&'a Arc<[SceneObject]>>, &'a [ModelParticleBatch]),
        lights: &[SceneLight],
        camera: SceneCamera,
        path: &'a [usize],
        layer_filter: Option<&'a Arc<SceneVisibility>>,
        fog_receiver: Option<[[f32; 3]; 8]>,
    ) -> Option<Self> {
        if !node.boolean("shadows", false) {
            return None;
        }
        let resolution = match node.properties.get("shadow-map-size") {
            Some(Value::Int(size)) => (*size).clamp(128, 2048) as u32,
            _ => 1024,
        };
        let mut eligible = lights.iter().enumerate().filter_map(|(index, light)| {
            ShadowLight::for_light(light).map(|projection| (index, projection))
        });
        let lights = std::array::from_fn(|_| eligible.next());
        lights[0]?;
        let directional = lights
            .iter()
            .flatten()
            .any(|(_, light)| matches!(light, ShadowLight::Directional(_)));
        let particles = model_batches
            .iter()
            .any(|batch| batch.shadow_bounds.is_some())
            .then(|| ParticleShadowKey {
                emitters: node.properties.get("particle-emitters").cloned(),
                affectors: node.properties.get("particle-affectors").cloned(),
                time: node.number("particle-time").unwrap_or(0.0),
                layers: camera.layer_mask(),
            });
        if objects.is_none() && particles.is_none() {
            return None;
        }
        Some(Self {
            path,
            resolution,
            lights,
            objects,
            particles,
            model_batches,
            layer_filter,
            fog_receiver,
            cascades: directional
                .then(|| CascadeRequest::new(node, camera))
                .flatten(),
        })
    }
}

impl Shadows {
    pub fn invalidate_item_textures(&mut self) {
        self.cache.retain(|atlas| {
            atlas.objects.as_ref().is_none_or(|objects| {
                !objects.iter().any(|object| {
                    object.casts_shadow && super::mesh_samples_item_alpha(&object.mesh)
                })
            })
        });
    }

    pub fn discard_recorded_layers(&mut self) {
        self.cache.clear();
    }

    pub fn new(
        device: &wgpu::Device,
        materials: wgpu::BindGroupLayout,
        deformations: DeformationLayouts,
    ) -> Self {
        let empty = create_atlas(device, &depth_texture(device, 1, 1), &ShadowPlan::default());
        Self {
            materials,
            deformations,
            pipeline: None,
            empty,
            cache: VecDeque::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D shadow comparison"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            passes: 0,
            particle_draws: 0,
        }
    }

    pub fn prepare(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        meshes: &mut MeshCache,
        geometry: GeometryBindings<'_>,
        request: Option<ShadowRequest<'_>>,
    ) -> (&ShadowAtlas, u64) {
        let Some(ShadowRequest {
            path,
            resolution,
            lights,
            objects,
            particles,
            model_batches,
            layer_filter,
            cascades,
            fog_receiver,
        }) = request
        else {
            return (&self.empty, self.particle_draws);
        };
        let resolution = resolution.min(ctx.device.limits().max_texture_dimension_2d);
        if let Some(index) = self.cache.iter().position(|cached| {
            cached.resolution == resolution
                && cached.lights == lights
                && cached.cascades == cascades
                && cached.fog_receiver == fog_receiver
                && cached.layer_filter.as_ref() == layer_filter
                && cached.particles == particles
                && same_objects(
                    cached.objects.as_ref(),
                    objects,
                    layer_filter.map(Arc::as_ref),
                )
        }) {
            self.cache.rotate_left(index);
            return (self.current_atlas(), self.particle_draws);
        }

        // Cascade projections snap to texels, so choose their resolution
        // before fitting any view. Use a conservative layer count to keep it
        // stable when a face or cascade becomes empty.
        let fit_resolution = if let Some(cascades) = cascades {
            let layers = lights
                .iter()
                .flatten()
                .map(|(_, light)| match light {
                    ShadowLight::Directional(_) => cascades.count,
                    ShadowLight::Point { .. } => CUBE_FACES,
                    ShadowLight::Spot { .. } => 1,
                })
                .sum::<usize>();
            budget_resolution(resolution, layers)
        } else {
            resolution
        };
        let plan = ShadowPlan::new(
            (
                objects.map_or(&[], |objects| objects.as_ref()),
                model_batches,
            ),
            layer_filter.map(Arc::as_ref),
            lights,
            cascades,
            fit_resolution,
            fog_receiver.as_ref(),
        );
        let gpu = if plan.views.iter().any(ShadowView::has_draws) {
            let layers = plan.views.len();
            let size = budget_resolution(fit_resolution, layers);
            let fits = |atlas: &RenderedAtlas| {
                atlas.texture.width() == size
                    && atlas.texture.depth_or_array_layers() == layers as u32
            };
            // Update a viewport in place when unchanged projections can
            // keep their depth. Distinct viewports may retain distinct atlases.
            let retain = self.cache.iter().position(|cached| {
                cached.path == path
                    && same_objects(cached.objects.as_ref(), objects, None)
                    && cached.gpu.as_ref().is_some_and(fits)
            });
            let previous = if let Some(index) = retain {
                self.cache.remove(index)
            } else if self.cache.len() == 2 {
                self.cache.pop_back()
            } else {
                None
            };
            let same_particles = previous
                .as_ref()
                .is_some_and(|entry| entry.particles == particles);
            let (texture, previous_layers) = previous
                .and_then(|entry| entry.gpu)
                .filter(fits)
                .map_or_else(
                    || (depth_texture(ctx.device, size, layers as u32), Vec::new()),
                    |atlas| {
                        (
                            atlas.texture,
                            if retain.is_some() {
                                atlas.layers
                            } else {
                                Vec::new()
                            },
                        )
                    },
                );
            Some(self.render(
                ctx,
                queue,
                meshes,
                (geometry, model_batches),
                plan,
                (texture, previous_layers, same_particles),
            ))
        } else {
            self.cache.truncate(1);
            None
        };
        self.cache.push_front(CachedAtlas {
            path: path.to_vec(),
            resolution,
            lights,
            objects: objects.cloned(),
            particles,
            layer_filter: layer_filter.cloned(),
            cascades,
            fog_receiver,
            gpu,
        });
        (self.current_atlas(), self.particle_draws)
    }

    fn current_atlas(&self) -> &ShadowAtlas {
        self.cache
            .front()
            .and_then(|entry| entry.gpu.as_ref())
            .map_or(&self.empty, |atlas| &atlas.bindings)
    }

    fn render(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        meshes: &mut MeshCache,
        (geometry, particles): (GeometryBindings<'_>, &[ModelParticleBatch]),
        plan: ShadowPlan<'_>,
        (texture, previous, same_particles): (wgpu::Texture, Vec<Option<LayerContents>>, bool),
    ) -> RenderedAtlas {
        let bindings = create_atlas(ctx.device, &texture, &plan);
        let mut layers = Vec::with_capacity(plan.views.len());
        let pipelines = self.pipeline.get_or_insert_with(|| {
            DepthPipelines::new(
                ctx.device,
                &self.materials,
                &self.deformations,
                wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 0.5,
                    clamp: 0.0,
                },
            )
        });
        for (layer, shadow) in plan.views.into_iter().enumerate() {
            let contents = shadow.contents();
            let unchanged = (same_particles || shadow.particle_batches.is_empty())
                && previous.get(layer) == Some(&contents);
            let empty = contents.is_none();
            layers.push(contents);
            if empty || unchanged {
                continue;
            }
            // Earlier viewport work in the encoder is already ordered before
            // recording a recycled layer.
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("XRack 3D shadow layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer as u32,
                array_layer_count: Some(1),
                ..Default::default()
            });
            let mut bytes = Vec::with_capacity(64);
            append_floats(&mut bytes, shadow.rows.into_iter().flatten());
            let camera = buffer(
                ctx.device,
                "XRack 3D light camera",
                &bytes,
                wgpu::BufferUsages::UNIFORM,
            );
            let globals = pipelines.camera_group(ctx.device, &camera);
            let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("XRack 3D shadow depth"),
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &globals, &[]);
            pass.set_vertex_buffer(1, geometry.instances.slice(..));
            let mut active = None;
            for (kind, range) in &shadow.draws {
                let mesh = meshes.get(ctx.device, queue, kind);
                pipelines.draw(
                    ctx.device,
                    &mut pass,
                    &mut active,
                    mesh,
                    (
                        range.clone(),
                        geometry.deformation(range.start),
                        DepthPass::Auxiliary(xrack_runtime::scene_effect::CaptureRenderables::All),
                    ),
                );
            }
            for index in &shadow.particle_batches {
                let batch = &particles[*index];
                pass.set_vertex_buffer(1, batch.instances.slice(..));
                for mesh in batch.meshes.iter().filter(|mesh| mesh.has_opaque_parts()) {
                    let mesh = meshes.get(ctx.device, queue, mesh);
                    self.particle_draws += pipelines.draw(
                        ctx.device,
                        &mut pass,
                        &mut active,
                        mesh,
                        (0..batch.count, None, DepthPass::Particles),
                    );
                }
            }
            self.passes = self.passes.saturating_add(1);
        }
        RenderedAtlas {
            bindings,
            texture,
            layers,
        }
    }
}

type Draw<'a> = (&'a Mesh3D, Range<u32>);

struct ShadowView<'a> {
    slot: usize,
    face: usize,
    rows: ClipRows,
    draws: Vec<Draw<'a>>,
    particle_batches: Vec<usize>,
}

impl ShadowView<'_> {
    fn has_draws(&self) -> bool {
        !(self.draws.is_empty() && self.particle_batches.is_empty())
    }

    fn contents(&self) -> Option<LayerContents> {
        self.has_draws().then(|| LayerContents {
            slot: self.slot,
            face: self.face,
            rows: self.rows,
            instances: self.draws.iter().map(|(_, range)| range.clone()).collect(),
            particle_batches: self.particle_batches.clone(),
        })
    }
}

type Bounds = [[f32; 3]; 2];

struct ParticleCaster {
    batch: usize,
    bounds: Bounds,
    corners: [[f32; 3]; 8],
}

#[derive(Default)]
struct ShadowPlan<'a> {
    views: Vec<ShadowView<'a>>,
    slots: [u8; MAX_SCENE_LIGHTS],
    parameters: [[f32; 4]; MAX_SHADOW_MAPS],
    cascade_ends: [[f32; MAX_CASCADES]; MAX_SHADOW_MAPS],
}

impl<'a> ShadowPlan<'a> {
    fn new(
        (objects, models): (&'a [SceneObject], &[ModelParticleBatch]),
        layer_filter: Option<&SceneVisibility>,
        lights: LightSlots,
        cascades: Option<CascadeRequest>,
        resolution: u32,
        fog_receiver: Option<&[[f32; 3]; 8]>,
    ) -> Self {
        let included = || {
            objects.iter().enumerate().filter(|(index, _)| {
                layer_filter.is_none_or(|filter| filter.contains(*index as u32))
            })
        };
        let casters: Vec<_> = included()
            .filter(|(_, object)| object.casts_shadow && object.mesh.has_auxiliary_depth())
            .collect();
        let particles: Vec<_> = models
            .iter()
            .enumerate()
            .filter_map(|(batch, model)| {
                let bounds = model.shadow_bounds?;
                let corners = std::array::from_fn(|corner| {
                    std::array::from_fn(|axis| bounds[(corner >> axis) & 1][axis])
                });
                Some(ParticleCaster {
                    batch,
                    bounds,
                    corners,
                })
            })
            .collect();
        if casters.is_empty() && particles.is_empty() {
            return Self::default();
        }
        let corners: Vec<_> = included()
            .flat_map(|(_, object)| object.bounds_corners())
            .chain(particles.iter().flat_map(|particle| particle.corners))
            .chain(fog_receiver.into_iter().flatten().copied())
            .collect();
        let corners = corners.as_slice();
        let has_point = lights
            .iter()
            .flatten()
            .any(|(_, light)| matches!(light, ShadowLight::Point { .. }));
        let caster_bounds: Vec<_> = if has_point {
            casters
                .iter()
                .map(|(_, object)| {
                    object.bounds_corners().iter().fold(
                        [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]],
                        |[min, max], p| {
                            [
                                std::array::from_fn(|axis| min[axis].min(p[axis])),
                                std::array::from_fn(|axis| max[axis].max(p[axis])),
                            ]
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut plan = Self::default();
        let splits = cascades.and_then(|request| request.splits(corners));
        for (slot, (light, projection)) in lights.into_iter().flatten().enumerate() {
            let start = plan.views.len();
            if let ShadowLight::Point {
                position,
                inverse_range_squared,
            } = projection
            {
                if let Some(cube) = PointShadowProjection::new(
                    position,
                    inverse_range_squared,
                    corners,
                    caster_bounds
                        .iter()
                        .copied()
                        .chain(particles.iter().map(|particle| particle.bounds)),
                ) {
                    let [near, far] = cube.near_far();
                    plan.parameters[slot] = [near, far, 0.0, 0.0];
                    for face in 0..CUBE_FACES {
                        let draws =
                            batches(casters.iter().copied().zip(&caster_bounds).filter_map(
                                |(caster, bounds)| cube.overlaps(face, *bounds).then_some(caster),
                            ));
                        let particle_batches = particles
                            .iter()
                            .filter_map(|particle| {
                                cube.overlaps(face, particle.bounds)
                                    .then_some(particle.batch)
                            })
                            .collect();
                        plan.push(ShadowView {
                            slot,
                            face,
                            rows: cube.rows(face),
                            draws,
                            particle_batches,
                        });
                    }
                }
            } else if let (ShadowLight::Directional(axis), Some(request)) = (projection, cascades) {
                if let Some(splits) = splits {
                    plan.directional_cascades(
                        slot,
                        (request, splits),
                        resolution,
                        axis,
                        &casters,
                        &particles,
                    );
                }
            } else if let Some(rows) = projection.planar_projection(corners) {
                plan.push(ShadowView {
                    slot,
                    face: 0,
                    rows,
                    draws: batches(casters.iter().copied()),
                    particle_batches: particles.iter().map(|particle| particle.batch).collect(),
                });
            }
            if plan.views[start..].iter().any(ShadowView::has_draws) {
                // Zero means that this light has no shadow views.
                plan.slots[light] = slot as u8 + 1;
            }
        }
        plan
    }

    fn directional_cascades(
        &mut self,
        slot: usize,
        (request, splits): (CascadeRequest, CascadeSplits),
        resolution: u32,
        axis: [f32; 3],
        casters: &[(usize, &'a SceneObject)],
        particles: &[ParticleCaster],
    ) {
        let space = DirectionalShadowSpace::new(axis);
        let projected: Vec<_> = casters
            .iter()
            .map(|(_, object)| space.bounds(&object.bounds_corners()))
            .collect();
        let particle_bounds: Vec<_> = particles
            .iter()
            .map(|particle| space.bounds(&particle.corners))
            .collect();
        self.parameters[slot] = [splits.near, splits.far, splits.count as f32, splits.blend];
        self.cascade_ends[slot] = splits.ends;
        for face in 0..splits.count {
            let (rows, draws, particle_batches) = space
                .cascade(&request.camera, splits.range(face), resolution)
                .map_or_else(
                    || ([[0.0; 4]; 4], Vec::new(), Vec::new()),
                    |mut volume| {
                        let draws = batches(casters.iter().copied().zip(&projected).filter_map(
                            |(caster, bounds)| volume.include_caster((*bounds)?).then_some(caster),
                        ));
                        let particle_batches = particles
                            .iter()
                            .zip(&particle_bounds)
                            .filter_map(|(particle, bounds)| {
                                volume.include_caster((*bounds)?).then_some(particle.batch)
                            })
                            .collect();
                        let rows = volume.rows();
                        if rows.iter().flatten().all(|value| value.is_finite()) {
                            (rows, draws, particle_batches)
                        } else {
                            ([[0.0; 4]; 4], Vec::new(), Vec::new())
                        }
                    },
                );
            // Reserve the layer even when the slice has no casters so
            // following layers can stay resident as the camera moves.
            self.views.push(ShadowView {
                slot,
                face,
                rows,
                draws,
                particle_batches,
            });
        }
    }

    fn push(&mut self, view: ShadowView<'a>) {
        if view.has_draws() && view.rows.iter().flatten().all(|value| value.is_finite()) {
            self.views.push(view);
        }
    }
}

fn batches<'a>(casters: impl Iterator<Item = (usize, &'a SceneObject)>) -> Vec<Draw<'a>> {
    let mut draws: Vec<Draw<'a>> = Vec::new();
    for (index, object) in casters {
        let index = index as u32;
        match draws.last_mut() {
            Some((previous, range))
                if object.mesh.influences().is_none()
                    && object.mesh.morph_geometry().is_none()
                    && *previous == &object.mesh
                    && range.end == index =>
            {
                range.end += 1;
            }
            _ => draws.push((&object.mesh, index..index + 1)),
        }
    }
    draws
}

fn budget_resolution(resolution: u32, layers: usize) -> u32 {
    resolution.min((MAX_SHADOW_TEXELS / layers as u32).isqrt())
}

fn same_objects(
    a: Option<&Arc<[SceneObject]>>,
    b: Option<&Arc<[SceneObject]>>,
    layers: Option<&SceneVisibility>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || same_geometry(a, b, layers),
        (None, None) => true,
        _ => false,
    }
}

fn same_geometry(a: &[SceneObject], b: &[SceneObject], layers: Option<&SceneVisibility>) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).enumerate().all(|(index, (a, b))| {
            if layers.is_some_and(|layers| !layers.contains(index as u32)) {
                return true;
            }
            // Noncasters contribute only their bounds to projection fitting.
            let same_mesh = if a.casts_shadow {
                a.mesh.same_shadow_mesh(&b.mesh)
            } else {
                match (&a.mesh, &b.mesh) {
                    (Mesh3D::Imported(left), Mesh3D::Imported(right)) => {
                        Arc::ptr_eq(left.vertex_streams(), right.vertex_streams())
                    }
                    _ => a.mesh == b.mesh,
                }
            };
            same_mesh
                && a.mesh.shader_displacement() == b.mesh.shader_displacement()
                && a.transform_rows() == b.transform_rows()
                && a.skin_palette() == b.skin_palette()
                && a.morph_weights() == b.morph_weights()
                && a.casts_shadow == b.casts_shadow
                && (!a.casts_shadow
                    || a.color.0 as u8 == b.color.0 as u8
                    || !a.mesh.materials().iter().any(|material| {
                        material
                            .depth_coverage()
                            .is_some_and(|coverage| coverage.samples_alpha())
                    }))
        })
}

fn depth_texture(device: &wgpu::Device, resolution: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack 3D shadow maps"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_atlas(
    device: &wgpu::Device,
    texture: &wgpu::Texture,
    plan: &ShadowPlan<'_>,
) -> ShadowAtlas {
    let depth = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("XRack 3D shadow texture array"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    // Match the WGSL layout: matrices, then two vec4s of layer
    // indices and two vec4s of depth data per light.
    let mut indices = [[0_i32; 8]; MAX_SHADOW_MAPS];
    let mut bytes = Vec::with_capacity(MAX_SHADOW_VIEWS * 64 + MAX_SHADOW_MAPS * 64);
    for (layer, view) in plan.views.iter().enumerate() {
        append_floats(&mut bytes, view.rows.iter().flatten().copied());
        if view.has_draws() {
            indices[view.slot][view.face] = layer as i32 + 1;
        }
    }
    bytes.resize(MAX_SHADOW_VIEWS * 64, 0);
    for (slot, indices) in indices.into_iter().enumerate() {
        bytes.extend(indices.into_iter().flat_map(i32::to_ne_bytes));
        append_floats(&mut bytes, plan.parameters[slot]);
        append_floats(&mut bytes, plan.cascade_ends[slot]);
    }
    let matrices = buffer(
        device,
        "XRack 3D shadow projections",
        &bytes,
        wgpu::BufferUsages::UNIFORM,
    );
    ShadowAtlas {
        depth,
        slots: plan.slots,
        matrices,
    }
}
