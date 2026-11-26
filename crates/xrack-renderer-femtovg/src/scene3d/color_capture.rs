//! Named, viewport-local, scene-linear inputs to the effect chain.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    sync::{Arc, Weak},
};

use xrack_runtime::{
    Color, Node, RowKey, Value,
    material_shader::MaterialHooks,
    scene_effect::{
        CaptureAttachment, CaptureAugment, CaptureInputs, CapturePipelineState, CaptureRead,
        CaptureRenderables, MATERIAL_DATA_ATTACHMENTS, MAX_BUFFERS,
    },
    scene3d::{CaptureMaterial, Mesh3D, MeshDepthDraw, ReflectionSource, SceneCamera, SceneModel},
};

use super::{
    Attachment, HDR_FORMAT, append_floats, buffer,
    capture_region::CaptureRegion,
    capture_stencil,
    custom_effect::{OutputFormats, effect_size, texture_bytes},
    effect_history::Owner,
    environment::SKYBOX_PROPERTIES,
    material::{
        BASE_MATERIAL_TEXTURES, FULL_MATERIAL_TEXTURES, augment_texture_slots,
        capture_material_texture_count, shader_texture_slots,
    },
    particles::ParticleDraw,
    pipeline::{NAMED_GEOMETRY_ATTACHMENTS, uses_material_prepass},
    reflection_capture::{CapturedCube, same_inputs},
    retained_view::Sampling,
    supersampling::{mip_count, mip_extent, mip_views},
};

pub(super) type CaptureMask = u16;
pub(super) type DataFormats = [wgpu::TextureFormat; MATERIAL_DATA_ATTACHMENTS.len()];

pub(super) type CaptureViews = BTreeMap<CaptureRead, CapturedTexture>;

#[derive(Clone)]
pub(super) struct CapturedTexture {
    pub view: wgpu::TextureView,
    pub allocation: u64,
    pub revision: u64,
    pub projection: Option<Arc<CaptureProjection>>,
    image: Weak<CaptureAllocation>,
    pub mip_views: Option<Arc<[wgpu::TextureView]>>,
}

impl CapturedTexture {
    pub fn render_view(&self) -> &wgpu::TextureView {
        self.mip_views
            .as_deref()
            .and_then(|levels| levels.first())
            .unwrap_or(&self.view)
    }
}

struct CaptureAllocation {
    attachment: Attachment,
    mip_views: Option<Arc<[wgpu::TextureView]>>,
    bytes: u64,
    id: u64,
}

/// A probe keeps these images immutable until its base faces are submitted.
pub(super) struct FrozenCaptureViews {
    pub views: CaptureViews,
    _images: Vec<Arc<CaptureAllocation>>,
}

