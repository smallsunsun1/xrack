//! Scene cubemaps captured and prefiltered entirely on the renderer's GPU queue.
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, OnceLock},
};

use xrack_runtime::{
    Node, Value,
    scene_effect::CaptureRead,
    scene3d::{ReflectionCapture, ReflectionProbe, ReflectionRefresh, ReflectionSlicing},
};

use super::{HDR_FORMAT, buffer};

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

struct Entry {
    path: Vec<usize>,
    slot: usize,
    generation: u64,
    layers: u64,
    published: Option<CaptureImage>,
    pending: Option<CaptureJob>,
    spare: Option<CapturedCube>,
    frame: u64,
}

struct CaptureImage {
    capture: ReflectionCapture,
    scene: Node,
    feeds: Vec<(CaptureRead, u64)>,
    cube: CapturedCube,
}

pub(super) struct ProbeInputs<'a> {
    pub scene: &'a Node,
    pub feeds: &'a [(CaptureRead, u64)],
    pub live_items: bool,
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
                || (self.feeds == sources.feeds && same_inputs(&self.scene, sources.scene)))
    }
}

struct CaptureJob {
    image: CaptureImage,
    slicing: ReflectionSlicing,
    step: u32,
    recorded_frame: Option<u64>,
    complete: bool,
}

pub(super) struct CaptureStep {
    pub cube: CapturedCube,
    pub scene: Node,
    pub faces: Range<u32>,
    pub levels: Range<u32>,
    pub filter_faces: Range<u32>,
    pub complete: bool,
    pub use_feeds: bool,
}

#[derive(Default)]
pub(super) struct ProbeFrame {
    pub output: Option<CapturedCube>,
    pub step: Option<CaptureStep>,
    pub needs_redraw: bool,
}

impl CaptureJob {
    fn next(&mut self, frame: u64) -> CaptureStep {
        let last_mip = self.image.cube.last_mip;
        let (faces, levels, filter_faces, complete) = match self.slicing {
            ReflectionSlicing::None => (0..FACES, 1..last_mip + 1, 0..FACES, true),
            ReflectionSlicing::IndividualFaces if self.step < FACES => {
                (self.step..self.step + 1, 0..0, 0..0, false)
            }
            ReflectionSlicing::IndividualFaces => {
                let face = self.step - FACES;
                (0..0, 1..last_mip + 1, face..face + 1, face + 1 == FACES)
            }
            ReflectionSlicing::AllFacesAtOnce if self.step == 0 => (0..FACES, 0..0, 0..0, false),
            ReflectionSlicing::AllFacesAtOnce => (
                0..0,
                self.step..self.step + 1,
                0..FACES,
                self.step == last_mip,
            ),
        };
        self.recorded_frame = Some(frame);
        self.step += 1;
        self.complete = complete;
        CaptureStep {
            cube: self.image.cube.clone(),
            scene: self.image.scene.clone(),
            faces,
            levels,
            filter_faces,
            complete,
            use_feeds: !self.image.feeds.is_empty(),
        }
    }
}

#[derive(Default)]
pub(super) struct Reflections {
    entries: VecDeque<Entry>,
    filter: Option<Filter>,
    frame: u64,
    revision: u64,
}

