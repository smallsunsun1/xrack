//! Share expanded primitive geometry between materials and rendering passes.

use std::{ops::Range, sync::Arc};

use xrack_runtime::scene3d::{Mesh3D, MeshDrawIndices, MeshIndexElements};

use super::{append_floats, buffer, deform::Deformation};

pub(super) type PrimitiveRanges = Option<Arc<[Range<usize>]>>;

pub(super) struct QuadRanges<'a> {
    pub points: &'a [Range<usize>],
    pub lines: &'a [Range<usize>],
    pub line_layout: &'a wgpu::BindGroupLayout,
}

pub(super) struct LineVertices {
    pub endpoints: wgpu::Buffer,
    pub rigid: wgpu::BindGroup,
}

pub(super) struct PrimitiveQuads {
    /// All GPU vertex rows, including the original mesh and each point's four corners.
    pub rows: Vec<u32>,
    pub indices: Vec<u32>,
    pub points: Vec<(usize, Range<u32>)>,
    pub lines: Vec<(usize, Range<u32>)>,
    line_start: u32,
    line_sources: Vec<usize>,
}

impl PrimitiveQuads {
    pub fn prepare(
        source_count: usize,
        compact: Option<&MeshDrawIndices>,
        points: &[Range<usize>],
        lines: &[Range<usize>],
    ) -> Self {
        let points_count = points.iter().map(Range::len).sum::<usize>();
        let line_vertices = lines.iter().map(Range::len).sum::<usize>();
        let base_rows = compact.map_or(source_count, |data| data.rows.len());
        let mut rows = Vec::with_capacity(
            base_rows.next_multiple_of(4) + points_count * 4 + line_vertices * 2,
        );
        let mut indices = Vec::with_capacity(source_count + points_count * 6 + line_vertices * 3);
        match compact {
            Some(data) => {
                rows.extend_from_slice(&data.rows);
                match &data.elements {
                    MeshIndexElements::U16(values) => {
                        indices.extend(values.iter().copied().map(u32::from));
                    }
                    MeshIndexElements::U32(values) => indices.extend_from_slice(values),
                }
            }
            None => {
                rows.extend(0..source_count as u32);
                indices.extend(0..source_count as u32);
            }
        }
        // Vertex index modulo four identifies the corner in every pass.
        rows.resize(base_rows.next_multiple_of(4), 0);
        let mut ranges = Vec::with_capacity(points.len());
        for range in points {
            let start = indices.len() as u32;
            for source in range.clone() {
                let base = rows.len() as u32;
                rows.extend([source as u32; 4]);
                indices.extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
            }
            ranges.push((range.start, start..indices.len() as u32));
        }
        let line_start = rows.len() as u32;
        let mut line_sources = Vec::with_capacity(line_vertices);
        let mut line_ranges = Vec::with_capacity(lines.len());
        for range in lines {
            let start = indices.len() as u32;
            for source in range.clone().step_by(2) {
                let base = rows.len() as u32;
                rows.extend([
                    source as u32,
                    source as u32,
                    source as u32 + 1,
                    source as u32 + 1,
                ]);
                indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                line_sources.extend([source, source + 1]);
            }
            line_ranges.push((range.start, start..indices.len() as u32));
        }
        Self {
            rows,
            indices,
            points: ranges,
            lines: line_ranges,
            line_start,
            line_sources,
        }
    }

    pub(super) fn line_vertices(
        &self,
        device: &wgpu::Device,
        mesh: &Mesh3D,
        layout: &wgpu::BindGroupLayout,
    ) -> Option<LineVertices> {
        if self.line_sources.is_empty() {
            return None;
        }
        let skinned = mesh.influences();
        // A one-record header stores the first expanded vertex index. Each
        // subsequent record contains one source endpoint, in segment order.
        let stride = if skinned.is_some() { 64 } else { 16 };
        let mut bytes = vec![0; 16];
        bytes[..4].copy_from_slice(&self.line_start.to_ne_bytes());
        bytes[4..8].copy_from_slice(&((stride / 4) as u32).to_ne_bytes());
        bytes.reserve(stride * self.line_sources.len());
        for &source in &self.line_sources {
            let [x, y, z] = mesh.vertices()[source].position;
            append_floats(&mut bytes, [x, y, z, 0.0]);
            if let Some(influences) = skinned {
                bytes.extend(influences[source].packed());
            }
        }
        let endpoints = buffer(
            device,
            "XRack 3D wide-line endpoints",
            &bytes,
            wgpu::BufferUsages::STORAGE,
        );
        // Skinned models can use a rigid fallback when no joint pose is bound.
        // The shader reads the stride from the header separately from the pipeline.
        let rigid = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack rigid wide-line bindings"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 4,
                resource: endpoints.as_entire_binding(),
            }],
        });
        Some(LineVertices { endpoints, rigid })
    }
}

