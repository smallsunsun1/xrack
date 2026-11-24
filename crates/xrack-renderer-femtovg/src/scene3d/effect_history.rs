//! Viewport-local double buffers published only after a successful frame.
use std::{collections::VecDeque, sync::Arc};
use xrack_runtime::{RowKey, scene_effect::Chain};

use super::{
    Attachment, attachment_view,
    custom_effect::{effect_format, effect_size, texture_bytes},
};

const IDLE_FRAMES: u64 = 120;
const BYTE_BUDGET: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct Owner<'a> {
    pub path: &'a [usize],
    pub generation: u64,
    pub scope: [Option<RowKey>; 2],
    pub id: Option<&'a str>,
    pub logical_size: [f32; 2],
    pub revision: i64,
}

#[derive(Clone, Copy, PartialEq)]
struct Stamp {
    size: [u32; 2],
    logical_size: [f32; 2],
    revision: i64,
}

struct Slot {
    name: String,
    shader: Arc<str>,
    read: Attachment,
    spare: Option<Attachment>,
}

struct Version {
    stamp: Stamp,
    slots: Vec<Slot>,
}

impl Version {
    fn bytes(&self) -> u64 {
        self.slots
            .iter()
            .map(|slot| pair_bytes(slot.read.size, slot.read.view.texture().format()))
            .sum()
    }
}

struct View {
    path: Vec<usize>,
    generation: u64,
    scope: [Option<RowKey>; 2],
    id: Option<String>,
    frame: u64,
    current: Option<Version>,
    pending: Option<Version>,
    retire: bool,
}

impl View {
    fn new(owner: Owner<'_>, frame: u64) -> Self {
        Self {
            path: owner.path.to_vec(),
            generation: owner.generation,
            scope: owner.scope,
            id: owner.id.map(str::to_owned),
            frame,
            current: None,
            pending: None,
            retire: false,
        }
    }

    fn matches(&self, owner: Owner<'_>) -> bool {
        self.path == owner.path
            && self.generation == owner.generation
            && self.scope == owner.scope
            && self.id.as_deref() == owner.id
    }

    fn bytes(&self) -> u64 {
        let current = self.current.as_ref().map_or(0, Version::bytes);
        current.max(self.pending.as_ref().map_or(0, Version::bytes))
    }
}

#[derive(Default)]
pub(super) struct History {
    views: VecDeque<View>,
    frame: u64,
}

impl History {
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.views
            .retain(|view| self.frame.wrapping_sub(view.frame) <= IDLE_FRAMES);
    }

    pub fn visit(&mut self, owner: Owner<'_>, active: bool) {
        if let Some(view) = self.views.iter_mut().find(|view| view.matches(owner)) {
            view.frame = self.frame;
            view.retire = !active;
        }
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        self.views.retain_mut(|view| {
            let pending = view.pending.take();
            let retire = std::mem::take(&mut view.retire);
            if submitted {
                if retire {
                    return false;
                }
                if let Some(version) = pending {
                    view.current = Some(version);
                }
            }
            view.current
                .as_ref()
                .is_some_and(|version| !version.slots.is_empty())
        });
    }

    pub fn prepare<'a>(
        &mut self,
        owner: Owner<'_>,
        size: [u32; 2],
        chain: &'a Chain,
    ) -> Option<Frame<'a>> {
        if chain.history_writers().is_empty() {
            return None;
        }
        let index = self.views.iter().position(|view| view.matches(owner));
        let mut view = index
            .and_then(|index| self.views.remove(index))
            .unwrap_or_else(|| View::new(owner, self.frame));
        view.frame = self.frame;
        view.retire = false;
        let mut requested = Vec::new();
        let mut bytes = 0;
        for &index in chain.history_writers() {
            let pass = &chain.passes()[index];
            let size = effect_size(size, pass.scale);
            let format = effect_format(pass.format);
            let cost = pair_bytes(size, format);
            if cost <= BYTE_BUDGET - bytes {
                requested.push(Writer {
                    index,
                    name: pass.output(),
                    shader: &pass.source,
                    size,
                    format,
                });
                bytes += cost;
            }
        }
        let mut resident = self.views.iter().map(View::bytes).sum::<u64>();
        self.views.retain(|view| {
            if resident + bytes > BYTE_BUDGET && view.frame != self.frame {
                resident -= view.bytes();
                false
            } else {
                true
            }
        });
        let mut free = BYTE_BUDGET.saturating_sub(resident);
        requested.retain(|writer| {
            let cost = pair_bytes(writer.size, writer.format);
            if cost > free {
                return false;
            }
            free -= cost;
            true
        });
        let stamp = Stamp {
            size,
            logical_size: owner.logical_size,
            revision: owner.revision,
        };
        Some(Frame {
            view,
            requested,
            next: Version {
                stamp,
                slots: Vec::new(),
            },
        })
    }

    pub fn record(&mut self, mut frame: Frame<'_>) {
        frame.view.pending = Some(frame.next);
        self.views.push_back(frame.view);
    }
}

fn pair_bytes(size: [u32; 2], format: wgpu::TextureFormat) -> u64 {
    2 * texture_bytes(size, format)
}

struct Writer<'a> {
    index: usize,
    name: &'a str,
    shader: &'a Arc<str>,
    size: [u32; 2],
    format: wgpu::TextureFormat,
}

pub(super) struct Frame<'a> {
    view: View,
    requested: Vec<Writer<'a>>,
    next: Version,
}

impl Frame<'_> {
    fn writer(&self, index: usize) -> Option<&Writer<'_>> {
        self.requested.iter().find(|writer| writer.index == index)
    }

    fn slot<'a>(&self, version: Option<&'a Version>, writer: &Writer<'_>) -> Option<&'a Slot> {
        version
            .filter(|version| version.stamp == self.next.stamp)?
            .slots
            .iter()
            .find(|slot| {
                slot.name == writer.name
                    && slot.shader == *writer.shader
                    && slot.read.size == writer.size
                    && slot.read.view.texture().format() == writer.format
            })
    }

    pub fn previous(&self, index: usize) -> Option<&wgpu::TextureView> {
        Some(
            &self
                .slot(self.view.current.as_ref(), self.writer(index)?)?
                .read
                .view,
        )
    }

    pub fn destination(&self, device: &wgpu::Device, index: usize) -> Option<(Attachment, bool)> {
        let writer = self.writer(index)?;
        // If the viewport is drawn twice in one encoder, both draws read the
        // committed image and the later draw can overwrite the pending target.
        if let Some(target) = self
            .slot(self.view.pending.as_ref(), writer)
            .map(|slot| &slot.read)
            .or_else(|| {
                self.slot(self.view.current.as_ref(), writer)?
                    .spare
                    .as_ref()
            })
        {
            return Some((target.clone(), false));
        }
        Some((
            Attachment {
                size: writer.size,
                view: attachment_view(
                    device,
                    writer.size,
                    writer.format,
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack scene effect history",
                ),
            },
            true,
        ))
    }

    pub fn written(&mut self, index: usize, output: &Attachment) {
        let Some(writer) = self.writer(index) else {
            return;
        };
        let spare = self
            .slot(self.view.current.as_ref(), writer)
            .map(|slot| slot.read.clone());
        self.next.slots.push(Slot {
            name: writer.name.to_owned(),
            shader: writer.shader.clone(),
            read: output.clone(),
            spare,
        });
    }
}
