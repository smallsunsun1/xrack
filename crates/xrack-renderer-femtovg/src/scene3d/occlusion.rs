//! Cached half-resolution depth and screen-space ambient occlusion.
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use xrack_runtime::{
    Node,
    scene3d::{SceneCamera, SceneObject, SceneVisibility},
};

use super::{
    DEPTH_FORMAT, append_floats, attachment_view, buffer, deform::DeformationBinding,
    depth::DepthPipelines, instances::InstanceSet, screen_depth,
};

const AO_HEADER_BYTES: u64 = 16;
const AO_SAMPLE_BYTES: u64 = 8;
const MAX_RESIDENT_VIEWS: usize = 3;

#[derive(Clone, Copy)]
pub(super) struct OcclusionSettings {
    pub strength: f32,
    pub radius: f32,
    bias: f32,
}

impl OcclusionSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("ambient-occlusion", false) {
            return None;
        }
        let radius = node.float("ao-distance").unwrap_or(1.0);
        let strength = node.float("ao-strength").unwrap_or(0.8).clamp(0.0, 1.0);
        let bias = node.float("ao-bias").unwrap_or(0.025).max(0.0);
        (strength > 0.0 && radius > bias).then_some(Self {
            strength,
            radius,
            bias,
        })
    }
}

pub(super) struct AmbientOcclusion {
    entries: VecDeque<Entry>,
    evaluators: [Option<Evaluator>; screen_depth::MODES],
    pub depth_pipeline: Option<DepthPipelines>,
    pub fallback: wgpu::Buffer,
}

pub(super) struct OcclusionFrame {
    pub depth_dirty: bool,
    pub occlusion_dirty: bool,
    pub size: [u32; 2],
    pub depth: wgpu::TextureView,
    pub map: wgpu::Buffer,
}

pub(super) struct OcclusionSource<'a> {
    pub instances: &'a Arc<InstanceSet>,
    pub visibility: Option<&'a Arc<SceneVisibility>>,
    pub objects: &'a Arc<[SceneObject]>,
    pub deformation: &'a [Option<DeformationBinding>],
    pub item_revision: u64,
}

impl OcclusionSource<'_> {
    fn pose_groups(&self) -> impl Iterator<Item = (u32, &wgpu::BindGroup)> {
        self.deformation
            .iter()
            .enumerate()
            .filter_map(|(index, binding)| {
                let binding = binding.as_ref()?;
                let slot = index as u32;
                (self.visibility.is_none_or(|mask| mask.contains(slot))
                    && self.objects[index].mesh.has_auxiliary_depth())
                .then_some((slot, &binding.group))
            })
    }
}

fn same_depth_inputs(
    before: &[SceneObject],
    after: &[SceneObject],
    visibility: Option<&SceneVisibility>,
) -> bool {
    before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .enumerate()
            .all(|(index, (before, after))| {
                if visibility.is_some_and(|visible| !visible.contains(index as u32)) {
                    return true;
                }
                let previous_depth = before.mesh.has_auxiliary_depth();
                let next_depth = after.mesh.has_auxiliary_depth();
                if !previous_depth && !next_depth {
                    return true;
                }
                previous_depth == next_depth
                    && before.mesh.has_opaque_parts() == after.mesh.has_opaque_parts()
                    && before.mesh.same_shadow_mesh(&after.mesh)
                    && before.transform_rows() == after.transform_rows()
                    && before.lod_coverage() == after.lod_coverage()
                    && before.same_vertex_program_instance(after)
                    && before.color.0 as u8 == after.color.0 as u8
            })
}

struct Entry {
    source: Weak<InstanceSet>,
    objects: Arc<[SceneObject]>,
    visibility: Option<Arc<SceneVisibility>>,
    pose_groups: Vec<(u32, wgpu::BindGroup)>,
    item_revision: u64,
    camera: SceneCamera,
    parameters: [f32; 2],
    size: [u32; 2],
    depth: wgpu::TextureView,
    map: wgpu::Buffer,
}

