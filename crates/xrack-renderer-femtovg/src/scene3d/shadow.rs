use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    sync::{Arc, Weak},
};

use xrack_runtime::{
    Node, PhysicsDebugVertex, Value,
    scene3d::{
        DirectionalShadowSpace, MAX_SCENE_LIGHTS, Mesh3D, PointShadowProjection, SceneCamera,
        SceneLight, SceneObject, SceneVisibility, ShadowLight, ShadowMapPrecision as Precision,
        shadow_clip_corners,
    },
};

use crate::GpuPaintContext;

use super::{
    DEPTH_FORMAT, MeshCache, append_floats, buffer,
    cascade::{CascadeRequest, CascadeSplits, MAX_CASCADES},
    deform::{DeformationLayouts, GeometryBindings, bind_instance_streams},
    depth::{DepthPass, DepthPipelines},
    particles::ModelParticleBatch,
    shadow_compact::{CompactTile, DepthPacker, pack_compact},
    shadow_debug::{self, ClipVolume, Settings as DebugSettings},
    shadow_tiles::{MIN_SHADOW_SIZE, ShadowPage, ShadowTile, used_layers},
};

pub(super) const MAX_SHADOW_MAPS: usize = 4;
const CUBE_FACES: usize = PointShadowProjection::FACE_COUNT;
const MAX_SHADOW_VIEWS: usize = MAX_SHADOW_MAPS * CUBE_FACES;
const MAX_SHADOW_BYTES: u32 = 64 * 1024 * 1024;

fn depth_format(precision: Precision) -> wgpu::TextureFormat {
    match precision {
        Precision::Depth32 => DEPTH_FORMAT,
        Precision::Depth16 => wgpu::TextureFormat::Depth16Unorm,
    }
}

#[derive(Clone, Copy)]
struct ShadowFormats {
    native: Precision,
    packed: [bool; MAX_SHADOW_MAPS],
}

impl ShadowFormats {
    fn new(lights: LightSlots) -> Self {
        let needs_32 = lights
            .iter()
            .flatten()
            .any(|light| light.precision == Precision::Depth32);
        Self {
            native: if needs_32 {
                Precision::Depth32
            } else {
                Precision::Depth16
            },
            packed: lights.map(|light| {
                needs_32 && light.is_some_and(|light| light.precision == Precision::Depth16)
            }),
        }
    }

