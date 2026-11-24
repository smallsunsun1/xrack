use std::{
    collections::{HashSet, VecDeque},
    ops::Range,
    sync::{Arc, OnceLock, Weak},
};

use xrack_runtime::{
    Color, ImageIdentity,
    scene_effect::CaptureRenderables,
    scene3d::{
        Mesh3D, MeshBlendMode, MeshData, MeshDepthDraw, MeshVertexStreams, SceneCamera,
        SceneGeometry, SceneObject, SceneVisibility, VisibilityIndex,
    },
};

use super::{INSTANCE_STRIDE, append_floats, buffer};

const MAX_RETAINED_SETS: usize = 4;
const MAX_PATCH_RANGES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Upload {
    Unchanged,
    New,
    Patched,
}

#[derive(Default)]
struct VertexTransfers {
    protected: HashSet<wgpu::Buffer>,
    bytes: u64,
    patches: u64,
    allocations: u64,
}

impl VertexTransfers {
    fn use_buffer(&mut self, buffer: &wgpu::Buffer) {
        self.protected.insert(buffer.clone());
    }

    fn upload<I, J>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        rows: I,
        previous: Option<(&wgpu::Buffer, J)>,
    ) -> (wgpu::Buffer, Upload)
    where
        I: ExactSizeIterator<Item = InstanceVertex> + Clone,
        J: ExactSizeIterator<Item = InstanceVertex>,
    {
        if let Some((previous, old_rows)) = previous
            && let Some(ranges) = changed_rows(old_rows, rows.clone())
        {
            if ranges.is_empty() {
                return (previous.clone(), Upload::Unchanged);
            }
            if !self.protected.contains(previous) {
                let mut bytes = Vec::new();
                for range in ranges {
                    bytes.clear();
                    for vertex in rows.clone().skip(range.start).take(range.len()) {
                        vertex.pack(&mut bytes);
                    }
                    queue.write_buffer(previous, range.start as u64 * INSTANCE_STRIDE, &bytes);
                    self.bytes = self.bytes.saturating_add(bytes.len() as u64);
                }
                self.patches = self.patches.saturating_add(1);
                return (previous.clone(), Upload::Patched);
            }
        }
        let mut bytes = Vec::with_capacity(rows.len() * INSTANCE_STRIDE as usize);
        for row in rows {
            row.pack(&mut bytes);
        }
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        self.allocations = self.allocations.saturating_add(1);
        (
            buffer(
                device,
                label,
                &bytes,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            ),
            Upload::New,
        )
    }
}

fn changed_rows(
    old: impl ExactSizeIterator<Item = InstanceVertex>,
    new: impl ExactSizeIterator<Item = InstanceVertex>,
) -> Option<Vec<Range<usize>>> {
    if old.len() != new.len() {
        return None;
    }
    let budget = new.len() / 2;
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut changed = 0;
    for (index, (old, new)) in old.zip(new).enumerate() {
        if old == new {
            continue;
        }
        changed += 1;
        if changed > budget {
            return None;
        }
        if let Some(last) = ranges.last_mut()
            && last.end == index
        {
            last.end += 1;
        } else {
            if ranges.len() == MAX_PATCH_RANGES {
                return None;
            }
            ranges.push(index..index + 1);
        }
    }
    Some(ranges)
}

pub(super) struct InstanceBuffers {
    sets: VecDeque<Arc<InstanceSet>>,
    visibility: VecDeque<VisibilityQuery>,
    color_plans: VecDeque<Arc<ColorPlan>>,
    transfers: VertexTransfers,
    pub color_uploads: u64,
    pub empty: wgpu::Buffer,
    pub uploads: u64,
}

