use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, OnceLock, Weak},
};

use xrack_runtime::{
    Color, ImageIdentity,
    scene3d::{
        Mesh3D, MeshData, SceneCamera, SceneGeometry, SceneObject, SceneVisibility, VisibilityIndex,
    },
};

use super::{INSTANCE_STRIDE, append_floats, buffer};

const MAX_RETAINED_SETS: usize = 4;

pub(super) struct InstanceBuffers {
    sets: VecDeque<Arc<InstanceSet>>,
    visibility: VecDeque<VisibilityQuery>,
    color_plans: VecDeque<Arc<ColorPlan>>,
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
        device: &wgpu::Device,
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
                    device,
                    set,
                    visibility,
                    objects,
                    include_blended,
                    (probes, lightmaps),
                );
                if plan.compact.is_some() {
                    self.color_uploads = self.color_uploads.saturating_add(1);
                }
                Arc::new(plan)
            });
        self.color_plans.truncate(MAX_RETAINED_SETS - 1);
        self.color_plans.push_front(plan.clone());
        Some(plan)
    }

    pub fn get(
        &mut self,
        device: &wgpu::Device,
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
                self.uploads = self.uploads.saturating_add(1);
                Arc::new(InstanceSet::new(device, objects, selected))
            });
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
    ) -> Self {
        let mut packed = Vec::new();
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
            let start = (packed.len() / INSTANCE_STRIDE as usize) as u32;
            for index in [first, second].into_iter().chain(ranges).flatten() {
                let key = &set.keys[index as usize];
                key.pack(&mut packed);
                if let Some(lightmaps) = lightmap_keys {
                    append_floats(
                        &mut packed_lightmaps,
                        lightmap_words(lightmaps[index as usize]),
                    );
                }
            }
            let end = (packed.len() / INSTANCE_STRIDE as usize) as u32;
            draws.push(ColorDraw {
                source: original.start,
                instances: start..end,
                compacted: true,
            });
        }
        let compact = (!packed.is_empty()).then(|| {
            buffer(
                device,
                "XRack visible 3D instances",
                &packed,
                wgpu::BufferUsages::VERTEX,
            )
        });
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
    Imported(Weak<MeshData>),
}

impl MeshKey {
    fn from_mesh(mesh: &Mesh3D) -> Self {
        match mesh {
            Mesh3D::Box => Self::Box,
            Mesh3D::Sphere => Self::Sphere,
            Mesh3D::Imported(mesh) => Self::Imported(Arc::downgrade(mesh)),
        }
    }

    fn matches(&self, mesh: &Mesh3D) -> bool {
        match (self, mesh) {
            (Self::Box, Mesh3D::Box) | (Self::Sphere, Mesh3D::Sphere) => true,
            (Self::Imported(a), Mesh3D::Imported(b)) => a.as_ptr() == Arc::as_ptr(b),
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
    transform: [[f32; 4]; 3],
    color: Color,
    flags: u8,
}

fn lightmap_words(lightmap: Option<InstanceLightmap>) -> [f32; 8] {
    lightmap.map_or([0.0; 8], |lightmap| lightmap.words)
}

impl InstanceKey {
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

impl InstanceSet {
    pub fn has_visible_transparency(&self, visibility: &SceneVisibility) -> bool {
        self.transparent
            .iter()
            .any(|part| visibility.contains(part.instance))
    }

    pub fn visible_transmission(&self, visibility: &SceneVisibility) -> Option<(f32, bool, bool)> {
        self.transmissive
            .iter()
            .filter_map(|&(instance, blur, writes_depth, volume)| {
                visibility
                    .contains(instance)
                    .then_some((blur, writes_depth, volume))
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

    fn matches(&self, objects: &Arc<[SceneObject]>, selected: &str) -> bool {
        self.selected == selected
            && (std::ptr::addr_eq(self.source.as_ptr(), Arc::as_ptr(objects))
                || self.keys.len() == objects.len()
                    && self.keys.iter().zip(objects.iter()).enumerate().all(
                        |(index, (key, object))| {
                            key.color == object.color
                                && key.flags == flags(object, selected)
                                && key.mesh.matches(&object.mesh)
                                && key.transform == object.transform_rows()
                                && self.lightmaps.as_ref().and_then(|maps| maps[index])
                                    == InstanceLightmap::for_object(object)
                        },
                    ))
    }

    fn new(device: &wgpu::Device, objects: &Arc<[SceneObject]>, selected: &str) -> Self {
        let mut bytes = Vec::with_capacity(objects.len() * INSTANCE_STRIDE as usize);
        let mut draws: Vec<Range<u32>> = Vec::new();
        let mut keys = Vec::with_capacity(objects.len());
        let mut lightmaps: Option<Vec<Option<InstanceLightmap>>> = None;
        let mut transparent = Vec::new();
        let mut transmissive = Vec::new();
        let mut rigid = true;
        for (index, object) in objects.iter().enumerate() {
            let flags = flags(object, selected);
            let transform = object.transform_rows();
            let key = InstanceKey {
                mesh: MeshKey::from_mesh(&object.mesh),
                transform,
                color: object.color,
                flags,
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
            key.pack(&mut bytes);
            keys.push(key);
            let instance = index as u32;
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
        let vertex_buffer = buffer(
            device,
            "XRack 3D instances",
            &bytes,
            wgpu::BufferUsages::VERTEX,
        );
        let lightmap_parameters = lightmaps.as_ref().map(|maps| {
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
        });
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
