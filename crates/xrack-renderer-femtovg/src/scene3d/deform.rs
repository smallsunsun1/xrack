use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use xrack_runtime::scene3d::{
    JointPalette, MAX_MORPH_TARGETS, MAX_SKIN_JOINTS, Mesh3D, MeshGeometry, MeshInfluence,
    MorphWeights, SceneObject,
};

use super::{MeshCache, append_floats, buffer, wireframe::RasterMode};

pub(super) const SKIN_LAYOUT: Option<wgpu::VertexBufferLayout<'static>> = Some(
    wgpu::VertexBufferLayout {
        array_stride: MeshInfluence::STRIDE as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![11 => Uint16x4, 12 => Uint16x4, 13 => Float32x4, 14 => Float32x4],
    },
);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Deformation {
    Rigid,
    Skin,
    Morph,
    SkinMorph,
}

impl Deformation {
    pub const ALL: [Self; 4] = [Self::Rigid, Self::Skin, Self::Morph, Self::SkinMorph];

    pub fn skin(self) -> bool {
        matches!(self, Self::Skin | Self::SkinMorph)
    }
    pub fn morph(self) -> bool {
        matches!(self, Self::Morph | Self::SkinMorph)
    }
}

#[derive(Clone)]
pub(super) struct DeformationLayouts([wgpu::BindGroupLayout; 3]);

impl DeformationLayouts {
    pub fn new(device: &wgpu::Device) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        Self(std::array::from_fn(|index| {
            let mode = Deformation::ALL[index + 1];
            let mut entries = Vec::new();
            if mode.skin() {
                entries.push(entry(0, wgpu::BufferBindingType::Uniform));
            }
            if mode.morph() {
                entries.push(entry(1, wgpu::BufferBindingType::Uniform));
                entries.push(entry(
                    2,
                    wgpu::BufferBindingType::Storage { read_only: true },
                ));
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("XRack glTF deformation layout"),
                entries: &entries,
            })
        }))
    }

    pub fn get(&self, mode: Deformation) -> Option<&wgpu::BindGroupLayout> {
        (mode as usize).checked_sub(1).map(|index| &self.0[index])
    }
}

pub(super) fn shader_source(source: &str) -> wgpu::ShaderSource<'static> {
    wgpu::ShaderSource::Wgsl(
        format!(
            "{}\n{}\n{}\n{}\n{}\n{source}",
            include_str!("skin.wgsl"),
            include_str!("morph.wgsl"),
            xrack_runtime::material_shader::TEXTURE_FOOTPRINT,
            include_str!("uv.wgsl"),
            include_str!("lod_coverage.wgsl"),
        )
        .into(),
    )
}

#[derive(Clone, Copy)]
pub(super) struct GeometryBindings<'a> {
    pub instances: &'a wgpu::Buffer,
    pub instance_data: Option<&'a wgpu::Buffer>,
    pub offsets: &'a [u32],
    pub zero_data: &'a wgpu::Buffer,
    pub deformations: &'a [Option<DeformationBinding>],
    pub wire_deformations: Option<&'a [Option<DeformationBinding>]>,
    pub lod_fade: bool,
}

impl GeometryBindings<'_> {
    pub fn offset(&self, instance: u32) -> u32 {
        self.offsets
            .get(instance as usize)
            .copied()
            .unwrap_or_default()
    }

    pub fn bind_instances(&self, pass: &mut wgpu::RenderPass<'_>, offset: u32) {
        pass.set_vertex_buffer(
            1,
            self.instances
                .slice(u64::from(offset) * super::INSTANCE_STRIDE..),
        );
        pass.set_vertex_buffer(
            4,
            self.instance_data.map_or_else(
                || self.zero_data.slice(..),
                |data| data.slice(u64::from(offset) * super::instances::INSTANCE_DATA_STRIDE..),
            ),
        );
    }

    pub fn bind_instance_range(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        active: &mut Option<u32>,
        first: u32,
    ) {
        let offset = self.offset(first);
        if *active != Some(offset) {
            self.bind_instances(pass, offset);
            *active = Some(offset);
        }
    }

    pub fn runs(
        &self,
        mut range: std::ops::Range<u32>,
    ) -> impl Iterator<Item = std::ops::Range<u32>> + '_ {
        std::iter::from_fn(move || {
            if range.is_empty() {
                return None;
            }
            let start = range.start;
            let end = if self.offsets.is_empty() {
                range.end
            } else {
                let offset = self.offsets[start as usize];
                start
                    + self.offsets[start as usize..range.end as usize]
                        .partition_point(|&candidate| candidate == offset)
                        as u32
            };
            range.start = end;
            Some(start..end)
        })
    }

    pub fn deformation(&self, instance: u32) -> Option<&DeformationBinding> {
        self.deformation_for(instance, RasterMode::Fill)
    }

    pub fn deformation_for(
        &self,
        instance: u32,
        raster: RasterMode,
    ) -> Option<&DeformationBinding> {
        self.deformations_for(raster)
            .get(instance as usize)?
            .as_ref()
    }

    pub fn deformations_for(&self, raster: RasterMode) -> &[Option<DeformationBinding>] {
        if raster == RasterMode::ShaderLine {
            self.wire_deformations.unwrap_or(self.deformations)
        } else {
            self.deformations
        }
    }
}

pub(super) fn bind_instance_streams(
    pass: &mut wgpu::RenderPass<'_>,
    instances: &wgpu::Buffer,
    data: &wgpu::Buffer,
) {
    pass.set_vertex_buffer(1, instances.slice(..));
    pass.set_vertex_buffer(4, data.slice(..));
}