impl InstanceBuffers {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            sets: VecDeque::new(),
            visibility: VecDeque::new(),
            color_plans: VecDeque::new(),
            transfers: VertexTransfers::default(),
            color_uploads: 0,
            empty: buffer(
                device,
                "XRack empty 3D instances",
                &[0; INSTANCE_STRIDE as usize],
                wgpu::BufferUsages::VERTEX,
            ),
            uploads: 0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.transfers.protected.clear();
    }

    pub fn record_transfer_stats(&self, stats: &mut super::SceneDrawStats) {
        stats.instance_upload_bytes = self.transfers.bytes;
        stats.partial_instance_uploads = self.transfers.patches;
        stats.instance_buffer_allocations = self.transfers.allocations;
    }

    pub fn visibility(
        &mut self,
        model: &Arc<SceneGeometry>,
        set: &Arc<InstanceSet>,
        camera: &SceneCamera,
        layer_filter: Option<&Arc<SceneVisibility>>,
    ) -> Arc<SceneVisibility> {
        let index = self.visibility.iter().position(|query| {
            let same_source = if set.rigid {
                query
                    .rigid_source
                    .as_ref()
                    .is_some_and(|source| source.as_ptr() == Arc::as_ptr(set))
            } else {
                std::ptr::addr_eq(query.source.as_ptr(), Arc::as_ptr(model.objects()))
            };
            same_source
                && query.camera.same_frustum(camera)
                && query.layer_filter.as_ref() == layer_filter
        });
        let query = index
            .and_then(|index| self.visibility.remove(index))
            .unwrap_or_else(|| {
                let sampled = if set.rigid {
                    set.rigid_visibility
                        .get_or_init(|| VisibilityIndex::new(model))
                        .as_ref()
                        .map_or_else(
                            || SceneVisibility::all(model.len()),
                            |index| index.visibility(camera),
                        )
                } else {
                    model.visibility(camera)
                };
                let sampled = Arc::new(sampled);
                let sampled = layer_filter.map_or_else(
                    || sampled.clone(),
                    |layers| SceneVisibility::intersect(&sampled, layers),
                );
                let result = self
                    .visibility
                    .iter()
                    .find(|query| query.result == sampled)
                    .map_or(sampled, |query| query.result.clone());
                VisibilityQuery {
                    source: Arc::downgrade(model.objects()),
                    rigid_source: set.rigid.then(|| Arc::downgrade(set)),
                    camera: *camera,
                    layer_filter: layer_filter.cloned(),
                    result,
                }
            });
        let result = query.result.clone();
        self.visibility.truncate(MAX_RETAINED_SETS - 1);
        self.visibility.push_front(query);
        result
    }

    pub fn color_plan(
        &mut self,
        gpu: (&wgpu::Device, &wgpu::Queue),
        set: &Arc<InstanceSet>,
        visibility: &Arc<SceneVisibility>,
        objects: &[SceneObject],
        include_blended: bool,
        (probes, lightmaps): (Option<Arc<[u8]>>, bool),
    ) -> Option<Arc<ColorPlan>> {
        let lightmaps = lightmaps && set.lightmap_parameters.is_some();
        if visibility.visible_count() == 0
            || !lightmaps && probes.is_none() && visibility.visible_count() == objects.len()
        {
            return None;
        }
        let index = self.color_plans.iter().position(|plan| {
            plan.source.as_ptr() == Arc::as_ptr(set)
                && plan.visibility == *visibility
                && plan.include_blended == include_blended
                && plan.probes == probes
                && plan.lightmaps == lightmaps
        });
        let plan = index
            .and_then(|index| self.color_plans.remove(index))
            .unwrap_or_else(|| {
                let plan = ColorPlan::new(
                    gpu.0,
                    set,
                    visibility,
                    objects,
                    include_blended,
                    (probes, lightmaps),
                    |rows| {
                        let previous = self
                            .color_plans
                            .iter()
                            .filter(|plan| plan.compact_rows.len() == rows.len())
                            .filter_map(|plan| plan.compact.as_ref().map(|buffer| (plan, buffer)))
                            .find(|(_, buffer)| !self.transfers.protected.contains(*buffer))
                            .map(|(plan, buffer)| (buffer, plan.compact_rows.iter().copied()));
                        let (buffer, update) = self.transfers.upload(
                            gpu.0,
                            gpu.1,
                            "XRack visible 3D instances",
                            rows.iter().copied(),
                            previous,
                        );
                        if update == Upload::Patched {
                            self.color_plans
                                .retain(|plan| plan.compact.as_ref() != Some(&buffer));
                        }
                        self.color_uploads = self
                            .color_uploads
                            .saturating_add(u64::from(update != Upload::Unchanged));
                        buffer
                    },
                );
                Arc::new(plan)
            });
        if let Some(buffer) = &plan.compact {
            self.transfers.use_buffer(buffer);
        }
        self.color_plans.truncate(MAX_RETAINED_SETS - 1);
        self.color_plans.push_front(plan.clone());
        Some(plan)
    }

    pub fn get(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        objects: &Arc<[SceneObject]>,
        selected: &str,
    ) -> Arc<InstanceSet> {
        let index = self
            .sets
            .iter()
            .position(|set| set.matches(objects, selected));
        let set = index
            .and_then(|index| self.sets.remove(index))
            .unwrap_or_else(|| {
                let geometry_only = self
                    .sets
                    .iter()
                    .find(|set| set.matches_vertex_data(objects, selected))
                    .cloned();
                Arc::new(InstanceSet::new(
                    device,
                    objects,
                    selected,
                    geometry_only.as_deref(),
                    |keys| {
                        let previous = self
                            .sets
                            .iter()
                            .find(|set| {
                                set.keys.len() == keys.len()
                                    && !self.transfers.protected.contains(&set.buffer)
                            })
                            .map(|set| (&set.buffer, set.keys.iter().map(|key| key.vertex)));
                        let (buffer, update) = self.transfers.upload(
                            device,
                            queue,
                            "XRack 3D instances",
                            keys.iter().map(|key| key.vertex),
                            previous,
                        );
                        if update == Upload::Patched {
                            // Several material snapshots may share these vertex bytes.
                            // Evict every alias before associating the buffer with its new data.
                            self.sets.retain(|set| set.buffer != buffer);
                        }
                        self.uploads = self
                            .uploads
                            .saturating_add(u64::from(update != Upload::Unchanged));
                        buffer
                    },
                ))
            });
        self.transfers.use_buffer(&set.buffer);
        self.sets.truncate(MAX_RETAINED_SETS - 1);
        self.sets.push_front(set.clone());
        set
    }
}

