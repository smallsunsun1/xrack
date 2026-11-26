use std::ops::Range;

use super::buffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CompactTile {
    pub offset: u32,
    pub size: u32,
}

impl CompactTile {
    pub fn stride(self) -> u32 {
        self.size.div_ceil(2)
    }

    pub fn words(self) -> u32 {
        self.stride() * self.size
    }

    fn range(self) -> Range<u32> {
        self.offset..self.offset + self.words()
    }
}

/// Depth16 rows are packed in pairs. Whole words are owned by one view.
pub(super) fn pack_compact(
    sizes: &[u32],
    previous: &[Option<CompactTile>],
    capacity: u32,
) -> Option<Vec<Option<CompactTile>>> {
    let mut tiles = vec![None; sizes.len()];
    let mut occupied = Vec::new();
    for (index, (&size, &old)) in sizes.iter().zip(previous).enumerate() {
        if let Some(tile) = old.filter(|tile| tile.size == size && tile.range().end <= capacity) {
            occupied.push(tile.range());
            tiles[index] = Some(tile);
        }
    }
    occupied.sort_by_key(|range| range.start);
    for (&size, tile) in sizes.iter().zip(&mut tiles) {
        if size == 0 || tile.is_some() {
            continue;
        }
        let mut next = CompactTile { offset: 0, size };
        let words = next.words();
        let index = occupied
            .iter()
            .position(|range| {
                if next.offset + words <= range.start {
                    true
                } else {
                    next.offset = range.end;
                    false
                }
            })
            .unwrap_or(occupied.len());
        if next.offset + words > capacity {
            return None;
        }
        occupied.insert(index, next.range());
        *tile = Some(next);
    }
    Some(tiles)
}

pub(super) struct DepthPacker {
    pipeline: wgpu::ComputePipeline,
    input_layout: wgpu::BindGroupLayout,
}

impl DepthPacker {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 16-bit shadow packing"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow_compact.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("XRack 16-bit shadow packing"),
            layout: None,
            module: &shader,
            entry_point: Some("pack_depth"),
            compilation_options: Default::default(),
            cache: None,
        });
        let input_layout = pipeline.get_bind_group_layout(0);
        Self {
            pipeline,
            input_layout,
        }
    }

    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        output: &wgpu::Buffer,
        tile: CompactTile,
    ) {
        let bytes = [tile.offset, tile.size, tile.stride(), 0].map(u32::to_ne_bytes);
        let parameters = buffer(
            device,
            "XRack 16-bit shadow tile",
            bytes.as_flattened(),
            wgpu::BufferUsages::UNIFORM,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 16-bit shadow packing"),
            layout: &self.input_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: parameters.as_entire_binding(),
                },
            ],
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack 16-bit shadow packing"),
                timestamp_writes: None,
            },
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(tile.stride().div_ceil(8), tile.size.div_ceil(8), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_depth_keeps_word_alignment_and_existing_faces() {
        let sizes = [129, 128, 0, 256];
        let capacity = sizes
            .iter()
            .map(|&size| CompactTile { offset: 0, size }.words())
            .sum();
        let placed = pack_compact(&sizes, &[], capacity).unwrap();
        assert_eq!(placed[0].unwrap().words(), 65 * 129);
        assert_eq!(placed[1].unwrap().offset, 65 * 129);
        let shorter = pack_compact(&[0, 128, 127, 256], &placed, capacity).unwrap();
        assert_eq!(shorter[1], placed[1]);
        assert_eq!(shorter[3], placed[3]);
        assert_eq!(shorter[2].unwrap().offset, 0);
    }
}
