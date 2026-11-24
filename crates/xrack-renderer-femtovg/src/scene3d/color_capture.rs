//! Named, viewport-local, scene-linear inputs to the effect chain.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    sync::Arc,
};

use xrack_runtime::{
    Color, Node, RowKey, Value,
    scene_effect::{
        CaptureAttachment, CaptureInputs, CapturePipelineState, CaptureRead, CaptureRenderables,
        MAX_BUFFERS,
    },
    scene3d::{CaptureMaterial, Mesh3D, ReflectionSource, SceneCamera, SceneModel},
};

use super::{
    Attachment, HDR_FORMAT, append_floats, attachment_view, buffer,
    capture_region::CaptureRegion,
    capture_stencil,
    custom_effect::{effect_format, effect_size, texture_bytes},
    effect_history::Owner,
    material::shader_texture_slots,
    particles::ParticleDraw,
    reflection_capture::{CapturedCube, same_inputs},
    retained_view::Sampling,
};

pub(super) type CaptureViews = BTreeMap<CaptureRead, CapturedTexture>;

#[derive(Clone)]
pub(super) struct CapturedTexture {
    pub view: wgpu::TextureView,
    pub allocation: u64,
    pub revision: u64,
    pub projection: Option<Arc<CaptureProjection>>,
}

pub(super) struct CaptureProjection {
    pub rows: [[f32; 4]; 4],
    pub revision: u64,
}

pub(super) type SharedProjectionBuffers = Rc<RefCell<ProjectionBuffers>>;

#[derive(Default)]
pub(super) struct ProjectionBuffers {
    entries: VecDeque<ProjectionBuffer>,
    pub uploads: u64,
}

struct ProjectionBuffer {
    revisions: [u64; MAX_BUFFERS],
    count: usize,
    gpu: wgpu::Buffer,
}

impl ProjectionBuffers {
    pub fn get<'a>(
        &mut self,
        device: &wgpu::Device,
        count: usize,
        projections: impl Iterator<Item = Option<&'a CaptureProjection>>,
    ) -> wgpu::Buffer {
        let mut projections = projections.chain(std::iter::repeat(None)).take(count);
        let slots: [_; MAX_BUFFERS] = std::array::from_fn(|_| projections.next().flatten());
        let revisions = slots.map(|slot| slot.map_or(0, |projection| projection.revision));
        let index = self
            .entries
            .iter()
            .position(|entry| entry.count == count && entry.revisions == revisions);
        let entry = index
            .and_then(|index| self.entries.remove(index))
            .unwrap_or_else(|| {
                let mut bytes = Vec::with_capacity(count * 64);
                for projection in slots.into_iter().take(count) {
                    let rows = projection.map_or([[0.0; 4]; 4], |projection| projection.rows);
                    append_floats(&mut bytes, rows.into_iter().flatten());
                }
                self.uploads += 1;
                ProjectionBuffer {
                    revisions,
                    count,
                    gpu: buffer(
                        device,
                        "XRack captured camera inverse clips",
                        &bytes,
                        wgpu::BufferUsages::UNIFORM,
                    ),
                }
            });
        let gpu = entry.gpu.clone();
        self.entries.truncate(31);
        self.entries.push_front(entry);
        gpu
    }
}

const IDLE_FRAMES: u64 = 120;
const BYTE_BUDGET: u64 = 128 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ColorCaptures {
    entries: VecDeque<Entry>,
    frame: u64,
    next_slot: usize,
    next_allocation: u64,
    next_revision: u64,
}

struct Entry {
    path: Vec<usize>,
    generation: u64,
    scope: [Option<RowKey>; 2],
    id: Option<String>,
    name: String,
    slot: usize,
    frame: u64,
    input: CaptureInput,
    images: [Option<(Attachment, u64)>; CaptureAttachment::ALL.len()],
    projection: Option<Arc<CaptureProjection>>,
    revision: u64,
    valid: bool,
    recorded: bool,
}

