//! Reuse completed viewport images across 2D frames and canvas changes.
use std::{
    collections::VecDeque,
    rc::Rc,
    sync::{Arc, Weak},
};

use xrack_runtime::{GeometryHandle, Node, RowKey, SharedNodeIdentity, Value};

use crate::transient_textures::{bytes, create_render_texture, render_bytes};

use super::{
    COLOR_FORMAT, HDR_FORMAT, attachment_view,
    environment::SKYBOX_PROPERTIES,
    lens_flare::FlareSettings,
    progressive::ProgressiveSample,
    temporal::{TemporalHistory, TemporalSample, TemporalSettings},
};

const MAX_TEXTURES: usize = 8;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DISPLAY_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const IDLE_FRAMES: u64 = 120;

const DISPLAY_PROPERTIES: &[&str] = &[
    "scene-exposure",
    "scene-tone-map",
    "scene-white-point",
    "scene-brightness",
    "scene-contrast",
    "scene-saturation",
    "scene-lut",
    "scene-lut-size",
    "scene-lut-strength",
    "scene-vignette-strength",
    "scene-vignette-radius",
    "scene-vignette-color",
    "scene-sharpness",
    "scene-dithering",
    "fxaa",
    "fxaa-edge-threshold",
    "fxaa-min-threshold",
    "fxaa-subpixel",
    "fxaa-strength",
    "fxaa-search-steps",
];

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ViewSize {
    pub pixels: [u32; 2],
    pub logical: [f32; 2],
    pub scissor: Option<[f64; 4]>,
    pub format: wgpu::TextureFormat,
}

#[derive(Default)]
pub(crate) struct TextureSizeObservations {
    pending: Vec<(GeometryHandle, [u32; 2], bool)>,
    exported: Vec<GeometryHandle>,
}

impl TextureSizeObservations {
    pub fn record(&mut self, node: &Node, size: [u32; 2], exported: bool) {
        for handle in node
            .geometry_handle
            .iter()
            .chain(node.instance_geometry_handle.iter())
        {
            if !handle.observes_scene_texture_size() {
                continue;
            }
            if let Some(entry) = self.pending.iter_mut().find(|entry| &entry.0 == handle) {
                entry.1 = size;
                entry.2 = exported;
            } else {
                self.pending.push((handle.clone(), size, exported));
            }
        }
    }

    pub fn finish_frame(&mut self, succeeded: bool) -> bool {
        if !succeeded {
            self.pending.clear();
            return false;
        }
        let mut changed = false;
        // Named output preparation runs for the whole frame, even when the
        // main paint is clipped or retained. A producer can lose its last reader.
        self.exported.retain(
            |old| match self.pending.iter().find(|entry| &entry.0 == old) {
                Some((_, _, exported)) => *exported,
                None => {
                    changed |= old.publish_scene_texture_size([0; 2]);
                    false
                }
            },
        );
        for (handle, extent, exported) in self.pending.drain(..) {
            changed |= handle.publish_scene_texture_size(extent);
            if exported && !self.exported.contains(&handle) {
                self.exported.push(handle);
            }
        }
        changed
    }
}

pub(super) fn viewport_bytes(size: [u32; 2], format: wgpu::TextureFormat) -> u64 {
    // Reserve space for a display resolve, while allocating it only on demand.
    render_bytes(size, format)
        + if format == COLOR_FORMAT {
            0
        } else {
            bytes(size)
        }
}