struct VisibilityQuery {
    source: Weak<[SceneObject]>,
    rigid_source: Option<Weak<InstanceSet>>,
    camera: SceneCamera,
    layer_filter: Option<Arc<SceneVisibility>>,
    result: Arc<SceneVisibility>,
}

pub(super) struct ColorPlan {
    source: Weak<InstanceSet>,
    visibility: Arc<SceneVisibility>,
    include_blended: bool,
    probes: Option<Arc<[u8]>>,
    lightmaps: bool,
    pub compact_lightmaps: Option<wgpu::Buffer>,
    pub compact: Option<wgpu::Buffer>,
    compact_rows: Vec<InstanceVertex>,
    pub draws: Vec<ColorDraw>,
}

#[derive(Clone)]
pub(super) struct ColorDraw {
    pub source: u32,
    pub instances: Range<u32>,
    pub compacted: bool,
}

impl ColorPlan {
    fn new(
        device: &wgpu::Device,
        set: &Arc<InstanceSet>,
        visibility: &Arc<SceneVisibility>,
        objects: &[SceneObject],
        include_blended: bool,
        (probes, lightmaps): (Option<Arc<[u8]>>, bool),
        upload: impl FnOnce(&[InstanceVertex]) -> wgpu::Buffer,
    ) -> Self {
        let mut compact_rows = Vec::new();
        let mut packed_lightmaps = Vec::new();
        let lightmap_keys = set.lightmaps.as_deref().filter(|_| lightmaps);
        let mut draws = Vec::new();
        for original in set
            .draws
            .iter()
            .flat_map(|range| probe_runs(range.clone(), probes.as_deref()))
            .flat_map(|range| lightmap_runs(range, lightmap_keys))
        {
            let mesh = &objects[original.start as usize].mesh;
            if !include_blended
                && !mesh.parts().is_empty()
                && mesh.parts().len() == mesh.blended_parts().len()
            {
                continue;
            }
            let mut ranges = visibility.runs(original.clone());
            let Some(first) = ranges.next() else {
                continue;
            };
            let Some(second) = ranges.next() else {
                draws.push(ColorDraw {
                    source: first.start,
                    instances: first,
                    compacted: false,
                });
                continue;
            };
            // A fragmented run still needs one draw. Copy just its visible
            // instance rows into a separate buffer used by the color pass.
            let start = compact_rows.len() as u32;
            for index in [first, second].into_iter().chain(ranges).flatten() {
                compact_rows.push(set.keys[index as usize].vertex);
                if let Some(lightmaps) = lightmap_keys {
                    append_floats(
                        &mut packed_lightmaps,
                        lightmap_words(lightmaps[index as usize]),
                    );
                }
            }
            let end = compact_rows.len() as u32;
            draws.push(ColorDraw {
                source: original.start,
                instances: start..end,
                compacted: true,
            });
        }
        let compact = (!compact_rows.is_empty()).then(|| upload(&compact_rows));
        Self {
            source: Arc::downgrade(set),
            visibility: visibility.clone(),
            include_blended,
            probes,
            lightmaps,
            compact_lightmaps: (!packed_lightmaps.is_empty()).then(|| {
                buffer(
                    device,
                    "XRack compacted lightmap parameters",
                    &packed_lightmaps,
                    wgpu::BufferUsages::STORAGE,
                )
            }),
            compact,
            compact_rows,
            draws,
        }
    }
}

