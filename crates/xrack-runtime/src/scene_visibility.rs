//! Conservative frustum queries for immutable posed scene models.
use crate::{
    Point2D,
    scene_bounds::{Bounds3D, BoundsTree},
    scene_pick::BoundsSegment,
    scene3d::{SceneCamera, SceneObject},
};
use nalgebra::Vector3;
use std::{ops::Range, sync::OnceLock};

pub(crate) const INDEX_THRESHOLD: usize = 32;
const LEAF_SIZE: usize = 8;
const ALL_PLANES: u8 = 0b11_1111;

struct Plane {
    coefficients: [f64; 4],
    uncertainty: [f64; 4],
}

impl Plane {
    fn new(a: [f32; 4], b: [f32; 4], sign: f64) -> Self {
        let coefficients = std::array::from_fn(|i| f64::from(a[i]) + sign * f64::from(b[i]));
        // The GPU evaluates clip coordinates independently, then compares them.
        // Preserve both terms' rounding error even when their difference is tiny.
        let uncertainty =
            std::array::from_fn(|i| f64::from(a[i].abs()) + sign.abs() * f64::from(b[i].abs()));
        Self {
            coefficients,
            uncertainty,
        }
    }

    fn extent(&self, bounds: Bounds3D) -> (f64, f64, f64) {
        let mut low = self.coefficients[3];
        let mut high = low;
        let mut error = self.uncertainty[3];
        for axis in 0..3 {
            let a = f64::from(bounds.min[axis]);
            let b = f64::from(bounds.max[axis]);
            let x = self.coefficients[axis] * a;
            let y = self.coefficients[axis] * b;
            low += x.min(y);
            high += x.max(y);
            error += self.uncertainty[axis] * a.abs().max(b.abs());
        }
        (low, high, error * f64::from(32.0 * f32::EPSILON))
    }
}

pub(crate) struct Frustum([Plane; 6]);

impl Frustum {
    pub(crate) fn new(camera: &SceneCamera) -> Self {
        let [x, y, z, w] = camera.clip_rows();
        Self([
            Plane::new(w, x, 1.0),
            Plane::new(w, x, -1.0),
            Plane::new(w, y, 1.0),
            Plane::new(w, y, -1.0),
            Plane::new(z, [0.0; 4], 1.0),
            Plane::new(w, z, -1.0),
        ])
    }

    pub(crate) fn selection(camera: &SceneCamera, [start, end]: [Point2D; 2]) -> Option<Self> {
        let [sx, sy, ex, ey] = [start.x.0, start.y.0, end.x.0, end.y.0];
        if ![sx, sy, ex, ey].into_iter().all(f64::is_finite) {
            return None;
        }
        let [x, y, width, height] = camera.content_rect();
        let clamp = |low: f64, high: f64, offset: f64, size: f64| {
            [
                (low - offset).clamp(0.0, size),
                (high - offset).clamp(0.0, size),
            ]
        };
        let [left, right] = clamp(sx.min(ex), sx.max(ex), x, width);
        let [top, bottom] = clamp(sy.min(ey), sy.max(ey), y, height);
        if left >= right || top >= bottom {
            return None;
        }
        let [left, right] = [left, right].map(|v| 2.0 * (v / width) - 1.0);
        let [top, bottom] = [top, bottom].map(|v| 1.0 - 2.0 * (v / height));
        let [x, y, z, w] = camera.clip_rows();
        Some(Self([
            Plane::new(x, w, -left),
            Plane::new(x.map(|v| -v), w, right),
            Plane::new(y, w, -bottom),
            Plane::new(y.map(|v| -v), w, top),
            Plane::new(z, [0.0; 4], 1.0),
            Plane::new(w, z, -1.0),
        ]))
    }