    fn mixed(self) -> bool {
        self.packed.contains(&true)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Address {
    Native(ShadowTile),
    Packed(CompactTile),
}

type LightSlots = [Option<LightSlot>; MAX_SHADOW_MAPS];

#[derive(Clone, Copy, PartialEq)]
struct LightSlot {
    index: usize,
    source: ShadowLight,
    far: Option<f32>,
    resolution: u32,
    precision: Precision,
    cascades: Option<CascadeRequest>,
}

impl LightSlot {
    fn max_views(self) -> usize {
        match self.source {
            ShadowLight::Directional(_) => self.cascades.map_or(1, |request| request.count),
            ShadowLight::Point { .. } => CUBE_FACES,
            ShadowLight::Spot { .. } => 1,
        }
    }
}
type ClipRows = [[f32; 4]; 4];

struct GuideCache {
    settings: DebugSettings,
    objects: Option<Weak<[SceneObject]>>,
    receivers: Vec<bool>,
    lines: Arc<[PhysicsDebugVertex]>,
}

pub(super) struct ShadowFrame<'a> {
    pub atlas: &'a ShadowAtlas,
    pub particle_draws: u64,
    pub fit_bounds: u64,
    pub packed_passes: u64,
    pub guides: Option<Arc<[PhysicsDebugVertex]>>,
}

pub(super) struct ShadowAtlas {
    pub matrices: wgpu::Buffer,
    pub depth: wgpu::TextureView,
    pub packed: wgpu::Buffer,
    pub slots: [u8; MAX_SCENE_LIGHTS],
    pub bytes: u64,
}

struct CachedAtlas {
    path: Vec<usize>,
    lights: LightSlots,
    objects: Option<Arc<[SceneObject]>>,
    particles: Option<ParticleShadowKey>,
    layer_filter: Option<Arc<SceneVisibility>>,
    fog_receiver: Option<[[f32; 3]; 8]>,
    item_revision: u64,
    gpu: Option<RenderedAtlas>,
    cascade_depths: [[f32; 4]; MAX_SHADOW_MAPS],
    cascade_ends: [[f32; MAX_CASCADES]; MAX_SHADOW_MAPS],
    guides: Option<GuideCache>,
}

struct RenderedAtlas {
    bindings: ShadowAtlas,
    texture: Option<wgpu::Texture>,
    compact: Option<wgpu::Buffer>,
    layers: [Option<LayerContents>; MAX_SHADOW_VIEWS],
}

#[derive(PartialEq)]
struct LayerContents {
    address: Address,
    rows: ClipRows,
    instances: Vec<Range<u32>>,
    particle_batches: Vec<usize>,
}

impl LayerContents {
    fn same_draw(&self, other: &Self) -> bool {
        self.rows == other.rows
            && self.instances == other.instances
            && self.particle_batches == other.particle_batches
    }
}

struct ShadowMemory {
    texture: Option<wgpu::Texture>,
    compact: Option<wgpu::Buffer>,
    prior_compact: Option<wgpu::Buffer>,
    prior: [Option<LayerContents>; MAX_SHADOW_VIEWS],
    addresses: [Option<Address>; MAX_SHADOW_VIEWS],
    scratch_side: u32,
    formats: ShadowFormats,
}

impl ShadowMemory {
    fn prepare(
        device: &wgpu::Device,
        plan: &ShadowPlan<'_>,
        formats: ShadowFormats,
        previous: Option<RenderedAtlas>,
        retain: bool,
    ) -> Option<Self> {
        let (old_texture, old_compact, mut prior) = previous.map_or_else(
            || (None, None, std::array::from_fn(|_| None)),
            |atlas| (atlas.texture, atlas.compact, atlas.layers),
        );
        if !retain {
            prior = std::array::from_fn(|_| None);
        }
        let mut sizes = [0; MAX_SHADOW_VIEWS];
        let mut packed_sizes = [0; MAX_SHADOW_VIEWS];
        for view in plan.views.iter().filter(|view| view.has_draws()) {
            let size = plan.sizes[view.slot];
            if formats.packed[view.slot] {
                packed_sizes[view.key()] = size;
            } else {
                sizes[view.key()] = size;
            }
        }
        let scratch_side = packed_sizes.iter().copied().max().unwrap_or(0);
        let scratch_bytes = image_bytes(scratch_side, 1, Precision::Depth16);
        let words: u32 = packed_sizes
            .iter()
            .map(|&size| CompactTile { size, offset: 0 }.words())
            .sum();
        let packed_bytes = u64::from(words) * 4;
        let active_sizes: Vec<_> = sizes.iter().copied().filter(|size| *size != 0).collect();
        let page = ShadowPage::new(&active_sizes);
        let fresh = page.pack(&sizes, &[], active_sizes.len())?;
        let needed = used_layers(&fresh);
        let uses_small_tiles = active_sizes.iter().any(|&size| size < page.size);
        let can_reuse = needed > 0
            && old_texture.as_ref().is_some_and(|texture| {
                let capacity = texture.depth_or_array_layers();
                let had_small_tiles = prior.iter().flatten().any(|entry| {
                matches!(entry.address, Address::Native(tile) if tile.size < texture.width())
            });
                texture.width() == page.size
                    && texture.format() == depth_format(formats.native)
                    && capacity >= needed
                    && (capacity == needed
                        || formats.mixed()
                        || uses_small_tiles == had_small_tiles)
                    && image_bytes(page.size, capacity, formats.native)
                        + packed_bytes
                        + scratch_bytes
                        <= u64::from(MAX_SHADOW_BYTES)
            });
        let (texture, native_addresses) = if can_reuse {
            let texture = old_texture?;
            let old = prior.each_ref().map(|entry| match entry.as_ref()?.address {
                Address::Native(tile) => Some(tile),
                Address::Packed(_) => None,
            });
            let addresses = page
                .pack(&sizes, &old, texture.depth_or_array_layers() as usize)
                .unwrap_or(fresh);
            (Some(texture), addresses)
        } else {
            for entry in &mut prior {
                if entry
                    .as_ref()
                    .is_some_and(|entry| matches!(entry.address, Address::Native(_)))
                {
                    *entry = None;
                }
            }
            (
                (needed != 0).then(|| depth_texture(device, page.size, needed, formats.native)),
                fresh,
            )
        };

        let native_bytes = texture.as_ref().map_or(0, |texture| {
            image_bytes(
                texture.width(),
                texture.depth_or_array_layers(),
                formats.native,
            )
        });
        let budget = u64::from(MAX_SHADOW_BYTES).saturating_sub(native_bytes + scratch_bytes);
        let old_tiles = prior.each_ref().map(|entry| match entry.as_ref()?.address {
            Address::Packed(tile) => Some(tile),
            Address::Native(_) => None,
        });
        let old_capacity = old_compact.as_ref().map_or(0, wgpu::Buffer::size);
        let (compact, prior_compact, compact_addresses) = if words == 0 {
            (None, None, vec![None; MAX_SHADOW_VIEWS])
        } else if packed_bytes <= old_capacity && old_capacity <= budget {
            let capacity = (old_capacity / 4) as u32;
            let addresses = pack_compact(&packed_sizes, &old_tiles, capacity)
                .or_else(|| pack_compact(&packed_sizes, &[], capacity))?;
            (old_compact, None, addresses)
        } else {
            let addresses = pack_compact(&packed_sizes, &old_tiles, words)
                .or_else(|| pack_compact(&packed_sizes, &[], words))?;
            let storage = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack optional packed 16-bit shadows"),
                size: packed_bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (Some(storage), old_compact.filter(|_| retain), addresses)
        };
        let addresses = std::array::from_fn(|i| {
            native_addresses[i]
                .map(Address::Native)
                .or_else(|| compact_addresses[i].map(Address::Packed))
        });
        Some(Self {
            texture,
            compact,
            prior_compact,
            prior,
            addresses,
            scratch_side,
            formats,
        })
    }
}

pub(super) struct Shadows {
    materials: wgpu::BindGroupLayout,
    deformations: DeformationLayouts,
    pipelines: [Option<DepthPipelines>; 2],
    clear: [Option<wgpu::RenderPipeline>; 2],
    packer: Option<DepthPacker>,
    scratch: Option<wgpu::Texture>,
    packed_passes: u64,
    empty: ShadowAtlas,
    cache: VecDeque<CachedAtlas>,
    pub sampler: wgpu::Sampler,
    pub passes: u64,
    fit_bounds: u64,
    particle_draws: u64,
}

pub(super) struct ShadowRequest<'a> {
    path: &'a [usize],
    lights: LightSlots,
    objects: Option<&'a Arc<[SceneObject]>>,
    particles: Option<ParticleShadowKey>,
    model_batches: &'a [ModelParticleBatch],
    layer_filter: Option<&'a Arc<SceneVisibility>>,
    fog_receiver: Option<[[f32; 3]; 8]>,
    debug: DebugSettings,
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
        (camera, cascade_camera): (SceneCamera, SceneCamera),
        (path, debug): (&'a [usize], DebugSettings),
        layer_filter: Option<&'a Arc<SceneVisibility>>,
        fog_receiver: Option<[[f32; 3]; 8]>,
    ) -> Option<Self> {
        if !node.boolean("shadows", false) {
            return None;
        }
        let resolution = match node.properties.get("shadow-map-size") {
            Some(Value::Int(size)) => (*size).clamp(i64::from(MIN_SHADOW_SIZE), 4096) as u32,
            _ => 1024,
        };
        let default_precision = node
            .text("shadow-map-precision")
            .and_then(Precision::from_name)
            .unwrap_or_default();
        let mut eligible = lights.iter().enumerate().filter_map(|(index, light)| {
            let source = ShadowLight::for_light(light)?;
            let directional = matches!(source, ShadowLight::Directional(_));
            let overrides = light.shadow_projection;
            Some(LightSlot {
                index,
                source,
                far: if directional { None } else { overrides.far },
                resolution: overrides.resolution.unwrap_or(resolution),
                precision: overrides.precision.unwrap_or(default_precision),
                cascades: directional
                    .then(|| CascadeRequest::new(node, cascade_camera, overrides))
                    .flatten(),
            })
        });
        let lights = std::array::from_fn(|_| eligible.next());
        lights[0]?;
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
            lights,
            objects,
            particles,
            model_batches,
            layer_filter,
            fog_receiver,
            debug,
        })
    }
}

