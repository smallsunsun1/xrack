use std::{
    cell::{OnceCell, RefCell},
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
};

use xrack_runtime::Node;
pub(super) use xrack_runtime::scene3d::ViewportEnvironment as EnvironmentInput;
use xrack_runtime::scene3d::{
    EnvironmentOrientation, EnvironmentProbe, EnvironmentSource, ReflectionProbe, ReflectionSource,
    SceneObject, SceneVisibility, SkySettings,
};

use super::{append_floats, buffer, reflection_capture::CapturedCube, sky_cube::SkyGenerator};

pub(super) const SKYBOX_PROPERTIES: [&str; 7] = [
    "skybox-source",
    "skybox-sky",
    "skybox-yaw",
    "skybox-pitch",
    "skybox-roll",
    "skybox-intensity",
    "skybox-blur",
];

type SkyboxRows = [[f32; 4]; 3];
const LIGHTING_BYTES: usize = 224;
const UNIFORM_BYTES: usize = 480;

pub(super) struct SkyboxInput<'a> {
    source: EnvironmentSource<'a>,
    rows: SkyboxRows,
    shares_lighting: bool,
}

impl<'a> SkyboxInput<'a> {
    pub fn from_node(node: &'a Node, lighting: Option<&EnvironmentInput<'a>>) -> Option<Self> {
        if !node.boolean("environment-skybox", false) {
            return None;
        }
        let dedicated = EnvironmentSource::from_node(node, "skybox-sky", "skybox-source");
        let inherited = lighting.filter(|_| dedicated.is_none());
        let source = dedicated.or_else(|| inherited.map(|input| input.source))?;
        let angle = |name, fallback| {
            node.number(name)
                .filter(|value| value.is_finite())
                .map_or(fallback, |value| {
                    value.rem_euclid(360.0).to_radians() as f32
                })
        };
        let [yaw, pitch, roll] = inherited.map_or([0.0; 3], |input| input.orientation.radians());
        let orientation = EnvironmentOrientation::from_radians([
            angle("skybox-yaw", yaw),
            angle("skybox-pitch", pitch),
            angle("skybox-roll", roll),
        ]);
        let intensity = node
            .number("skybox-intensity")
            .filter(|value| !value.is_nan())
            .map_or_else(
                || inherited.map_or(1.0, |input| input.intensity),
                |value| value.clamp(0.0, 100.0) as f32,
            );
        let blur = node
            .float("skybox-blur")
            .filter(|value| !value.is_nan())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let shares_lighting = inherited.is_some_and(|input| {
            input.orientation == orientation
                && input.intensity == intensity
                && input.horizon == 0.0
                && blur == 0.0
        });
        Some(Self {
            source,
            rows: skybox_rows(orientation, intensity, blur * source.last_mip() as f32),
            shares_lighting,
        })
    }
}

fn skybox_rows(orientation: EnvironmentOrientation, intensity: f32, lod: f32) -> SkyboxRows {
    let [[ax, ay, az], [bx, by, bz], [cx, cy, cz]] = orientation.rows();
    [
        [ax, ay, az, intensity],
        [bx, by, bz, lod],
        [cx, cy, cz, 0.0],
    ]
}

#[derive(Clone, Copy, PartialEq)]
struct LightingSettings {
    orientation: EnvironmentOrientation,
    intensity: f32,
    horizon: f32,
}

impl From<&EnvironmentInput<'_>> for LightingSettings {
    fn from(input: &EnvironmentInput<'_>) -> Self {
        Self {
            orientation: input.orientation,
            intensity: input.intensity,
            horizon: input.horizon,
        }
    }
}

struct ResidentProbe {
    identity: Weak<EnvironmentProbe>,
    view: wgpu::TextureView,
    binding: Option<(LightingSettings, wgpu::BindGroup)>,
    skybox: Option<(SkyboxRows, wgpu::BindGroup)>,
}

struct LocalBinding {
    probe: ReflectionProbe,
    ambient: Option<(EnvironmentIdentity, LightingSettings)>,
    captured: Option<wgpu::TextureView>,
    sample_origin: [f32; 3],
    group: wgpu::BindGroup,
}

#[derive(Clone, PartialEq)]
enum EnvironmentIdentity {
    Panorama(Arc<EnvironmentProbe>),
    Sky(SkySettings),
}

struct ResidentSky {
    settings: SkySettings,
    horizon: f32,
    view: wgpu::TextureView,
    binding: Option<(LightingSettings, wgpu::BindGroup)>,
    skybox: Option<(SkyboxRows, wgpu::BindGroup)>,
    committed: bool,
}