    pub(crate) fn triangle_intersects(&self, points: [[f32; 3]; 3], planes: u8) -> bool {
        // A convex triangle gains at most one vertex per clipping plane.
        let mut polygon = [[0.0; 3]; 9];
        let mut clipped = [[0.0; 3]; 9];
        polygon[..3].copy_from_slice(&points.map(|p| p.map(f64::from)));
        let mut count = 3;
        for (index, plane) in self.0.iter().enumerate() {
            if planes & (1 << index) == 0 {
                continue;
            }
            let [a, b, c, d] = plane.coefficients;
            let distance = |[x, y, z]: [f64; 3]| a * x + b * y + c * z + d;
            let mut values = [0.0; 9];
            for (value, &point) in values[..count].iter_mut().zip(&polygon[..count]) {
                *value = distance(point);
            }
            let values = &values[..count];
            if values.iter().all(|&value| value >= 0.0) {
                continue;
            }
            if values.iter().all(|&value| value < 0.0) {
                return false;
            }
            let mut len = 0;
            for next in 0..count {
                let previous = if next == 0 { count - 1 } else { next - 1 };
                let a = values[previous];
                let b = values[next];
                if a < 0.0 && b > 0.0 || a > 0.0 && b < 0.0 {
                    let t = a / (a - b);
                    let Some(point) = clipped.get_mut(len) else {
                        return false;
                    };
                    *point = std::array::from_fn(|axis| {
                        t * polygon[next][axis] + (1.0 - t) * polygon[previous][axis]
                    });
                    len += 1;
                }
                if b >= 0.0 {
                    let Some(point) = clipped.get_mut(len) else {
                        return false;
                    };
                    *point = polygon[next];
                    len += 1;
                }
            }
            if len == 0 {
                return false;
            }
            count = len;
            std::mem::swap(&mut polygon, &mut clipped);
        }
        true
    }

    pub(crate) fn bounds_planes(&self, bounds: Bounds3D) -> Option<u8> {
        self.intersecting_planes(bounds, ALL_PLANES)
    }

    pub(crate) fn intersects(&self, bounds: Bounds3D) -> bool {
        self.intersecting_planes(bounds, ALL_PLANES).is_some()
    }

    /// Return the planes that still intersect the bounds, or reject the bounds.
    pub(crate) fn intersecting_planes(&self, bounds: Bounds3D, active: u8) -> Option<u8> {
        let mut remaining = active;
        for (index, plane) in self.0.iter().enumerate() {
            let bit = 1 << index;
            if active & bit == 0 {
                continue;
            }
            let (low, high, padding) = plane.extent(bounds);
            if high < -padding {
                return None;
            }
            if low > padding {
                remaining &= !bit;
            }
        }
        Some(remaining)
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct VisibilityIndex {
    union: Bounds3D,
    objects: Box<[Bounds3D]>,
    tree: OnceLock<BoundsTree<usize>>,
}

impl VisibilityIndex {
    pub fn new(objects: &[SceneObject]) -> Option<Self> {
        let bounds: Box<[_]> = objects.iter().map(SceneObject::world_bounds).collect();
        if !bounds.iter().all(|bounds| bounds.is_finite()) {
            return None;
        }
        let union = bounds.iter().copied().reduce(Bounds3D::union)?;
        Some(Self {
            union,
            objects: bounds,
            tree: OnceLock::new(),
        })
    }

    pub fn visibility(&self, camera: &SceneCamera) -> SceneVisibility {
        let count = self.objects.len();
        let frustum = Frustum::new(camera);
        let Some(planes) = frustum.intersecting_planes(self.union, ALL_PLANES) else {
            return SceneVisibility::none();
        };
        if planes == 0 {
            return SceneVisibility::all(count);
        }
        let mut words = vec![0_u64; count.div_ceil(64)];
        let mut visible = 0;
        self.visit_visible(&frustum, planes, &mut |index, _| {
            words[index / 64] |= 1 << (index % 64);
            visible += 1;
        });
        match visible {
            0 => SceneVisibility::none(),
            n if n == count => SceneVisibility::all(n),
            n => SceneVisibility {
                visible: n,
                mask: Mask::Mixed(words.into()),
            },
        }
    }

    pub(crate) fn visit_selection(&self, frustum: &Frustum, mut visit: impl FnMut(usize, u8)) {
        if let Some(planes) = frustum.bounds_planes(self.union) {
            self.visit_visible(frustum, planes, &mut visit);
        }
    }

    fn visit_visible(&self, frustum: &Frustum, planes: u8, visit: &mut impl FnMut(usize, u8)) {
        if planes == 0 {
            for index in 0..self.objects.len() {
                visit(index, 0);
            }
        } else if self.objects.len() >= INDEX_THRESHOLD {
            self.visit(self.tree(), 0, frustum, planes, visit);
        } else {
            for (index, &bounds) in self.objects.iter().enumerate() {
                if let Some(remaining) = frustum.intersecting_planes(bounds, planes) {
                    visit(index, remaining);
                }
            }
        }
    }

    fn tree(&self) -> &BoundsTree<usize> {
        self.tree.get_or_init(|| {
            BoundsTree::new(
                self.objects
                    .iter()
                    .copied()
                    .zip(0..self.objects.len())
                    .collect(),
                LEAF_SIZE,
            )
        })
    }

    pub(crate) fn visit_sweep(
        &self,
        mut span: BoundsSegment,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        if span.entry(self.union).is_none() {
            return;
        }
        if self.objects.len() >= INDEX_THRESHOLD {
            span.visit(self.tree(), |&index, closest| visit(index, closest));
        } else {
            for (index, &bounds) in self.objects.iter().enumerate() {
                if span.entry(bounds).is_some() {
                    span.max = visit(index, span.max);
                }
            }
        }
    }

    pub(crate) fn visit_nearest(
        &self,
        point: Vector3<f64>,
        mut limit: f64,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        if self.union.distance_squared(point) > limit {
            return;
        }
        if self.objects.len() >= INDEX_THRESHOLD {
            self.tree().visit_nearest(
                limit,
                |bounds| bounds.distance_squared(point),
                |&index, limit| {
                    if self.objects[index].distance_squared(point) <= limit {
                        visit(index, limit)
                    } else {
                        limit
                    }
                },
            );
        } else {
            for (index, &bounds) in self.objects.iter().enumerate() {
                if bounds.distance_squared(point) <= limit {
                    limit = visit(index, limit);
                }
            }
        }
    }

    fn visit(
        &self,
        tree: &BoundsTree<usize>,
        index: usize,
        frustum: &Frustum,
        planes: u8,
        mark: &mut impl FnMut(usize, u8),
    ) {
        let node = &tree.nodes[index];
        let Some(planes) = frustum.intersecting_planes(node.bounds, planes) else {
            return;
        };
        if planes == 0 {
            for &index in &tree.items[node.range.clone()] {
                mark(index, 0);
            }
        } else if let Some([a, b]) = node.children {
            self.visit(tree, a, frustum, planes, mark);
            self.visit(tree, b, frustum, planes, mark);
        } else {
            for &index in &tree.items[node.range.clone()] {
                if let Some(remaining) = frustum.intersecting_planes(self.objects[index], planes) {
                    mark(index, remaining);
                }
            }
        }
    }
}

/// Camera visibility in GPU instance order. Unchecked offscreen bounds remain conservative.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct SceneVisibility {
    visible: usize,
    mask: Mask,
}

#[derive(Debug, PartialEq, Eq)]
enum Mask {
    All,
    None,
    Mixed(Box<[u64]>),
}

impl SceneVisibility {
    pub(crate) fn from_layers(objects: &[SceneObject], layers: u64) -> Self {
        if layers == 0 {
            return Self::none();
        }
        Self::matching(objects, |object| object.layers & layers != 0)
    }

