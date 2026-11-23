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
    depth::DepthPipelines,
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
    objects: Arc<[SceneObject]>,
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
}

pub(super) struct Shadows {
    materials: wgpu::BindGroupLayout,
    deformations: DeformationLayouts,
    pipeline: Option<DepthPipelines>,
    empty: ShadowAtlas,
    cache: VecDeque<CachedAtlas>,
    pub sampler: wgpu::Sampler,
    pub passes: u64,
}

pub(super) struct ShadowRequest<'a> {
    path: &'a [usize],
    resolution: u32,
    lights: LightSlots,
    objects: &'a Arc<[SceneObject]>,
    layer_filter: Option<&'a Arc<SceneVisibility>>,
    cascades: Option<CascadeRequest>,
    fog_receiver: Option<[[f32; 3]; 8]>,
}

impl<'a> ShadowRequest<'a> {
    pub fn for_view(
        node: &Node,
        objects: &'a Arc<[SceneObject]>,
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
        Some(Self {
            path,
            resolution,
            lights,
            objects,
            layer_filter,
            fog_receiver,
            cascades: directional
                .then(|| CascadeRequest::new(node, camera))
                .flatten(),
        })
    }
}

impl Shadows {
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
        }
    }

    pub fn prepare(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        meshes: &mut MeshCache,
        geometry: GeometryBindings<'_>,
        request: Option<ShadowRequest<'_>>,
    ) -> &ShadowAtlas {
        let Some(ShadowRequest {
            path,
            resolution,
            lights,
            objects,
            layer_filter,
            cascades,
            fog_receiver,
        }) = request
        else {
            return &self.empty;
        };
        let resolution = resolution.min(ctx.device.limits().max_texture_dimension_2d);
        if let Some(index) = self.cache.iter().position(|cached| {
            cached.resolution == resolution
                && cached.lights == lights
                && cached.cascades == cascades
                && cached.fog_receiver == fog_receiver
                && cached.layer_filter.as_ref() == layer_filter
                && same_objects(&cached.objects, objects, layer_filter.map(Arc::as_ref))
        }) {
            self.cache.rotate_left(index);
            return self.current_atlas();
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
            objects,
            layer_filter.map(Arc::as_ref),
            lights,
            cascades,
            fit_resolution,
            fog_receiver.as_ref(),
        );
        let gpu = if plan.views.iter().any(|view| !view.draws.is_empty()) {
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
                    && same_objects(&cached.objects, objects, None)
                    && cached.gpu.as_ref().is_some_and(|atlas| {
                        fits(atlas)
                            && atlas.layers.iter().zip(&plan.views).any(|(old, view)| {
                                old.as_ref().is_some_and(|old| view.matches_contents(old))
                            })
                    })
            });
            let previous = if let Some(index) = retain {
                self.cache.remove(index)
            } else if self.cache.len() == 2 {
                self.cache.pop_back()
            } else {
                None
            };
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
                geometry,
                plan,
                (texture, previous_layers),
            ))
        } else {
            self.cache.truncate(1);
            None
        };
        self.cache.push_front(CachedAtlas {
            path: path.to_vec(),
            resolution,
            lights,
            objects: objects.clone(),
            layer_filter: layer_filter.cloned(),
            cascades,
            fog_receiver,
            gpu,
        });
        self.current_atlas()
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
        geometry: GeometryBindings<'_>,
        plan: ShadowPlan<'_>,
        (texture, previous): (wgpu::Texture, Vec<Option<LayerContents>>),
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
            let unchanged = previous.get(layer) == Some(&contents);
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
                    (range.clone(), geometry.deformation(range.start)),
                );
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
}