impl FrozenCaptureViews {
    pub fn new(views: &CaptureViews) -> Self {
        Self {
            views: views.clone(),
            _images: views
                .values()
                .filter_map(|view| view.image.upgrade())
                .collect(),
        }
    }
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
    images: [Option<Arc<CaptureAllocation>>; CaptureAttachment::ALL.len()],
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
    pub mipmaps: bool,
    pub material_data_formats: DataFormats,
    pub cubes: Vec<Option<u64>>,
    pub dependencies: Vec<(CaptureRead, u64)>,
    pub composition: CaptureInputs,
    pub override_material: Option<Arc<CaptureMaterial>>,
    pub augment: Option<CaptureAugment>,
    pub augment_items: Vec<Option<[u64; 2]>>,
    pub renderables: CaptureRenderables,
    pub pipeline_state: CapturePipelineState,
    pub region: CaptureRegion,
    pub depth_clear: f32,
    pub stencil_clear: u8,
    pub attachments: CaptureMask,
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
            let storage: u64 = entry.images.iter().flatten().map(|image| image.bytes).sum();
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
                    && attachment_class(entry.input.attachments)
                        == attachment_class(input.attachments)
            })
            .and_then(|index| self.entries.remove(index));
        let mut allocated_bytes = 0;
        let next_allocation = &mut self.next_allocation;
        let mut create_image = |size: [u32; 2], format, levels| {
            *next_allocation = next_allocation.wrapping_add(1);
            let bytes: u64 = (0..levels)
                .map(|level| texture_bytes(mip_extent(size, level as usize), format))
                .sum();
            allocated_bytes += bytes;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("XRack named scene capture"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | if format == capture_stencil::FORMAT {
                        wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST
                    } else {
                        wgpu::TextureUsages::empty()
                    },
                view_formats: &[],
            });
            Arc::new(CaptureAllocation {
                attachment: Attachment {
                    size,
                    view: texture.create_view(&Default::default()),
                },
                mip_views: (levels > 1).then(|| mip_views(&texture).into()),
                bytes,
                id: *next_allocation,
            })
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
                    augment: None,
                    augment_items: Vec::new(),
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
            || input_mask & MATERIAL_DATA_MASK != 0
                && (entry.input.augment != input.augment
                    || entry.input.augment_items != input.augment_items)
            || entry.input.pipeline_state != input.pipeline_state
            || entry.input.region != input.region
            || entry.input.depth_clear != input.depth_clear
            || entry.input.stencil_clear != input.stencil_clear
            || entry.input.renderables != input.renderables
            || entry.input.size != input.size
            || entry.input.color_format != input.color_format
            || input_mask & COLOR_ATTACHMENT != 0 && entry.input.mipmaps != input.mipmaps
            || MATERIAL_DATA_ATTACHMENTS
                .into_iter()
                .enumerate()
                .any(|(slot, attachment)| {
                    input_mask & (1 << attachment.index()) != 0
                        && entry.input.material_data_formats[slot]
                            != input.material_data_formats[slot]
                })
            || input.attachments & !entry.input.attachments != 0
            || !same_inputs(&entry.input.scene, &input.scene);
        if render {
            for attachment in CaptureAttachment::ALL {
                let slot = &mut entry.images[attachment.index()];
                if input.attachments & (1 << attachment.index()) == 0 {
                    // An attachment not updated with the other outputs is stale.
                    *slot = None;
                } else {
                    let format = match attachment {
                        CaptureAttachment::Color => input.color_format,
                        _ => attachment.material_data_slot().map_or_else(
                            || attachment_format(attachment),
                            |index| input.material_data_formats[index],
                        ),
                    };
                    let levels = if attachment == CaptureAttachment::Color && input.mipmaps {
                        mip_count(input.size)
                    } else {
                        1
                    };
                    if slot.as_ref().is_none_or(|image| {
                        Arc::strong_count(image) > 1
                            || image.attachment.size != input.size
                            || image.attachment.view.texture().format() != format
                            || image.attachment.view.texture().mip_level_count() != levels
                    }) {
                        *slot = Some(create_image(input.size, format, levels));
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
                let image = entry.images[index].as_ref()?;
                Some(CapturedTexture {
                    view: image.attachment.view.clone(),
                    allocation: image.id,
                    revision: entry.revision,
                    projection: if index == CaptureAttachment::Depth.index() {
                        entry.projection.clone()
                    } else {
                        None
                    },
                    image: Arc::downgrade(image),
                    mip_views: image.mip_views.clone(),
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

const COLOR_ATTACHMENT: CaptureMask = 1 << CaptureAttachment::Color.index();
const STENCIL_ATTACHMENT: CaptureMask = 1 << CaptureAttachment::Stencil.index();
const MATERIAL_DATA_MASK: CaptureMask = 0b111 << CaptureAttachment::MaterialData.index();

fn geometry_attachments(mask: CaptureMask) -> bool {
    mask & !(COLOR_ATTACHMENT | STENCIL_ATTACHMENT) != 0
}

#[derive(PartialEq, Eq)]
enum CapturePassKind {
    Color,
    Geometry,
    MaterialData(CaptureMask),
    Combined,
}

fn attachment_class(mask: CaptureMask) -> CapturePassKind {
    if !geometry_attachments(mask) {
        CapturePassKind::Color
    } else if mask & COLOR_ATTACHMENT != 0 {
        CapturePassKind::Combined
    } else if mask & !MATERIAL_DATA_MASK == 0 {
        CapturePassKind::MaterialData(mask)
    } else {
        CapturePassKind::Geometry
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct CaptureTarget {
    pub camera: SceneCamera,
    pub size: [u32; 2],
    pub background: Color,
    pub color_format: wgpu::TextureFormat,
    pub mipmaps: bool,
    pub material_data_formats: DataFormats,
    pub geometry: bool,
    material_data: bool,
    combine_attachments: bool,
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
    pub augment: Option<CaptureAugment>,
    reads: Vec<(CaptureRead, usize)>,
    removed_feedback: bool,
}

impl CaptureStage {
    pub fn attachments(&self) -> CaptureMask {
        self.outputs
            .iter()
            .fold(0, |mask, output| mask | (1 << output.attachment.index()))
    }

    pub fn has_geometry(&self) -> bool {
        geometry_attachments(self.attachments())
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
        (device, formats): (&wgpu::Device, OutputFormats),
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
        let limits = device.limits();
        let default_augment_slots = augment_texture_slots(device, FULL_MATERIAL_TEXTURES);
        let mut capture_augment_limits = BTreeMap::new();
        let viewport_camera = node.text("camera").unwrap_or_default();
        let target_for = |read: &CaptureRead| {
            let feed = captures.get(read.name.as_ref())?;
            let geometry = !matches!(
                read.attachment,
                CaptureAttachment::Color | CaptureAttachment::Stencil
            );
            let material_data = read.attachment.material_data_slot().is_some();
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
                material_data,
                combine_attachments: feed.combine_attachments,
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
                    if renderables == CaptureRenderables::None && feed.inputs.color.is_none() {
                        formats.sampled(feed.color_format)
                    } else {
                        formats.blended(feed.color_format)
                    }
                },
                mipmaps: read.attachment == CaptureAttachment::Color && feed.mipmaps,
                material_data_formats: if material_data {
                    feed.material_data_formats
                        .map(|format| formats.sampled(format))
                } else {
                    [HDR_FORMAT; MATERIAL_DATA_ATTACHMENTS.len()]
                },
            })
        };
        let depth_matches = |read: &CaptureRead, camera: SceneCamera| {
            target_for(read).is_some_and(|target| {
                (!target.region.is_empty() || target.depth_clear != 1.0)
                    && target.raster_camera().clip_rows() == camera.clip_rows()
            })
        };
        let internal_passes = !node.scene_internal_passes_disabled();
        let mut roots = camera
            .filter(|_| internal_passes)
            .map_or_else(BTreeSet::new, |camera| {
                material_sources(
                    node,
                    model,
                    device,
                    camera,
                    (false, CaptureRenderables::All, false),
                    None,
                    None,
                )
            });
        if let Some(Value::SceneEffects(effects)) = node.properties.get("scene-effects") {
            roots.extend(effects.capture_sources().cloned());
        }
        let inputs = super::composite_input::capture_reads(node);
        if camera.is_some() || !internal_passes {
            roots.extend(inputs.color);
        }
        if let Some(camera) = camera.filter(|_| internal_passes) {
            roots.extend(inputs.depth.filter(|read| depth_matches(read, camera)));
        }
        roots.extend(additional.iter().cloned());
        let mut stages = Vec::<CaptureStage>::new();
        let mut discovered = BTreeMap::<CaptureRead, Option<usize>>::new();
        let mut discover =
            |read: CaptureRead, stages: &mut Vec<CaptureStage>, pending: &mut VecDeque<usize>| {
                *discovered.entry(read.clone()).or_insert_with(|| {
                    let target = target_for(&read)?;
                    let definition = captures.get(read.name.as_ref())?;
                    let inputs = &definition.inputs;
                    let override_material = (target.renderables != CaptureRenderables::None)
                        .then(|| definition.override_material.clone())
                        .flatten();
                    let augment = target
                        .material_data
                        .then(|| {
                            let mut augment = definition.augment.clone()?;
                            let requested = augment
                                .textures
                                .len()
                                .min(xrack_runtime::material_shader::MAX_TEXTURES);
                            if requested > default_augment_slots {
                                let slots = *capture_augment_limits
                                    .entry(read.name.clone())
                                    .or_insert_with(|| {
                                        capture_augment_texture_slots(
                                            node,
                                            model,
                                            device,
                                            target,
                                            override_material.as_ref(),
                                        )
                                    });
                                augment.textures.truncate(slots);
                            } else {
                                augment.textures.truncate(requested);
                            }
                            Some(augment)
                        })
                        .flatten();
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
                    // A stage that declares inputs keeps its name while resolving
                    // cycles. Merging a self-referencing stage with a consumer
                    // could incorrectly turn the consumer's valid read into a loop.
                    let matching = if composition.is_empty() && override_material.is_none() {
                        stages.iter().position(|stage| {
                            stage.target == target
                                && stage.composition.is_empty()
                                && stage.override_material == override_material
                                && stage.augment == augment
                                && (!target.material_data
                                    || attachment_limits_match(
                                        &limits,
                                        None,
                                        target.material_data_formats,
                                        stage.attachments() | (1 << read.attachment.index()),
                                    ))
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
                            augment,
                            reads: Vec::new(),
                            removed_feedback: false,
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
                    (
                        false,
                        target.renderables,
                        stages[index].attachments() & MATERIAL_DATA_MASK != 0
                            && stages[index].augment.is_none(),
                    ),
                    stages[index].override_material.as_ref(),
                    None,
                )
            };
            if !target.region.is_empty()
                && let Some(augment) = &stages[index].augment
            {
                dependencies.extend(augment.capture_sources(augment.textures.len()).cloned());
            }
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
                        && previous.augment == stage.augment
                        && previous.reads == stage.reads
                        && (!stage.target.material_data
                            || attachment_limits_match(
                                &limits,
                                None,
                                stage.target.material_data_formats,
                                previous.attachments() | stage.attachments(),
                            ))
                })
            {
                previous.removed_feedback |= stage.removed_feedback;
                previous.outputs.append(&mut stage.outputs);
            } else {
                ordered.push(stage);
            }
        }
        Self {
            stages: combine_capture_stages(ordered, node, model, limits),
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

fn combine_capture_stages(
    ordered: Vec<CaptureStage>,
    node: &Node,
    model: Option<&SceneModel>,
    limits: wgpu::Limits,
) -> Vec<CaptureStage> {
    if !ordered.iter().any(|stage| stage.target.combine_attachments)
        || node.boolean("environment-skybox", false)
        || matches!(node.properties.get("fog-volumes"), Some(Value::SceneFogVolumes(volumes)) if !volumes.is_empty())
    {
        return ordered;
    }
    let eligible = |stage: &CaptureStage| {
        stage.target.combine_attachments
            && stage.target.renderables == CaptureRenderables::Opaque
            && stage.target.pipeline_state.depth_write != Some(false)
            && stage.override_material.is_none()
            && !stage.target.region.is_empty()
            && stage.composition.depth.is_none()
            && !stage.removed_feedback
    };
    let has_dependencies = ordered.iter().any(|stage| !stage.reads.is_empty());
    let mut owners: BTreeMap<_, _> = if has_dependencies {
        ordered
            .iter()
            .enumerate()
            .flat_map(|(index, stage)| stage.outputs.iter().cloned().map(move |read| (read, index)))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut stages: Vec<_> = ordered.into_iter().map(Some).collect();
    let mut merged = false;
    for index in 0..stages.len() {
        let Some(stage) = stages[index].as_ref().filter(|stage| eligible(stage)) else {
            continue;
        };
        let previous = stages[..index]
            .iter()
            .enumerate()
            .find_map(|(candidate, previous)| {
                let previous = previous.as_ref()?;
                if !eligible(previous)
                    || previous.target.geometry == stage.target.geometry
                    || previous.attachments() & stage.attachments() != 0
                {
                    return None;
                }
                let (color, geometry) = if stage.target.geometry {
                    (previous, stage)
                } else {
                    (stage, previous)
                };
                (can_combine(&limits, color, geometry)
                    && !depends_on(&stages, &owners, index, candidate)
                    && !depends_on(&stages, &owners, candidate, index)
                    && supports_combined_geometry(node, model, color))
                .then_some(candidate)
            });
        let Some(previous) = previous else {
            continue;
        };
        let (earlier, later) = stages.split_at_mut(index);
        let (Some(previous_stage), Some(mut stage)) = (earlier[previous].as_mut(), later[0].take())
        else {
            continue;
        };
        if previous_stage.target.geometry {
            std::mem::swap(previous_stage, &mut stage);
        }
        previous_stage.target.material_data_formats = merged_data_formats(previous_stage, &stage);
        if stage.attachments() & MATERIAL_DATA_MASK != 0 {
            previous_stage.augment = stage.augment;
        }
        previous_stage.outputs.append(&mut stage.outputs);
        previous_stage.reads.append(&mut stage.reads);
        previous_stage.reads.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        previous_stage.reads.dedup_by(|a, b| a.0 == b.0);
        if has_dependencies {
            for output in &previous_stage.outputs {
                owners.insert(output.clone(), previous);
            }
        }
        merged = true;
    }
    if !merged || !has_dependencies {
        return stages.into_iter().flatten().collect();
    }
    // A joint pass can move after an input required by just one of its
    // outputs. Follow the new owners so neither inputs nor consumers race it.
    capture_postorder(stages.len(), |stage, next| {
        let (read, _) = stages[stage].as_ref()?.reads.get(next)?;
        owners.get(read).copied()
    })
    .into_iter()
    .filter_map(|index| stages[index].take())
    .collect()
}

fn depends_on(
    stages: &[Option<CaptureStage>],
    owners: &BTreeMap<CaptureRead, usize>,
    from: usize,
    target: usize,
) -> bool {
    if stages[from]
        .as_ref()
        .is_none_or(|stage| stage.reads.is_empty())
    {
        return false;
    }
    let mut pending = vec![from];
    let mut visited = vec![false; stages.len()];
    while let Some(index) = pending.pop() {
        if index == target {
            return true;
        }
        if !std::mem::replace(&mut visited[index], true)
            && let Some(stage) = &stages[index]
        {
            pending.extend(
                stage
                    .reads
                    .iter()
                    .filter_map(|(read, _)| owners.get(read).copied()),
            );
        }
    }
    false
}

fn can_combine(limits: &wgpu::Limits, color: &CaptureStage, geometry: &CaptureStage) -> bool {
    let color_target = color.target;
    let mut pipeline_state = color_target.pipeline_state;
    pipeline_state.blend = None;
    pipeline_state.blend_constant = None;
    pipeline_state.color_write_mask = 15;
    let geometry_target = CaptureTarget {
        geometry: true,
        material_data: geometry.target.material_data,
        background: Color(0),
        color_format: HDR_FORMAT,
        mipmaps: false,
        material_data_formats: geometry.target.material_data_formats,
        pipeline_state,
        ..color_target
    };
    geometry_target == geometry.target
        && (color.attachments() & MATERIAL_DATA_MASK == 0
            || geometry.attachments() & MATERIAL_DATA_MASK == 0
            || color.augment == geometry.augment)
        && color.composition.stencil == geometry.composition.stencil
        && color.outputs.iter().any(|source| {
            geometry
                .outputs
                .iter()
                .any(|output| source.name == output.name)
        })
        && attachment_limits_match(
            limits,
            Some(color_target.color_format),
            merged_data_formats(color, geometry),
            color.attachments() | geometry.attachments(),
        )
}

fn merged_data_formats(color: &CaptureStage, geometry: &CaptureStage) -> DataFormats {
    let mask = geometry.attachments();
    std::array::from_fn(|slot| {
        if mask & (1 << MATERIAL_DATA_ATTACHMENTS[slot].index()) != 0 {
            geometry.target.material_data_formats[slot]
        } else {
            color.target.material_data_formats[slot]
        }
    })
}

fn attachment_limits_match(
    limits: &wgpu::Limits,
    color: Option<wgpu::TextureFormat>,
    data: DataFormats,
    mask: CaptureMask,
) -> bool {
    let present = |attachment: CaptureAttachment| mask & (1 << attachment.index()) != 0;
    let data_slots = MATERIAL_DATA_ATTACHMENTS
        .iter()
        .rposition(|&attachment| present(attachment))
        .map_or(0, |index| index + 1);
    let color_slots = if color.is_some() {
        NAMED_GEOMETRY_ATTACHMENTS.len() + 1
    } else {
        0
    };
    limits.max_color_attachments >= (color_slots + data_slots) as u32
        && NAMED_GEOMETRY_ATTACHMENTS
            .into_iter()
            .filter(|&attachment| present(attachment))
            .map(attachment_format)
            .chain(color)
            .chain(
                MATERIAL_DATA_ATTACHMENTS
                    .into_iter()
                    .zip(data)
                    .filter_map(|(attachment, format)| present(attachment).then_some(format)),
            )
            .try_fold(0u32, |bytes, format| {
                let cost = format.target_pixel_byte_cost()?;
                let alignment = format.target_component_alignment()?;
                Some(bytes.next_multiple_of(alignment) + cost)
            })
            .is_some_and(|bytes| bytes <= limits.max_color_attachment_bytes_per_sample)
}

fn capture_augment_texture_slots(
    node: &Node,
    model: Option<&SceneModel>,
    device: &wgpu::Device,
    target: CaptureTarget,
    override_material: Option<&Arc<CaptureMaterial>>,
) -> usize {
    if target.renderables == CaptureRenderables::None || target.region.is_empty() {
        return 0;
    }
    if let Some(material) = override_material {
        return augment_texture_slots(
            device,
            capture_material_texture_count(device, material.material()),
        );
    }

    // The capture uses these projected meshes. Include every section in the
    // selected layers so clipping and alpha coverage can only lower the cost.
    let mut used = BASE_MATERIAL_TEXTURES;
    let mut include_mesh = |mesh: &Mesh3D| {
        for (_, material) in mesh.sections() {
            if let Some(material) = material {
                used = used.max(capture_material_texture_count(device, material));
            }
        }
    };
    if let Some(model) = model {
        let geometry = model.render_geometry(
            &target.camera,
            f64::from(node.length("lod-fade-width").unwrap_or_default()),
        );
        for object in geometry
            .objects()
            .iter()
            .filter(|object| object.layers & target.camera.layer_mask() != 0)
        {
            include_mesh(&object.mesh);
        }
    }
    if let Some(particles) =
        ParticleDraw::sample_with_capture_material(node, &target.camera, target.renderables, None)
    {
        for mesh in particles.model_meshes() {
            include_mesh(mesh);
        }
    }
    augment_texture_slots(device, used)
}

fn supports_combined_geometry(
    node: &Node,
    model: Option<&SceneModel>,
    stage: &CaptureStage,
) -> bool {
    let camera = &stage.target.camera;
    let state = stage.target.pipeline_state;
    let material_prepass = uses_material_prepass(state);
    let compatible = |mesh: &Mesh3D| {
        mesh.sections()
            .filter_map(|(_, material)| material)
            .filter(|material| !material.is_blended())
            .all(|material| {
                material.transmission_blur().is_none()
                    && (material.depth_coverage().is_some() || state.depth_write == Some(true))
                    && !(material_prepass
                        && material.depth_draw_mode == MeshDepthDraw::OpaquePrepass)
            })
    };
    model.is_none_or(|model| {
        let geometry = model.render_geometry(
            camera,
            f64::from(node.length("lod-fade-width").unwrap_or_default()),
        );
        let eye = camera.eye_position();
        geometry.objects().iter().all(|object| {
            object.layers & camera.layer_mask() == 0
                || !object.visible_at_distance(eye)
                || compatible(&object.mesh)
        })
    }) && ParticleDraw::sample_with_capture_material(node, camera, CaptureRenderables::Opaque, None)
        .is_none_or(|draw| {
            draw.model_meshes_with_fading()
                .all(|(mesh, fading)| fading || compatible(mesh))
        })
}

pub(super) fn material_sources(
    node: &Node,
    model: Option<&SceneModel>,
    device: &wgpu::Device,
    camera: SceneCamera,
    (reflecting, renderables, material_data): (bool, CaptureRenderables, bool),
    override_material: Option<&Arc<CaptureMaterial>>,
    mut item_names: Option<&mut BTreeSet<String>>,
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
    let collect_items = item_names.is_some();
    let uses_textures = |data: &xrack_runtime::scene3d::MeshData| {
        !data.capture_sources().is_empty() || collect_items && data.uses_item_textures()
    };
    let mut collect = |mesh: &Mesh3D, fading| {
        let Mesh3D::Imported(data) = mesh else { return };
        if !uses_textures(data) {
            return;
        }
        for (_, material) in mesh.sections() {
            let Some(material) = material else {
                continue;
            };
            if !effective_filter.includes(fading || material.is_blended()) {
                continue;
            }
            if let Some(names) = &mut item_names
                && let Some(item) = &material.base_item
            {
                names.insert(item.source().to_owned());
            }
            let Some(shader) = material.shader.as_ref() else {
                continue;
            };
            if !MaterialHooks::from_source(&shader.source).uses_fragment(material_data) {
                continue;
            }
            if let Some(names) = &mut item_names {
                names.extend(shader.item_sources().map(str::to_owned));
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
        let geometry = model.render_geometry(
            &camera,
            f64::from(node.length("lod-fade-width").unwrap_or_default()),
        );
        let geometry = match override_material {
            Some(material) => {
                geometry.with_capture_material(camera.layer_mask(), renderables, material)
            }
            None => geometry,
        };
        let objects = geometry.objects();
        let eye = camera.eye_position();
        let candidates = objects.iter().enumerate().filter(|(_, object)| {
            object.layers & camera.layer_mask() != 0
                && object.visible_at_distance(eye)
                && (!reflecting || object.casts_reflections)
                && matches!(&object.mesh, Mesh3D::Imported(data) if uses_textures(data))
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

pub(super) fn capture_postorder(
    len: usize,
    dependency: impl Fn(usize, usize) -> Option<usize>,
) -> Vec<usize> {
    let mut order = Vec::with_capacity(len);
    let mut visited = vec![false; len];
    let mut stack = Vec::new();
    for root in 0..len {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        stack.push((root, 0));
        while let Some((stage, next)) = stack.last_mut() {
            if let Some(dependency) = dependency(*stage, *next) {
                *next += 1;
                if !visited[dependency] {
                    visited[dependency] = true;
                    stack.push((dependency, 0));
                }
            } else {
                order.push(*stage);
                stack.pop();
            }
        }
    }
    order
}

fn capture_order(stages: &mut [CaptureStage], roots: &[usize]) -> (Vec<usize>, u64) {
    let mut order = capture_postorder(stages.len(), |stage, next| {
        stages[stage]
            .reads
            .get(next)
            .map(|(_, dependency)| *dependency)
    });

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
        stage.removed_feedback = before != stage.reads.len();
        feedback_edges += (before - stage.reads.len()) as u64;
    }
    // Removing feedback edges can leave producers with no surviving consumer.
    let mut visited = vec![false; stages.len()];
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
        "lod-fade-width",
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
        "shadows",
        "shadow-map-size",
        "shadow-map-precision",
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
    ]
    .into_iter()
    .chain(SKYBOX_PROPERTIES)
    {
        if let Some(value) = view.properties.get(name) {
            scene.properties.insert(name, value.clone());
        }
    }
    if let Some(Value::SceneReflections(probes)) = view.properties.get("reflection-probes") {
        let probes = if static_probes || probes.iter().any(|probe| probe.debug_view) {
            probes
                .iter()
                .filter(|probe| {
                    !static_probes || matches!(probe.source, ReflectionSource::Panorama(_))
                })
                .cloned()
                .map(|probe| probe.with_debug_view(false))
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
