//! Bounded per-pixel fragment lists. The weighted attachments provide the
//! result on devices with insufficient capacity and for overflowing pixels.
use std::collections::HashMap;

use super::{buffer, pipeline::PACKED_SHADOW_BINDING};

const HEADS: u32 = PACKED_SHADOW_BINDING + 1;
const FRAGMENTS: u32 = HEADS + 1;
const DEPTH: u32 = FRAGMENTS + 1;
const REGION: u32 = DEPTH + 1;
const WORKING_SET_BYTES: u64 = 64 * 1024 * 1024;
const NODE_BYTES: u64 = 16;
const NODES_PER_SAMPLE: u64 = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkedSamples {
    Single,
    Multisampled,
}

impl LinkedSamples {
    pub const fn from_multisampling(multisampling: bool) -> Self {
        if multisampling {
            Self::Multisampled
        } else {
            Self::Single
        }
    }

    pub const fn multisampled(self) -> bool {
        matches!(self, Self::Multisampled)
    }

    pub const fn slot(self) -> usize {
        self as usize
    }
}

pub(super) fn capture_shader(source: &str, samples: LinkedSamples) -> String {
    let (depth_type, depth_index, sample_count, sample_index, parameter) = if samples.multisampled()
    {
        (
            "texture_depth_multisampled_2d",
            "i32(sample)",
            "textureNumSamples(xr_linked_depth)",
            "xr_sample",
            ", @builtin(sample_index) xr_sample: u32",
        )
    } else {
        ("texture_depth_2d", "0", "1u", "0u", "")
    };
    let capture = include_str!("linked_capture.wgsl")
        .replace("XR_LINKED_HEADS_SLOT", &HEADS.to_string())
        .replace("XR_LINKED_FRAGMENTS_SLOT", &FRAGMENTS.to_string())
        .replace("XR_LINKED_DEPTH_SLOT", &DEPTH.to_string())
        .replace("XR_LINKED_REGION_SLOT", &REGION.to_string())
        .replace("XR_LINKED_DEPTH_TYPE", depth_type)
        .replace("XR_LINKED_DEPTH_INDEX", depth_index)
        .replace("XR_LINKED_SAMPLES", sample_count);
    let scene = if samples.multisampled() {
        source
            .replace(
                "// XRack linked sample varying",
                "@location(12) @interpolate(perspective, sample) xr_clip_depth: vec2<f32>,",
            )
            .replace("// XRack linked clip depth", "clip.zw,")
            .replace("// XRack linked offset clip depth", "offset_clip.zw,")
            .replace("// XRack linked ribbon depth", "sprite.xr_clip_depth,")
    } else {
        source.to_owned()
    };
    let mut scene = scene.replace("/* XRack linked sample */", parameter);
    for vertex in ["vertex", "sprite"] {
        let depth = if samples.multisampled() {
            format!("clamp({vertex}.xr_clip_depth.x / {vertex}.xr_clip_depth.y, 0.0, 1.0)")
        } else {
            format!("{vertex}.position.z")
        };
        scene = scene.replace(
            &format!("// XRack linked fragment {vertex}.position"),
            &format!("xr_record_fragment(color, {vertex}.position.xy, {depth}, {sample_index});"),
        );
    }
    format!("{capture}\n{scene}")
}

pub(super) fn capture_layout_entries(samples: LinkedSamples) -> [wgpu::BindGroupLayoutEntry; 4] {
    [
        storage_slot(HEADS, false),
        storage_slot(FRAGMENTS, false),
        wgpu::BindGroupLayoutEntry {
            binding: DEPTH,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: samples.multisampled(),
            },
            count: None,
        },
        region_slot(REGION),
    ]
}