impl Reflections {
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
        self.entries
            .retain(|entry| self.frame.wrapping_sub(entry.frame) <= 120);
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        for entry in &mut self.entries {
            let Some(job) = entry.pending.as_ref() else {
                continue;
            };
            if job.recorded_frame != Some(self.frame) {
                continue;
            }
            if !submitted {
                // Incomplete passes may have reached a nested encoder. Start from
                // a fresh set of base faces before letting the cube be sampled.
                entry.spare = entry.pending.take().map(|job| job.image.cube);
            } else if job.complete
                && let Some(job) = entry.pending.take()
            {
                let previous = entry.published.replace(job.image);
                entry.spare = previous.map(|image| image.cube);
            }
        }
    }

    pub fn first_frame_ready(
        &self,
        (path, slot, generation): (&[usize], usize, u64),
        probe: &ReflectionProbe,
        capture: ReflectionCapture,
        layers: u64,
    ) -> bool {
        capture.refresh == ReflectionRefresh::FirstFrame
            && self.entries.iter().any(|entry| {
                entry.path == path
                    && entry.slot == slot
                    && entry.generation == generation
                    && entry.layers == layers
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
        (path, slot, generation): (&[usize], usize, u64),
        probe: &ReflectionProbe,
        capture: ReflectionCapture,
        layers: u64,
    ) -> ProbeFrame {
        let same_scope = |entry: &Entry| entry.generation == generation && entry.layers == layers;
        let same_slot =
            |entry: &Entry| same_scope(entry) && entry.path == path && entry.slot == slot;
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
                path: path.to_vec(),
                slot,
                generation,
                layers,
                published: None,
                pending: None,
                spare: None,
                frame: self.frame,
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
            CaptureJob {
                image: CaptureImage {
                    capture,
                    scene: sources.scene.clone(),
                    feeds: sources.feeds.to_vec(),
                    cube,
                },
                // Sample a coherent source frame for all six directions.
                slicing: match capture.slicing {
                    ReflectionSlicing::IndividualFaces
                        if sources.live_items || !sources.feeds.is_empty() =>
                    {
                        ReflectionSlicing::AllFacesAtOnce
                    }
                    slicing => slicing,
                },
                step: 0,
                recorded_frame: None,
                complete: false,
            }
        });
        let mut output = entry.published.as_ref().map(|image| image.cube.clone());
        let step = (job.recorded_frame != Some(self.frame)).then(|| job.next(self.frame));
        if step.is_some() && job.complete {
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
        if step.levels.is_empty() || step.filter_faces.is_empty() {
            return 0;
        }
        let filter = self.filter.get_or_insert_with(|| Filter::new(device));
        for level in step.levels.clone() {
            for face in step.filter_faces.clone() {
                let index = ((level - 1) * FACES + face) as usize;
                let mip = step.cube.gpu.filtered[index]
                    .get_or_init(|| filter.mip_face(device, &step.cube, level, face));
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("XRack reflection prefilter roughness mip"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &mip.target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&filter.pipeline);
                pass.set_bind_group(0, &mip.input, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        (step.levels.len() * step.filter_faces.len()) as u64
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
        "lights",
        "imported-lights",
        "environment",
        "environment-yaw",
        "environment-intensity",
        "environment-skybox",
        "ambient-color",
        "ambient-intensity",
        "light-yaw",
        "light-elevation",
        "scene-background",
        "shadows",
        "shadow-map-size",
        "shadow-bias",
        "order-independent-transparency",
    ] {
        if let Some(value) = view.properties.get(name) {
            scene.properties.insert(name, value.clone());
        }
    }
    if let Some(Value::SceneParticleEmitters(emitters)) = view.properties.get("particle-emitters") {
        let eligible = |emitter: &&xrack_runtime::scene3d::ParticleEmitter| {
            emitter.layers & layers != 0
                && emitter
                    .model
                    .as_ref()
                    .is_some_and(|model| model.casts_reflections)
        };
        let reflected_layers = emitters
            .iter()
            .filter(eligible)
            .fold(0, |mask, emitter| mask | emitter.layers);
        if reflected_layers != 0 {
            scene.properties.insert(
                "particle-emitters",
                Value::SceneParticleEmitters(emitters.iter().filter(eligible).cloned().collect()),
            );
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
                            .copied()
                            .filter(|affector| affector.layers & reflected_layers & layers != 0)
                            .collect(),
                    ),
                );
            }
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
            source: wgpu::ShaderSource::Wgsl(include_str!("reflection_filter.wgsl").into()),
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