pub(super) struct CaptureInput {
    pub camera: SceneCamera,
    pub scene: Node,
    pub size: [u32; 2],
    pub color_format: wgpu::TextureFormat,
    pub cubes: Vec<Option<u64>>,
    pub dependencies: Vec<(CaptureRead, u64)>,
    pub composition: CaptureInputs,
    pub override_material: Option<Arc<CaptureMaterial>>,
    pub renderables: CaptureRenderables,
    pub pipeline_state: CapturePipelineState,
    pub region: CaptureRegion,
    pub depth_clear: f32,
    pub stencil_clear: u8,
    pub attachments: u8,
}

pub(super) struct CaptureWork {
    pub slot: usize,
    pub views: [Option<CapturedTexture>; CaptureAttachment::ALL.len()],
    pub render: bool,
    pub allocated_bytes: u64,
}

impl ColorCaptures {
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        let mut bytes = 0u64;
        self.entries.retain_mut(|entry| {
            if std::mem::take(&mut entry.recorded) {
                entry.valid = submitted;
            }
            let storage: u64 = entry
                .images
                .iter()
                .flatten()
                .map(|(image, _)| texture_bytes(image.size, image.view.texture().format()))
                .sum();
            let next_bytes = bytes.saturating_add(storage);
            if self.frame.wrapping_sub(entry.frame) > IDLE_FRAMES || next_bytes > BYTE_BUDGET {
                return false;
            }
            bytes = next_bytes;
            true
        });
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        owner: Owner<'_>,
        name: &str,
        input: CaptureInput,
    ) -> CaptureWork {
        let previous = self
            .entries
            .iter()
            .position(|entry| {
                entry.path == owner.path
                    && entry.generation == owner.generation
                    && entry.scope == owner.scope
                    && entry.id.as_deref() == owner.id
                    && entry.name == name
                    && geometry_attachments(entry.input.attachments)
                        == geometry_attachments(input.attachments)
            })
            .and_then(|index| self.entries.remove(index));
        let mut allocated_bytes = 0;
        let next_allocation = &mut self.next_allocation;
        let mut create_image = |size, format| {
            *next_allocation = next_allocation.wrapping_add(1);
            allocated_bytes += texture_bytes(size, format);
            (
                Attachment {
                    size,
                    view: attachment_view(
                        device,
                        size,
                        format,
                        1,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | if format == capture_stencil::FORMAT {
                                wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST
                            } else {
                                wgpu::TextureUsages::empty()
                            },
                        "XRack named scene capture",
                    ),
                },
                *next_allocation,
            )
        };
        let mut entry = previous.unwrap_or_else(|| {
            let slot = self.next_slot;
            self.next_slot = self.next_slot.wrapping_add(1);
            Entry {
                path: owner.path.to_vec(),
                generation: owner.generation,
                scope: owner.scope,
                id: owner.id.map(str::to_owned),
                name: name.to_owned(),
                slot,
                frame: self.frame,
                input: CaptureInput {
                    scene: input.scene.clone(),
                    cubes: Vec::new(),
                    dependencies: Vec::new(),
                    composition: CaptureInputs::default(),
                    override_material: None,
                    ..input
                },
                images: Default::default(),
                projection: None,
                revision: 0,
                valid: false,
                recorded: false,
            }
        });
        let input_mask = input.attachments;
        let render = !(entry.valid || entry.recorded)
            || entry.input.camera != input.camera
            || entry.input.cubes != input.cubes
            || entry.input.dependencies != input.dependencies
            || entry.input.composition != input.composition
            || entry.input.override_material != input.override_material
            || entry.input.pipeline_state != input.pipeline_state
            || entry.input.region != input.region
            || entry.input.depth_clear != input.depth_clear
            || entry.input.stencil_clear != input.stencil_clear
            || entry.input.renderables != input.renderables
            || entry.input.size != input.size
            || entry.input.color_format != input.color_format
            || input.attachments & !entry.input.attachments != 0
            || !same_inputs(&entry.input.scene, &input.scene);
        if render {
            for attachment in CaptureAttachment::ALL {
                let slot = &mut entry.images[attachment.index()];
                if input.attachments & (1 << attachment.index()) == 0 {
                    // An attachment not updated with the other outputs is stale.
                    *slot = None;
                } else {
                    let format = if attachment == CaptureAttachment::Color {
                        input.color_format
                    } else {
                        attachment_format(attachment)
                    };
                    if slot.as_ref().is_none_or(|(image, _)| {
                        image.size != input.size || image.view.texture().format() != format
                    }) {
                        *slot = Some(create_image(input.size, format));
                    }
                }
            }
            self.next_revision = self.next_revision.wrapping_add(1);
            entry.revision = self.next_revision;
            if (input.region.is_empty() && input.depth_clear == 1.0)
                || input_mask & (1 << CaptureAttachment::Depth.index()) == 0
            {
                entry.projection = None;
            } else if entry.projection.is_none() || !entry.input.camera.same_frustum(&input.camera)
            {
                entry.projection = input.camera.inverse_clip_rows().map(|rows| {
                    entry
                        .projection
                        .as_ref()
                        .filter(|projection| projection.rows == rows)
                        .cloned()
                        .unwrap_or_else(|| {
                            Arc::new(CaptureProjection {
                                rows,
                                revision: entry.revision,
                            })
                        })
                });
            }
            entry.input = input;
            entry.valid = false;
            entry.recorded = true;
        }
        entry.frame = self.frame;
        let work = CaptureWork {
            slot: entry.slot,
            views: std::array::from_fn(|index| {
                if input_mask & (1 << index) == 0 {
                    return None;
                }
                let (image, allocation) = entry.images[index].as_ref()?;
                Some(CapturedTexture {
                    view: image.view.clone(),
                    allocation: *allocation,
                    revision: entry.revision,
                    projection: if index == CaptureAttachment::Depth.index() {
                        entry.projection.clone()
                    } else {
                        None
                    },
                })
            }),
            render,
            allocated_bytes,
        };
        self.entries.push_front(entry);
        work
    }
}

