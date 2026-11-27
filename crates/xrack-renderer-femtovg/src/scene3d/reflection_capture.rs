//! Scene cubemaps captured and prefiltered entirely on the renderer's GPU queue.
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, OnceLock},
};

use xrack_runtime::{
    Node, RowKey, Value,
    scene_effect::CaptureRead,
    scene3d::{ReflectionCapture, ReflectionProbe, ReflectionRefresh, ReflectionSlicing},
};

use super::{
    HDR_FORMAT, buffer,
    color_capture::{CaptureViews, FrozenCaptureViews},
    effect_history::Owner,
    environment::SKYBOX_PROPERTIES,
    item_textures::{FrozenItemTextures, ItemTextureViews},
};

const MAX_PROBES: usize = 8;
pub(super) const FACES: u32 = 6;

#[derive(Clone)]
pub(crate) struct CapturedCube {
    gpu: Arc<CubeImages>,
    pub view: wgpu::TextureView,
    pub last_mip: u32,
    pub revision: u64,
    pub position: [f32; 3],
}

struct CubeImages {
    texture: wgpu::Texture,
    base: wgpu::TextureView,
    faces: [wgpu::TextureView; FACES as usize],
    filtered: Box<[OnceLock<MipFace>]>,
}

struct MipFace {
    target: wgpu::TextureView,
    input: wgpu::BindGroup,
}

impl CapturedCube {
    fn new(device: &wgpu::Device, size: u32) -> Self {
        let last_mip = size.ilog2();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack reflection capture cubemap"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: FACES,
            },
            mip_level_count: last_mip + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let faces = std::array::from_fn(|face| face_view(&texture, face as u32, 0));
        let base = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            mip_level_count: Some(1),
            ..Default::default()
        });
        Self {
            gpu: Arc::new(CubeImages {
                texture,
                base,
                faces,
                filtered: std::iter::repeat_with(OnceLock::new)
                    .take((last_mip * FACES) as usize)
                    .collect(),
            }),
            view,
            last_mip,
            revision: 0,
            position: [0.0; 3],
        }
    }

    pub fn size(&self) -> u32 {
        self.gpu.texture.width()
    }

    pub fn face(&self, face: u32) -> &wgpu::TextureView {
        &self.gpu.faces[face as usize]
    }
}

