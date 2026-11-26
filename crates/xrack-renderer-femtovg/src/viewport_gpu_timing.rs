//! GPU pass timestamps attributed to the viewport that records each pass.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::mpsc};

use xrack_runtime::{ViewportGpuFrame, ViewportGpuPass, ViewportGpuStats};

const MAX_PASSES: usize = 512;
const QUERY_COUNT: u32 = 2 * MAX_PASSES as u32;

#[derive(Default)]
pub(super) struct ViewportGpuTimer {
    slots: [Option<Slot>; 3],
    recording: Option<Recording>,
    frame: Option<u64>,
    epoch: u64,
}

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: Option<Pending>,
}

struct Recording {
    slot: usize,
    frame: u64,
    passes: Rc<RefCell<Passes>>,
}

struct Pending {
    frame: u64,
    epoch: u64,
    passes: Passes,
    ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

struct Passes {
    queries: wgpu::QuerySet,
    viewports: Vec<(Vec<usize>, Option<String>)>,
    entries: Vec<(usize, String)>,
    truncated: bool,
}

struct ActiveViewport {
    passes: Rc<RefCell<Passes>>,
    view: usize,
}

thread_local! {
    static ACTIVE: RefCell<Option<ActiveViewport>> = const { RefCell::new(None) };
}

pub(super) struct ViewportScope(Option<ActiveViewport>);

impl Drop for ViewportScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(self.0.take()));
    }
}

impl Slot {
    fn new(device: &wgpu::Device) -> Self {
        let bytes = u64::from(QUERY_COUNT) * u64::from(wgpu::QUERY_SIZE);
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("XRack 3D GPU pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: QUERY_COUNT,
            }),
            resolve: buffer(
                "XRack 3D timestamp resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(
                "XRack 3D timestamp readback",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            pending: None,
        }
    }
}

impl ViewportGpuTimer {
    pub fn start_frame(&mut self, frame: Option<u64>) {
        self.recording = None;
        self.frame = frame;
    }

    pub fn invalidate(&mut self) {
        self.start_frame(None);
        self.epoch = self.epoch.wrapping_add(1);
    }

    pub fn enter(
        &mut self,
        device: &wgpu::Device,
        path: &[usize],
        id: Option<&str>,
    ) -> Option<ViewportScope> {
        let frame = self.frame?;
        if self.recording.is_none() {
            let index = self
                .slots
                .iter()
                .position(|slot| slot.as_ref().is_none_or(|slot| slot.pending.is_none()))?;
            let slot = self.slots[index].get_or_insert_with(|| Slot::new(device));
            self.recording = Some(Recording {
                slot: index,
                frame,
                passes: Rc::new(RefCell::new(Passes {
                    queries: slot.queries.clone(),
                    viewports: Vec::new(),
                    entries: Vec::new(),
                    truncated: false,
                })),
            });
        }
        let passes = self.recording.as_ref()?.passes.clone();
        let mut collection = passes.borrow_mut();
        let view = collection.viewports.len();
        collection
            .viewports
            .push((path.to_vec(), id.map(str::to_owned)));
        drop(collection);
        Some(ViewportScope(ACTIVE.with(|active| {
            active.replace(Some(ActiveViewport { view, passes }))
        })))
    }