/// Specialize only wide-line vertex modules. Native primitives keep their
/// original bind-group layouts, including rigid meshes without group three.
pub(super) fn line_shader(source: &str, mode: Deformation) -> String {
    let current = include_str!("line_raster.wgsl")
        .replace(
            "// XRack line current morph",
            if mode.morph() {
                "local += morph_displacement(xr_line_peer_index(index), 0u).position;"
            } else {
                ""
            },
        )
        .replace(
            "// XRack line current skin",
            if mode.skin() {
                "local = skin_point(skin_transform(xr_line_skin_weights(index)), local);"
            } else {
                ""
            },
        );
    if source.contains("// XRack widen motion") {
        let previous_morph = if mode.morph() {
            "local = previous_morph_point(xr_line_peer_index(index), local);"
        } else {
            ""
        };
        let previous_skin = if mode.skin() {
            "local = previous_skin_point(xr_line_skin_weights(index), local);"
        } else {
            ""
        };
        let motion = source.replace("// XRack widen motion", r#"
            let peer = motion_world(vertex, xr_line_local_peer(vertex.index)) * cameras.current.clip;
            let old_peer = previous_motion_world(old, previous_line_peer(vertex.index)) * cameras.previous.clip;
            current_clip = xr_line_corner(current_clip, peer, vertex.index, material.primitive_sizes.y, cameras.extent.xy);
            previous_clip = xr_line_corner(previous_clip, old_peer, vertex.index, material.primitive_sizes.y, cameras.extent.xy);
        "#);
        format!(
            "{current}\n{motion}\nfn previous_line_peer(index: u32) -> vec3<f32> {{
            var local = xr_line_raw_peer(index);
            {previous_morph}
            {previous_skin}
            return local;
        }}"
        )
    } else {
        let source = source.replace("// XRack widen color", r#"
            let peer = mesh_clip(vertex, xr_line_local_peer(vertex.index));
            clip = xr_line_corner(clip, peer, vertex.index, material.primitive_sizes.y, globals.ao.zw);
        "#).replace("// XRack widen depth", r#"
            let peer = geometry_clip(vertex, xr_line_local_peer(vertex.index));
            position = xr_line_corner(position, peer, vertex.index, material.primitive_sizes.y, point_extent.xy);
        "#);
        format!("{current}\n{source}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_lines_pair_the_source_endpoints_after_point_quads() {
        let quads = PrimitiveQuads::prepare(7, None, &[0..1], &[1..5]);
        assert_eq!(quads.line_start, 12);
        assert_eq!(quads.lines, [(1, 13..25)]);
        assert_eq!(quads.line_sources, [1, 2, 3, 4]);
        assert_eq!(&quads.rows[12..], &[1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(&quads.indices[13..19], &[12, 14, 13, 13, 14, 15]);
    }

    #[test]
    fn wide_groups_append_aligned_corners_with_native_draws_intact() {
        let quads = PrimitiveQuads::prepare(5, None, &[1..2, 3..5], &[]);
        assert_eq!(&quads.indices[..5], &[0, 1, 2, 3, 4]);
        assert_eq!(&quads.rows[8..], &[1, 1, 1, 1, 3, 3, 3, 3, 4, 4, 4, 4]);
        assert_eq!(&quads.indices[5..11], &[8, 9, 10, 10, 9, 11]);
        assert_eq!(quads.points, [(1, 5..11), (3, 11..23)]);
    }
}