fn probe_runs(mut range: Range<u32>, slots: Option<&[u8]>) -> impl Iterator<Item = Range<u32>> {
    std::iter::from_fn(move || {
        let start = range.start;
        if start == range.end {
            return None;
        }
        let end = slots.map_or(range.end, |slots| {
            let slot = slots[start as usize];
            let run = &slots[start as usize..range.end as usize];
            run.iter()
                .position(|&other| other != slot)
                .map_or(range.end, |len| start + len as u32)
        });
        range.start = end;
        Some(start..end)
    })
}

fn lightmap_runs(
    mut range: Range<u32>,
    keys: Option<&[Option<InstanceLightmap>]>,
) -> impl Iterator<Item = Range<u32>> + '_ {
    std::iter::from_fn(move || {
        if range.is_empty() {
            return None;
        }
        let start = range.start;
        let end = keys.map_or(range.end, |keys| {
            let texture = keys[start as usize].map(|map| map.texture);
            keys[(start + 1) as usize..range.end as usize]
                .iter()
                .position(|key| key.map(|map| map.texture) != texture)
                .map_or(range.end, |len| start + len as u32 + 1)
        });
        range.start = end;
        Some(start..end)
    })
}

pub(super) struct InstanceSet {
    source: Weak<[SceneObject]>,
    selected: String,
    keys: Vec<InstanceKey>,
    rigid: bool,
    rigid_visibility: OnceLock<Option<VisibilityIndex>>,
    pub buffer: wgpu::Buffer,
    lightmaps: Option<Box<[Option<InstanceLightmap>]>>,
    pub lightmap_parameters: Option<wgpu::Buffer>,
    pub draws: Vec<Range<u32>>,
    transparent: Vec<TransparentPart>,
    sorted_blends: Vec<u32>,
    prepasses: [Vec<u32>; 2],
    blended_depth: Vec<u32>,
    transmissive: Vec<(u32, f32, bool, bool)>,
}

pub(super) struct TransparentPart {
    pub instance: u32,
    pub part: usize,
    blend_slot: usize,
    center: [f32; 3],
}

// Key immutable GPU instances without extending the lifetime of imported
// meshes. A changed model can reuse the buffer when it projects identically.
enum MeshKey {
    Box,
    Sphere,
    Imported(Weak<MeshData>, Weak<MeshVertexStreams>),
}

impl MeshKey {
    fn from_mesh(mesh: &Mesh3D) -> Self {
        match mesh {
            Mesh3D::Box => Self::Box,
            Mesh3D::Sphere => Self::Sphere,
            Mesh3D::Imported(mesh) => {
                Self::Imported(Arc::downgrade(mesh), Arc::downgrade(mesh.vertex_streams()))
            }
        }
    }

