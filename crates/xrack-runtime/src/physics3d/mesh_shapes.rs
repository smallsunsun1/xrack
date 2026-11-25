//! Cook and reuse collider geometry from the same assets used by SceneView3D.

use std::{cell::RefCell, collections::VecDeque};

use rapier3d::{
    parry::utils::Array2,
    prelude::{HeightFieldFlags, SharedShape, TriMeshFlags, Vector},
};

use crate::{
    scene_hierarchy::rest_mesh_objects,
    scene3d::{MAX_IMPORTED_VERTICES, MeshAsset, MeshHeightField},
};

const MESH_SHAPE_CACHE_SIZE: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MeshShapeKind {
    Convex,
    Triangle,
    HeightField,
}

struct Cooked {
    asset: MeshAsset,
    scale: Vector,
    kind: MeshShapeKind,
    shape: SharedShape,
}

thread_local! {
    static COOKED_SHAPES: RefCell<VecDeque<Cooked>> = const { RefCell::new(VecDeque::new()) };
}

pub(super) fn prepare_mesh_shape(
    asset: MeshAsset,
    scale: Vector,
    kind: MeshShapeKind,
) -> Option<SharedShape> {
    let reused = COOKED_SHAPES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache
            .iter()
            .position(|entry| entry.asset == asset && entry.scale == scale && entry.kind == kind)?;
        let entry = cache.remove(index)?;
        let shape = entry.shape.clone();
        cache.push_front(entry);
        Some(shape)
    });
    if reused.is_some() {
        return reused;
    }
    let shape = cook(&asset, scale, kind)?;
    COOKED_SHAPES.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push_front(Cooked {
            asset,
            scale,
            kind,
            shape: shape.clone(),
        });
        cache.truncate(MESH_SHAPE_CACHE_SIZE);
    });
    Some(shape)
}

fn cook(asset: &MeshAsset, scale: Vector, kind: MeshShapeKind) -> Option<SharedShape> {
    if kind == MeshShapeKind::HeightField {
        let field = heightfield(asset)?;
        let [width, height] = field.dimensions();
        // Rapier stores the [z, x] matrix in column-major order.
        let samples: Vec<f32> = (0..width)
            .flat_map(|x| field.samples().iter().skip(x).step_by(width).copied())
            .collect();
        return Some(SharedShape::heightfield_with_flags(
            Array2::new(height, width, samples),
            scale,
            HeightFieldFlags::FIX_INTERNAL_EDGES,
        ));
    }
    let mut vertices = Vec::new();
    for part in rest_mesh_objects(asset) {
        for triangle in part.posed_triangles() {
            if vertices.len() > MAX_IMPORTED_VERTICES - 3 {
                return None;
            }
            let triangle = triangle.map(|point| Vector::from_array(point) * scale);
            if !triangle.iter().all(|point| point.is_finite()) {
                return None;
            }
            vertices.extend(triangle);
        }
    }
    match kind {
        MeshShapeKind::Convex => SharedShape::convex_hull(&vertices),
        MeshShapeKind::Triangle => {
            let count = u32::try_from(vertices.len()).ok()?;
            let indices = (0..count).step_by(3).map(|i| [i, i + 1, i + 2]).collect();
            SharedShape::trimesh_with_flags(
                vertices,
                indices,
                TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED,
            )
            .ok()
        }
        MeshShapeKind::HeightField => None,
    }
}

fn heightfield(asset: &MeshAsset) -> Option<&MeshHeightField> {
    match asset {
        MeshAsset::Static(mesh) => mesh.heightfield(),
        MeshAsset::Lods(lods) => heightfield(lods.primary()),
        MeshAsset::Variants(variants) => heightfield(variants.primary()),
        MeshAsset::Animated(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene3d::Mesh3D;
    use std::sync::Arc;

    #[test]
    fn convex_cooking_reuses_only_the_matching_asset_scale_and_kind() {
        let asset = MeshAsset::Static(Mesh3D::Box);
        let first = prepare_mesh_shape(asset.clone(), Vector::ONE, MeshShapeKind::Convex).unwrap();
        let reused = prepare_mesh_shape(asset.clone(), Vector::ONE, MeshShapeKind::Convex).unwrap();
        assert!(Arc::ptr_eq(&first.0, &reused.0));
        let other_scale =
            prepare_mesh_shape(asset.clone(), Vector::splat(2.0), MeshShapeKind::Convex).unwrap();
        assert!(!Arc::ptr_eq(&first.0, &other_scale.0));
        assert!(other_scale.as_convex_polyhedron().is_some());
        let surface = prepare_mesh_shape(asset, Vector::ONE, MeshShapeKind::Triangle).unwrap();
        assert!(surface.as_trimesh().is_some());
    }

    #[test]
    fn non_square_heightfield_preserves_both_axes() {
        let mesh = Mesh3D::builtin("primitive:rectangle")
            .unwrap()
            .with_heightfield([3, 2], vec![0.0, 0.2, 0.4, 0.5, 0.7, 1.0])
            .unwrap();
        let field = cook(
            &MeshAsset::Static(mesh),
            Vector::new(6.0, 2.0, 3.0),
            MeshShapeKind::HeightField,
        )
        .unwrap();
        let field = field.as_heightfield().unwrap();
        assert_eq!(field.heights().nrows(), 2);
        assert_eq!(field.heights().ncols(), 3);
        assert_eq!(field.heights().data(), [0.0, 0.5, 0.2, 0.7, 0.4, 1.0]);
    }
}