impl CachedAtlas {
    fn guides(
        &mut self,
        settings: DebugSettings,
        objects: Option<&Arc<[SceneObject]>>,
        particles: &[ModelParticleBatch],
    ) -> Arc<[PhysicsDebugVertex]> {
        // A cached atlas already implies unchanged fitting geometry and
        // caster flags. Receiver flags can change independently of the depth.
        let receivers = || {
            objects
                .into_iter()
                .flat_map(|objects| objects.iter())
                .map(|object| object.receives_shadow)
        };
        let tracks_receivers = settings.receivers || settings.intersections;
        let object_stamp = objects.filter(|_| tracks_receivers).map(Arc::downgrade);
        if let Some(cache) = &mut self.guides
            && cache.settings == settings
            && (!tracks_receivers
                || cache
                    .objects
                    .as_ref()
                    .zip(object_stamp.as_ref())
                    .is_some_and(|(a, b)| a.ptr_eq(b))
                || cache.receivers.iter().copied().eq(receivers()))
        {
            cache.objects = object_stamp;
            return cache.lines.clone();
        }
        let mut lines = Vec::new();
        let mut intersections = Vec::new();
        if settings.cascades || settings.intersections {
            for (slot, light) in self.lights.iter().enumerate() {
                let Some(cascade) = light.and_then(|light| light.cascades) else {
                    continue;
                };
                let [mut near, _, count, _] = self.cascade_depths[slot];
                for (index, &far) in self.cascade_ends[slot][..count as usize].iter().enumerate() {
                    let (corners, _, _) = cascade.camera.frustum_slice(near, far);
                    if settings.cascades {
                        shadow_debug::outline(&mut lines, &corners, shadow_debug::CASCADES[index]);
                    }
                    if settings.intersections {
                        intersections.extend(ClipVolume::new(corners));
                    }
                    near = far;
                }
            }
        }
        if settings.light_volumes
            && let Some(atlas) = &self.gpu
        {
            for layer in atlas.layers.iter().flatten() {
                if let Some(corners) = shadow_clip_corners(layer.rows) {
                    shadow_debug::outline(&mut lines, &corners, shadow_debug::LIGHT_VOLUME);
                }
            }
        }
        if settings.needs_objects() {
            let mut envelopes = HashMap::<_, u8>::new();
            for (index, object) in objects
                .into_iter()
                .flat_map(|objects| objects.iter())
                .enumerate()
            {
                if self
                    .layer_filter
                    .as_ref()
                    .is_some_and(|visible| !visible.contains(index as u32))
                {
                    continue;
                }
                let casts = object.casts_shadow && object.mesh.has_auxiliary_depth();
                let receives = object.receives_shadow;
                let mut flags = u8::from(settings.casters && casts)
                    | (u8::from(settings.receivers && receives) << 1)
                    | (u8::from(settings.intersections && (casts || receives)) << 2);
                if flags == 0 {
                    continue;
                }
                let corners = if let Some(bounds) = object.instance_shadow_bounds() {
                    let seen = envelopes.entry(Arc::as_ptr(bounds)).or_default();
                    let unseen = flags & !*seen;
                    *seen |= flags;
                    flags = unseen;
                    if flags == 0 {
                        continue;
                    }
                    box_corners(**bounds)
                } else {
                    object.bounds_corners()
                };
                if flags & 1 != 0 {
                    shadow_debug::outline(&mut lines, &corners, shadow_debug::CASTER);
                }
                if flags & 2 != 0 {
                    shadow_debug::outline(&mut lines, &corners, shadow_debug::RECEIVER);
                }
                if flags & 4 != 0 {
                    for volume in &intersections {
                        volume.intersect(&corners, &mut lines);
                    }
                }
            }
            if settings.casters || settings.intersections {
                for bounds in particles.iter().filter_map(|batch| batch.shadow_bounds) {
                    let corners = box_corners(bounds);
                    if settings.casters {
                        shadow_debug::outline(&mut lines, &corners, shadow_debug::CASTER);
                    }
                    for volume in &intersections {
                        volume.intersect(&corners, &mut lines);
                    }
                }
            }
        }
        let lines: Arc<[_]> = lines.into();
        self.guides = Some(GuideCache {
            settings,
            objects: object_stamp,
            receivers: if tracks_receivers {
                receivers().collect()
            } else {
                Vec::new()
            },
            lines: lines.clone(),
        });
        lines
    }
}