    pub(crate) fn matching(
        objects: &[SceneObject],
        included: impl Fn(&SceneObject) -> bool,
    ) -> Self {
        let mut words = vec![0u64; objects.len().div_ceil(64)];
        let mut count = 0;
        for (index, object) in objects.iter().enumerate() {
            if included(object) {
                words[index / 64] |= 1 << (index % 64);
                count += 1;
            }
        }
        match count {
            0 => Self::none(),
            count if count == objects.len() => Self::all(count),
            visible => Self {
                visible,
                mask: Mask::Mixed(words.into()),
            },
        }
    }

    /// The indices are distinct and in the same instance order as the geometry.
    pub(crate) fn excluding(count: usize, indices: impl Iterator<Item = usize>) -> Self {
        let mut indices = indices.peekable();
        if indices.peek().is_none() {
            return Self::all(count);
        }
        let mut words = vec![u64::MAX; count.div_ceil(64)];
        if let Some(last) = words.last_mut() {
            *last >>= (64 - count % 64) % 64;
        }
        let mut visible = count;
        for index in indices {
            words[index / 64] &= !(1 << (index % 64));
            visible -= 1;
        }
        if visible == 0 {
            Self::none()
        } else {
            Self {
                visible,
                mask: Mask::Mixed(words.into()),
            }
        }
    }