pub(crate) fn viewport_format(device: &wgpu::Device, node: &Node) -> wgpu::TextureFormat {
    let format = match node.text("render-format") {
        Some("rgba16f") => HDR_FORMAT,
        Some("rgba32f") => wgpu::TextureFormat::Rgba32Float,
        _ => return COLOR_FORMAT,
    };
    let features = format.guaranteed_format_features(device.features());
    if features
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        && features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
    {
        format
    } else {
        COLOR_FORMAT
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ViewInputs {
    pub reflections: [u64; 8],
    pub items: u64,
    /// A change of automatic camera is a cut, even when the camera string stays empty.
    pub automatic_camera: Option<(u32, Option<usize>)>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Sampling {
    progressive: u8,
    temporal: Option<TemporalSettings>,
    feedback: bool,
    animate_feedback: bool,
}

impl Sampling {
    pub(crate) fn from_node(node: &Node) -> Self {
        if super::material_debug::MaterialDebug::from_node(node).active() {
            return Self::default();
        }
        let progressive = match node.int("progressive-aa") {
            Some(samples @ (2 | 4 | 8)) => samples as u8,
            _ => 0,
        };
        let temporal =
            if node.boolean("temporal-aa", false) && !node.boolean("multisampling", false) {
                let strength = node
                    .float("temporal-aa-strength")
                    .unwrap_or(0.3)
                    .clamp(0.0, 1.0);
                (strength > 0.0).then(|| TemporalSettings {
                    strength,
                    motion_vectors: node
                        .text("temporal-aa-mode")
                        .is_some_and(|mode| mode == "motion-vectors"),
                })
            } else {
                None
            };
        Self {
            progressive: if temporal.is_some_and(|settings| settings.motion_vectors) {
                0
            } else {
                progressive
            },
            temporal,
            feedback: false,
            animate_feedback: false,
        }
    }

    pub(crate) fn with_feedback(mut self, active: bool, animate: bool) -> Self {
        self.feedback = active;
        self.animate_feedback = active && animate;
        self
    }

    pub(crate) fn is_active(self) -> bool {
        self.progressive != 0 || self.temporal.is_some()
    }

    pub(crate) fn retains_display(self, node: &Node) -> bool {
        !self.is_active()
            && !self.feedback
            && node.boolean("cache-rendering", false)
            && (node.properties.contains_key("lens-flare-strength")
                || DISPLAY_PROPERTIES
                    .iter()
                    .any(|name| node.properties.contains_key(*name)))
    }
}

pub(crate) enum AaSample {
    Progressive(ProgressiveSample),
    Temporal(TemporalSample),
}

impl AaSample {
    pub(super) fn clip_offset(&self, [width, height]: [u32; 2]) -> [f32; 2] {
        let [x, y] = match self {
            Self::Progressive(sample) => sample.offset,
            Self::Temporal(sample) => sample.offset,
        };
        [2.0 * x / width as f32, -2.0 * y / height as f32]
    }
}

/// A completed color target and the lifetime used by retained GPU readers.
#[derive(Clone)]
pub(crate) struct ViewOutput {
    pub texture: wgpu::Texture,
    pub revision: u64,
    pub readers: Weak<()>,
}

impl ViewOutput {
    pub fn attachment(&self) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.texture.format().add_srgb_suffix()),
            ..Default::default()
        })
    }
}

/// The actual inputs of the final color pass. Source views are pinned until this
/// frame is evicted, so passes from other viewports may safely reuse their pools.
pub(crate) struct DisplayFrame {
    pub color: wgpu::TextureView,
    pub glow: Option<wgpu::TextureView>,
    pub flare: Option<wgpu::TextureView>,
    pub hdr: bool,
    pub composite_alpha: bool,
}

impl DisplayFrame {
    fn bytes(&self) -> u64 {
        std::iter::once(&self.color)
            .chain(self.glow.as_ref())
            .chain(self.flare.as_ref())
            .map(|view| {
                let texture = view.texture();
                render_bytes([texture.width(), texture.height()], texture.format())
            })
            .sum()
    }
}

pub(crate) enum ViewTexture {
    Ready(ViewOutput),
    Reprocess(ViewOutput, Rc<DisplayFrame>),
    Render {
        output: ViewOutput,
        sample: Option<AaSample>,
        needs_redraw: bool,
    },
}

struct Progress {
    next: u8,
    history: Option<wgpu::TextureView>,
}