struct MaskedProbe {
    identity: Weak<EnvironmentProbe>,
    horizon: f32,
    view: wgpu::TextureView,
    binding: Option<(LightingSettings, wgpu::BindGroup)>,
    committed: bool,
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

    fn binding<'a>(&'a self, slot: u8, fallback: &'a wgpu::BindGroup) -> &'a wgpu::BindGroup {
        slot.checked_sub(1)
            .and_then(|i| self.bindings.get(usize::from(i)))
            .and_then(Option::as_ref)
            .unwrap_or(fallback)
    }
}

type PanoramaKey = (wgpu::TextureView, wgpu::Sampler);

pub(super) struct MaterialReflections<'a> {
    device: &'a wgpu::Device,
    layout: wgpu::BindGroupLayout,
    global: Option<&'a EnvironmentInput<'a>>,
    uniform: OnceCell<wgpu::Buffer>,
    groups: RefCell<HashMap<PanoramaKey, wgpu::BindGroup>>,
}

impl<'a> MaterialReflections<'a> {
    pub fn new(
        device: &'a wgpu::Device,
        layout: wgpu::BindGroupLayout,
        global: Option<&'a EnvironmentInput<'a>>,
    ) -> Self {
        Self {
            device,
            layout,
            global,
            uniform: OnceCell::new(),
            groups: RefCell::default(),
        }
    }

    fn binding(&self, map: &PanoramaKey) -> wgpu::BindGroup {
        self.groups
            .borrow_mut()
            .entry(map.clone())
            .or_insert_with(|| {
                let settings = self.uniform.get_or_init(|| {
                    buffer(
                        self.device,
                        "XRack material reflection view lighting",
                        &environment_uniforms(self.global, None),
                        wgpu::BufferUsages::UNIFORM,
                    )
                });
                bind_environment(self.device, &self.layout, &map.0, &map.1, settings)
            })
            .clone()
    }
}

#[derive(Clone, Copy)]
pub(super) struct ColorEnvironments<'a> {
    pub fallback: &'a wgpu::BindGroup,
    pub local: Option<&'a LocalReflections<'a>>,
    pub material: &'a MaterialReflections<'a>,
}

impl ColorEnvironments<'_> {
    pub fn bind(
        self,
        pass: &mut wgpu::RenderPass<'_>,
        active: &mut wgpu::BindGroup,
        object: Option<u32>,
        material: Option<&PanoramaKey>,
    ) {
        let map;
        let binding = if let Some(reflection) = material {
            map = self.material.binding(reflection);
            &map
        } else {
            self.local
                .zip(object)
                .map_or(self.fallback, |(local, object)| {
                    local.binding(local.slot(object), self.fallback)
                })
        };
        if active != binding {
            pass.set_bind_group(2, binding, &[]);
            *active = binding.clone();
        }
    }

    pub fn restore(self, pass: &mut wgpu::RenderPass<'_>, active: &wgpu::BindGroup) {
        if active != self.fallback {
            pass.set_bind_group(2, self.fallback, &[]);
        }
    }
}

pub(super) struct EnvironmentGpu {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    fallback: wgpu::BindGroup,
    empty_cube: wgpu::TextureView,
    analytic_skybox: Option<(SkySettings, SkyboxRows, wgpu::BindGroup)>,
    probes: VecDeque<ResidentProbe>,
    skies: VecDeque<ResidentSky>,
    masked_probes: VecDeque<MaskedProbe>,
    sky_generator: Option<SkyGenerator>,
    local_bindings: VecDeque<LocalBinding>,
    assignments: VecDeque<ProbeAssignment>,
    pub uploads: u64,
    pub sky_generations: u64,
    pub masked_generations: u64,
}