impl Shadows {
    pub fn discard_recorded_layers(&mut self) {
        self.cache.clear();
        self.scratch = None;
    }

    pub fn new(
        device: &wgpu::Device,
        materials: wgpu::BindGroupLayout,
        deformations: DeformationLayouts,
    ) -> Self {
        let depth = depth_texture(device, 1, 1, Precision::Depth32).create_view(
            &wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            },
        );
        let packed = buffer(
            device,
            "XRack empty shadow words",
            &[0; 4],
            wgpu::BufferUsages::STORAGE,
        );
        let empty = create_atlas(
            device,
            depth,
            packed,
            &ShadowPlan::default(),
            &[None; MAX_SHADOW_VIEWS],
            0,
        );
        Self {
            materials,
            deformations,
            pipelines: [None, None],
            clear: [None, None],
            packer: None,
            scratch: None,
            packed_passes: 0,
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
            fit_bounds: 0,
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
    ) -> ShadowFrame<'_> {
        let Some(ShadowRequest {
            path,
            lights,
            objects,
            particles,
            model_batches,
            layer_filter,
            fog_receiver,
            debug,
        }) = request
        else {
            return ShadowFrame {
                atlas: &self.empty,
                particle_draws: self.particle_draws,
                fit_bounds: self.fit_bounds,
                packed_passes: self.packed_passes,
                guides: None,
            };
        };