    fn matches(&self, mesh: &Mesh3D) -> bool {
        match (self, mesh) {
            (Self::Box, Mesh3D::Box) | (Self::Sphere, Mesh3D::Sphere) => true,
            (Self::Imported(a, _), Mesh3D::Imported(b)) => a.as_ptr() == Arc::as_ptr(b),
            _ => false,
        }
    }
    fn matches_geometry(&self, mesh: &Mesh3D) -> bool {
        match (self, mesh) {
            (Self::Box, Mesh3D::Box) | (Self::Sphere, Mesh3D::Sphere) => true,
            (Self::Imported(_, geometry), Mesh3D::Imported(mesh)) => {
                geometry.as_ptr() == Arc::as_ptr(mesh.vertex_streams())
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct InstanceLightmap {
    texture: ImageIdentity,
    words: [f32; 8],
}

impl InstanceLightmap {
    fn for_object(object: &SceneObject) -> Option<Self> {
        let lightmap = object.lightmap.as_ref()?;
        Some(Self {
            texture: lightmap.identity(),
            words: lightmap.instance_words(),
        })
    }
}

struct InstanceKey {
    mesh: MeshKey,
    vertex: InstanceVertex,
}

#[derive(Clone, Copy, PartialEq)]
struct InstanceVertex {
    transform: [[f32; 4]; 3],
    color: Color,
    flags: u8,
}

fn lightmap_words(lightmap: Option<InstanceLightmap>) -> [f32; 8] {
    lightmap.map_or([0.0; 8], |lightmap| lightmap.words)
}

impl InstanceVertex {
    fn from_object(object: &SceneObject, selected: &str) -> Self {
        Self {
            transform: object.transform_rows(),
            color: object.color,
            flags: flags(object, selected),
        }
    }

    fn pack(&self, bytes: &mut Vec<u8>) {
        let [r, g, b, a] = self.color.0.to_be_bytes().map(|c| f32::from(c) / 255.0);
        append_floats(bytes, self.transform.into_iter().flatten());
        append_floats(bytes, [r, g, b, a, f32::from(self.flags)]);
    }
}

fn flags(object: &SceneObject, selected: &str) -> u8 {
    u8::from(!selected.is_empty() && selected == object.id)
        | (u8::from(object.receives_shadow) << 1)
}

pub(super) fn mesh_material_prepass(
    mesh: &Mesh3D,
    renderables: CaptureRenderables,
    stage: usize,
) -> bool {
    if renderables == CaptureRenderables::All {
        return mesh.depth_participation().0[stage];
    }
    mesh.parts().iter().any(|part| {
        let material = &mesh.materials()[part.material];
        renderables.includes(material.is_blended())
            && material.depth_draw_mode == MeshDepthDraw::OpaquePrepass
            && usize::from(material.transmission_blur().is_some()) == stage
    })
}

pub(super) fn mesh_transmission(
    mesh: &Mesh3D,
    renderables: CaptureRenderables,
    fading: bool,
) -> Option<(f32, bool, bool)> {
    if renderables == CaptureRenderables::All {
        return Some((
            mesh.transmission_blur()?,
            !fading && mesh.has_depth_writing_transmission(),
            mesh.has_volume(),
        ));
    }
    mesh.parts()
        .iter()
        .filter_map(|part| {
            let material = &mesh.materials()[part.material];
            let blended = fading || material.is_blended();
            if !renderables.includes(blended) {
                return None;
            }
            let blur = material.transmission_blur()?;
            let volume = material
                .pbr
                .as_ref()
                .is_some_and(|pbr| pbr.volume().is_some());
            Some((blur, !blended, volume))
        })
        .reduce(|(a, aw, av), (b, bw, bv)| (a.max(b), aw || bw, av || bv))
}

impl InstanceSet {
    pub fn has_visible_transparency(&self, visibility: &SceneVisibility) -> bool {
        self.transparent
            .iter()
            .any(|part| visibility.contains(part.instance))
    }

    pub fn requires_sorted_transparency(&self, visibility: &SceneVisibility) -> bool {
        self.sorted_blends
            .iter()
            .any(|&index| visibility.contains(index))
    }

    pub fn visible_prepasses(
        &self,
        visibility: &SceneVisibility,
        objects: &[SceneObject],
        renderables: CaptureRenderables,
    ) -> [bool; 2] {
        std::array::from_fn(|stage| {
            self.prepasses[stage].iter().any(|&index| {
                visibility.contains(index)
                    && (renderables == CaptureRenderables::All
                        || mesh_material_prepass(&objects[index as usize].mesh, renderables, stage))
            })
        })
    }

    pub fn has_visible_blended_depth(&self, visibility: &SceneVisibility) -> bool {
        self.blended_depth
            .iter()
            .any(|&index| visibility.contains(index))
    }

    pub fn visible_transmission(
        &self,
        visibility: &SceneVisibility,
        objects: &[SceneObject],
        renderables: CaptureRenderables,
    ) -> Option<(f32, bool, bool)> {
        self.transmissive
            .iter()
            .filter(|&&(instance, ..)| visibility.contains(instance))
            .filter_map(|&(instance, blur, writes_depth, volume)| {
                if renderables == CaptureRenderables::All {
                    Some((blur, writes_depth, volume))
                } else {
                    mesh_transmission(&objects[instance as usize].mesh, renderables, false)
                }
            })
            .reduce(|(a, aw, av), (b, bw, bv)| (a.max(b), aw || bw, av || bv))
    }

    pub fn color_draws<'a>(
        &'a self,
        plan: Option<&'a ColorPlan>,
        visibility: &SceneVisibility,
    ) -> impl Iterator<Item = ColorDraw> + 'a {
        let direct = plan.is_none() && visibility.visible_count() != 0;
        self.draws
            .iter()
            .filter(move |_| direct)
            .map(|range| ColorDraw {
                source: range.start,
                instances: range.clone(),
                compacted: false,
            })
            .chain(plan.into_iter().flat_map(|plan| plan.draws.iter().cloned()))
    }

    fn matches_vertex_data(&self, objects: &[SceneObject], selected: &str) -> bool {
        self.keys.len() == objects.len()
            && self
                .keys
                .iter()
                .zip(objects)
                .enumerate()
                .all(|(index, (key, object))| {
                    key.mesh.matches_geometry(&object.mesh)
                        && key.vertex == InstanceVertex::from_object(object, selected)
                        && self.lightmaps.as_ref().and_then(|maps| maps[index])
                            == InstanceLightmap::for_object(object)
                })
    }

    fn matches(&self, objects: &Arc<[SceneObject]>, selected: &str) -> bool {
        self.selected == selected
            && (std::ptr::addr_eq(self.source.as_ptr(), Arc::as_ptr(objects))
                || self.matches_vertex_data(objects, selected)
                    && self
                        .keys
                        .iter()
                        .zip(objects.iter())
                        .all(|(key, object)| key.mesh.matches(&object.mesh)))
    }

    fn new(
        device: &wgpu::Device,
        objects: &Arc<[SceneObject]>,
        selected: &str,
        retained: Option<&Self>,
        upload: impl FnOnce(&[InstanceKey]) -> wgpu::Buffer,
    ) -> Self {
        let mut draws: Vec<Range<u32>> = Vec::new();
        let mut keys = Vec::with_capacity(objects.len());
        let mut lightmaps: Option<Vec<Option<InstanceLightmap>>> = None;
        let mut transparent = Vec::new();
        let mut sorted_blends = Vec::new();
        let mut prepasses: [Vec<u32>; 2] = Default::default();
        let mut blended_depth = Vec::new();
        let mut transmissive = Vec::new();
        let mut rigid = true;
        for (index, object) in objects.iter().enumerate() {
            let vertex = InstanceVertex::from_object(object, selected);
            let transform = vertex.transform;
            let key = InstanceKey {
                mesh: MeshKey::from_mesh(&object.mesh),
                vertex,
            };
            let lightmap = InstanceLightmap::for_object(object);
            if lightmap.is_some() || lightmaps.is_some() {
                lightmaps
                    .get_or_insert_with(|| {
                        let mut maps = Vec::with_capacity(objects.len());
                        maps.resize(index, None);
                        maps
                    })
                    .push(lightmap);
            }
            keys.push(key);
            let instance = index as u32;
            let (depth_prepasses, depth_transparency) = object.mesh.depth_participation();
            for (rows, enabled) in prepasses.iter_mut().zip(depth_prepasses) {
                if enabled {
                    rows.push(instance);
                }
            }
            if depth_transparency {
                blended_depth.push(instance);
            }
            if let Some(blur) = object.mesh.transmission_blur() {
                transmissive.push((
                    instance,
                    blur,
                    object.mesh.has_depth_writing_transmission(),
                    object.mesh.has_volume(),
                ));
            }
            if object.color.0 as u8 != 0 {
                for (blend_slot, &(part, [x, y, z])) in
                    object.mesh.blended_parts().iter().enumerate()
                {
                    let material = object.mesh.parts()[part].material;
                    let material = &object.mesh.materials()[material];
                    let ordered = material.blend_mode != MeshBlendMode::SourceOver
                        || material.depth_draw_mode == MeshDepthDraw::Always;
                    if ordered && sorted_blends.last() != Some(&instance) {
                        sorted_blends.push(instance);
                    }
                    transparent.push(TransparentPart {
                        instance,
                        part,
                        blend_slot,
                        center: transform.map(|[a, b, c, offset]| a * x + b * y + c * z + offset),
                    });
                }
            }
            let instance_rigid =
                object.mesh.influences().is_none() && object.mesh.morph_geometry().is_none();
            rigid &= instance_rigid;
            // Preserve scene order at equal depth, batching adjacent meshes.
            match draws.last_mut() {
                Some(range)
                    if instance_rigid && objects[range.start as usize].mesh == object.mesh =>
                {
                    range.end += 1
                }
                _ => draws.push(instance..instance + 1),
            }
        }
        let vertex_buffer = retained.map_or_else(|| upload(&keys), |set| set.buffer.clone());
        let lightmap_parameters = if let Some(set) = retained {
            set.lightmap_parameters.clone()
        } else {
            lightmaps.as_ref().map(|maps| {
                let bytes: Vec<_> = maps
                    .iter()
                    .copied()
                    .flat_map(lightmap_words)
                    .flat_map(f32::to_ne_bytes)
                    .collect();
                buffer(
                    device,
                    "XRack lightmap instance parameters",
                    &bytes,
                    wgpu::BufferUsages::STORAGE,
                )
            })
        };
        Self {
            source: Arc::downgrade(objects),
            selected: selected.to_owned(),
            keys,
            rigid,
            rigid_visibility: OnceLock::new(),
            buffer: vertex_buffer,
            lightmaps: lightmaps.map(Vec::into_boxed_slice),
            lightmap_parameters,
            draws,
            transparent,
            sorted_blends,
            prepasses,
            blended_depth,
            transmissive,
        }
    }

    pub fn transparent_draws(
        &self,
        camera: &SceneCamera,
        objects: &[SceneObject],
        visibility: &SceneVisibility,
    ) -> impl Iterator<Item = &TransparentPart> {
        let mut draws: Vec<_> = self
            .transparent
            .iter()
            .filter(|part| visibility.contains(part.instance))
            .filter_map(|part| {
                let object = &objects[part.instance as usize];
                let center = if object.skin_palette().is_some() || object.morph_weights().is_some()
                {
                    let (_, [x, y, z]) = object.blended_parts()[part.blend_slot];
                    object
                        .transform_rows()
                        .map(|[a, b, c, offset]| a * x + b * y + c * z + offset)
                } else {
                    part.center
                };
                let depth = camera.view_depth(center);
                depth.is_finite().then_some((depth, part))
            })
            .collect();
        draws.sort_unstable_by(|(a_depth, a), (b_depth, b)| {
            b_depth
                .total_cmp(a_depth)
                .then_with(|| a.instance.cmp(&b.instance))
                .then_with(|| a.part.cmp(&b.part))
        });
        draws.into_iter().map(|(_, part)| part)
    }
}