#[derive(Clone)]
pub(super) struct DeformationBinding {
    pub mode: Deformation,
    pub group: wgpu::BindGroup,
    morph_source: Option<Weak<MeshGeometry>>,
}

#[derive(PartialEq)]
struct BindingKey {
    palette: Option<wgpu::Buffer>,
    morph: Option<(wgpu::Buffer, wgpu::Buffer)>,
}

pub(super) struct DeformationBuffers {
    layouts: DeformationLayouts,
    palettes: VecDeque<(JointPalette, wgpu::Buffer)>,
    morphs: VecDeque<(([u32; 2], MorphWeights), wgpu::Buffer)>,
    bindings: VecDeque<(BindingKey, DeformationBinding)>,
    pub skin_uploads: u64,
    pub morph_uploads: u64,
}

impl DeformationBuffers {
    pub fn new(layouts: DeformationLayouts) -> Self {
        Self {
            layouts,
            palettes: VecDeque::new(),
            morphs: VecDeque::new(),
            bindings: VecDeque::new(),
            skin_uploads: 0,
            morph_uploads: 0,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (meshes, raster): (&mut MeshCache, RasterMode),
        objects: &[SceneObject],
        eligible: impl Fn(u32, &SceneObject) -> bool,
    ) -> Vec<Option<DeformationBinding>> {
        self.bindings.retain(|(_, binding)| {
            binding
                .morph_source
                .as_ref()
                .is_none_or(|source| source.strong_count() != 0)
        });
        if !objects.iter().enumerate().any(|(index, object)| {
            (object.skin_palette().is_some() || object.morph_weights().is_some())
                && eligible(index as u32, object)
        }) {
            return Vec::new();
        }
        objects
            .iter()
            .enumerate()
            .map(|(index, object)| {
                if !eligible(index as u32, object) {
                    return None;
                }
                let palette = object
                    .skin_palette()
                    .map(|pose| self.palette_buffer(device, pose));
                let mut morph_source = None;
                let morph = if let Some(weights) = object.morph_weights() {
                    let Mesh3D::Imported(source) = &object.mesh else {
                        return None;
                    };
                    morph_source = Some(Arc::downgrade(source.geometry()));
                    let mesh = meshes.get_for_raster(device, queue, &object.mesh, raster);
                    let morph = mesh.morph.as_ref()?;
                    Some((
                        self.morph_buffer(device, morph.table_layout, weights),
                        morph.entries.clone(),
                    ))
                } else {
                    None
                };
                let mode = match (palette.is_some(), morph.is_some()) {
                    (true, false) => Deformation::Skin,
                    (false, true) => Deformation::Morph,
                    (true, true) => Deformation::SkinMorph,
                    (false, false) => return None,
                };
                let layout = self.layouts.get(mode)?;
                Some(cached(
                    &mut self.bindings,
                    BindingKey { palette, morph },
                    |key| {
                        let mut entries = Vec::new();
                        if let Some(palette) = &key.palette {
                            entries.push(wgpu::BindGroupEntry {
                                binding: 0,
                                resource: palette.as_entire_binding(),
                            });
                        }
                        if let Some((weights, data)) = &key.morph {
                            entries.extend([
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: weights.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: data.as_entire_binding(),
                                },
                            ]);
                        }
                        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("XRack glTF deformation bindings"),
                            layout,
                            entries: &entries,
                        });
                        DeformationBinding {
                            mode,
                            group,
                            morph_source,
                        }
                    },
                ))
            })
            .collect()
    }

    fn palette_buffer(&mut self, device: &wgpu::Device, pose: &JointPalette) -> wgpu::Buffer {
        cached(&mut self.palettes, pose.clone(), |pose| {
            let mut bytes = Vec::with_capacity(MAX_SKIN_JOINTS * 48);
            append_floats(&mut bytes, pose.iter().flatten().flatten().copied());
            bytes.resize(MAX_SKIN_JOINTS * 48, 0);
            self.skin_uploads = self.skin_uploads.saturating_add(1);
            buffer(
                device,
                "XRack glTF joint matrices",
                &bytes,
                wgpu::BufferUsages::UNIFORM,
            )
        })
    }

    fn morph_buffer(
        &mut self,
        device: &wgpu::Device,
        layout: [u32; 2],
        weights: &MorphWeights,
    ) -> wgpu::Buffer {
        cached(
            &mut self.morphs,
            (layout, weights.clone()),
            |(layout, weights)| {
                let mut bytes = Vec::with_capacity(16 + MAX_MORPH_TARGETS * 4);
                bytes.extend(
                    [layout[0], layout[1], 0, 0]
                        .into_iter()
                        .flat_map(u32::to_ne_bytes),
                );
                append_floats(&mut bytes, weights.iter().copied());
                bytes.resize(16 + MAX_MORPH_TARGETS * 4, 0);
                self.morph_uploads = self.morph_uploads.saturating_add(1);
                buffer(
                    device,
                    "XRack glTF morph weights",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                )
            },
        )
    }
}

fn cached<K: PartialEq, V: Clone>(
    entries: &mut VecDeque<(K, V)>,
    key: K,
    create: impl FnOnce(&K) -> V,
) -> V {
    const CACHE_SIZE: usize = 32;
    let index = entries.iter().position(|(old, _)| *old == key);
    let entry = index
        .and_then(|index| entries.remove(index))
        .unwrap_or_else(|| {
            let value = create(&key);
            (key, value)
        });
    let value = entry.1.clone();
    entries.truncate(CACHE_SIZE - 1);
    entries.push_front(entry);
    value
}