fn region_slot(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_slot(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[derive(Default)]
pub(super) struct LinkedTransparency {
    layouts: [Option<wgpu::BindGroupLayout>; 2],
    pipelines: HashMap<(wgpu::TextureFormat, bool), wgpu::RenderPipeline>,
    buffers: Option<Buffers>,
    allocations: u64,
}

struct Buffers {
    size: [u32; 2],
    samples: u32,
    heads: wgpu::Buffer,
    fragments: wgpu::Buffer,
    region: Region,
    resolve_bindings: HashMap<Option<wgpu::TextureView>, wgpu::BindGroup>,
}

struct Region {
    bounds: [u32; 4],
    uniform: wgpu::Buffer,
}

impl Region {
    fn new(device: &wgpu::Device, bounds: [u32; 4]) -> Self {
        Self {
            bounds,
            uniform: buffer(
                device,
                "XRack transparency scissor",
                bounds.map(u32::to_ne_bytes).as_flattened(),
                wgpu::BufferUsages::UNIFORM,
            ),
        }
    }
}

pub(super) struct LinkedFrame<'a> {
    heads: &'a wgpu::Buffer,
    fragments: &'a wgpu::Buffer,
    resolve_bindings: &'a wgpu::BindGroup,
    region: &'a Region,
    multisampling: bool,
    pub allocations: u64,
    pipeline: &'a wgpu::RenderPipeline,
}

impl LinkedTransparency {
    pub fn prepare<'a>(
        &'a mut self,
        device: &wgpu::Device,
        (target, region): ([u32; 2], [u32; 4]),
        format: wgpu::TextureFormat,
        colors: &[wgpu::TextureView; 2],
        opaque_samples: Option<&wgpu::TextureView>,
    ) -> Option<LinkedFrame<'a>> {
        let multisampling = opaque_samples.is_some();
        let samples = opaque_samples.map_or(1, |view| view.texture().sample_count());
        let [_, _, width, height] = region;
        let size = [width, height];
        let sample_count = size.into_iter().map(u64::from).product::<u64>() * u64::from(samples);
        let heads_bytes = (sample_count + 1) * 4;
        let limits = device.limits();
        let storage_limit = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        // Keep the previous fragment capacity for dense areas brought into view
        // by a scissor. Compact heads leave additional room under the same cap.
        let target_samples =
            target.into_iter().map(u64::from).product::<u64>() * u64::from(samples);
        let bytes = (target_samples * NODES_PER_SAMPLE + 1)
            .saturating_mul(NODE_BYTES)
            .min(WORKING_SET_BYTES.saturating_sub(heads_bytes))
            .min(storage_limit);
        let fragments_bytes = bytes / NODE_BYTES * NODE_BYTES;
        if heads_bytes > storage_limit
            || fragments_bytes < (sample_count.min(256 * 1024) + 1) * NODE_BYTES
        {
            return None;
        }

        let layout = self.layouts[usize::from(multisampling)]
            .get_or_insert_with(|| resolve_layout(device, multisampling));
        if self.buffers.as_ref().is_some_and(|buffers| {
            buffers.size != size
                || buffers.samples != samples
                || buffers.fragments.size() != fragments_bytes
        }) {
            self.buffers = None;
        }
        let buffers = self.buffers.get_or_insert_with(|| {
            self.allocations = self.allocations.saturating_add(1);
            let heads = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack transparency fragment heads"),
                size: heads_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let fragments = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack transparency linked fragments"),
                size: fragments_bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            Buffers {
                size,
                samples,
                heads,
                fragments,
                region: Region::new(device, region),
                resolve_bindings: HashMap::new(),
            }
        });
        if buffers.region.bounds != region {
            buffers.region = Region::new(device, region);
            buffers.resolve_bindings.clear();
        }
        let resolve_bindings = buffers
            .resolve_bindings
            .entry(opaque_samples.cloned())
            .or_insert_with(|| {
                let first = super::transparency::first_binding(multisampling);
                let mut entries: Vec<_> = colors
                    .iter()
                    .chain(opaque_samples)
                    .enumerate()
                    .map(|(index, view)| wgpu::BindGroupEntry {
                        binding: first + index as u32,
                        resource: wgpu::BindingResource::TextureView(view),
                    })
                    .collect();
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: buffers.heads.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: buffers.fragments.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: buffers.region.uniform.as_entire_binding(),
                    },
                ]);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("XRack transparent fragment list resolve bindings"),
                    layout,
                    entries: &entries,
                })
            });
        let pipeline = self
            .pipelines
            .entry((format, multisampling))
            .or_insert_with(|| {
                super::transparency::composite_pipeline(device, layout, format, multisampling, true)
            });
        Some(LinkedFrame {
            heads: &buffers.heads,
            fragments: &buffers.fragments,
            resolve_bindings,
            region: &buffers.region,
            multisampling,
            allocations: self.allocations,
            pipeline,
        })
    }

    pub fn invalidate_bindings(&mut self, multisampling: bool) {
        if let Some(buffers) = self.buffers.as_mut()
            && (buffers.samples > 1) == multisampling
        {
            buffers.resolve_bindings.clear();
        }
    }
}

impl LinkedFrame<'_> {
    pub fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(self.heads, 0, None);
    }

    pub fn bytes(&self) -> u64 {
        self.heads.size() + self.fragments.size()
    }

    pub fn capture_entries<'a>(
        &'a self,
        depth: &'a wgpu::TextureView,
    ) -> [wgpu::BindGroupEntry<'a>; 4] {
        [
            wgpu::BindGroupEntry {
                binding: HEADS,
                resource: self.heads.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: FRAGMENTS,
                resource: self.fragments.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: DEPTH,
                resource: wgpu::BindingResource::TextureView(depth),
            },
            wgpu::BindGroupEntry {
                binding: REGION,
                resource: self.region.uniform.as_entire_binding(),
            },
        ]
    }

    pub fn resolve(&self, pass: &mut wgpu::RenderPass<'_>) {
        // The multisampled composite also resolves opaque samples outside the scissor.
        if !self.multisampling {
            let [x, y, width, height] = self.region.bounds;
            pass.set_scissor_rect(x, y, width, height);
        }
        pass.set_pipeline(self.pipeline);
        pass.set_bind_group(0, self.resolve_bindings, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn resolve_layout(device: &wgpu::Device, multisampled: bool) -> wgpu::BindGroupLayout {
    let mut entries: Vec<_> = super::transparency::composite_textures(multisampled).collect();
    entries.extend([storage_slot(5, true), storage_slot(6, true), region_slot(7)]);
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("XRack transparent fragment list resolve layout"),
        entries: &entries,
    })
}
