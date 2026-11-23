//! Import-time planar charts with verified interior separation and texel gutters.
use std::collections::{BTreeMap, HashMap, VecDeque};

use nalgebra::{Vector2, Vector3};
use xrack_runtime::scene3d::{Mesh3D, MeshAsset};

type Point = Vector2<f64>;
type Face = [Vector3<f64>; 3];
type Triangle = [Point; 3];
type VertexKey = [u64; 3];
type Edge = (VertexKey, VertexKey);

const GUTTER: u32 = 2;
const MIN_INTERIOR: u32 = 3;
const FIRST_SIZE: u32 = 64;
const LAST_SIZE: u32 = 512;
const NORMAL_LIMIT: f64 = 0.72;
const GRID_CELLS_PER_FACE: usize = 96;

pub(super) fn generate(asset: MeshAsset) -> Result<MeshAsset, String> {
    let MeshAsset::Static(mesh) = asset else {
        return Err("lightmap-uv requires a static glTF mesh".into());
    };
    let (chart, size) = project_charts(&mesh)?;
    mesh.with_generated_lightmap_uv(chart, size)
        .map(MeshAsset::Static)
        .ok_or_else(|| "could not store generated lightmap coordinates".into())
}

struct Chart {
    faces: Vec<(usize, Triangle)>,
    min: Point,
    max: Point,
}

impl Chart {
    fn size(&self) -> Point {
        self.max - self.min
    }
}

fn project_charts(mesh: &Mesh3D) -> Result<(Vec<[f32; 2]>, u32), String> {
    let faces: Vec<Face> = mesh
        .vertices()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| triangle.map(|vertex| Vector3::from(vertex.position.map(f64::from))))
        .collect();
    let normals: Vec<_> = faces
        .iter()
        .map(|[a, b, c]| (b - a).cross(&(c - a)).try_normalize(1e-28))
        .collect();
    let adjacency = adjacency(&faces, &normals);
    let (min, max) = faces.iter().flatten().fold(
        (
            Vector3::repeat(f64::INFINITY),
            Vector3::repeat(f64::NEG_INFINITY),
        ),
        |(min, max), point| (min.inf(point), max.sup(point)),
    );
    let extent = (max - min).norm().max(1e-9);
    let cell = extent / (faces.len() as f64).sqrt().max(1.0);
    let tolerance = extent * 1e-10;

    let mut charts = Vec::new();
    let mut assigned = vec![false; faces.len()];
    let mut considered = vec![usize::MAX; faces.len()];
    for (seed, normal) in normals.iter().enumerate() {
        let Some(normal) = normal.filter(|_| !assigned[seed]) else {
            continue;
        };
        let anchor = if normal.z.abs() < 0.9 {
            Vector3::z()
        } else {
            Vector3::y()
        };
        let tangent = anchor.cross(&normal).normalize();
        let bitangent = normal.cross(&tangent);
        let origin = faces[seed][0];
        let project = |face: Face| {
            face.map(|point| {
                let delta = point - origin;
                Point::new(delta.dot(&tangent), delta.dot(&bitangent))
            })
        };

        let chart_index = charts.len();
        let mut queue = VecDeque::from([seed]);
        let mut grid = ChartGrid::new(cell);
        let mut chart = Chart {
            faces: Vec::new(),
            min: Point::repeat(f64::INFINITY),
            max: Point::repeat(f64::NEG_INFINITY),
        };
        while let Some(index) = queue.pop_front() {
            if assigned[index] || considered[index] == chart_index {
                continue;
            }
            considered[index] = chart_index;
            if normals[index].is_none_or(|other| other.dot(&normal) < NORMAL_LIMIT) {
                continue;
            }
            let projected = project(faces[index]);
            if grid.overlaps(&chart.faces, &projected, tolerance) {
                continue;
            }
            grid.insert(&projected, chart.faces.len());
            let (min, max) = bounds(&projected);
            chart.min = chart.min.inf(&min);
            chart.max = chart.max.sup(&max);
            chart.faces.push((index, projected));
            assigned[index] = true;
            queue.extend(adjacency[index].into_iter().flatten());
        }
        if !chart.faces.is_empty() {
            charts.push(chart);
        }
    }

    if charts.is_empty() {
        return Err("lightmap-uv found no nondegenerate triangles".into());
    }
    let extents: Vec<_> = charts.iter().map(Chart::size).collect();
    let (resolution, placements) = pack(&extents).ok_or(
        "the generated lightmap charts cannot fit a 512-pixel bake tile with a two-texel gutter",
    )?;
    let mut coordinates = vec![[-1.0; 2]; mesh.vertices().len()];
    for (chart, placement) in charts.iter().zip(placements) {
        let scale = chart.size().map(|value| value.max(1e-20));
        let interior = Point::new(
            f64::from(placement.width - 2 * GUTTER - 1),
            f64::from(placement.height - 2 * GUTTER - 1),
        );
        let inset = Point::new(
            f64::from(placement.x + GUTTER) + 0.5,
            f64::from(placement.y + GUTTER) + 0.5,
        );
        for &(face, uv) in &chart.faces {
            for (corner, point) in uv.into_iter().enumerate() {
                let local = (point - chart.min).component_div(&scale);
                let packed = (inset + local.component_mul(&interior)) / f64::from(resolution);
                coordinates[face * 3 + corner] = [packed.x as f32, packed.y as f32];
            }
        }
    }
    Ok((coordinates, resolution))
}