        let mut item_revision = meshes.item_texture_revision;
        if item_revision != 0
            && !objects.is_some_and(|objects| {
                objects.iter().enumerate().any(|(index, object)| {
                    object.casts_shadow
                        && layer_filter.is_none_or(|filter| filter.contains(index as u32))
                        && super::mesh_samples_item_alpha(&object.mesh)
                })
            })
            && !model_batches.iter().any(|batch| {
                batch.shadow_bounds.is_some()
                    && batch.meshes.iter().any(super::mesh_samples_item_alpha)
            })
        {
            item_revision = 0;
        }
        if let Some(index) = self.cache.iter().position(|cached| {
            cached.item_revision == item_revision
                && cached.lights == lights
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
            return self.frame_output(debug, objects, model_batches);
        }

        let limit = ctx.device.limits();
        let formats = ShadowFormats::new(lights);
        let (mut sizes, defer_uniform_budget) = light_sizes(
            lights,
            limit.max_texture_dimension_2d,
            u64::from(limit.max_storage_buffer_binding_size),
            formats,
        );
        let mut plan = ShadowPlan::new(
            (
                objects.map_or(&[], |objects| objects.as_ref()),
                model_batches,
            ),
            layer_filter.map(Arc::as_ref),
            lights,
            sizes,
            fog_receiver.as_ref(),
        );
        self.fit_bounds = self.fit_bounds.saturating_add(plan.fit_bounds);
        let cascade_depths = plan.parameters;
        let cascade_ends = plan.cascade_ends;
        let gpu = if plan.views.iter().any(ShadowView::has_draws) {
            if defer_uniform_budget {
                let count = plan.views.iter().filter(|view| view.has_draws()).count();
                sizes = sizes.map(|size| budget_resolution(size, count, formats.native));
                plan.sizes = sizes;
            }
            let retain = self.cache.iter().position(|cached| {
                cached.path == path
                    && cached.item_revision == item_revision
                    && same_objects(cached.objects.as_ref(), objects, None)
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
            let previous = previous.and_then(|entry| entry.gpu);
            ShadowMemory::prepare(ctx.device, &plan, formats, previous, retain.is_some()).map(
                |memory| {
                    self.render(
                        ctx,
                        queue,
                        meshes,
                        (geometry, model_batches),
                        plan,
                        memory,
                        same_particles,
                    )
                },
            )
        } else {
            self.cache.truncate(1);
            None
        };
        self.cache.push_front(CachedAtlas {
            path: path.to_vec(),
            lights,
            objects: objects.cloned(),
            particles,
            layer_filter: layer_filter.cloned(),
            fog_receiver,
            item_revision,
            gpu,
            cascade_depths,
            cascade_ends,
            guides: None,
        });
        if self.scratch.is_some()
            && self.cache.iter().all(|entry| {
                entry
                    .gpu
                    .as_ref()
                    .is_none_or(|atlas| atlas.compact.is_none())
            })
        {
            self.scratch = None;
        }
        self.frame_output(debug, objects, model_batches)
    }

    fn frame_output(
        &mut self,
        debug: DebugSettings,
        objects: Option<&Arc<[SceneObject]>>,
        batches: &[ModelParticleBatch],
    ) -> ShadowFrame<'_> {
        let guides = if debug.active() {
            self.cache
                .front_mut()
                .map(|atlas| atlas.guides(debug, objects, batches))
        } else {
            None
        };
        ShadowFrame {
            atlas: self.current_atlas(),
            particle_draws: self.particle_draws,
            fit_bounds: self.fit_bounds,
            packed_passes: self.packed_passes,
            guides,
        }
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
        memory: ShadowMemory,
        same_particles: bool,
    ) -> RenderedAtlas {
        let ShadowMemory {
            texture,
            compact,
            prior_compact,
            prior,
            addresses,
            scratch_side,
            formats,
        } = memory;
        let depth = texture.as_ref().map_or_else(
            || self.empty.depth.clone(),
            |texture| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("XRack 3D shadow texture array"),
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                })
            },
        );
        let packed = compact.as_ref().unwrap_or(&self.empty.packed).clone();
        let bytes = texture.as_ref().map_or(0, |tex| {
            image_bytes(tex.width(), tex.depth_or_array_layers(), formats.native)
        }) + compact.as_ref().map_or(0, wgpu::Buffer::size);
        let bindings = create_atlas(ctx.device, depth, packed, &plan, &addresses, bytes);
        let mut layers = std::array::from_fn(|_| None);
        for shadow in plan.views {
            let index = shadow.key();
            let Some(address) = addresses[index] else {
                continue;
            };
            let contents = shadow.contents(address);
            let previous = prior[index].as_ref();
            let unchanged_geometry = (same_particles || shadow.particle_batches.is_empty())
                && previous.is_some_and(|old| old.same_draw(&contents));

            if unchanged_geometry && let Some(old) = previous {
                if old.address == address
                    && (prior_compact.is_none() || matches!(address, Address::Native(_)))
                {
                    layers[index] = Some(contents);
                    continue;
                }
                if let (Address::Packed(old), Address::Packed(next), Some(src), Some(dst)) =
                    (old.address, address, &prior_compact, &compact)
                    && old.size == next.size
                {
                    ctx.encoder.copy_buffer_to_buffer(
                        src,
                        u64::from(old.offset) * 4,
                        dst,
                        u64::from(next.offset) * 4,
                        Some(u64::from(next.words()) * 4),
                    );
                    layers[index] = Some(contents);
                    continue;
                }
            }

            layers[index] = Some(contents);
            let (target, tile, precision) = match address {
                Address::Native(tile) => {
                    let Some(target) = &texture else { continue };
                    (target, tile, formats.native)
                }
                Address::Packed(tile) => {
                    if self
                        .scratch
                        .as_ref()
                        .is_none_or(|target| target.width() != scratch_side)
                    {
                        self.scratch = Some(depth_texture(
                            ctx.device,
                            scratch_side,
                            1,
                            Precision::Depth16,
                        ));
                    }
                    let Some(target) = &self.scratch else {
                        continue;
                    };
                    (
                        target,
                        ShadowTile {
                            layer: 0,
                            origin: [0; 2],
                            size: tile.size,
                        },
                        Precision::Depth16,
                    )
                }
            };
            let pipelines = self.pipelines[precision as usize].get_or_insert_with(|| {
                DepthPipelines::with_format(
                    ctx.device,
                    &self.materials,
                    &self.deformations,
                    wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 0.5,
                        clamp: 0.0,
                    },
                    depth_format(precision),
                )
            });
            let partial = tile.size < target.width();
            let view = target.create_view(&wgpu::TextureViewDescriptor {
                label: Some("XRack 3D shadow layer"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: tile.layer,
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
            let mut pass = crate::viewport_gpu_timing::render_pass(
                ctx.encoder,
                &wgpu::RenderPassDescriptor {
                    label: Some("XRack 3D shadow depth"),
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &view,
                        depth_ops: Some(wgpu::Operations {
                            load: if partial {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(1.0)
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                },
            );
            if partial {
                let [x, y] = tile.origin;
                let edge = tile.size;
                let clear = self.clear[precision as usize]
                    .get_or_insert_with(|| clear_pipeline(ctx.device, precision));
                pass.set_scissor_rect(x, y, edge, edge);
                pass.set_viewport(x as f32, y as f32, edge as f32, edge as f32, 0.0, 1.0);
                pass.set_pipeline(clear);
                pass.draw(0..3, 0..1);
            }
            pass.set_bind_group(0, &globals, &[]);
            let mut bound_instances = None;
            let mut active = None;
            for (object, range) in &shadow.draws {
                let mesh = meshes.get(ctx.device, queue, &object.mesh);
                for range in geometry.runs(range.clone()) {
                    geometry.bind_instance_range(&mut pass, &mut bound_instances, range.start);
                    let deformation = geometry.deformation(range.start);
                    pipelines.draw(
                        ctx.device,
                        &mut pass,
                        &mut active,
                        mesh,
                        (
                            range,
                            deformation,
                            object.is_reflected(),
                            DepthPass::Auxiliary {
                                renderables: xrack_runtime::scene_effect::CaptureRenderables::All,
                                reverse_winding: false,
                                reverse_depth: false,
                                lod_fade: geometry.lod_fade,
                            },
                        ),
                    );
                }
            }
            for index in &shadow.particle_batches {
                let batch = &particles[*index];
                bind_instance_streams(&mut pass, &batch.instances, geometry.zero_data);
                for mesh in batch.meshes.iter().filter(|mesh| mesh.has_opaque_parts()) {
                    let mesh = meshes.get(ctx.device, queue, mesh);
                    self.particle_draws += pipelines.draw(
                        ctx.device,
                        &mut pass,
                        &mut active,
                        mesh,
                        (0..batch.count, None, false, DepthPass::Particles),
                    );
                }
            }
            drop(pass);
            if let Address::Packed(tile) = address
                && let Some(output) = &compact
            {
                self.packer
                    .get_or_insert_with(|| DepthPacker::new(ctx.device))
                    .encode(ctx.device, ctx.encoder, &view, output, tile);
                self.packed_passes = self.packed_passes.saturating_add(1);
            }
            self.passes = self.passes.saturating_add(1);
        }
        RenderedAtlas {
            bindings,
            texture,
            compact,
            layers,
        }
    }
}

type Draw<'a> = (&'a SceneObject, Range<u32>);

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

    fn key(&self) -> usize {
        self.slot * CUBE_FACES + self.face
    }

    fn contents(&self, address: Address) -> LayerContents {
        LayerContents {
            address,
            rows: self.rows,
            instances: self.draws.iter().map(|(_, range)| range.clone()).collect(),
            particle_batches: self.particle_batches.clone(),
        }
    }
}

type Bounds = [[f32; 3]; 2];
type Corners = [[f32; 3]; 8];

fn box_corners(bounds: Bounds) -> Corners {
    std::array::from_fn(|corner| std::array::from_fn(|axis| bounds[(corner >> axis) & 1][axis]))
}

fn enclosing_box(corners: &Corners) -> Bounds {
    corners.iter().fold(
        [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]],
        |[min, max], point| {
            [
                std::array::from_fn(|axis| min[axis].min(point[axis])),
                std::array::from_fn(|axis| max[axis].max(point[axis])),
            ]
        },
    )
}

