//! Reuse completed viewport images across 2D frames and canvas changes.
use std::{collections::VecDeque, sync::Arc};

use xrack_runtime::{Node, RowKey, SharedNodeIdentity, Value};

use crate::transient_textures::{bytes, create_render_texture};

use super::{
    COLOR_FORMAT, HDR_FORMAT, attachment_view,
    progressive::ProgressiveSample,
    temporal::{TemporalHistory, TemporalSample, TemporalSettings},
};

const MAX_TEXTURES: usize = 8;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const IDLE_FRAMES: u64 = 120;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ViewSize {
    pub pixels: [u32; 2],
    pub logical: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ViewInputs {
    pub reflections: [u64; 8],
    pub items: u64,
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

pub(crate) enum ViewTexture {
    Ready(wgpu::Texture),
    Render {
        texture: wgpu::Texture,
        sample: Option<AaSample>,
        needs_redraw: bool,
    },
}

struct Progress {
    next: u8,
    history: Option<wgpu::TextureView>,
}

struct Entry {
    path: Vec<usize>,
    generation: u64,
    size: ViewSize,
    node: SharedNodeIdentity,
    settings: Vec<(&'static str, Value)>,
    inputs: ViewInputs,
    texture: wgpu::Texture,
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

    fn render(&mut self, device: &wgpu::Device, stable: bool) -> ViewTexture {
        self.committed = false;
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
            texture: self.texture.clone(),
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
            && self.sampling == sampling
            && self.size == size
            && self.id == node.id
            && self.inputs == inputs
            && [
                "camera",
                "layer-mask",
                "environment",
                "scene-hdr",
                "scene-background",
                "scene-color-capture",
                "scene-depth-capture",
                "particle-emitters",
                "particle-time",
                "particle-affectors",
            ]
            .iter()
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
        let required = bytes(size.pixels);
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
                entry.render(device, true)
            } else {
                ViewTexture::Ready(entry.texture.clone())
            };
            self.entries.push_front(entry);
            return Some(result);
        }

        // Earlier frames are ordered on the same queue. A texture still
        // referenced in this frame must stay intact until composition.
        let index = self.entries.iter().position(|entry| {
            entry.frame != self.frame && same_owner(entry) && entry.size.pixels == size.pixels
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
            .map(|entry| bytes(entry.size.pixels))
            .sum();
        while self.entries.len() >= MAX_TEXTURES || resident + required > MAX_BYTES {
            // Keep sampling viewports painted in consecutive frames
            // resident so a busy window cannot repeatedly restart convergence.
            let index = self.entries.iter().rposition(|entry| {
                entry.frame != self.frame
                    && (!entry.sampling.is_active() || entry.frame.wrapping_add(1) != self.frame)
            })?;
            let evicted = self.entries.remove(index)?;
            resident -= bytes(evicted.size.pixels);
        }
        let (texture, history, temporal) = if let Some(entry) = recycled {
            let keep_history = entry.preserves_temporal_history(node, size, inputs, sampling);
            (
                entry.texture,
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
                create_render_texture(
                    device,
                    size.pixels,
                    COLOR_FORMAT,
                    "XRack retained 3D viewport",
                ),
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
        let result = entry.render(device, false);
        self.entries.push_front(entry);
        Some(result)
    }
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
                | "texture-input"
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
