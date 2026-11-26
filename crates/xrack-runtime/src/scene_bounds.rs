//! Shared bounds and spatial partitioning for scene and mesh queries.
use nalgebra::Vector3;
use std::ops::Range;

/// A finite axis-aligned box in scene units. The default represents an empty box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneBounds3D(Option<[crate::Point3D; 2]>);

impl SceneBounds3D {
    pub fn new(min: crate::Point3D, max: crate::Point3D) -> Self {
        let ordered = [(min.x, max.x), (min.y, max.y), (min.z, max.z)]
            .into_iter()
            .all(|(a, b)| a.is_finite() && b.is_finite() && a <= b && (b - a).is_finite());
        Self(ordered.then_some([min, max]))
    }

    pub fn valid(self) -> bool {
        self.0.is_some()
    }

    pub fn min(self) -> crate::Point3D {
        self.0.map(|[min, _]| min).unwrap_or_default()
    }

    pub fn max(self) -> crate::Point3D {
        self.0.map(|[_, max]| max).unwrap_or_default()
    }

    pub fn center(self) -> crate::Point3D {
        let [min, max] = self.0.unwrap_or_default();
        crate::Point3D::new(
            min.x.midpoint(max.x),
            min.y.midpoint(max.y),
            min.z.midpoint(max.z),
        )
    }

    pub fn size(self) -> crate::Vector3D {
        let [min, max] = self.0.unwrap_or_default();
        crate::Vector3D::new(max.x - min.x, max.y - min.y, max.z - min.z)
    }

    pub(crate) fn from_spatial(bounds: Option<Bounds3D>) -> Self {
        bounds.map_or_else(Self::default, |bounds| {
            let point = |[x, y, z]: [f32; 3]| crate::Point3D::new(x.into(), y.into(), z.into());
            Self::new(point(bounds.min), point(bounds.max))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds3D {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds3D {
    pub fn point(point: [f32; 3]) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|axis| self.min[axis].min(other.min[axis])),
            max: std::array::from_fn(|axis| self.max[axis].max(other.max[axis])),
        }
    }

    pub fn is_finite(self) -> bool {
        self.min.into_iter().chain(self.max).all(f32::is_finite)
    }

    /// Conservatively transform an AABB by the same f32 matrix as the renderer.
    pub fn transform(self, rows: [[f32; 4]; 3]) -> Self {
        let mut min = [0.0; 3];
        let mut max = [0.0; 3];
        for (axis, row) in rows.into_iter().enumerate() {
            let mut low = f64::from(row[3]);
            let mut high = low;
            let mut magnitude = low.abs();
            for (source, coefficient) in row[..3].iter().copied().enumerate() {
                let a = f64::from(coefficient) * f64::from(self.min[source]);
                let b = f64::from(coefficient) * f64::from(self.max[source]);
                low += a.min(b);
                high += a.max(b);
                magnitude += a.abs().max(b.abs());
            }
            let pad = magnitude * f64::from(16.0 * f32::EPSILON);
            min[axis] = ((low - pad) as f32).next_down();
            max[axis] = ((high + pad) as f32).next_up();
        }
        Self { min, max }
    }

    pub fn distance_squared(self, point: Vector3<f64>) -> f64 {
        (0..3)
            .map(|axis| {
                let low = f64::from(self.min[axis]);
                let high = f64::from(self.max[axis]);
                let separation = (low - point[axis]).max(point[axis] - high).max(0.0);
                separation * separation
            })
            .sum()
    }

    fn center(self, axis: usize) -> f32 {
        f32::midpoint(self.min[axis], self.max[axis])
    }
}

#[derive(Debug)]
pub(crate) struct BoundsNode {
    pub bounds: Bounds3D,
    pub children: Option<[usize; 2]>,
    pub range: Range<usize>,
}

#[derive(Debug)]
pub(crate) struct BoundsTree<T> {
    pub nodes: Box<[BoundsNode]>,
    pub items: Box<[T]>,
}

impl<T> BoundsTree<T> {
    pub fn new(mut candidates: Vec<(Bounds3D, T)>, leaf_size: usize) -> Self {
        let mut nodes = Vec::new();
        if !candidates.is_empty() {
            Self::build(&mut candidates, 0, leaf_size.max(1), &mut nodes);
        }
        Self {
            nodes: nodes.into_boxed_slice(),
            items: candidates.into_iter().map(|(_, value)| value).collect(),
        }
    }

    /// Visit nearer nodes first, allowing the caller to reduce its search radius.
    pub fn visit_nearest(
        &self,
        mut limit: f64,
        distance: impl Fn(Bounds3D) -> f64,
        mut visit: impl FnMut(&T, f64) -> f64,
    ) {
        if self
            .nodes
            .first()
            .is_some_and(|root| distance(root.bounds) <= limit)
        {
            self.visit_node(0, &mut limit, &distance, &mut visit);
        }
    }

    fn visit_node(
        &self,
        index: usize,
        limit: &mut f64,
        distance: &impl Fn(Bounds3D) -> f64,
        visit: &mut impl FnMut(&T, f64) -> f64,
    ) {
        let node = &self.nodes[index];
        if let Some([left, right]) = node.children {
            let left_distance = distance(self.nodes[left].bounds);
            let right_distance = distance(self.nodes[right].bounds);
            let mut children = [(left, left_distance), (right, right_distance)];
            if right_distance < left_distance {
                children.swap(0, 1);
            }
            for (child, distance_to_bounds) in children {
                if distance_to_bounds <= *limit {
                    self.visit_node(child, limit, distance, visit);
                }
            }
        } else {
            for item in &self.items[node.range.clone()] {
                *limit = visit(item, *limit);
            }
        }
    }

    fn build(
        candidates: &mut [(Bounds3D, T)],
        start: usize,
        leaf_size: usize,
        nodes: &mut Vec<BoundsNode>,
    ) -> usize {
        let bounds = candidates[1..]
            .iter()
            .fold(candidates[0].0, |bounds, item| bounds.union(item.0));
        let index = nodes.len();
        nodes.push(BoundsNode {
            bounds,
            children: None,
            range: start..start + candidates.len(),
        });
        if candidates.len() > leaf_size {
            let extent = |axis: usize| f64::from(bounds.max[axis]) - f64::from(bounds.min[axis]);
            let axis = (0..3)
                .max_by(|&a, &b| extent(a).total_cmp(&extent(b)))
                .unwrap_or(0);
            let mid = candidates.len() / 2;
            candidates
                .select_nth_unstable_by(mid, |a, b| a.0.center(axis).total_cmp(&b.0.center(axis)));
            let (left, right) = candidates.split_at_mut(mid);
            let left = Self::build(left, start, leaf_size, nodes);
            let right = Self::build(right, start + mid, leaf_size, nodes);
            nodes[index].children = Some([left, right]);
        }
        index
    }
}