struct ShadowCaster<'a> {
    instance: (usize, &'a SceneObject),
    fit: usize,
}

#[derive(Default)]
struct ShadowFit<'a> {
    points: Vec<[f32; 3]>,
    casting: Vec<bool>,
    casters: Vec<ShadowCaster<'a>>,
}

impl<'a> ShadowFit<'a> {
    fn new(objects: impl Iterator<Item = (usize, &'a SceneObject)>) -> Self {
        let mut fit = Self::default();
        let mut shared = HashMap::new();
        for (index, object) in objects {
            let group = if let Some(bounds) = object.instance_shadow_bounds() {
                *shared
                    .entry(Arc::as_ptr(bounds))
                    .or_insert_with(|| fit.add(box_corners(**bounds)))
            } else {
                fit.add(object.bounds_corners())
            };
            if object.casts_shadow && object.mesh.has_auxiliary_depth() {
                fit.casting[group] = true;
                fit.casters.push(ShadowCaster {
                    instance: (index, object),
                    fit: group,
                });
            }
        }
        fit
    }

    fn add(&mut self, corners: Corners) -> usize {
        let index = self.casting.len();
        self.points.extend(corners);
        self.casting.push(false);
        index
    }

    fn boxes(&self) -> impl Iterator<Item = (&Corners, bool)> {
        self.points
            .as_chunks::<8>()
            .0
            .iter()
            .zip(self.casting.iter().copied())
    }
}

struct ParticleCaster {
    batch: usize,
    bounds: Bounds,
    corners: [[f32; 3]; 8],
}

#[derive(Default)]
struct ShadowPlan<'a> {
    views: Vec<ShadowView<'a>>,
    slots: [u8; MAX_SCENE_LIGHTS],
    sizes: [u32; MAX_SHADOW_MAPS],
    parameters: [[f32; 4]; MAX_SHADOW_MAPS],
    cascade_ends: [[f32; MAX_CASCADES]; MAX_SHADOW_MAPS],
    cascade_reference: [[f32; 4]; MAX_SHADOW_MAPS],
    fit_bounds: u64,
}