fn adjacency(faces: &[Face], normals: &[Option<Vector3<f64>>]) -> Vec<[Option<usize>; 3]> {
    // Only manifold edges join charts. Coincident corners do not bridge separate surfaces.
    let mut edges: BTreeMap<Edge, Vec<(usize, usize)>> = BTreeMap::new();
    for (face, vertices) in faces
        .iter()
        .enumerate()
        .filter(|(face, _)| normals[*face].is_some())
    {
        let keys = vertices.map(|vertex| {
            [vertex.x, vertex.y, vertex.z]
                .map(|value| if value == 0.0 { 0 } else { value.to_bits() })
        });
        for side in 0..3 {
            let [a, b] = [keys[side], keys[(side + 1) % 3]];
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((face, side));
        }
    }
    let mut result = vec![[None; 3]; faces.len()];
    for edge in edges.values() {
        if let [(left, a), (right, b)] = edge.as_slice() {
            result[*left][*a] = Some(*right);
            result[*right][*b] = Some(*left);
        }
    }
    result
}

fn bounds(triangle: &Triangle) -> (Point, Point) {
    let [a, b, c] = triangle;
    (a.inf(b).inf(c), a.sup(b).sup(c))
}

struct ChartGrid {
    cell: f64,
    bins: HashMap<(i64, i64), Vec<usize>>,
    broad: Vec<usize>,
}

impl ChartGrid {
    fn new(cell: f64) -> Self {
        Self {
            cell,
            bins: HashMap::new(),
            broad: Vec::new(),
        }
    }

    fn cells(&self, triangle: &Triangle) -> Option<impl Iterator<Item = (i64, i64)> + use<>> {
        let (min, max) = bounds(triangle);
        let min = min.map(|value| (value / self.cell).floor() as i64);
        let max = max.map(|value| (value / self.cell).floor() as i64);
        let width = (max.x - min.x + 1) as usize;
        let height = (max.y - min.y + 1) as usize;
        (width.saturating_mul(height) <= GRID_CELLS_PER_FACE)
            .then(|| (min.y..=max.y).flat_map(move |y| (min.x..=max.x).map(move |x| (x, y))))
    }

    fn insert(&mut self, triangle: &Triangle, index: usize) {
        if let Some(cells) = self.cells(triangle) {
            for cell in cells {
                self.bins.entry(cell).or_default().push(index);
            }
        } else {
            self.broad.push(index);
        }
    }