struct Display {
    texture: wgpu::Texture,
    revision: Option<u64>,
    pending: bool,
}

struct Entry {
    path: Vec<usize>,
    generation: u64,
    size: ViewSize,
    node: SharedNodeIdentity,
    settings: Vec<(&'static str, Value)>,
    inputs: ViewInputs,
    texture: wgpu::Texture,
    display: Option<Display>,
    display_frame: Option<Rc<DisplayFrame>>,
    readers: Arc<()>,
    revision: u64,
    frame: u64,
    committed: bool,
    id: Option<&'static str>,
    scope: [Option<RowKey>; 2],
    feedback_id: Option<String>,
    sampling: Sampling,
    progress: Option<Progress>,
    temporal: TemporalHistory,
    needs_redraw: bool,
    temporal_samples: u8,
}

impl Entry {
    fn matches(&self, node: &Node, size: ViewSize, inputs: ViewInputs) -> bool {
        if self.size != size
            || self.inputs != inputs
            || self.sampling.temporal.is_some() && self.id != node.id
            || self.sampling.feedback && self.feedback_id.as_deref() != node.element_id()
        {
            return false;
        }
        if self.node.matches(node) {
            return true;
        }
        let mut settings = scene_settings(node);
        self.settings.iter().all(|&(name, ref value)| {
            settings.next().is_some_and(|(other_name, other_value)| {
                name == other_name && same_value(value, other_value)
            })
        }) && settings.next().is_none()
    }

    fn output(&self) -> ViewOutput {
        ViewOutput {
            texture: self.texture.clone(),
            revision: self.revision,
            readers: Arc::downgrade(&self.readers),
        }
    }

    fn matches_display_inputs(&self, node: &Node, size: ViewSize, inputs: ViewInputs) -> bool {
        if self.size != size
            || self.inputs != inputs
            // An explicit tone curve also enables the HDR raster target.
            || self.settings.iter().any(|(name, _)| *name == "scene-tone-map")
                != node.properties.contains_key("scene-tone-map")
        {
            return false;
        }
        let reuses_flare = self
            .display_frame
            .as_ref()
            .is_some_and(|frame| frame.flare.is_some())
            && FlareSettings::strength(node).is_some();
        let upstream = |name: &str| {
            !DISPLAY_PROPERTIES.contains(&name) && !(reuses_flare && name == "lens-flare-strength")
        };
        let mut current = scene_settings(node).filter(|(name, _)| upstream(name));
        self.settings
            .iter()
            .filter(|(name, _)| upstream(name))
            .all(|(name, old)| {
                current
                    .next()
                    .is_some_and(|(next, new)| *name == next && same_value(old, new))
            })
            && current.next().is_none()
    }

    fn begin_output(
        &mut self,
        device: &wgpu::Device,
        (allocations, revision): (&mut u64, &mut u64),
        aliases_input: bool,
    ) {
        // Sliced probes and retained inputs can still read the old output.
        if aliases_input || Arc::strong_count(&self.readers) > 1 {
            *allocations = allocations.saturating_add(1);
            self.texture = viewport_texture(device, self.size);
            self.readers = Arc::new(());
        }
        *revision = revision.wrapping_add(1).max(1);
        self.revision = *revision;
        self.committed = false;
    }

