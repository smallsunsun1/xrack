use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use xrack_runtime::{
    Node, Value,
    scene3d::{EnvironmentProbe, ReflectionProbe, ReflectionSource, SceneObject, SceneVisibility},
};

use super::{append_floats, buffer, reflection_capture::CapturedCube};

pub(super) struct EnvironmentInput<'a> {
    probe: &'a Arc<EnvironmentProbe>,
    intensity: f32,
    yaw: f32,
    pub skybox: bool,
}

impl<'a> EnvironmentInput<'a> {
    pub fn contributes_light(&self) -> bool {
        self.intensity > 0.0
    }

    pub fn from_node(node: &'a Node) -> Option<Self> {
        let Some(Value::SceneEnvironment(Some(probe))) = node.properties.get("environment") else {
            return None;
        };
        Some(Self {
            probe,
            intensity: node
                .float("environment-intensity")
                .unwrap_or(1.0)
                .clamp(0.0, 100.0),
            yaw: node
                .number("environment-yaw")
                .unwrap_or(0.0)
                .rem_euclid(360.0)
                .to_radians() as f32,
            skybox: node.boolean("environment-skybox", false),
        })
    }
}

struct ResidentProbe {
    identity: Weak<EnvironmentProbe>,
    view: wgpu::TextureView,
    binding: Option<([f32; 2], wgpu::BindGroup)>,
}

struct LocalBinding {
    probe: ReflectionProbe,
    ambient: Option<(Arc<EnvironmentProbe>, [f32; 2])>,
    captured: Option<wgpu::TextureView>,
    sample_origin: [f32; 3],
    group: wgpu::BindGroup,
}

struct ProbeAssignment {
    objects: Weak<[SceneObject]>,
    probes: Arc<[ReflectionProbe]>,
    layers: u64,
    slots: Option<Arc<[u8]>>,
}

pub(super) struct LocalReflections<'a> {
    pub slots: &'a [u8],
    pub bindings: &'a [Option<wgpu::BindGroup>],
}

impl LocalReflections<'_> {
    pub fn slot(&self, source: u32) -> u8 {
        self.slots[source as usize]
    }

    pub fn bind(&self, pass: &mut wgpu::RenderPass<'_>, slot: u8, fallback: &wgpu::BindGroup) {
        let binding = slot
            .checked_sub(1)
            .and_then(|i| self.bindings.get(usize::from(i)))
            .and_then(Option::as_ref)
            .unwrap_or(fallback);
        pass.set_bind_group(2, binding, &[]);
    }
}

pub(super) struct EnvironmentGpu {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    fallback: wgpu::BindGroup,
    probes: VecDeque<ResidentProbe>,
    local_bindings: VecDeque<LocalBinding>,
    assignments: VecDeque<ProbeAssignment>,
    pub uploads: u64,
}

impl EnvironmentGpu {
    pub fn retain_captured_bindings(&mut self, live: impl Fn(&wgpu::TextureView) -> bool) {
        self.local_bindings
            .retain(|entry| entry.captured.as_ref().is_none_or(&live));
    }

    pub fn new(device: &wgpu::Device, layout: wgpu::BindGroupLayout) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("XRack environment sampler"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let fallback = cube_texture(device, 1, 1).create_view(&cube_view());
        let bytes = environment_uniforms(None, None);
        let fallback = bind_probe(device, &layout, &fallback, &sampler, &bytes);
        Self {
            layout,
            sampler,
            fallback,
            probes: VecDeque::new(),
            local_bindings: VecDeque::new(),
            assignments: VecDeque::new(),
            uploads: 0,
        }
    }