impl AmbientOcclusion {
    pub fn discard_recorded_maps(&mut self) {
        self.entries.clear();
    }

    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            entries: VecDeque::new(),
            evaluators: [const { None }; screen_depth::MODES],
            depth_pipeline: None,
            fallback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack disabled ambient occlusion"),
                size: AO_HEADER_BYTES + AO_SAMPLE_BYTES,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        source: OcclusionSource<'_>,
        camera: SceneCamera,
        settings: OcclusionSettings,
    ) -> OcclusionFrame {
        let mut size = size.map(|side| side.div_ceil(2).max(1));
        let bytes = |[width, height]: [u32; 2]| {
            AO_HEADER_BYTES + AO_SAMPLE_BYTES * u64::from(width) * u64::from(height)
        };
        let limits = device.limits();
        let budget = u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        // Very large viewports continue to use AO at a smaller raster size.
        while bytes(size) > budget {
            size = size.map(|side| side.div_ceil(2));
        }
        let map_bytes = bytes(size);
        let parameters = [settings.radius, settings.bias];
        let item_revision = if source.item_revision != 0
            && source.objects.iter().enumerate().any(|(index, object)| {
                source
                    .visibility
                    .is_none_or(|visible| visible.contains(index as u32))
                    && super::mesh_samples_item_alpha(&object.mesh)
            }) {
            source.item_revision
        } else {
            0
        };
        let hit = self.entries.iter().position(|entry| {
            entry.size == size
                && entry.item_revision == item_revision
                && entry.camera.same_frustum(&camera)
                && entry.visibility.as_deref() == source.visibility.map(Arc::as_ref)
                && (entry.source.as_ptr() == Arc::as_ptr(source.instances)
                    || same_depth_inputs(
                        &entry.objects,
                        source.objects,
                        entry.visibility.as_deref(),
                    ))
                && entry
                    .pose_groups
                    .iter()
                    .map(|(slot, group)| (*slot, group))
                    .eq(source.pose_groups())
        });
        let cached = hit.and_then(|index| self.entries.remove(index));
        let depth_dirty = cached.is_none();
        let mut entry = cached.unwrap_or_else(|| {
            let evicted = (self.entries.len() == MAX_RESIDENT_VIEWS)
                .then(|| self.entries.pop_back())
                .flatten();
            let (depth, map) = evicted.filter(|entry| entry.size == size).map_or_else(
                || {
                    let depth = attachment_view(
                        device,
                        size,
                        DEPTH_FORMAT,
                        1,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        "XRack half-resolution ambient occlusion depth",
                    );
                    let map = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("XRack half-resolution ambient occlusion samples"),
                        size: map_bytes,
                        usage: wgpu::BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    });
                    (depth, map)
                },
                |entry| (entry.depth, entry.map),
            );
            Entry {
                source: Arc::downgrade(source.instances),
                objects: source.objects.clone(),
                visibility: source.visibility.cloned(),
                pose_groups: source
                    .pose_groups()
                    .map(|(slot, group)| (slot, group.clone()))
                    .collect(),
                item_revision,
                camera,
                parameters,
                size,
                depth,
                map,
            }
        });
        let occlusion_dirty = depth_dirty || entry.parameters != parameters;
        entry.parameters = parameters;
        entry.source = Arc::downgrade(source.instances);
        entry.objects = source.objects.clone();
        let frame = OcclusionFrame {
            depth_dirty,
            occlusion_dirty,
            size,
            depth: entry.depth.clone(),
            map: entry.map.clone(),
        };
        self.entries.push_front(entry);
        frame
    }

    pub fn evaluate(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &OcclusionFrame,
        camera: SceneCamera,
        settings: OcclusionSettings,
    ) {
        let mode = screen_depth::mode(camera);
        let evaluator =
            self.evaluators[mode as usize].get_or_insert_with(|| Evaluator::new(device, mode));
        let mut bytes = Vec::with_capacity(screen_depth::UNIFORM_BYTES + 16);
        screen_depth::write(&mut bytes, camera);
        append_floats(&mut bytes, [settings.radius, settings.bias, 0.0, 0.0]);
        let uniform = buffer(
            device,
            "XRack ambient occlusion settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let inputs = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack ambient occlusion inputs"),
            layout: &evaluator.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&frame.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: frame.map.as_entire_binding(),
                },
            ],
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack screen-space ambient occlusion"),
                timestamp_writes: None,
            },
        );
        pass.set_pipeline(&evaluator.pipeline);
        pass.set_bind_group(0, &inputs, &[]);
        pass.dispatch_workgroups(frame.size[0].div_ceil(8), frame.size[1].div_ceil(8), 1);
    }
}

struct Evaluator {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

impl Evaluator {
    fn new(device: &wgpu::Device, mode: screen_depth::Mode) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack screen-space ambient occlusion"),
            source: wgpu::ShaderSource::Wgsl(
                [screen_depth::SHADER, include_str!("occlusion.wgsl")]
                    .join("\n")
                    .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("XRack screen-space ambient occlusion"),
            layout: None,
            module: &shader,
            entry_point: Some("compute"),
            compilation_options: screen_depth::compilation(mode),
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Self { pipeline, layout }
    }
}