impl<'a> ShadowPlan<'a> {
    fn new(
        (objects, models): (&'a [SceneObject], &[ModelParticleBatch]),
        layer_filter: Option<&SceneVisibility>,
        lights: LightSlots,
        sizes: [u32; MAX_SHADOW_MAPS],
        fog_receiver: Option<&[[f32; 3]; 8]>,
    ) -> Self {
        let included = || {
            objects.iter().enumerate().filter(|(index, _)| {
                layer_filter.is_none_or(|filter| filter.contains(*index as u32))
            })
        };
        let particles: Vec<_> = models
            .iter()
            .enumerate()
            .filter_map(|(batch, model)| {
                let bounds = model.shadow_bounds?;
                let corners = box_corners(bounds);
                Some(ParticleCaster {
                    batch,
                    bounds,
                    corners,
                })
            })
            .collect();
        if particles.is_empty()
            && !included()
                .any(|(_, object)| object.casts_shadow && object.mesh.has_auxiliary_depth())
        {
            return Self::default();
        }
        let mut fit = ShadowFit::new(included());
        fit.points
            .extend(particles.iter().flat_map(|particle| particle.corners));
        fit.points
            .extend(fog_receiver.into_iter().flatten().copied());
        let corners = &fit.points;
        let uses_punctual_bounds = lights
            .iter()
            .flatten()
            .any(|light| matches!(light.source, ShadowLight::Point { .. }) || light.far.is_some());
        let fit_boxes: Vec<_> = if uses_punctual_bounds {
            fit.boxes()
                .map(|(corners, _)| enclosing_box(corners))
                .collect()
        } else {
            Vec::new()
        };
        let caster_bounds: Vec<_> = if uses_punctual_bounds {
            fit.casters
                .iter()
                .map(|caster| {
                    let object = caster.instance.1;
                    if object.instance_shadow_bounds().is_some() {
                        enclosing_box(&object.bounds_corners())
                    } else {
                        fit_boxes[caster.fit]
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut plan = Self {
            fit_bounds: fit.casting.len() as u64,
            sizes,
            ..Self::default()
        };
        for (slot, light) in lights.into_iter().flatten().enumerate() {
            let projection = light.source;
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
                    light.far,
                    fit_boxes
                        .iter()
                        .copied()
                        .zip(&fit.casting)
                        .filter_map(|(bounds, &casting)| casting.then_some(bounds))
                        .chain(particles.iter().map(|particle| particle.bounds)),
                ) {
                    let [near, far] = cube.near_far();
                    plan.parameters[slot] = [near, far, 0.0, light.far.unwrap_or(0.0)];
                    let in_range = |bounds| within_shadow_range(position, light.far, bounds);
                    for face in 0..CUBE_FACES {
                        let draws = batches(fit.casters.iter().zip(&caster_bounds).filter_map(
                            |(caster, bounds)| {
                                (in_range(*bounds) && cube.overlaps(face, *bounds))
                                    .then_some(caster.instance)
                            },
                        ));
                        let particle_batches = particles
                            .iter()
                            .filter_map(|particle| {
                                (in_range(particle.bounds) && cube.overlaps(face, particle.bounds))
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
            } else if let (ShadowLight::Directional(axis), Some(request)) =
                (projection, light.cascades)
            {
                if let Some(splits) = request.splits(corners) {
                    plan.directional_cascades(
                        slot,
                        (request, splits),
                        sizes[slot],
                        axis,
                        &fit,
                        &particles,
                    );
                }
            } else if let Some(rows) = projection.planar_projection(corners, light.far) {
                let (draws, particle_batches) = if let (
                    ShadowLight::Spot { position, .. },
                    Some(far),
                ) = (projection, light.far)
                {
                    plan.parameters[slot][3] = far;
                    let in_range = |bounds| within_shadow_range(position, Some(far), bounds);
                    (
                        batches(fit.casters.iter().zip(&caster_bounds).filter_map(
                            |(caster, bounds)| in_range(*bounds).then_some(caster.instance),
                        )),
                        particles
                            .iter()
                            .filter_map(|particle| {
                                in_range(particle.bounds).then_some(particle.batch)
                            })
                            .collect(),
                    )
                } else {
                    (
                        batches(fit.casters.iter().map(|caster| caster.instance)),
                        particles.iter().map(|particle| particle.batch).collect(),
                    )
                };
                plan.push(ShadowView {
                    slot,
                    face: 0,
                    rows,
                    draws,
                    particle_batches,
                });
            }
            if plan.views[start..].iter().any(ShadowView::has_draws) {
                // Zero means that this light has no shadow views.
                plan.slots[light.index] = slot as u8 + 1;
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
        fit: &ShadowFit<'a>,
        particles: &[ParticleCaster],
    ) {
        let space = DirectionalShadowSpace::new(axis);
        let projected_fit: Vec<_> = fit
            .boxes()
            .map(|(bounds, casting)| casting.then(|| space.bounds(bounds)).flatten())
            .collect();
        let projected: Vec<_> = fit
            .casters
            .iter()
            .map(|caster| {
                let object = caster.instance.1;
                if object.instance_shadow_bounds().is_some() {
                    space.bounds(&object.bounds_corners())
                } else {
                    projected_fit[caster.fit]
                }
            })
            .collect();
        let particle_bounds: Vec<_> = particles
            .iter()
            .map(|particle| space.bounds(&particle.corners))
            .collect();
        self.parameters[slot] = [splits.near, splits.far, splits.count as f32, splits.blend];
        self.cascade_ends[slot] = splits.ends;
        self.cascade_reference[slot] = request.camera.depth_row();
        for face in 0..splits.count {
            let (rows, draws, particle_batches) = space
                .cascade(
                    &request.camera,
                    splits.range(face),
                    resolution,
                    request.lock_texels,
                )
                .map_or_else(
                    || ([[0.0; 4]; 4], Vec::new(), Vec::new()),
                    |mut volume| {
                        for bounds in projected_fit.iter().flatten() {
                            volume.include_caster(*bounds);
                        }
                        let draws = batches(fit.casters.iter().zip(&projected).filter_map(
                            |(caster, bounds)| {
                                volume
                                    .overlaps_caster((*bounds)?)
                                    .then_some(caster.instance)
                            },
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

fn within_shadow_range(position: [f32; 3], far: Option<f32>, [min, max]: Bounds) -> bool {
    let Some(far) = far else { return true };
    let distance_squared: f32 = (0..3)
        .map(|axis| {
            (min[axis] - position[axis])
                .max(position[axis] - max[axis])
                .max(0.0)
                .powi(2)
        })
        .sum();
    distance_squared <= far * far
}

fn batches<'a>(casters: impl Iterator<Item = (usize, &'a SceneObject)>) -> Vec<Draw<'a>> {
    let mut draws: Vec<Draw<'a>> = Vec::new();
    for (index, object) in casters {
        let index = index as u32;
        match draws.last_mut() {
            Some((previous, range))
                if object.mesh.influences().is_none()
                    && object.mesh.morph_geometry().is_none()
                    && previous.mesh == object.mesh
                    && previous.is_reflected() == object.is_reflected()
                    && range.end == index =>
            {
                range.end += 1;
            }
            _ => draws.push((object, index..index + 1)),
        }
    }
    draws
}

fn image_bytes(side: u32, layers: u32, precision: Precision) -> u64 {
    u64::from(side) * u64::from(side) * u64::from(layers) * u64::from(precision.bytes())
}

fn budget_resolution(resolution: u32, layers: usize, precision: Precision) -> u32 {
    resolution.min((MAX_SHADOW_BYTES / precision.bytes() / layers as u32).isqrt())
}

fn light_sizes(
    lights: LightSlots,
    limit: u32,
    compact_limit: u64,
    formats: ShadowFormats,
) -> ([u32; MAX_SHADOW_MAPS], bool) {
    let counts = lights.map(|light| light.map_or(0, LightSlot::max_views));
    let mut sizes = lights.map(|light| light.map_or(0, |light| light.resolution.min(limit)));
    let mut valid = sizes.iter().copied().filter(|&value| value != 0);
    let first = valid.next().unwrap_or(MIN_SHADOW_SIZE);
    if !formats.mixed() && valid.all(|size| size == first) {
        if lights
            .iter()
            .flatten()
            .any(|light| light.cascades.is_some())
        {
            return (
                sizes.map(|size| budget_resolution(size, counts.iter().sum(), formats.native)),
                false,
            );
        }
        return (sizes, true);
    }

    // Size the full possible set before snapping cascade projections.
    let max_page = 1 << limit.ilog2();
    sizes = sizes.map(|size| size.min(max_page));
    loop {
        let mut native = Vec::new();
        let mut packed_bytes = 0;
        let mut scratch = 0;
        for (slot, (size, count)) in sizes.into_iter().zip(counts).enumerate() {
            if formats.packed[slot] && count != 0 {
                packed_bytes +=
                    u64::from(CompactTile { offset: 0, size }.words()) * 4 * count as u64;
                scratch = scratch.max(size);
            } else {
                native.extend(std::iter::repeat_n(size, count));
            }
        }
        let page = ShadowPage::new(&native);
        let layers = page
            .pack(&native, &[], native.len())
            .map_or(0, |tiles| used_layers(&tiles));
        let budget = image_bytes(page.size, layers, formats.native)
            + packed_bytes
            + image_bytes(scratch, 1, Precision::Depth16);
        if budget <= u64::from(MAX_SHADOW_BYTES) && packed_bytes <= compact_limit {
            return (sizes, false);
        }
        let Some((slot, maximum)) = sizes
            .iter()
            .copied()
            .enumerate()
            .filter(|&(slot, size)| {
                size > MIN_SHADOW_SIZE && (packed_bytes <= compact_limit || formats.packed[slot])
            })
            .max_by_key(|&(slot, size)| (size, counts[slot]))
        else {
            return (sizes, false);
        };
        sizes[slot] = (maximum.next_power_of_two() / 2).max(MIN_SHADOW_SIZE);
    }
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
                && a.instance_shadow_bounds() == b.instance_shadow_bounds()
                && a.mesh.shader_displacement() == b.mesh.shader_displacement()
                && a.transform_rows() == b.transform_rows()
                && a.skin_palette() == b.skin_palette()
                && a.morph_weights() == b.morph_weights()
                && a.casts_shadow == b.casts_shadow
                && (!a.casts_shadow || a.lod_coverage() == b.lod_coverage())
                && (!a.casts_shadow || a.same_vertex_program_instance(b))
                && (!a.casts_shadow
                    || a.color.0 as u8 == b.color.0 as u8
                    || !a.mesh.materials().iter().any(|material| {
                        material
                            .depth_coverage()
                            .is_some_and(|coverage| coverage.samples_alpha())
                    }))
        })
}

fn depth_texture(
    device: &wgpu::Device,
    resolution: u32,
    layers: u32,
    precision: Precision,
) -> wgpu::Texture {
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
        format: depth_format(precision),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_atlas(
    device: &wgpu::Device,
    depth: wgpu::TextureView,
    packed: wgpu::Buffer,
    plan: &ShadowPlan<'_>,
    destinations: &[Option<Address>],
    allocated_bytes: u64,
) -> ShadowAtlas {
    // Matrices and tiles are addressed by logical shadow view, independently
    // of the array layer and pixel rectangle chosen by the allocator.
    let mut indices = [[0_i32; 8]; MAX_SHADOW_MAPS];
    let mut bytes = Vec::with_capacity(MAX_SHADOW_VIEWS * 96 + MAX_SHADOW_MAPS * 80);
    let mut rows = [[[0.0; 4]; 4]; MAX_SHADOW_VIEWS];
    let mut tiles = [[0_u32; 4]; MAX_SHADOW_VIEWS];
    let mut uv_scales = [[0.0_f32; 4]; MAX_SHADOW_VIEWS];
    let half_norm = |row: [f32; 4]| {
        0.5 * row[..3]
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt()
    };
    for view in &plan.views {
        if let Some(address) = destinations[view.key()] {
            let index = view.key();
            rows[index] = view.rows;
            uv_scales[index] = [half_norm(view.rows[0]), half_norm(view.rows[1]), 0.0, 0.0];
            tiles[index] = match address {
                Address::Native(tile) => [tile.origin[0], tile.origin[1], tile.size, tile.layer],
                Address::Packed(tile) => [tile.offset, tile.stride(), tile.size, u32::MAX],
            };
            indices[view.slot][view.face] = index as i32 + 1;
        }
    }
    append_floats(&mut bytes, rows.into_iter().flatten().flatten());
    bytes.extend(tiles.into_iter().flatten().flat_map(u32::to_ne_bytes));
    append_floats(&mut bytes, uv_scales.into_iter().flatten());
    for (slot, mut indices) in indices.into_iter().enumerate() {
        indices[6] = plan.sizes[slot] as i32;
        bytes.extend(indices.into_iter().flat_map(i32::to_ne_bytes));
        append_floats(&mut bytes, plan.parameters[slot]);
        append_floats(&mut bytes, plan.cascade_ends[slot]);
        append_floats(&mut bytes, plan.cascade_reference[slot]);
    }
    let matrices = buffer(
        device,
        "XRack 3D shadow projections",
        &bytes,
        wgpu::BufferUsages::UNIFORM,
    );
    ShadowAtlas {
        depth,
        packed,
        slots: plan.slots,
        bytes: allocated_bytes,
        matrices,
    }
}

fn clear_pipeline(device: &wgpu::Device, precision: Precision) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack clear shadow tile"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex
fn clear(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(corners[vertex], 1.0, 1.0);
}
"#
            .into(),
        ),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack clear shadow tile"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("clear"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: None,
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format(precision),
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