    pub fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: Option<&EnvironmentInput<'_>>,
    ) -> wgpu::BindGroup {
        let Some(input) = input else {
            return self.fallback.clone();
        };
        let mut entry = self.resident(device, queue, input.probe);
        let settings = [input.yaw, input.intensity];
        let binding = if let Some((cached, binding)) = &entry.binding
            && *cached == settings
        {
            binding.clone()
        } else {
            let bytes = environment_uniforms(Some(input), None);
            let binding = bind_probe(device, &self.layout, &entry.view, &self.sampler, &bytes);
            entry.binding = Some((settings, binding.clone()));
            binding
        };
        self.retain(entry);
        binding
    }

    pub fn assignments(
        &mut self,
        probes: &Arc<[ReflectionProbe]>,
        objects: &Arc<[SceneObject]>,
        layers: u64,
    ) -> Option<Arc<[u8]>> {
        if probes.is_empty() {
            return None;
        }
        let hit = self.assignments.iter().position(|entry| {
            std::ptr::addr_eq(entry.objects.as_ptr(), Arc::as_ptr(objects))
                && entry.probes == *probes
                && entry.layers == layers
        });
        let entry = hit
            .and_then(|index| self.assignments.remove(index))
            .unwrap_or_else(|| {
                let slots: Arc<[u8]> = objects
                    .iter()
                    .map(|object| {
                        let matching = object.layers & layers;
                        if !object.receives_reflections
                            || matching == 0
                            || !object.mesh.sections().any(|(_, material)| {
                                material.is_some_and(|material| material.pbr.is_some())
                            })
                        {
                            return 0;
                        }
                        let origin = object.transform_rows().map(|row| row[3]);
                        ReflectionProbe::nearest(probes, origin, matching)
                            .map_or(0, |probe| (probe + 1) as u8)
                    })
                    .collect();
                ProbeAssignment {
                    objects: Arc::downgrade(objects),
                    probes: probes.clone(),
                    layers,
                    slots: slots.iter().any(|&slot| slot != 0).then_some(slots),
                }
            });
        let slots = entry.slots.clone();
        self.assignments.truncate(3);
        self.assignments.push_front(entry);
        slots
    }

    pub fn local_bindings(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        ambient: Option<&EnvironmentInput<'_>>,
        probes: &[ReflectionProbe],
        slots: &[u8],
        visibility: &SceneVisibility,
        captures: &[Option<CapturedCube>],
    ) -> Vec<Option<wgpu::BindGroup>> {
        let used = slots
            .iter()
            .enumerate()
            .filter(|&(index, slot)| *slot != 0 && visibility.contains(index as u32))
            .fold(0u16, |mask, (_, slot)| mask | (1 << slot));
        let ambient_key = ambient.map(|input| (input.probe.clone(), [input.yaw, input.intensity]));
        probes
            .iter()
            .enumerate()
            .map(|(index, probe)| {
                if used & (1 << (index + 1)) == 0 {
                    return None;
                }
                let capture = match &probe.source {
                    ReflectionSource::Panorama(_) => None,
                    ReflectionSource::Scene(_) => Some(captures.get(index)?.as_ref()?),
                };
                let view = capture.map(|cube| &cube.view);
                let sample_origin = capture.map_or(probe.position, |cube| cube.position);
                let index = self.local_bindings.iter().position(|entry| {
                    entry.probe == *probe
                        && entry.ambient == ambient_key
                        && entry.captured.as_ref() == view
                        && entry.sample_origin == sample_origin
                });
                let entry = index
                    .and_then(|index| self.local_bindings.remove(index))
                    .or_else(|| {
                        let group = match &probe.source {
                            ReflectionSource::Panorama(environment) => {
                                let texture = self.resident(device, queue, environment);
                                let bytes = environment_uniforms(
                                    ambient,
                                    Some((probe, EnvironmentProbe::LEVELS - 1, sample_origin)),
                                );
                                let group = bind_probe(
                                    device,
                                    &self.layout,
                                    &texture.view,
                                    &self.sampler,
                                    &bytes,
                                );
                                self.retain(texture);
                                group
                            }
                            ReflectionSource::Scene(_) => {
                                let cube = capture?;
                                let bytes = environment_uniforms(
                                    ambient,
                                    Some((probe, cube.last_mip, sample_origin)),
                                );
                                bind_probe(device, &self.layout, &cube.view, &self.sampler, &bytes)
                            }
                        };
                        Some(LocalBinding {
                            probe: probe.clone(),
                            ambient: ambient_key.clone(),
                            captured: view.cloned(),
                            sample_origin,
                            group,
                        })
                    })?;
                let group = entry.group.clone();
                self.local_bindings.truncate(15);
                self.local_bindings.push_front(entry);
                Some(group)
            })
            .collect()
    }

    fn resident(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        probe: &Arc<EnvironmentProbe>,
    ) -> ResidentProbe {
        self.probes
            .retain(|entry| entry.identity.strong_count() != 0);
        let index = self
            .probes
            .iter()
            .position(|entry| entry.identity.as_ptr() == Arc::as_ptr(probe));
        index
            .and_then(|index| self.probes.remove(index))
            .unwrap_or_else(|| {
                self.uploads = self.uploads.saturating_add(1);
                ResidentProbe {
                    identity: Arc::downgrade(probe),
                    view: upload_probe(device, queue, probe),
                    binding: None,
                }
            })
    }

    fn retain(&mut self, entry: ResidentProbe) {
        self.probes.truncate(8);
        self.probes.push_front(entry);
    }
}