    fn render(
        &mut self,
        device: &wgpu::Device,
        stable: bool,
        (allocations, revision): (&mut u64, &mut u64),
    ) -> ViewTexture {
        self.display_frame = None;
        self.begin_output(device, (allocations, revision), false);
        let sample = if let Some(progress) = self.progress.as_mut().filter(|_| stable) {
            // A stationary scene can start accumulating independent jitter samples.
            self.temporal = TemporalHistory::default();
            let history = progress.history.get_or_insert_with(|| {
                attachment_view(
                    device,
                    self.size.pixels,
                    HDR_FORMAT,
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack progressive 3D AA history",
                )
            });
            let sample =
                ProgressiveSample::new(history.clone(), progress.next, self.sampling.progressive);
            progress.next += 1;
            self.needs_redraw = progress.next < self.sampling.progressive;
            Some(AaSample::Progressive(sample))
        } else {
            let sample = self.sampling.temporal.map(|settings| {
                self.temporal_samples += 1;
                AaSample::Temporal(self.temporal.sample(device, self.size.pixels, settings))
            });
            // Reprojection converges over a balanced eight-position sequence.
            self.needs_redraw = self.sampling.temporal.is_some_and(|settings| {
                self.temporal_samples < if settings.motion_vectors { 8 } else { 2 }
            }) || !stable && self.progress.is_some();
            sample
        };
        ViewTexture::Render {
            output: self.output(),
            sample,
            needs_redraw: self.needs_redraw,
        }
    }

    fn preserves_temporal_history(
        &self,
        node: &Node,
        size: ViewSize,
        inputs: ViewInputs,
        sampling: Sampling,
    ) -> bool {
        sampling.temporal.is_some()
            // The guide tests depth without producing depth or motion vectors.
            && !node.boolean("scene-grid", false)
            && self.sampling == sampling
            && self.size == size
            && self.id == node.id
            && self.inputs == inputs
            && [
                "camera",
                "auto-camera",
                "layer-mask",
                "environment",
                "environment-sky",
                "environment-skybox",
                "environment-yaw",
                "environment-pitch",
                "environment-roll",
                "environment-horizon",
                "environment-intensity",
                "scene-hdr",
                "scene-background",
                "scene-grid",
                "scene-color-capture",
                "scene-depth-capture",
                "particle-emitters",
                "particle-time",
                "particle-affectors",
            ]
            .iter()
            .chain(SKYBOX_PROPERTIES.iter())
            .all(|name| {
                self.settings
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
                    == scene_settings(node)
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value)
            })
    }
}

#[derive(Default)]
pub(crate) struct ViewportTextures {
    entries: VecDeque<Entry>,
    frame: u64,
    pub allocations: u64,
    revision: u64,
}