pub(super) fn attachment_format(attachment: CaptureAttachment) -> wgpu::TextureFormat {
    match attachment {
        CaptureAttachment::Depth => wgpu::TextureFormat::R32Float,
        CaptureAttachment::Stencil => capture_stencil::FORMAT,
        _ => HDR_FORMAT,
    }
}

fn geometry_attachments(mask: u8) -> bool {
    let color_pass =
        (1 << CaptureAttachment::Color.index()) | (1 << CaptureAttachment::Stencil.index());
    mask & color_pass == 0
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct CaptureTarget {
    pub camera: SceneCamera,
    pub size: [u32; 2],
    pub background: Color,
    pub color_format: wgpu::TextureFormat,
    pub geometry: bool,
    pub renderables: CaptureRenderables,
    pub pipeline_state: CapturePipelineState,
    pub region: CaptureRegion,
    pub depth_clear: f32,
    pub stencil_clear: u8,
}

impl CaptureTarget {
    pub fn raster_camera(self) -> SceneCamera {
        self.region.camera(self.camera)
    }
}

pub(super) struct CaptureStage {
    pub target: CaptureTarget,
    pub outputs: Vec<CaptureRead>,
    pub composition: CaptureInputs,
    pub override_material: Option<Arc<CaptureMaterial>>,
    reads: Vec<(CaptureRead, usize)>,
}

impl CaptureStage {
    pub fn attachments(&self) -> u8 {
        self.outputs
            .iter()
            .fold(0, |mask, output| mask | (1 << output.attachment.index()))
    }

    fn samples_materials(&self) -> bool {
        !self.composition.is_empty()
            || self
                .outputs
                .iter()
                .any(|output| output.attachment != CaptureAttachment::Depth)
    }

    pub fn inputs(&self, ready: &CaptureViews) -> CaptureViews {
        self.reads
            .iter()
            .filter_map(|(name, _)| Some((name.clone(), ready.get(name)?.clone())))
            .collect()
    }
}

#[derive(Default)]
pub(super) struct CapturePlan {
    pub stages: Vec<CaptureStage>,
    pub feedback_edges: u64,
}

impl CapturePlan {
    pub fn for_view(
        node: &Node,
        device: &wgpu::Device,
        size: [u32; 2],
        camera: Option<SceneCamera>,
        additional: &BTreeSet<CaptureRead>,
    ) -> Self {
        let Some(Value::SceneCaptures(captures)) = node.properties.get("scene-captures") else {
            return Self::default();
        };
        if captures.is_empty() {
            return Self::default();
        }
        let model = match node.properties.get("model") {
            Some(Value::SceneObjects(model)) => Some(model.as_ref()),
            _ => None,
        };
        let viewport_camera = node.text("camera").unwrap_or_default();
        let target_for = |read: &CaptureRead| {
            let feed = captures.get(read.name.as_ref())?;
            let geometry = !matches!(
                read.attachment,
                CaptureAttachment::Color | CaptureAttachment::Stencil
            );
            if read.attachment == CaptureAttachment::Depth
                && feed.pipeline_state.depth_write == Some(false)
            {
                return None;
            }
            let draws_geometry = feed.renderables.includes(false)
                && feed.override_material.as_ref().is_none_or(|style| {
                    let material = style.material();
                    !material.is_blended()
                        && material.transmission_blur().is_none()
                        && (material.depth_coverage().is_some()
                            || feed.pipeline_state.depth_write == Some(true))
                });
            if geometry
                && !draws_geometry
                && !(read.attachment == CaptureAttachment::Depth && feed.depth_clear != 1.0)
            {
                return None;
            }
            let renderables = if !geometry {
                feed.renderables
            } else if draws_geometry {
                CaptureRenderables::Opaque
            } else {
                CaptureRenderables::None
            };
            let mut pipeline_state = feed.pipeline_state;
            if renderables == CaptureRenderables::None {
                pipeline_state = CapturePipelineState::default();
            } else if geometry {
                pipeline_state.blend = None;
                pipeline_state.blend_constant = None;
                pipeline_state.color_write_mask = 15;
            }
            let mut size = effect_size(size, feed.scale);
            // The viewport size is already cropped to its camera's content.
            // Reuse that frustum so render-scale rounding changes only the
            // texture dimensions, never the world rays of the same camera.
            let camera = if feed.camera.is_empty() || feed.camera == viewport_camera {
                camera?
            } else {
                let camera = model?.camera(&feed.camera, size.map(f64::from))?;
                let [_, _, width, height] = camera.content_rect();
                size = [width, height].map(|side| side.round().max(1.0) as u32);
                camera
            }
            .with_layer_mask(feed.layers);
            Some(CaptureTarget {
                camera,
                size,
                geometry,
                pipeline_state,
                region: if renderables == CaptureRenderables::None {
                    CaptureRegion::Full
                } else {
                    CaptureRegion::new(feed.raster, size)
                },
                renderables,
                depth_clear: feed.depth_clear,
                stencil_clear: feed.stencil_clear,
                background: if geometry { Color(0) } else { feed.background },
                color_format: if geometry {
                    HDR_FORMAT
                } else {
                    effect_format(feed.color_format)
                },
            })
        };
        let depth_matches = |read: &CaptureRead, camera: SceneCamera| {
            target_for(read).is_some_and(|target| {
                (!target.region.is_empty() || target.depth_clear != 1.0)
                    && target.raster_camera().clip_rows() == camera.clip_rows()
            })
        };
        let mut roots = camera.map_or_else(BTreeSet::new, |camera| {
            material_sources(
                node,
                model,
                device,
                camera,
                (false, CaptureRenderables::All),
                None,
            )
        });
        if let Some(Value::SceneEffects(effects)) = node.properties.get("scene-effects") {
            roots.extend(effects.capture_sources().cloned());
        }
        if let Some(camera) = camera {
            let inputs = super::composite_input::capture_reads(node);
            roots.extend(inputs.color);
            roots.extend(inputs.depth.filter(|read| depth_matches(read, camera)));
        }
        roots.extend(additional.iter().cloned());
        let mut stages = Vec::<CaptureStage>::new();
        let mut discovered = BTreeMap::<CaptureRead, Option<usize>>::new();
        let mut discover =
            |read: CaptureRead, stages: &mut Vec<CaptureStage>, pending: &mut VecDeque<usize>| {
                *discovered.entry(read.clone()).or_insert_with(|| {
                    let target = target_for(&read)?;
                    let inputs = &captures.get(read.name.as_ref())?.inputs;
                    let composition = CaptureInputs {
                        color: if target.geometry {
                            None
                        } else {
                            inputs
                                .color
                                .clone()
                                .filter(|source| target_for(source).is_some())
                        },
                        depth: if target.geometry {
                            None
                        } else {
                            inputs
                                .depth
                                .clone()
                                .filter(|source| depth_matches(source, target.raster_camera()))
                        },
                        stencil: inputs.stencil.clone().filter(|source| {
                            target_for(source).is_some_and(|producer| producer.size == target.size)
                                && capture_stencil::can_copy(device, target.size)
                        }),
                    };
                    let override_material = (target.renderables != CaptureRenderables::None)
                        .then(|| captures.get(read.name.as_ref())?.override_material.clone())
                        .flatten();
                    // A stage that declares inputs keeps its name while resolving
                    // cycles. Merging a self-referencing stage with a consumer
                    // could incorrectly turn the consumer's valid read into a loop.
                    let matching = if composition.is_empty() && override_material.is_none() {
                        stages.iter().position(|stage| {
                            stage.target == target
                                && stage.composition.is_empty()
                                && stage.override_material == override_material
                        })
                    } else {
                        None
                    };
                    let index = if let Some(index) = matching {
                        let stage = &mut stages[index];
                        let before = stage.samples_materials();
                        stage.outputs.push(read);
                        if !before && stage.samples_materials() {
                            pending.push_back(index);
                        }
                        index
                    } else {
                        let index = stages.len();
                        let stage = CaptureStage {
                            target,
                            outputs: vec![read],
                            composition,
                            override_material,
                            reads: Vec::new(),
                        };
                        if stage.samples_materials() {
                            pending.push_back(index);
                        }
                        stages.push(stage);
                        index
                    };
                    Some(index)
                })
            };
        let mut pending = VecDeque::new();
        let root_stages: Vec<_> = roots
            .iter()
            .cloned()
            .filter_map(|read| discover(read, &mut stages, &mut pending))
            .collect();
        while let Some(index) = pending.pop_front() {
            let target = stages[index].target;
            let mut dependencies = if target.region.is_empty() {
                BTreeSet::new()
            } else {
                material_sources(
                    node,
                    model,
                    device,
                    target.camera,
                    (false, target.renderables),
                    stages[index].override_material.as_ref(),
                )
            };
            dependencies.extend(stages[index].composition.iter().cloned());
            let reads = dependencies
                .into_iter()
                .filter_map(|read| Some((read.clone(), discover(read, &mut stages, &mut pending)?)))
                .collect();
            stages[index].reads = reads;
        }
        let (order, feedback_edges) = capture_order(&mut stages, &root_stages);
        let mut active = roots;
        for &index in &order {
            active.extend(stages[index].reads.iter().map(|(read, _)| read.clone()));
        }
        for &index in &order {
            stages[index].outputs.retain(|read| active.contains(read));
        }
        let mut stages: Vec<_> = stages.into_iter().map(Some).collect();
        let mut ordered = Vec::<CaptureStage>::with_capacity(order.len());
        for index in order {
            let Some(mut stage) = stages[index].take() else {
                continue;
            };
            if (!stage.composition.is_empty() || stage.override_material.is_some())
                && let Some(previous) = ordered.iter_mut().find(|previous| {
                    previous.target == stage.target
                        && previous.composition == stage.composition
                        && previous.override_material == stage.override_material
                        && previous.reads == stage.reads
                })
            {
                previous.outputs.append(&mut stage.outputs);
            } else {
                ordered.push(stage);
            }
        }
        Self {
            stages: ordered,
            feedback_edges,
        }
    }

    /// Pull the named inputs and their producers into the probe phase, keeping
    /// the relative order within both phases.
    pub fn take_dependencies(&mut self, roots: &BTreeSet<CaptureRead>) -> Vec<CaptureStage> {
        let mut needed = roots.clone();
        let mut early = vec![false; self.stages.len()];
        for (index, stage) in self.stages.iter().enumerate().rev() {
            if stage.outputs.iter().any(|read| needed.contains(read)) {
                early[index] = true;
                needed.extend(stage.reads.iter().map(|(name, _)| name.clone()));
            }
        }
        let (early, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.stages)
            .into_iter()
            .zip(early)
            .partition(|(_, early)| *early);
        self.stages = later.into_iter().map(|(stage, _)| stage).collect();
        early.into_iter().map(|(stage, _)| stage).collect()
    }
}

pub(super) fn material_sources(
    node: &Node,
    model: Option<&SceneModel>,
    device: &wgpu::Device,
    camera: SceneCamera,
    (reflecting, renderables): (bool, CaptureRenderables),
    override_material: Option<&Arc<CaptureMaterial>>,
) -> BTreeSet<CaptureRead> {
    let mut sources = BTreeSet::new();
    if renderables == CaptureRenderables::None {
        return sources;
    }
    let effective_filter = if override_material.is_some() {
        CaptureRenderables::All
    } else {
        renderables
    };
    let mut collect = |mesh: &Mesh3D, fading| {
        let Mesh3D::Imported(data) = mesh else { return };
        if data.capture_sources().is_empty() {
            return;
        }
        for (_, material) in mesh.sections() {
            let Some((material, shader)) =
                material.and_then(|material| Some((material, material.shader.as_ref()?)))
            else {
                continue;
            };
            if !effective_filter.includes(fading || material.is_blended()) {
                continue;
            }
            let coated = material
                .pbr
                .as_ref()
                .and_then(|pbr| pbr.clearcoat.as_ref())
                .is_some_and(|coat| coat.amount > 0.0);
            sources.extend(
                shader
                    .capture_sources(shader_texture_slots(device, coated))
                    .cloned(),
            );
        }
    };
    if let Some(model) = model {
        let geometry = model.geometry(&camera);
        let geometry = match override_material {
            Some(material) => {
                geometry.with_capture_material(camera.layer_mask(), renderables, material)
            }
            None => geometry,
        };
        let objects = geometry.objects();
        let candidates = objects.iter().enumerate().filter(|(_, object)| {
            object.layers & camera.layer_mask() != 0
                && (!reflecting || object.casts_reflections)
                && matches!(&object.mesh, Mesh3D::Imported(data) if !data.capture_sources().is_empty())
        });
        // The viewport can still jitter after capture planning. Keep the
        // layer-only demand in that mode to cover objects near clip edges.
        let visible = (!reflecting
            && node.boolean("frustum-culling", true)
            && !Sampling::from_node(node).is_active()
            && candidates.clone().next().is_some())
        .then(|| geometry.visibility(&camera));
        for (index, object) in candidates {
            if visible
                .as_ref()
                .is_none_or(|visible| visible.contains(index as u32))
            {
                collect(&object.mesh, false);
            }
        }
    }
    if let Some(particles) =
        ParticleDraw::sample_with_capture_material(node, &camera, renderables, override_material)
    {
        for (mesh, fading) in particles.model_meshes_with_fading() {
            collect(mesh, fading);
        }
    }
    sources
}

fn capture_order(stages: &mut [CaptureStage], roots: &[usize]) -> (Vec<usize>, u64) {
    let mut order = Vec::with_capacity(stages.len());
    let mut visited = vec![false; stages.len()];
    let mut stack = Vec::new();
    for root in 0..stages.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        stack.push((root, 0));
        while let Some((stage, next)) = stack.last_mut() {
            if let Some((_, dependency)) = stages[*stage].reads.get(*next) {
                *next += 1;
                if !visited[*dependency] {
                    visited[*dependency] = true;
                    stack.push((*dependency, 0));
                }
            } else {
                order.push(*stage);
                stack.pop();
            }
        }
    }

    // Label strongly connected components along the reversed edges. Remove
    // all internal reads, so a cycle's behavior is independent of root order.
    let mut consumers = vec![Vec::new(); stages.len()];
    for (stage, data) in stages.iter().enumerate() {
        for (_, dependency) in &data.reads {
            consumers[*dependency].push(stage);
        }
    }
    let mut components = vec![None; stages.len()];
    let mut pending = Vec::new();
    for &root in order.iter().rev() {
        if components[root].is_some() {
            continue;
        }
        components[root] = Some(root);
        pending.push(root);
        while let Some(stage) = pending.pop() {
            for &consumer in &consumers[stage] {
                if components[consumer].is_none() {
                    components[consumer] = Some(root);
                    pending.push(consumer);
                }
            }
        }
    }
    let mut feedback_edges = 0;
    for (index, stage) in stages.iter_mut().enumerate() {
        let before = stage.reads.len();
        stage
            .reads
            .retain(|(_, dependency)| components[*dependency] != components[index]);
        feedback_edges += (before - stage.reads.len()) as u64;
    }
    // Removing feedback edges can leave producers with no surviving consumer.
    visited.fill(false);
    pending.extend_from_slice(roots);
    while let Some(stage) = pending.pop() {
        if !std::mem::replace(&mut visited[stage], true) {
            pending.extend(
                stages[stage]
                    .reads
                    .iter()
                    .map(|(_, dependency)| *dependency),
            );
        }
    }
    order.retain(|&stage| visited[stage]);
    (order, feedback_edges)
}