impl ShadowView<'_> {
    fn matches_contents(&self, old: &LayerContents) -> bool {
        self.slot == old.slot
            && self.face == old.face
            && self.rows == old.rows
            && self.draws.iter().map(|(_, range)| range).eq(&old.instances)
    }

    fn contents(&self) -> Option<LayerContents> {
        (!self.draws.is_empty()).then(|| LayerContents {
            slot: self.slot,
            face: self.face,
            rows: self.rows,
            instances: self.draws.iter().map(|(_, range)| range.clone()).collect(),
        })
    }
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
        objects: &'a [SceneObject],
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
            .filter(|(_, object)| object.casts_shadow && object.mesh.has_opaque_parts())
            .collect();
        if casters.is_empty() {
            return Self::default();
        }
        let bounds: Vec<_> = included()
            .map(|(_, object)| object.bounds_corners())
            .collect();
        let object_corners = bounds.as_flattened();
        let receiver_corners = fog_receiver.map(|fog| [object_corners, fog.as_slice()].concat());
        let corners = receiver_corners.as_deref().unwrap_or(object_corners);
        let has_point = lights
            .iter()
            .flatten()
            .any(|(_, light)| matches!(light, ShadowLight::Point { .. }));
        let caster_bounds: Vec<_> = if has_point {
            casters
                .iter()
                .map(|(index, object)| {
                    let corners = if layer_filter.is_none() {
                        bounds[*index]
                    } else {
                        object.bounds_corners()
                    };
                    corners.iter().fold(
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
                    caster_bounds.iter().copied(),
                ) {
                    let [near, far] = cube.near_far();
                    plan.parameters[slot] = [near, far, 0.0, 0.0];
                    for face in 0..CUBE_FACES {
                        let draws =
                            batches(casters.iter().copied().zip(&caster_bounds).filter_map(
                                |(caster, bounds)| cube.overlaps(face, *bounds).then_some(caster),
                            ));
                        plan.push(slot, face, cube.rows(face), draws);
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
                        &bounds,
                    );
                }
            } else if let Some(rows) = projection.planar_projection(corners) {
                plan.push(slot, 0, rows, batches(casters.iter().copied()));
            }
            if plan.views[start..]
                .iter()
                .any(|view| !view.draws.is_empty())
            {
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
        bounds: &[[[f32; 3]; 8]],
    ) {
        let space = DirectionalShadowSpace::new(axis);
        let projected: Vec<_> = casters
            .iter()
            .map(|(index, _)| space.bounds(&bounds[*index]))
            .collect();
        self.parameters[slot] = [splits.near, splits.far, splits.count as f32, splits.blend];
        self.cascade_ends[slot] = splits.ends;
        for face in 0..splits.count {
            let (rows, draws) = space
                .cascade(&request.camera, splits.range(face), resolution)
                .map_or_else(
                    || ([[0.0; 4]; 4], Vec::new()),
                    |mut volume| {
                        let draws = batches(casters.iter().copied().zip(&projected).filter_map(
                            |(caster, bounds)| volume.include_caster((*bounds)?).then_some(caster),
                        ));
                        let rows = volume.rows();
                        if rows.iter().flatten().all(|value| value.is_finite()) {
                            (rows, draws)
                        } else {
                            ([[0.0; 4]; 4], Vec::new())
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
            });
        }
    }

    fn push(&mut self, slot: usize, face: usize, rows: ClipRows, draws: Vec<Draw<'a>>) {
        if !draws.is_empty() && rows.iter().flatten().all(|value| value.is_finite()) {
            self.views.push(ShadowView {
                slot,
                face,
                rows,
                draws,
            });
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
    a: &Arc<[SceneObject]>,
    b: &Arc<[SceneObject]>,
    layers: Option<&SceneVisibility>,
) -> bool {
    Arc::ptr_eq(a, b) || same_geometry(a, b, layers)
}

fn same_geometry(a: &[SceneObject], b: &[SceneObject], layers: Option<&SceneVisibility>) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).enumerate().all(|(index, (a, b))| {
            if layers.is_some_and(|layers| !layers.contains(index as u32)) {
                return true;
            }
            a.mesh == b.mesh
                && a.transform_rows() == b.transform_rows()
                && a.skin_palette() == b.skin_palette()
                && a.morph_weights() == b.morph_weights()
                && a.casts_shadow == b.casts_shadow
                && (!a.casts_shadow
                    || a.color.0 as u8 == b.color.0 as u8
                    || !a.mesh.materials().iter().any(|mat| {
                        matches!(
                            mat.alpha_mode,
                            xrack_runtime::scene3d::MeshAlphaMode::Mask(_)
                        )
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
        if !view.draws.is_empty() {
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