    fn overlaps(
        &self,
        existing: &[(usize, Triangle)],
        triangle: &Triangle,
        tolerance: f64,
    ) -> bool {
        let intersects = |index: usize| triangles_overlap(triangle, &existing[index].1, tolerance);
        let Some(cells) = self.cells(triangle) else {
            return (0..existing.len()).any(intersects);
        };
        self.broad.iter().copied().any(intersects)
            || cells
                .flat_map(|cell| self.bins.get(&cell).into_iter().flatten().copied())
                .any(intersects)
    }
}

fn triangles_overlap(a: &Triangle, b: &Triangle, tolerance: f64) -> bool {
    // Treat shared boundaries as touching rather than overlapping interiors.
    [a, b].into_iter().all(|shape| {
        (0..3).all(|edge| {
            let direction = shape[(edge + 1) % 3] - shape[edge];
            let axis = Point::new(-direction.y, direction.x);
            let project = |triangle: &Triangle| {
                triangle
                    .iter()
                    .map(|point| point.dot(&axis))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let (a0, a1) = project(a);
            let (b0, b1) = project(b);
            a1.min(b1) - a0.max(b0) > tolerance * axis.norm()
        })
    })
}

#[derive(Clone, Copy, Default)]
struct Placement {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn pack(sizes: &[Point]) -> Option<(u32, Vec<Placement>)> {
    let max_extent = sizes
        .iter()
        .map(|size| size.x.max(size.y))
        .fold(1e-20, f64::max);
    let mut resolution = FIRST_SIZE;
    while resolution <= LAST_SIZE {
        if let Some(mut best) = place(sizes, 0.0, resolution) {
            let mut low = 0.0;
            let mut high = f64::from(resolution) / max_extent;
            for _ in 0..22 {
                let guess = 0.5 * (low + high);
                if let Some(packed) = place(sizes, guess, resolution) {
                    low = guess;
                    best = packed;
                } else {
                    high = guess;
                }
            }
            return Some((resolution, best));
        }
        resolution *= 2;
    }
    None
}

fn place(sizes: &[Point], scale: f64, resolution: u32) -> Option<Vec<Placement>> {
    let extent = |span: f64| (span * scale).ceil() as u32;
    let mut rectangles: Vec<_> = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            let width = extent(size.x).max(MIN_INTERIOR) + 2 * GUTTER;
            let height = extent(size.y).max(MIN_INTERIOR) + 2 * GUTTER;
            (index, width, height)
        })
        .collect();
    rectangles.sort_unstable_by_key(|&(index, width, height)| {
        (u32::MAX - height, u32::MAX - width, index)
    });
    let mut placements = vec![Placement::default(); sizes.len()];
    let mut shelves: Vec<(u32, u32, u32)> = Vec::new();
    let mut bottom = 0;
    for (index, width, height) in rectangles {
        if width > resolution || height > resolution {
            return None;
        }
        let best = shelves
            .iter()
            .enumerate()
            .filter(|&(_, &(_, row_height, used))| {
                height <= row_height && width + used <= resolution
            })
            .min_by_key(|&(_, &(_, _, used))| resolution - used - width)
            .map(|(index, _)| index);
        let (x, y) = if let Some(row) = best {
            let (top, _, used) = &mut shelves[row];
            let x = *used;
            *used += width;
            (x, *top)
        } else {
            if bottom + height > resolution {
                return None;
            }
            let y = bottom;
            shelves.push((y, height, width));
            bottom += height;
            (0, y)
        };
        placements[index] = Placement {
            x,
            y,
            width,
            height,
        };
    }
    Some(placements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::scene3d::{MeshAlphaMode, MeshMaterial, MeshPart, MeshVertex};

    fn mesh(positions: &[[f32; 3]]) -> Result<Mesh3D, &'static str> {
        let vertices = positions
            .iter()
            .map(|&position| MeshVertex {
                position,
                normal: [0.0, 0.0, 1.0],
                uv: [0.2, 0.75],
                uv1: [0.85, 0.1],
                color: [1.0; 4],
            })
            .collect();
        Mesh3D::imported(
            vertices,
            Vec::new(),
            vec![MeshPart {
                vertices: 0..positions.len(),
                material: 0,
                volume_inverse: None,
            }],
            vec![MeshMaterial {
                base_color: [1.0; 4],
                base_texture: None,
                alpha_mode: MeshAlphaMode::Opaque,
                double_sided: false,
                pbr: None,
            }],
        )
    }

    #[test]
    fn planar_neighbors_keep_shared_texels_without_changing_material_coordinates()
    -> Result<(), String> {
        let corners = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        let source = mesh(&indices.map(|index| corners[index]))?;
        let generated = generate(MeshAsset::Static(source.clone()))?;
        let MeshAsset::Static(generated) = generated else {
            return Err("expected static chart".into());
        };
        let uv = generated.lightmap_uvs();
        assert_eq!(uv[0], uv[3]);
        assert_eq!(uv[2], uv[4]);
        assert_eq!(generated.lightmap_min_size(), 64);
        assert!(source.lightmap_uvs().is_empty());
        assert!(
            generated
                .vertices()
                .iter()
                .zip(source.vertices())
                .all(|(a, b)| a.uv == b.uv && a.uv1 == b.uv1)
        );
        let (repeat, size) = project_charts(&source)?;
        assert_eq!(uv, repeat);
        assert_eq!(generated.lightmap_min_size(), size);
        Ok(())
    }

    #[test]
    fn projected_interiors_that_overlap_get_distinct_padded_charts() -> Result<(), String> {
        let source = mesh(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.18, 0.47, 0.07],
        ])?;
        let (uv, size) = project_charts(&source)?;
        let a = std::array::from_fn(|i| Point::from(uv[i].map(f64::from)));
        let b = std::array::from_fn(|i| Point::from(uv[3 + i].map(f64::from)));
        let (amin, amax) = bounds(&a);
        let (bmin, bmax) = bounds(&b);
        let gap = (0..2)
            .map(|axis| (bmin[axis] - amax[axis]).max(amin[axis] - bmax[axis]))
            .fold(0.0, f64::max);
        assert!(gap * f64::from(size) >= 2.0 * f64::from(GUTTER) - 0.01);
        assert!(!triangles_overlap(&a, &b, 1e-8));
        Ok(())
    }

    #[test]
    fn automatic_lod_retains_the_generated_texel_mapping() -> Result<(), String> {
        let mut triangles = Vec::new();
        for row in 0..14 {
            for column in 0..14 {
                let x = column as f32 / 14.0;
                let y = row as f32 / 14.0;
                let x1 = (column + 1) as f32 / 14.0;
                let y1 = (row + 1) as f32 / 14.0;
                let quad = [[x, y, 0.0], [x1, y, 0.0], [x1, y1, 0.0], [x, y1, 0.0]];
                triangles.extend([0, 1, 2, 0, 2, 3].map(|index| quad[index]));
            }
        }
        let source = generate(MeshAsset::Static(mesh(&triangles)?))?;
        let MeshAsset::Static(original) = &source else {
            return Err("expected static mesh".into());
        };
        let min_size = original.lightmap_min_size();
        let charts: HashMap<_, _> = original
            .vertices()
            .iter()
            .zip(original.lightmap_uvs())
            .map(|(vertex, uv)| (vertex.position.map(f32::to_bits), *uv))
            .collect();
        let asset = crate::mesh_lod::generate(source);
        let MeshAsset::Lods(lods) = asset else {
            return Err("planar grid should produce LODs".into());
        };
        assert!(!lods.levels().is_empty());
        for level in lods.levels() {
            for mesh in &level.meshes {
                assert_eq!(mesh.lightmap_min_size(), min_size);
                assert_eq!(mesh.vertices().len(), mesh.lightmap_uvs().len());
                assert!(
                    mesh.vertices()
                        .iter()
                        .zip(mesh.lightmap_uvs())
                        .all(|(vertex, uv)| {
                            charts.get(&vertex.position.map(f32::to_bits)) == Some(uv)
                        })
                );
            }
        }
        Ok(())
    }
}