impl EnvironmentGpu {
    pub fn finish_frame(&mut self, submitted: bool) {
        if submitted {
            for sky in &mut self.skies {
                sky.committed = true;
            }
            for probe in &mut self.masked_probes {
                probe.committed = true;
            }
        } else {
            self.skies.retain(|sky| sky.committed);
            self.masked_probes.retain(|probe| probe.committed);
        }
    }

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
        let empty_cube = cube_texture(device, 1, 1).create_view(&cube_view());
        let bytes = environment_uniforms(None, None);
        let fallback = bind_probe(device, &layout, &empty_cube, &sampler, &bytes);
        Self {
            layout,
            sampler,
            fallback,
            empty_cube,
            analytic_skybox: None,
            probes: VecDeque::new(),
            skies: VecDeque::new(),
            masked_probes: VecDeque::new(),
            sky_generator: None,
            local_bindings: VecDeque::new(),
            assignments: VecDeque::new(),
            uploads: 0,
            sky_generations: 0,
            masked_generations: 0,
        }
    }

    pub fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: Option<&EnvironmentInput<'_>>,
    ) -> wgpu::BindGroup {
        let Some(input) = input else {
            return self.fallback.clone();
        };
        match input.source {
            EnvironmentSource::Panorama(probe) if input.horizon > 0.0 => {
                let mut entry = self.resident_masked(device, queue, encoder, probe, input.horizon);
                let binding = self.global_binding(device, input, &entry.view, &mut entry.binding);
                self.masked_probes.truncate(3);
                self.masked_probes.push_front(entry);
                binding
            }
            EnvironmentSource::Panorama(probe) => {
                let mut entry = self.resident(device, queue, probe);
                let binding = self.global_binding(device, input, &entry.view, &mut entry.binding);
                self.retain(entry);
                binding
            }
            EnvironmentSource::Sky(sky) => {
                let mut entry = self.resident_sky(device, encoder, sky, input.horizon);
                let binding = self.global_binding(device, input, &entry.view, &mut entry.binding);
                self.retain_sky(entry);
                binding
            }
        }
    }

    pub fn bind_skybox(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: Option<&SkyboxInput<'_>>,
    ) -> Option<wgpu::BindGroup> {
        let input = input.filter(|input| !input.shares_lighting)?;
        Some(match input.source {
            EnvironmentSource::Panorama(probe) => {
                let mut entry = self.resident(device, queue, probe);
                let group = self.skybox_binding(device, input, &entry.view, &mut entry.skybox);
                self.retain(entry);
                group
            }
            EnvironmentSource::Sky(sky) if input.rows[1][3] == 0.0 => {
                if let Some((cached, rows, group)) = &self.analytic_skybox
                    && *cached == sky
                    && *rows == input.rows
                {
                    return Some(group.clone());
                }
                let group = self.skybox_binding(device, input, &self.empty_cube, &mut None);
                self.analytic_skybox = Some((sky, input.rows, group.clone()));
                group
            }
            EnvironmentSource::Sky(sky) => {
                let mut entry = self.resident_sky(device, encoder, sky, 0.0);
                let group = self.skybox_binding(device, input, &entry.view, &mut entry.skybox);
                self.retain_sky(entry);
                group
            }
        })
    }

    fn skybox_binding(
        &self,
        device: &wgpu::Device,
        input: &SkyboxInput<'_>,
        view: &wgpu::TextureView,
        cached: &mut Option<(SkyboxRows, wgpu::BindGroup)>,
    ) -> wgpu::BindGroup {
        if let Some((rows, group)) = cached
            && *rows == input.rows
        {
            return group.clone();
        }
        let mut bytes = Vec::with_capacity(UNIFORM_BYTES);
        bytes.resize(LIGHTING_BYTES, 0); // Lighting is bound separately for draws of geometry.
        append_skybox_uniforms(&mut bytes, Some(input.source), input.rows);
        bytes.resize(UNIFORM_BYTES, 0);
        let group = bind_probe(device, &self.layout, view, &self.sampler, &bytes);
        *cached = Some((input.rows, group.clone()));
        group
    }

    fn resident_sky(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        sky: SkySettings,
        horizon: f32,
    ) -> ResidentSky {
        let index = self
            .skies
            .iter()
            .position(|entry| entry.settings == sky && entry.horizon == horizon);
        index
            .and_then(|index| self.skies.remove(index))
            .unwrap_or_else(|| {
                self.sky_generations += 1;
                let generator = self
                    .sky_generator
                    .get_or_insert_with(|| SkyGenerator::new(device));
                ResidentSky {
                    settings: sky,
                    horizon,
                    view: generator.generate(device, encoder, sky, horizon),
                    binding: None,
                    skybox: None,
                    committed: false,
                }
            })
    }

    fn resident_masked(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        probe: &Arc<EnvironmentProbe>,
        horizon: f32,
    ) -> MaskedProbe {
        self.masked_probes
            .retain(|entry| entry.identity.strong_count() != 0);
        let index = self.masked_probes.iter().position(|entry| {
            entry.identity.as_ptr() == Arc::as_ptr(probe) && entry.horizon == horizon
        });
        index
            .and_then(|index| self.masked_probes.remove(index))
            .unwrap_or_else(|| {
                let source = self.resident(device, queue, probe);
                let generator = self
                    .sky_generator
                    .get_or_insert_with(|| SkyGenerator::new(device));
                let view =
                    generator.attenuate(device, encoder, &source.view, &self.sampler, horizon);
                self.retain(source);
                self.masked_generations += 1;
                MaskedProbe {
                    identity: Arc::downgrade(probe),
                    horizon,
                    view,
                    binding: None,
                    committed: false,
                }
            })
    }

    fn retain_sky(&mut self, sky: ResidentSky) {
        // At 256² and RGBA16F four entries use less than 16 MiB.
        self.skies.truncate(3);
        self.skies.push_front(sky);
    }

    fn global_binding(
        &self,
        device: &wgpu::Device,
        input: &EnvironmentInput<'_>,
        view: &wgpu::TextureView,
        binding: &mut Option<(LightingSettings, wgpu::BindGroup)>,
    ) -> wgpu::BindGroup {
        let settings = LightingSettings::from(input);
        if let Some((cached, group)) = binding
            && *cached == settings
        {
            return group.clone();
        }
        let bytes = environment_uniforms(Some(input), None);
        let group = bind_probe(device, &self.layout, view, &self.sampler, &bytes);
        *binding = Some((settings, group.clone()));
        group
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
                && entry.probes.len() == probes.len()
                && entry.probes.iter().zip(probes.iter()).all(|(old, new)| {
                    old.position == new.position
                        && old.min == new.min
                        && old.max == new.max
                        && old.layers == new.layers
                })
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
        let ambient_key = ambient.map(|input| {
            let source = match input.source {
                EnvironmentSource::Panorama(probe) => {
                    EnvironmentIdentity::Panorama(Arc::clone(probe))
                }
                EnvironmentSource::Sky(sky) => EnvironmentIdentity::Sky(sky),
            };
            (source, LightingSettings::from(input))
        });
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
                    entry.probe.same_lighting(probe)
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
                    skybox: None,
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
    let mut bytes = Vec::with_capacity(UNIFORM_BYTES);
    let settings = |intensity: f32, last_mip: u32| [intensity, last_mip as f32, 0.0, 0.0];
    let global_settings = global.map_or([0.0, -1.0, 0.0, 0.0], |input| {
        settings(input.intensity, input.source.last_mip())
    });
    append_floats(&mut bytes, global_settings);
    let coefficients = global.map_or([[0.0; 3]; 9], |input| match input.source {
        EnvironmentSource::Panorama(probe) => {
            let full = probe.diffuse();
            let upper = probe.upper_diffuse();
            std::array::from_fn(|i| {
                std::array::from_fn(|channel| {
                    full[i][channel] * (1.0 - input.horizon) + upper[i][channel] * input.horizon
                })
            })
        }
        EnvironmentSource::Sky(sky) => sky.diffuse(input.horizon),
    });
    for [r, g, b] in coefficients {
        append_floats(&mut bytes, [r, g, b, 0.0]);
    }
    append_floats(
        &mut bytes,
        local.map_or(global_settings, |(probe, last_mip, _)| {
            settings(probe.intensity, last_mip)
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
    let rows = global.map_or([[0.0; 4]; 3], |input| {
        skybox_rows(input.orientation, input.intensity, 0.0)
    });
    append_skybox_uniforms(&mut bytes, global.map(|input| input.source), rows);
    let diffuse = global.map_or_else(EnvironmentOrientation::default, |input| input.orientation);
    let specular = local.map_or(diffuse, |(probe, _, _)| probe.orientation);
    append_floats(
        &mut bytes,
        [diffuse, specular].into_iter().flat_map(|orientation| {
            orientation
                .rows()
                .into_iter()
                .flat_map(|[x, y, z]| [x, y, z, 0.0])
        }),
    );
    bytes
}

fn append_skybox_uniforms(
    bytes: &mut Vec<u8>,
    source: Option<EnvironmentSource<'_>>,
    rows: SkyboxRows,
) {
    let synthetic = source.map_or([[0.0; 4]; 7], |source| match source {
        EnvironmentSource::Panorama(_) => [[0.0; 4]; 7],
        EnvironmentSource::Sky(sky) => {
            let mut parameters = sky.uniforms();
            parameters[6][1] = 1.0; // Draw the visible sky analytically.
            parameters
        }
    });
    append_floats(bytes, synthetic.into_iter().chain(rows).flatten());
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
    bind_environment(device, layout, texture, sampler, &settings)
}

fn bind_environment(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    settings: &wgpu::Buffer,
) -> wgpu::BindGroup {
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