fn environment_uniforms(
    global: Option<&EnvironmentInput<'_>>,
    local: Option<(&ReflectionProbe, u32, [f32; 3])>,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(224);
    let settings = |yaw: f32, intensity: f32, last_mip: u32| {
        let (sin, cos) = yaw.sin_cos();
        [sin, cos, intensity, last_mip as f32]
    };
    let global_settings = global.map_or([0.0, 1.0, 0.0, -1.0], |input| {
        settings(input.yaw, input.intensity, EnvironmentProbe::LEVELS - 1)
    });
    append_floats(&mut bytes, global_settings);
    for &[r, g, b] in global.map_or(&[[0.0; 3]; 9], |input| input.probe.diffuse()) {
        append_floats(&mut bytes, [r, g, b, 0.0]);
    }
    append_floats(
        &mut bytes,
        local.map_or(global_settings, |(probe, last_mip, _)| {
            settings(probe.yaw, probe.intensity, last_mip)
        }),
    );
    let [position, min, max] = local.map_or([[0.0; 4]; 3], |(probe, _, [x, y, z])| {
        let [a, b, c] = probe.min;
        let [d, e, f] = probe.max;
        [
            [x, y, z, f32::from(u8::from(probe.parallax))],
            [a, b, c, 0.0],
            [d, e, f, 0.0],
        ]
    });
    append_floats(&mut bytes, position.into_iter().chain(min).chain(max));
    bytes
}

fn extent([width, height]: [u32; 2], layers: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: layers,
    }
}

fn cube_texture(device: &wgpu::Device, size: u32, levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack prefiltered HDR environment"),
        size: extent([size, size], 6),
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgb9e5Ufloat,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn cube_view() -> wgpu::TextureViewDescriptor<'static> {
    wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    }
}

fn upload_probe(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    probe: &EnvironmentProbe,
) -> wgpu::TextureView {
    let texture = cube_texture(
        device,
        EnvironmentProbe::FACE_SIZE,
        EnvironmentProbe::LEVELS,
    );
    let mut offset = 0;
    for level in 0..EnvironmentProbe::LEVELS {
        let size = EnvironmentProbe::FACE_SIZE >> level;
        let length = (size * size * 6 * 4) as usize;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: level,
                ..texture.as_image_copy()
            },
            &probe.texels()[offset..offset + length],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            extent([size, size], 6),
        );
        offset += length;
    }
    texture.create_view(&cube_view())
}

fn bind_probe(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    bytes: &[u8],
) -> wgpu::BindGroup {
    let settings = buffer(
        device,
        "XRack IBL uniforms",
        bytes,
        wgpu::BufferUsages::UNIFORM,
    );
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("XRack IBL bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: settings.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(texture),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
