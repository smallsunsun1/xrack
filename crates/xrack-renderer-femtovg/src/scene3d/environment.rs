use std::{
    cell::{OnceCell, RefCell},
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
};

pub(super) use xrack_runtime::scene3d::ViewportEnvironment as EnvironmentInput;
use xrack_runtime::scene3d::{
    EnvironmentOrientation, EnvironmentProbe, EnvironmentSource, ReflectionProbe, ReflectionSource,
    SceneObject, SceneVisibility, SkyProgram, SkyProgramSettings, SkySettings, SkyShader,
};
use xrack_runtime::{Node, RowKey, Value, scene_effect::BufferInput};

use super::{
    append_floats, buffer,
    effect_history::Owner,
    reflection_capture::CapturedCube,
    screen_sky::ScreenSkyInput,
    sky_cube::{SkyGenerator, SkyPrefilter},
};

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
        let screen = matches!(source, EnvironmentSource::Program(program) if program.skybox_mode.screen_divisor().is_some());
        let shares_lighting = !screen
            && inherited.is_some_and(|input| {
                input.orientation == orientation
                    && input.intensity == intensity
                    && input.horizon == 0.0
                    && blur == 0.0
            });
        Some(Self {
            source,
            rows: skybox_rows(
                orientation,
                intensity,
                if screen {
                    0.0
                } else {
                    blur * source.last_mip() as f32
                },
            ),
            shares_lighting,
        })
    }

    pub fn screen(&self) -> Option<ScreenSkyInput<'a>> {
        let EnvironmentSource::Program(program) = self.source else {
            return None;
        };
        Some(ScreenSkyInput {
            program,
            basis: self.rows,
            divisor: program.skybox_mode.screen_divisor()?,
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
    Program(
        Arc<SkyShader>,
        SkyProgramSettings,
        Arc<[BufferInput]>,
        Arc<[[u32; 4]]>,
    ),
}

struct ResidentSky {
    settings: SkySettings,
    horizon: f32,
    view: wgpu::TextureView,
    binding: Option<(LightingSettings, wgpu::BindGroup)>,
    skybox: Option<(SkyboxRows, wgpu::BindGroup)>,
    committed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProgramWork {
    Background,
    BlurredBackground,
    Lighting,
}

struct SkyFilterJob {
    gpu: SkyPrefilter,
    slices: u32,
    frames: u32,
    last_frame: Option<u64>,
}

#[derive(Clone)]
struct SkyRequest {
    program: Arc<SkyProgram>,
    horizon: f32,
}

#[derive(Clone)]
struct PublishedSky {
    settings: SkyRequest,
    filtered: wgpu::TextureView,
}

struct SkyViewport {
    path: Vec<usize>,
    generation: u64,
    scope: [Option<RowKey>; 2],
    frame: u64,
    request: Option<SkyRequest>,
    published: Option<PublishedSky>,
    recorded: Option<PublishedSky>,
}

impl SkyViewport {
    fn owns(&self, owner: Owner<'_>) -> bool {
        self.generation == owner.generation && self.scope == owner.scope && self.path == owner.path
    }

    fn latest(&self) -> Option<&PublishedSky> {
        self.recorded.as_ref().or(self.published.as_ref())
    }
}

struct ResidentProgram {
    program: Arc<SkyProgram>,
    horizon: f32,
    view: wgpu::TextureView,
    diffuse: Option<wgpu::Buffer>,
    filter: Option<SkyFilterJob>,
    filter_frame: Option<u64>,
    last_used: u64,
    binding: Option<(LightingSettings, wgpu::BindGroup)>,
    skybox: Option<(SkyboxRows, wgpu::BindGroup)>,
    committed: bool,
}

impl ResidentProgram {
    fn matches(&self, program: &SkyProgram, horizon: f32) -> bool {
        self.horizon == horizon && self.program.same_radiance(program)
    }
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
    screen_skybox: Option<(SkyboxRows, wgpu::BindGroup)>,
    probes: VecDeque<ResidentProbe>,
    skies: VecDeque<ResidentSky>,
    programs: VecDeque<ResidentProgram>,
    masked_probes: VecDeque<MaskedProbe>,
    sky_generator: Option<SkyGenerator>,
    pub sky_programs: super::sky_shader::SkyShaders,
    sky_viewports: Vec<SkyViewport>,
    frame: u64,
    local_bindings: VecDeque<LocalBinding>,
    assignments: VecDeque<ProbeAssignment>,
    pub uploads: u64,
    pub sky_generations: u64,
    pub masked_generations: u64,
    pub sky_filter_slices: u64,
}

impl EnvironmentGpu {
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        self.sky_viewports
            .retain(|view| frame.wrapping_sub(view.frame) <= 120);
        self.programs
            .retain(|sky| sky.filter.is_none() || frame.wrapping_sub(sky.last_used) <= 1);
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        for view in &mut self.sky_viewports {
            if let Some(recorded) = view.recorded.take().filter(|_| submitted) {
                view.published = Some(recorded);
            }
        }
        if submitted {
            for sky in &mut self.skies {
                sky.committed = true;
            }
            for sky in &mut self.programs {
                sky.committed = true;
            }
            for probe in &mut self.masked_probes {
                probe.committed = true;
            }
        } else {
            self.skies.retain(|sky| sky.committed);
            self.programs
                .retain(|sky| sky.committed && sky.filter_frame != Some(self.frame));
            // A copy from the GPU's diffuse coefficients may have been discarded.
            self.local_bindings.retain(|binding| {
                !matches!(binding.ambient, Some((EnvironmentIdentity::Program(..), _)))
            });
            self.masked_probes.retain(|probe| probe.committed);
        }
    }

    pub fn retain_captured_bindings(&mut self, live: impl Fn(&wgpu::TextureView) -> bool) {
        self.local_bindings
            .retain(|entry| entry.captured.as_ref().is_none_or(&live));
    }

    pub fn new(
        device: &wgpu::Device,
        layout: wgpu::BindGroupLayout,
        images: super::material::SharedImages,
        native_anisotropy: bool,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("XRack environment sampler"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let empty_cube = cube_texture(device, 1, 1).create_view(&cube_view());
        let bytes = environment_uniforms(None, None);
        let fallback = bind_probe(device, &layout, &empty_cube, &sampler, &bytes, None);
        Self {
            layout,
            sampler,
            fallback,
            empty_cube,
            analytic_skybox: None,
            screen_skybox: None,
            probes: VecDeque::new(),
            skies: VecDeque::new(),
            programs: VecDeque::new(),
            masked_probes: VecDeque::new(),
            sky_generator: None,
            sky_programs: super::sky_shader::SkyShaders::new(images, native_anisotropy),
            sky_viewports: Vec::new(),
            frame: 0,
            local_bindings: VecDeque::new(),
            assignments: VecDeque::new(),
            uploads: 0,
            sky_generations: 0,
            masked_generations: 0,
            sky_filter_slices: 0,
        }
    }

    pub fn prepare_program(
        &mut self,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        node: &Node,
        owner: Owner<'_>,
    ) -> (Option<Node>, bool) {
        let Some(input) = EnvironmentInput::from_node(node) else {
            return (None, false);
        };
        let EnvironmentSource::Program(desired) = input.source else {
            return (None, false);
        };
        let index = self.sky_viewports.iter().position(|view| view.owns(owner));
        let mut view = index
            .map(|index| self.sky_viewports.swap_remove(index))
            .unwrap_or_else(|| SkyViewport {
                path: owner.path.to_vec(),
                generation: owner.generation,
                scope: owner.scope,
                frame: self.frame,
                request: None,
                published: None,
                recorded: None,
            });
        view.frame = self.frame;
        let latest = SkyRequest {
            program: Arc::clone(desired),
            horizon: input.horizon,
        };
        // Finish the captured request even while authored values change. The
        // next job reads the newest request rather than queuing stale edits.
        if desired.render_frames == 0
            || self
                .programs
                .iter()
                .any(|entry| entry.matches(desired, latest.horizon) && entry.diffuse.is_some())
        {
            view.request = None;
        }
        let request = view.request.get_or_insert_with(|| latest.clone()).clone();
        let program = &request.program;
        let work = if program.render_frames == 0 {
            ProgramWork::Lighting
        } else {
            ProgramWork::Background
        };
        let mut resident =
            self.resident_program((device, queue, encoder), program, request.horizon, work);
        if resident.diffuse.is_none() {
            self.advance_program(device, encoder, &mut resident, program.render_frames);
        }
        if resident.diffuse.is_some() {
            view.recorded = Some(PublishedSky {
                settings: request.clone(),
                filtered: resident.view.clone(),
            });
            view.request = None;
        }
        let ready = view.latest();
        let current = ready.is_some_and(|ready| {
            ready.settings.horizon == latest.horizon
                && ready.settings.program.same_radiance(desired)
        });
        let needs_redraw = view.request.is_some() || !current;
        let staged = (!current).then(|| stage_program_node(node, &input, &latest, ready));
        self.sky_viewports.push(view);
        self.retain_program(resident);
        (staged, needs_redraw)
    }

    fn advance_program(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &mut ResidentProgram,
        frames: u32,
    ) {
        let samples = resident.program.settings.samples;
        let job = resident.filter.get_or_insert_with(|| SkyFilterJob {
            gpu: SkyPrefilter::new(device, resident.view.clone(), resident.horizon, samples),
            slices: 0,
            frames: frames.max(1),
            last_frame: None,
        });
        if job.last_frame == Some(self.frame) {
            return;
        }
        resident.filter_frame = Some(self.frame);
        job.last_frame = Some(self.frame);
        let generator = self
            .sky_generator
            .get_or_insert_with(|| SkyGenerator::new(device));
        if job.slices == job.frames {
            let (filtered, diffuse) = generator.publish(device, encoder, &self.sampler, &job.gpu);
            resident.view = filtered;
            resident.diffuse = Some(diffuse);
            resident.filter = None;
            resident.binding = None;
            resident.skybox = None;
        } else {
            let first = samples * job.slices / job.frames;
            job.slices += 1;
            let end = samples * job.slices / job.frames;
            generator.accumulate(device, encoder, &self.sampler, &job.gpu, first..end);
            self.sky_filter_slices += 1;
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
                let binding =
                    self.global_binding(device, input, &entry.view, &mut entry.binding, None);
                self.masked_probes.truncate(3);
                self.masked_probes.push_front(entry);
                binding
            }
            EnvironmentSource::Panorama(probe) => {
                let mut entry = self.resident(device, queue, probe);
                let binding =
                    self.global_binding(device, input, &entry.view, &mut entry.binding, None);
                self.retain(entry);
                binding
            }
            EnvironmentSource::Sky(sky) => {
                let mut entry = self.resident_sky(device, encoder, sky, input.horizon);
                let binding =
                    self.global_binding(device, input, &entry.view, &mut entry.binding, None);
                self.retain_sky(entry);
                binding
            }
            EnvironmentSource::Program(program) => {
                let mut entry = self.resident_program(
                    (device, queue, encoder),
                    program,
                    input.horizon,
                    ProgramWork::Lighting,
                );
                let diffuse = entry
                    .diffuse
                    .as_ref()
                    .map(|diffuse| (&mut *encoder, diffuse));
                let binding =
                    self.global_binding(device, input, &entry.view, &mut entry.binding, diffuse);
                self.retain_program(entry);
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
        if input.screen().is_some() {
            let mut cached = self.screen_skybox.take();
            let group = self.skybox_binding(device, input, &self.empty_cube, &mut cached);
            self.screen_skybox = cached;
            return Some(group);
        }
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
            EnvironmentSource::Program(program) => {
                let work = if input.rows[1][3] > 0.0 {
                    ProgramWork::BlurredBackground
                } else {
                    ProgramWork::Background
                };
                let mut entry = self.resident_program((device, queue, encoder), program, 0.0, work);
                let group = self.skybox_binding(device, input, &entry.view, &mut entry.skybox);
                self.retain_program(entry);
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
        let group = bind_probe(device, &self.layout, view, &self.sampler, &bytes, None);
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

    fn resident_program(
        &mut self,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        program: &Arc<SkyProgram>,
        horizon: f32,
        work: ProgramWork,
    ) -> ResidentProgram {
        let index = self
            .programs
            .iter()
            .position(|entry| entry.matches(program, horizon));
        let old = index.and_then(|index| self.programs.remove(index));
        let old = match old {
            Some(entry)
                if work == ProgramWork::Background
                    || work == ProgramWork::BlurredBackground
                        && entry.view.texture().mip_level_count() > 1
                    || work == ProgramWork::Lighting && entry.diffuse.is_some() =>
            {
                return entry;
            }
            old => old,
        };
        self.sky_generations += 1;
        let generator = self
            .sky_generator
            .get_or_insert_with(|| SkyGenerator::new(device));
        let radiance = old
            .as_ref()
            .filter(|entry| entry.view.texture().mip_level_count() == 1)
            .map(|entry| entry.view.clone())
            .unwrap_or_else(|| {
                super::sky_cube::program_cube(
                    &mut self.sky_programs,
                    (device, queue, encoder),
                    program,
                )
            });
        let diffuse = (work == ProgramWork::Lighting)
            .then(|| generator.diffuse(device, encoder, &radiance, &self.sampler, horizon));
        let view = if work == ProgramWork::Background {
            radiance
        } else {
            generator.attenuate(
                device,
                encoder,
                &radiance,
                &self.sampler,
                (horizon, program.settings.samples),
            )
        };
        ResidentProgram {
            program: Arc::clone(program),
            horizon,
            view,
            diffuse,
            filter: None,
            filter_frame: None,
            last_used: self.frame,
            binding: None,
            skybox: None,
            committed: false,
        }
    }

    fn retain_program(&mut self, mut entry: ResidentProgram) {
        entry.last_used = self.frame;
        self.programs.push_front(entry);
        // Keep working jobs and the probe currently lighting each view alive.
        // Only the spare cubes compete for the original four-entry budget.
        let mut spares = 0;
        self.programs.retain(|entry| {
            if entry.filter.is_some()
                || self
                    .sky_viewports
                    .iter()
                    .any(|view| view.latest().is_some_and(|sky| sky.filtered == entry.view))
            {
                true
            } else {
                spares += 1;
                spares <= 4
            }
        });
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
                    generator.attenuate(device, encoder, &source.view, &self.sampler, (horizon, 0));
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
        diffuse: Option<(&mut wgpu::CommandEncoder, &wgpu::Buffer)>,
    ) -> wgpu::BindGroup {
        let settings = LightingSettings::from(input);
        if let Some((cached, group)) = binding
            && *cached == settings
        {
            return group.clone();
        }
        let bytes = environment_uniforms(Some(input), None);
        let group = bind_probe(device, &self.layout, view, &self.sampler, &bytes, diffuse);
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
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
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
        let mut coefficients = None;
        let ambient_key = ambient.and_then(|input| {
            let source = match input.source {
                EnvironmentSource::Panorama(probe) => {
                    EnvironmentIdentity::Panorama(Arc::clone(probe))
                }
                EnvironmentSource::Sky(sky) => EnvironmentIdentity::Sky(sky),
                EnvironmentSource::Program(program) => {
                    let resident = self
                        .programs
                        .iter()
                        .find(|entry| entry.matches(program, input.horizon))?;
                    coefficients = resident.diffuse.clone();
                    EnvironmentIdentity::Program(
                        Arc::clone(&program.shader),
                        program.settings,
                        Arc::clone(&program.textures),
                        Arc::clone(&program.uniforms),
                    )
                }
            };
            Some((source, LightingSettings::from(input)))
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
                                    coefficients.as_ref().map(|buffer| (&mut *encoder, buffer)),
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
                                bind_probe(
                                    device,
                                    &self.layout,
                                    &cube.view,
                                    &self.sampler,
                                    &bytes,
                                    coefficients.as_ref().map(|buffer| (&mut *encoder, buffer)),
                                )
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

fn stage_program_node(
    node: &Node,
    desired: &EnvironmentInput<'_>,
    latest: &SkyRequest,
    published: Option<&PublishedSky>,
) -> Node {
    let mut staged = node.clone();
    if node.boolean("environment-skybox", false)
        && EnvironmentSource::from_node(node, "skybox-sky", "skybox-source").is_none()
    {
        // An unblurred backdrop can display the latest radiance without
        // waiting for the lighting job. Blurred backdrops need complete mips.
        let screen = matches!(desired.source, EnvironmentSource::Program(program) if program.skybox_mode.screen_divisor().is_some());
        let blurred = !screen && node.number("skybox-blur").is_some_and(|blur| blur > 0.0);
        let previous = published.filter(|sky| {
            blurred
                && sky.settings.horizon == 0.0
                && sky.settings.program.skybox_mode.screen_divisor().is_none()
        });
        let program = previous.map_or(&latest.program, |sky| &sky.settings.program);
        staged
            .properties
            .insert("skybox-sky", Value::SceneSky(Arc::clone(program)));
        if previous.is_none() {
            staged.properties.insert("skybox-blur", Value::Float(0.0));
        }
        for (name, value) in ["skybox-yaw", "skybox-pitch", "skybox-roll"]
            .into_iter()
            .zip(desired.orientation.radians())
        {
            staged
                .properties
                .entry(name)
                .or_insert_with(|| Value::Float(f64::from(value).to_degrees()));
        }
        staged
            .properties
            .entry("skybox-intensity")
            .or_insert(Value::Float(f64::from(desired.intensity)));
    }
    if let Some(published) = published {
        staged.properties.insert(
            "environment-sky",
            Value::SceneSky(Arc::clone(&published.settings.program)),
        );
        staged.properties.insert(
            "environment-horizon",
            Value::Float(f64::from(published.settings.horizon)),
        );
    } else {
        staged.properties.remove("environment-sky");
    }
    staged
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
        EnvironmentSource::Program(_) => [[0.0; 3]; 9], // Copied from the GPU projection.
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
        EnvironmentSource::Panorama(_) | EnvironmentSource::Program(_) => [[0.0; 4]; 7],
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
    diffuse: Option<(&mut wgpu::CommandEncoder, &wgpu::Buffer)>,
) -> wgpu::BindGroup {
    let mut usage = wgpu::BufferUsages::UNIFORM;
    if diffuse.is_some() {
        usage |= wgpu::BufferUsages::COPY_DST;
    }
    let settings = buffer(device, "XRack IBL uniforms", bytes, usage);
    if let Some((encoder, coefficients)) = diffuse {
        // The first vec4 holds probe settings. The next nine hold SH coefficients.
        encoder.copy_buffer_to_buffer(coefficients, 0, &settings, 16, 9 * 16);
    }
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