impl ViewportTextures {
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.entries
            .retain(|entry| self.frame.wrapping_sub(entry.frame) <= IDLE_FRAMES);
    }

    pub(crate) fn finish_frame(&mut self, submitted: bool) {
        if submitted {
            for entry in &mut self.entries {
                entry.committed = true;
                if !entry.needs_redraw {
                    entry.progress = None;
                }
            }
        } else {
            self.entries.retain(|entry| entry.committed);
        }
        for display in self
            .entries
            .iter_mut()
            .filter_map(|entry| entry.display.as_mut())
        {
            if std::mem::take(&mut display.pending) && !submitted {
                display.revision = None;
            }
        }
    }

    pub(crate) fn display_target(
        &mut self,
        device: &wgpu::Device,
        source: &wgpu::Texture,
    ) -> Option<(wgpu::Texture, bool)> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.texture == *source)?;
        let display = entry.display.get_or_insert_with(|| Display {
            texture: create_render_texture(
                device,
                entry.size.pixels,
                wgpu::TextureFormat::Rgba8Unorm,
                "XRack viewport display",
            ),
            revision: None,
            pending: false,
        });
        let refresh = display.revision != Some(entry.revision);
        if refresh {
            display.revision = Some(entry.revision);
            display.pending = true;
        }
        Some((display.texture.clone(), refresh))
    }

    pub(crate) fn keep_display_frame(&mut self, output: &ViewOutput, frame: Rc<DisplayFrame>) {
        let bytes = frame.bytes();
        if bytes > MAX_DISPLAY_INPUT_BYTES {
            return;
        }
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.revision == output.revision && entry.texture == output.texture)
        else {
            return;
        };
        let mut resident: u64 = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                (slot != index).then(|| {
                    entry
                        .display_frame
                        .as_ref()
                        .map_or(0, |frame| frame.bytes())
                })
            })
            .sum();
        for (slot, entry) in self.entries.iter_mut().enumerate().rev() {
            if resident + bytes <= MAX_DISPLAY_INPUT_BYTES {
                break;
            }
            if slot != index
                && let Some(frame) = entry.display_frame.take()
            {
                resident -= frame.bytes();
            }
        }
        self.entries[index].display_frame = Some(frame);
    }

    pub(crate) fn clear_disabled_temporal(
        &mut self,
        (path, generation, scope): (&[usize], u64, [Option<RowKey>; 2]),
        sampling: Sampling,
    ) {
        if sampling.temporal.is_none() {
            self.entries.retain(|entry| {
                entry.sampling.temporal.is_none()
                    || entry.generation != generation
                    || entry.path != path
                    || entry.scope != scope
            });
        }
    }

    pub(crate) fn get(
        &mut self,
        device: &wgpu::Device,
        node: &Node,
        (path, generation, scope): (&[usize], u64, [Option<RowKey>; 2]),
        size: ViewSize,
        inputs: ViewInputs,
        sampling: Sampling,
    ) -> Option<ViewTexture> {
        let required = viewport_bytes(size.pixels, size.format);
        if required > MAX_BYTES {
            return None;
        }
        let same_owner = |entry: &Entry| {
            entry.path == path && entry.generation == generation && entry.scope == scope
        };
        let index = self.entries.iter().position(|entry| {
            entry.sampling == sampling
                && (sampling.temporal.is_none()
                    && !sampling.feedback
                    && entry.committed
                    && !entry.needs_redraw
                    || same_owner(entry) && (entry.committed || entry.frame == self.frame))
                && entry.matches(node, size, inputs)
        });
        if let Some(mut entry) = index.and_then(|index| self.entries.remove(index)) {
            let solve_next_sample =
                (entry.needs_redraw || sampling.animate_feedback) && entry.frame != self.frame;
            entry.frame = self.frame;
            entry.node = node.snapshot_identity();
            let result = if solve_next_sample {
                entry.render(device, true, (&mut self.allocations, &mut self.revision))
            } else {
                ViewTexture::Ready(entry.output())
            };
            self.entries.push_front(entry);
            return Some(result);
        }

        if sampling.retains_display(node)
            && let Some(index) = self.entries.iter().position(|entry| {
                entry.committed
                    && !entry.needs_redraw
                    && entry.frame != self.frame
                    && same_owner(entry)
                    && entry.sampling == sampling
                    && entry.display_frame.is_some()
                    && entry.matches_display_inputs(node, size, inputs)
            })
            && let Some(mut entry) = self.entries.remove(index)
            && let Some(frame) = entry.display_frame.clone()
        {
            entry.frame = self.frame;
            entry.node = node.snapshot_identity();
            entry.settings = scene_settings(node)
                .map(|(name, value)| (name, value.clone()))
                .collect();
            let aliases_input = frame.color.texture() == &entry.texture;
            entry.begin_output(
                device,
                (&mut self.allocations, &mut self.revision),
                aliases_input,
            );
            let output = entry.output();
            self.entries.push_front(entry);
            return Some(ViewTexture::Reprocess(output, frame));
        }

        // Earlier frames are ordered on the same queue. A texture still
        // referenced in this frame must stay intact until composition.
        let index = self.entries.iter().position(|entry| {
            entry.frame != self.frame
                && same_owner(entry)
                && entry.size.pixels == size.pixels
                && entry.size.format == size.format
        });
        let recycled = index.and_then(|index| self.entries.remove(index));
        if sampling.is_active() {
            // A resized view starts a new sequence. Release its previous
            // history before admitting the new resolution.
            self.entries.retain(|entry| {
                !entry.sampling.is_active() || entry.frame == self.frame || !same_owner(entry)
            });
        }
        // The color budget also bounds history. Each viewport holds at
        // most two RGBA16F textures and two R32Uint identity maps.
        let mut resident: u64 = self
            .entries
            .iter()
            .map(|entry| viewport_bytes(entry.size.pixels, entry.size.format))
            .sum();
        while self.entries.len() >= MAX_TEXTURES || resident + required > MAX_BYTES {
            // Keep sampling viewports painted in consecutive frames
            // resident so a busy window cannot repeatedly restart convergence.
            let index = self.entries.iter().rposition(|entry| {
                entry.frame != self.frame
                    && (!entry.sampling.is_active() || entry.frame.wrapping_add(1) != self.frame)
            })?;
            let evicted = self.entries.remove(index)?;
            resident -= viewport_bytes(evicted.size.pixels, evicted.size.format);
        }
        let (texture, display, readers, history, temporal) = if let Some(entry) = recycled {
            let keep_history = entry.preserves_temporal_history(node, size, inputs, sampling);
            (
                entry.texture,
                entry.display,
                entry.readers,
                entry
                    .progress
                    .filter(|_| sampling.temporal.is_none())
                    .and_then(|progress| progress.history),
                if keep_history {
                    entry.temporal
                } else {
                    TemporalHistory::default()
                },
            )
        } else {
            self.allocations = self.allocations.saturating_add(1);
            (
                viewport_texture(device, size),
                None,
                Arc::new(()),
                None,
                TemporalHistory::default(),
            )
        };
        let mut entry = Entry {
            path: path.to_vec(),
            generation,
            size,
            node: node.snapshot_identity(),
            settings: scene_settings(node)
                .map(|(name, value)| (name, value.clone()))
                .collect(),
            inputs,
            texture,
            display,
            display_frame: None,
            readers,
            revision: 0,
            frame: self.frame,
            committed: false,
            id: node.id,
            scope,
            feedback_id: if sampling.feedback {
                node.element_id().map(str::to_owned)
            } else {
                None
            },
            sampling,
            progress: (sampling.progressive != 0).then_some(Progress { next: 0, history }),
            temporal,
            needs_redraw: false,
            temporal_samples: 0,
        };
        let result = entry.render(device, false, (&mut self.allocations, &mut self.revision));
        self.entries.push_front(entry);
        Some(result)
    }
}