fn face_view(texture: &wgpu::Texture, face: u32, level: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_mip_level: level,
        mip_level_count: Some(1),
        base_array_layer: face,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

struct ViewportKey {
    path: Vec<usize>,
    generation: u64,
    scope: [Option<RowKey>; 2],
    id: Option<String>,
}

impl ViewportKey {
    fn new(owner: Owner<'_>) -> Self {
        Self {
            path: owner.path.to_vec(),
            generation: owner.generation,
            scope: owner.scope,
            id: owner.id.map(str::to_owned),
        }
    }

    fn matches(&self, owner: Owner<'_>) -> bool {
        self.path == owner.path
            && self.generation == owner.generation
            && self.scope == owner.scope
            && self.id.as_deref() == owner.id
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProbeBudget {
    faces: u32,
    filters: u32,
    filter_texels: u32,
}

impl ProbeBudget {
    pub fn from_node(node: &Node) -> Self {
        let limit = |name| {
            node.int(name)
                .filter(|value| *value > 0)
                .map_or(u32::MAX, |value| u32::try_from(value).unwrap_or(u32::MAX))
        };
        Self {
            faces: limit("reflection-faces-per-frame"),
            filters: limit("reflection-filter-passes-per-frame"),
            filter_texels: limit("reflection-filter-texels-per-frame"),
        }
    }

    fn limited(self) -> bool {
        self.faces != u32::MAX || self.filters != u32::MAX || self.filter_texels != u32::MAX
    }
}

struct FrameBudget {
    viewport: ViewportKey,
    remaining: ProbeBudget,
}

struct Entry {
    viewport: ViewportKey,
    slot: usize,
    layers: u64,
    published: Option<CaptureImage>,
    pending: Option<CaptureJob>,
    spare: Option<CapturedCube>,
    frame: u64,
    last_work_frame: Option<u64>,
}

impl Entry {
    fn owns_probe(&self, key: (Owner<'_>, usize, u64)) -> bool {
        let (owner, slot, layers) = key;
        self.slot == slot && self.layers == layers && self.viewport.matches(owner)
    }
}

struct CaptureImage {
    capture: ReflectionCapture,
    scene: Node,
    feeds: Vec<(CaptureRead, u64)>,
    items: Vec<(String, u64)>,
    cube: CapturedCube,
}

pub(super) struct ProbeInputs<'a> {
    pub scene: &'a Node,
    pub feeds: &'a CaptureViews,
    pub items: &'a ItemTextureViews,
    pub item_revision: u64,
}

impl CaptureImage {
    fn inputs_match(
        &self,
        sources: &ProbeInputs<'_>,
        settings: ReflectionCapture,
        position: [f32; 3],
    ) -> bool {
        self.capture == settings
            && self.cube.position == position
            && (settings.refresh == ReflectionRefresh::FirstFrame
                || (self
                    .feeds
                    .iter()
                    .map(|(name, revision)| (name, *revision))
                    .eq(sources
                        .feeds
                        .iter()
                        .map(|(name, image)| (name, image.revision)))
                    && self
                        .items
                        .iter()
                        .map(|(name, revision)| (name, *revision))
                        .eq(sources
                            .items
                            .iter()
                            .map(|(name, image)| (name, image.revision)))
                    && same_inputs(&self.scene, sources.scene)))
    }
}

struct CaptureJob {
    image: CaptureImage,
    slicing: ReflectionSlicing,
    captured_faces: u32,
    filtered: u32,
    filter_offset: u32,
    recorded_frame: Option<u64>,
    complete: bool,
    frozen_inputs: Option<Arc<FrozenProbeInputs>>,
}

pub(super) struct FrozenProbeInputs {
    pub feeds: FrozenCaptureViews,
    pub items: FrozenItemTextures,
}

pub(super) struct CaptureStep {
    pub cube: CapturedCube,
    pub scene: Node,
    pub faces: Range<u32>,
    filters: Range<u32>,
    filter_by_face: bool,
    first_filter_texel: u32,
    last_filter_end: Option<u32>,
    pub filter_texels: u64,
    pub complete: bool,
    pub use_feeds: bool,
    pub frozen_inputs: Option<Arc<FrozenProbeInputs>>,
}

#[derive(Default)]
pub(super) struct ProbeFrame {
    pub output: Option<CapturedCube>,
    pub step: Option<CaptureStep>,
    pub needs_redraw: bool,
}

impl CaptureJob {
    fn next(&mut self, frame: u64, budget: &mut ProbeBudget) -> Option<CaptureStep> {
        if self.recorded_frame == Some(frame) || self.complete {
            return None;
        }
        let last_mip = self.image.cube.last_mip;
        let face_limit = if self.slicing == ReflectionSlicing::IndividualFaces {
            1
        } else {
            FACES
        };
        let faces = self.captured_faces
            ..self.captured_faces
                + (FACES - self.captured_faces)
                    .min(face_limit)
                    .min(budget.faces);
        let total_filters = last_mip * FACES;
        let filter_limit = if faces.end == FACES
            && self.filtered < total_filters
            && (faces.is_empty() || self.slicing == ReflectionSlicing::None)
        {
            match self.slicing {
                ReflectionSlicing::None => total_filters - self.filtered,
                ReflectionSlicing::IndividualFaces => last_mip - self.filtered % last_mip,
                ReflectionSlicing::AllFacesAtOnce => FACES - self.filtered % FACES,
            }
        } else {
            0
        };
        let filter_by_face = self.slicing == ReflectionSlicing::IndividualFaces;
        let first_filter = self.filtered;
        let first_filter_texel = self.filter_offset;
        let before = budget.filter_texels;
        let mut passes = 0;
        for _ in 0..filter_limit.min(budget.filters) {
            if budget.filter_texels == 0 {
                break;
            }
            let (level, _) = filter_face(self.filtered, last_mip, filter_by_face);
            let size = (self.image.cube.size() >> level).max(1);
            let face_texels = size * size;
            let texels = (face_texels - self.filter_offset).min(budget.filter_texels);
            budget.filter_texels -= texels;
            passes += 1;
            self.filter_offset += texels;
            if self.filter_offset == face_texels {
                self.filtered += 1;
                self.filter_offset = 0;
            }
        }
        let filters = first_filter..first_filter + passes;
        if faces.is_empty() && filters.is_empty() {
            return None;
        }
        budget.faces -= faces.end - faces.start;
        budget.filters -= passes;
        self.recorded_frame = Some(frame);
        self.captured_faces = faces.end;
        self.complete = self.captured_faces == FACES && self.filtered == total_filters;
        Some(CaptureStep {
            cube: self.image.cube.clone(),
            scene: self.image.scene.clone(),
            faces,
            filters,
            filter_by_face,
            first_filter_texel,
            last_filter_end: (self.filter_offset != 0).then_some(self.filter_offset),
            filter_texels: u64::from(before - budget.filter_texels),
            complete: self.complete,
            use_feeds: !self.image.feeds.is_empty(),
            frozen_inputs: self.frozen_inputs.clone(),
        })
    }
}

fn filter_face(unit: u32, last_mip: u32, by_face: bool) -> (u32, u32) {
    if by_face {
        (1 + unit % last_mip, unit / last_mip)
    } else {
        (1 + unit / FACES, unit % FACES)
    }
}

#[derive(Default)]
pub(super) struct Reflections {
    entries: VecDeque<Entry>,
    filter: Option<Filter>,
    frame: u64,
    revision: u64,
    budgets: Vec<FrameBudget>,
}

impl Reflections {
    pub fn frame_budget(
        &mut self,
        owner: Owner<'_>,
        limits: ProbeBudget,
    ) -> (Option<usize>, ProbeBudget) {
        if let Some(index) = self
            .budgets
            .iter()
            .position(|entry| entry.viewport.matches(owner))
        {
            return (Some(index), self.budgets[index].remaining);
        }
        if !limits.limited() {
            return (None, limits);
        }
        let index = self.budgets.len();
        self.budgets.push(FrameBudget {
            viewport: ViewportKey::new(owner),
            remaining: limits,
        });
        (Some(index), limits)
    }

    pub fn store_budget(&mut self, slot: Option<usize>, remaining: ProbeBudget) {
        if let Some(entry) = slot.and_then(|index| self.budgets.get_mut(index)) {
            entry.remaining = remaining;
        }
    }

    pub fn last_work(&self, key: (Owner<'_>, usize, u64)) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| entry.owns_probe(key))?
            .last_work_frame
    }

    pub fn contains_view(&self, view: &wgpu::TextureView) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .published
                .as_ref()
                .is_some_and(|image| image.cube.view == *view)
                || entry.spare.as_ref().is_some_and(|cube| cube.view == *view)
        })
    }

    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.budgets.clear();
        self.entries
            .retain(|entry| self.frame.wrapping_sub(entry.frame) <= 120);
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        for entry in &mut self.entries {
            let Some(job) = entry.pending.as_mut() else {
                continue;
            };
            if job.recorded_frame != Some(self.frame) {
                continue;
            }
            if !submitted {
                // Incomplete passes may have reached a nested encoder. Start from
                // a fresh set of base faces before letting the cube be sampled.
                entry.spare = entry.pending.take().map(|job| job.image.cube);
            } else if job.complete {
                if let Some(job) = entry.pending.take() {
                    let previous = entry.published.replace(job.image);
                    entry.spare = previous.map(|image| image.cube);
                }
            } else if job.captured_faces == FACES {
                job.frozen_inputs = None;
            }
        }
    }

    pub fn first_frame_ready(
        &self,
        key: (Owner<'_>, usize, u64),
        probe: &ReflectionProbe,
        capture: ReflectionCapture,
    ) -> bool {
        capture.refresh == ReflectionRefresh::FirstFrame
            && self.entries.iter().any(|entry| {
                entry.owns_probe(key)
                    && entry.pending.is_none()
                    && entry.published.as_ref().is_some_and(|image| {
                        image.capture == capture && image.cube.position == probe.position
                    })
            })
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        sources: ProbeInputs<'_>,
        key: (Owner<'_>, usize, u64),
        probe: &ReflectionProbe,
        capture: ReflectionCapture,
        budget: &mut ProbeBudget,
    ) -> ProbeFrame {
        let (viewport, slot, layers) = key;
        let same_scope = |entry: &Entry| {
            entry.viewport.generation == viewport.generation && entry.layers == layers
        };
        let same_slot = |entry: &Entry| entry.owns_probe(key);
        let owner = self.entries.iter().position(same_slot);
        let working = owner.is_some_and(|index| self.entries[index].pending.is_some());
        if !working && capture.refresh != ReflectionRefresh::EveryFrame {
            let index = self.entries.iter().position(|entry| {
                same_scope(entry)
                    && (capture.refresh != ReflectionRefresh::FirstFrame || same_slot(entry))
                    && entry
                        .published
                        .as_ref()
                        .is_some_and(|image| image.inputs_match(&sources, capture, probe.position))
            });
            if let Some(mut entry) = index.and_then(|index| self.entries.remove(index)) {
                entry.frame = self.frame;
                let output = entry.published.as_ref().map(|image| image.cube.clone());
                self.entries.push_front(entry);
                return ProbeFrame {
                    output,
                    ..ProbeFrame::default()
                };
            }
        }
        if !working && budget.faces == 0 {
            let output = owner.and_then(|index| {
                let entry = self.entries.get_mut(index)?;
                entry.frame = self.frame;
                entry.published.as_ref().map(|image| image.cube.clone())
            });
            return ProbeFrame {
                output,
                needs_redraw: true,
                ..ProbeFrame::default()
            };
        }
        let mut entry = if let Some(entry) = owner.and_then(|index| self.entries.remove(index)) {
            entry
        } else {
            if self.entries.len() >= MAX_PROBES {
                let Some(index) = self
                    .entries
                    .iter()
                    .rposition(|entry| entry.frame != self.frame)
                else {
                    return ProbeFrame::default();
                };
                self.entries.remove(index);
            }
            Entry {
                viewport: ViewportKey::new(viewport),
                slot,
                layers,
                published: None,
                pending: None,
                spare: None,
                frame: self.frame,
                last_work_frame: None,
            }
        };
        entry.frame = self.frame;
        let job = entry.pending.get_or_insert_with(|| {
            let mut cube = entry
                .spare
                .take()
                .filter(|cube| cube.size() == capture.size)
                .unwrap_or_else(|| CapturedCube::new(device, capture.size));
            self.revision = self.revision.wrapping_add(1);
            cube.revision = self.revision;
            cube.position = probe.position;
            let slicing = capture.slicing;
            CaptureJob {
                image: CaptureImage {
                    capture,
                    scene: sources.scene.clone(),
                    feeds: sources
                        .feeds
                        .iter()
                        .map(|(name, image)| (name.clone(), image.revision))
                        .collect(),
                    items: sources
                        .items
                        .iter()
                        .map(|(name, image)| (name.clone(), image.revision))
                        .collect(),
                    cube,
                },
                slicing,
                captured_faces: 0,
                filtered: 0,
                filter_offset: 0,
                recorded_frame: None,
                complete: false,
                frozen_inputs: (slicing == ReflectionSlicing::IndividualFaces
                    || budget.faces < FACES)
                    .then(|| {
                        Arc::new(FrozenProbeInputs {
                            feeds: FrozenCaptureViews::new(sources.feeds),
                            items: FrozenItemTextures::new(sources.items, sources.item_revision),
                        })
                    }),
            }
        });
        let mut output = entry.published.as_ref().map(|image| image.cube.clone());
        let step = job.next(self.frame, budget);
        if step.is_some() {
            entry.last_work_frame = Some(self.frame);
        }
        if job.complete {
            output = Some(job.image.cube.clone());
        }
        let needs_redraw =
            !job.complete || !job.image.inputs_match(&sources, capture, probe.position);
        self.entries.push_front(entry);
        ProbeFrame {
            output,
            step,
            needs_redraw,
        }
    }

    pub fn prefilter(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        step: &CaptureStep,
    ) -> u64 {
        if step.filters.is_empty() {
            return 0;
        }
        let filter = self.filter.get_or_insert_with(|| Filter::new(device));
        for unit in step.filters.clone() {
            let (level, face) = filter_face(unit, step.cube.last_mip, step.filter_by_face);
            let size = (step.cube.size() >> level).max(1);
            let first = if unit == step.filters.start {
                step.first_filter_texel
            } else {
                0
            };
            let end = if unit + 1 == step.filters.end {
                step.last_filter_end.unwrap_or(size * size)
            } else {
                size * size
            };
            let full_face = first == 0 && end == size * size;
            let index = ((level - 1) * FACES + face) as usize;
            let mip = step.cube.gpu.filtered[index]
                .get_or_init(|| filter.mip_face(device, &step.cube, level, face));
            let mut pass = crate::viewport_gpu_timing::render_pass(
                encoder,
                &wgpu::RenderPassDescriptor {
                    label: Some("XRack reflection prefilter roughness mip"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mip.target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: if full_face {
                                wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                },
            );
            pass.set_pipeline(&filter.pipeline);
            pass.set_bind_group(0, &mip.input, &[]);
            if full_face {
                pass.draw(0..3, 0..1);
            } else {
                // Draw whole rows together and clip the two partial rows
                // independently. Fragment coordinates remain face-relative.
                let mut pixel = first;
                while pixel < end {
                    let (x, y) = (pixel % size, pixel / size);
                    let remaining = end - pixel;
                    let (width, height) = if x == 0 && remaining >= size {
                        (size, remaining / size)
                    } else {
                        ((size - x).min(remaining), 1)
                    };
                    pass.set_scissor_rect(x, y, width, height);
                    pass.draw(0..3, 0..1);
                    pixel += width * height;
                }
            }
        }
        step.filters.len() as u64
    }
}

impl Filter {
    fn mip_face(
        &self,
        device: &wgpu::Device,
        cube: &CapturedCube,
        level: u32,
        face: u32,
    ) -> MipFace {
        let size = (cube.size() >> level).max(1);
        let roughness = level as f32 / cube.last_mip as f32;
        let words = [face, size, roughness.to_bits(), 0];
        let bytes: Vec<_> = words.into_iter().flat_map(u32::to_ne_bytes).collect();
        let settings = buffer(
            device,
            "XRack reflection prefilter face",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let input = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack reflection prefilter inputs"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cube.gpu.base),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: settings.as_entire_binding(),
                },
            ],
        });
        MipFace {
            target: face_view(&cube.gpu.texture, face, level),
            input,
        }
    }
}

pub(super) fn same_inputs(a: &Node, b: &Node) -> bool {
    a.properties.len() == b.properties.len()
        && a.properties
            .iter()
            .zip(&b.properties)
            .all(|((a_name, a), (b_name, b))| {
                a_name == b_name
                    && match (a, b) {
                        (Value::SceneObjects(a), Value::SceneObjects(b)) => Arc::ptr_eq(a, b),
                        _ => a == b,
                    }
            })
}

/// Capture inputs have no dependencies on the observing camera or screen effects.
pub(super) fn capture_node(view: &Node, layers: u64) -> Node {
    let mut scene = Node::new("SceneView3D", None);
    for name in [
        "model",
        "lod-fade-width",
        "lights",
        "imported-lights",
        "environment",
        "environment-sky",
        "environment-yaw",
        "environment-pitch",
        "environment-roll",
        "environment-horizon",
        "environment-intensity",
        "environment-skybox",
        "ambient-color",
        "ambient-intensity",
        "light-yaw",
        "light-elevation",
        "scene-background",
        "shadows",
        "shadow-map-size",
        "shadow-map-precision",
        "shadow-bias",
        "order-independent-transparency",
    ]
    .into_iter()
    .chain(SKYBOX_PROPERTIES)
    {
        if let Some(value) = view.properties.get(name) {
            scene.properties.insert(name, value.clone());
        }
    }
    if let Some(Value::SceneParticleEmitters(emitters)) = view.properties.get("particle-emitters") {
        let casts = |emitter: &xrack_runtime::scene3d::ParticleEmitter| {
            emitter.layers & layers != 0
                && emitter.casts_reflections
                && (emitter.follow_emitter.is_empty() || emitter.follow_source(emitters).is_some())
        };
        let sources: Vec<_> = emitters
            .iter()
            .filter(|emitter| casts(emitter))
            .filter_map(|emitter| emitter.follow_source(emitters))
            .collect();
        let reflected: Arc<[_]> = emitters
            .iter()
            .filter(|emitter| {
                casts(emitter) || sources.iter().any(|source| std::ptr::eq(*emitter, *source))
            })
            .cloned()
            .collect();
        if !reflected.is_empty() {
            // Cubemap capture supports ordinary blending and weighted accumulation.
            // Linked-list transparency uses the weighted path on capture targets.
            if reflected
                .iter()
                .any(|emitter| casts(emitter) && emitter.model.is_none())
                && let Some(mode) = view.text("oit-method")
            {
                scene.properties.insert(
                    "order-independent-transparency",
                    Value::Bool(matches!(mode, "weighted" | "linked-list")),
                );
            }
            if let Some(time) = view.properties.get("particle-time") {
                scene.properties.insert("particle-time", time.clone());
            }
            if let Some(Value::SceneParticleAffectors(affectors)) =
                view.properties.get("particle-affectors")
            {
                scene.properties.insert(
                    "particle-affectors",
                    Value::SceneParticleAffectors(
                        affectors
                            .iter()
                            .filter(|affector| {
                                reflected.iter().any(|emitter| {
                                    let mask = if emitter.layers & layers == 0 {
                                        emitter.layers
                                    } else {
                                        layers
                                    };
                                    affector.matches_emitter(emitter, mask)
                                })
                            })
                            .cloned()
                            .collect(),
                    ),
                );
            }
            scene
                .properties
                .insert("particle-emitters", Value::SceneParticleEmitters(reflected));
        }
    }
    scene
}

struct Filter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Filter {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack cubemap GGX prefilter"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("cube_direction.wgsl"),
                    "\n",
                    include_str!("reflection_filter.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack cubemap GGX prefilter"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = pipeline.get_bind_group_layout(0);
        Self {
            pipeline,
            layout,
            sampler,
        }
    }
}