    pub fn finish(&mut self, device: &wgpu::Device, encoder: &mut Option<wgpu::CommandEncoder>) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let Ok(passes) = Rc::try_unwrap(recording.passes).map(RefCell::into_inner) else {
            return;
        };
        if passes.entries.is_empty() {
            return;
        }
        let Some(slot) = self.slots[recording.slot].as_mut() else {
            return;
        };
        let count = 2 * passes.entries.len() as u32;
        let bytes = u64::from(count) * u64::from(wgpu::QUERY_SIZE);
        let encoder = encoder.get_or_insert_with(|| {
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("XRack 3D timestamp readback"),
            })
        });
        encoder.resolve_query_set(&slot.queries, 0..count, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, bytes);
        let (complete, ready) = mpsc::sync_channel(1);
        encoder.map_buffer_on_submit(
            &slot.readback,
            wgpu::MapMode::Read,
            0..bytes,
            move |result| {
                let _ = complete.send(result);
            },
        );
        slot.pending = Some(Pending {
            frame: recording.frame,
            epoch: self.epoch,
            passes,
            ready,
        });
    }

    pub fn pending(&self) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|slot| slot.pending.is_some())
    }

    pub fn poll(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<ViewportGpuFrame> {
        if !self.pending() {
            return None;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        let mut latest: Option<ViewportGpuFrame> = None;
        for slot in self.slots.iter_mut().flatten() {
            let Some(readiness) = slot
                .pending
                .as_ref()
                .map(|pending| pending.ready.try_recv())
            else {
                continue;
            };
            if matches!(readiness, Err(mpsc::TryRecvError::Empty)) {
                continue;
            }
            let Some(pending) = slot.pending.take() else {
                continue;
            };
            if !matches!(readiness, Ok(Ok(()))) {
                continue;
            }
            if pending.epoch == self.epoch
                && latest
                    .as_ref()
                    .is_none_or(|latest| latest.frame < pending.frame)
            {
                let bytes = pending.passes.entries.len() as u64 * 2 * u64::from(wgpu::QUERY_SIZE);
                if let Ok(mapped) = slot.readback.slice(0..bytes).get_mapped_range() {
                    latest = Some(sum_passes(
                        pending.frame,
                        pending.passes,
                        &mapped,
                        queue.get_timestamp_period(),
                    ));
                }
            }
            slot.readback.unmap();
        }
        latest
    }
}

fn sum_passes(frame: u64, passes: Passes, bytes: &[u8], period: f32) -> ViewportGpuFrame {
    let mut totals: Vec<BTreeMap<String, (u32, u64)>> = (0..passes.viewports.len())
        .map(|_| BTreeMap::new())
        .collect();
    let mut truncated = passes.truncated;
    for ((index, label), pair) in passes.entries.into_iter().zip(bytes.as_chunks::<16>().0) {
        let [begin, end] = pair.as_chunks::<8>().0 else {
            continue;
        };
        let begin = u64::from_ne_bytes(*begin);
        let end = u64::from_ne_bytes(*end);
        if begin == 0 || end < begin {
            truncated = true;
            continue;
        }
        let nanos = ((end - begin) as f64 * f64::from(period)).round() as u64;
        let result = totals[index].entry(label).or_default();
        result.0 += 1;
        result.1 = result.1.saturating_add(nanos);
    }
    let views = passes
        .viewports
        .into_iter()
        .zip(totals)
        .filter(|(_, totals)| !totals.is_empty())
        .map(|((path, id), totals)| {
            let gpu_nanos = totals
                .values()
                .map(|&(_, nanos)| nanos)
                .fold(0u64, u64::saturating_add);
            let passes = totals
                .into_iter()
                .map(|(name, (count, gpu_nanos))| ViewportGpuPass {
                    name,
                    count,
                    gpu_nanos,
                })
                .collect();
            ViewportGpuStats {
                path,
                id,
                gpu_nanos,
                passes,
            }
        })
        .collect();
    ViewportGpuFrame {
        frame,
        truncated,
        views,
    }
}

fn timestamp_pair(label: Option<&str>) -> Option<(wgpu::QuerySet, u32)> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let active = active.as_ref()?;
        let mut passes = active.passes.borrow_mut();
        if passes.entries.len() == MAX_PASSES {
            passes.truncated = true;
            return None;
        }
        let begin = 2 * passes.entries.len() as u32;
        passes
            .entries
            .push((active.view, label.unwrap_or("3D pass").to_owned()));
        Some((passes.queries.clone(), begin))
    })
}

pub(crate) fn render_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    descriptor: &wgpu::RenderPassDescriptor<'_>,
) -> wgpu::RenderPass<'a> {
    if descriptor.timestamp_writes.is_none()
        && let Some((queries, begin)) = timestamp_pair(descriptor.label)
    {
        let mut descriptor = descriptor.clone();
        descriptor.timestamp_writes = Some(wgpu::RenderPassTimestampWrites {
            query_set: &queries,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(begin + 1),
        });
        encoder.begin_render_pass(&descriptor)
    } else {
        encoder.begin_render_pass(descriptor)
    }
}

pub(crate) fn compute_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    descriptor: &wgpu::ComputePassDescriptor<'_>,
) -> wgpu::ComputePass<'a> {
    if descriptor.timestamp_writes.is_none()
        && let Some((queries, begin)) = timestamp_pair(descriptor.label)
    {
        let mut descriptor = descriptor.clone();
        descriptor.timestamp_writes = Some(wgpu::ComputePassTimestampWrites {
            query_set: &queries,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(begin + 1),
        });
        encoder.begin_compute_pass(&descriptor)
    } else {
        encoder.begin_compute_pass(descriptor)
    }
}