fn viewport_texture(device: &wgpu::Device, size: ViewSize) -> wgpu::Texture {
    create_render_texture(
        device,
        size.pixels,
        size.format,
        "XRack retained 3D viewport",
    )
}

fn scene_settings(node: &Node) -> impl Iterator<Item = (&'static str, &Value)> {
    let has_emitters = matches!(
        node.properties.get("particle-emitters"),
        Some(Value::SceneParticleEmitters(emitters)) if !emitters.is_empty()
    );
    node.properties.iter().filter_map(move |(&name, value)| {
        // Layout extent is part of ViewSize. Other 2D properties apply during composition.
        (!matches!(
            name,
            "cache-rendering"
                | "render-stats-revision"
                | "scissor-rect" // Normalized bounds are part of ViewSize.
                | "preload-model"
                | "render-mode"
                | "texture-input"
                | "texture-source"
                | "texture-only"
                | "texture-mipmaps"
                | "x"
                | "y"
                | "z"
                | "width"
                | "height"
                | "opacity"
                | "visible"
                | "enabled"
                | "clip"
                | "transform"
                | "transform-origin-x"
                | "transform-origin-y"
                | "scale-x"
                | "scale-y"
                | "rotation"
        ) && (has_emitters || !matches!(name, "particle-time" | "particle-affectors")))
        .then_some((name, value))
    })
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // The compiler and interpreter already share immutable model projections.
        // Keep a bounded lookup even when animated scenes hold thousands of objects.
        (Value::SceneObjects(a), Value::SceneObjects(b)) => Arc::ptr_eq(a, b),
        _ => a == b,
    }
}