    #[doc(hidden)]
    pub fn intersect(a: &std::sync::Arc<Self>, b: &std::sync::Arc<Self>) -> std::sync::Arc<Self> {
        match (&a.mask, &b.mask) {
            (Mask::None, _) | (_, Mask::All) => a.clone(),
            (_, Mask::None) | (Mask::All, _) => b.clone(),
            (Mask::Mixed(a_words), Mask::Mixed(b_words)) => {
                let words: Box<[_]> = a_words.iter().zip(b_words).map(|(&a, &b)| a & b).collect();
                let visible = words.iter().map(|word| word.count_ones() as usize).sum();
                if visible == a.visible {
                    a.clone()
                } else if visible == b.visible {
                    b.clone()
                } else if visible == 0 {
                    std::sync::Arc::new(Self::none())
                } else {
                    std::sync::Arc::new(Self {
                        visible,
                        mask: Mask::Mixed(words),
                    })
                }
            }
        }
    }
    pub fn all(count: usize) -> Self {
        Self {
            visible: count,
            mask: Mask::All,
        }
    }

    pub fn none() -> Self {
        Self {
            visible: 0,
            mask: Mask::None,
        }
    }

    pub fn visible_count(&self) -> usize {
        self.visible
    }

    pub fn contains(&self, index: u32) -> bool {
        match &self.mask {
            Mask::All => true,
            Mask::None => false,
            Mask::Mixed(words) => {
                let index = index as usize;
                words
                    .get(index / 64)
                    .is_some_and(|word| word & (1 << (index % 64)) != 0)
            }
        }
    }

    /// Keep the original order and storage of GPU instances, splitting on holes.
    pub fn runs(&self, range: Range<u32>) -> impl Iterator<Item = Range<u32>> + '_ {
        let mut cursor = range.start;
        std::iter::from_fn(move || {
            if cursor >= range.end {
                return None;
            }
            match &self.mask {
                Mask::All => Some(std::mem::replace(&mut cursor, range.end)..range.end),
                Mask::None => None,
                Mask::Mixed(words) => {
                    let start = next_bit(words, cursor, range.end, true);
                    if start == range.end {
                        return None;
                    }
                    cursor = next_bit(words, start, range.end, false);
                    Some(start..cursor)
                }
            }
        })
    }
}

fn next_bit(words: &[u64], start: u32, end: u32, value: bool) -> u32 {
    let start_word = start / 64;
    for word_index in start_word..end.div_ceil(64) {
        let bits = words[word_index as usize];
        let mut matches = if value { bits } else { !bits };
        if word_index == start_word {
            matches &= u64::MAX << (start % 64);
        }
        if matches != 0 {
            return (word_index * 64 + matches.trailing_zeros()).min(end);
        }
    }
    end
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::{
        Length,
        scene_camera::{CameraNode, SceneCameraSettings},
    };
    use nalgebra::UnitQuaternion;

    #[test]
    fn selection_clips_triangles_and_respects_letterbox_and_depth() {
        let settings =
            SceneCameraSettings::orthographic(2.0, 2.0, 0.5, 10.0).expect("valid test camera");
        let camera = SceneCamera::from_node(
            &CameraNode {
                eye: Vector3::new(0.0, 0.0, 8.0),
                rotation: UnitQuaternion::identity(),
                settings,
                lod_bias: 1.0,
                layers: 1,
            },
            [300.0, 200.0],
        )
        .expect("valid frame");
        let point = |x, y| Point2D::new(Length(x), Length(y));
        assert_eq!(camera.content_rect(), [50.0, 0.0, 200.0, 200.0]);
        assert!(Frustum::selection(&camera, [point(5.0, 10.0), point(49.0, 190.0)]).is_none());
        let region = Frustum::selection(&camera, [point(140.0, 90.0), point(160.0, 110.0)])
            .expect("inside active camera");
        let big = [[-2.0, -1.0, 0.0], [2.0, -1.0, 0.0], [0.0, 2.0, 0.0]];
        assert!(region.triangle_intersects(big, ALL_PLANES));
        assert!(!region.triangle_intersects(big.map(|[x, y, _]| [x, y, 7.6]), ALL_PLANES));
        assert!(!region.triangle_intersects(big.map(|[x, y, _]| [x, y, -2.2]), ALL_PLANES));

        let diagonal = [[-1.8, -0.6, 0.0], [-1.8, 1.8, 0.0], [1.2, 1.8, 0.0]];
        let bounds = diagonal
            .map(Bounds3D::point)
            .into_iter()
            .reduce(Bounds3D::union)
            .expect("three vertices");
        assert!(region.intersects(bounds));
        assert!(!region.triangle_intersects(diagonal, ALL_PLANES));
        let crossing_near = [[-2.0, -1.0, 7.8], [2.0, -1.0, 7.8], [0.0, 2.0, 6.5]];
        assert!(region.triangle_intersects(crossing_near, ALL_PLANES));
    }
}