pub(super) fn geometry_scene(view: &Node, item_revision: u64) -> Node {
    let mut scene = Node::new("SceneView3D", None);
    for name in [
        "model",
        "frustum-culling",
        "particle-emitters",
        "particle-affectors",
        "particle-time",
    ] {
        if let Some(value) = view.properties.get(name) {
            scene.properties.insert(name, value.clone());
        }
    }
    if item_revision != 0 {
        scene
            .properties
            .insert("item-texture-revision", Value::Int(item_revision as i64));
    }
    scene
}

pub(super) fn capture_scene(
    view: &Node,
    background: Color,
    item_revision: u64,
    static_probes: bool,
) -> Node {
    let mut scene = Node::new("SceneView3D", None);
    // Use the same projected data and material passes as a viewport. Project
    // only inputs consumed by this offscreen stage so effect changes can reuse it.
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
        "shadows",
        "shadow-map-size",
        "shadow-bias",
        "shadow-cascades",
        "shadow-distance",
        "shadow-cascade-blend",
        "shadow-split-balance",
        "order-independent-transparency",
        "frustum-culling",
        "specular-aa",
        "ambient-occlusion",
        "ao-strength",
        "ao-distance",
        "ao-bias",
        "fog-color",
        "fog-near",
        "fog-far",
        "fog-density",
        "fog-depth-enabled",
        "fog-height-enabled",
        "fog-transmit-enabled",
        "fog-depth-curve",
        "fog-height-start",
        "fog-height-end",
        "fog-height-curve",
        "fog-transmit-curve",
        "fog-volumes",
        "fog-volume-scattering",
        "fog-volume-anisotropy",
        "fog-volume-samples",
        "fog-volume-density-samples",
        "particle-emitters",
        "particle-affectors",
        "particle-time",
    ] {
        if let Some(value) = view.properties.get(name) {
            scene.properties.insert(name, value.clone());
        }
    }
    if let Some(Value::SceneReflections(probes)) = view.properties.get("reflection-probes") {
        let probes = if static_probes {
            probes
                .iter()
                .filter(|probe| matches!(probe.source, ReflectionSource::Panorama(_)))
                .cloned()
                .collect()
        } else {
            probes.clone()
        };
        if !probes.is_empty() {
            scene
                .properties
                .insert("reflection-probes", Value::SceneReflections(probes));
        }
    }
    scene
        .properties
        .insert("scene-background", Value::Color(background));
    if item_revision != 0 {
        scene
            .properties
            .insert("item-texture-revision", Value::Int(item_revision as i64));
    }
    scene
}

pub(super) fn cube_revisions(cubes: &[Option<CapturedCube>]) -> Vec<Option<u64>> {
    cubes
        .iter()
        .map(|cube| cube.as_ref().map(|cube| cube.revision))
        .collect()
}
