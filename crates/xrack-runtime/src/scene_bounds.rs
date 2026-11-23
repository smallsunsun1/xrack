//! Shared bounds and spatial partitioning for scene and mesh queries.
use std::ops::Range;

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
