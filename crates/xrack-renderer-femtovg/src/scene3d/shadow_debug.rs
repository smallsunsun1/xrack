//! World-space guides for shadow fitting and an optional frozen cascade camera.
use xrack_runtime::{Node, PhysicsDebugVertex, RowKey, scene3d::SceneCamera};

use super::effect_history::Owner;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Settings {
    pub cascades: bool,
    pub light_volumes: bool,
    pub casters: bool,
    pub receivers: bool,
    pub intersections: bool,
}

impl Settings {
    pub fn from_node(node: &Node) -> Self {
        Self {
            cascades: node.boolean("shadow-debug-cascades", false),
            light_volumes: node.boolean("shadow-debug-light-volumes", false),
            casters: node.boolean("shadow-debug-casters", false),
            receivers: node.boolean("shadow-debug-receivers", false),
            intersections: node.boolean("shadow-debug-intersections", false),
        }
    }

    pub fn active(self) -> bool {
        self.cascades || self.light_volumes || self.casters || self.receivers || self.intersections
    }

    pub fn needs_objects(self) -> bool {
        self.casters || self.receivers || self.intersections
    }
}

#[derive(Default)]
pub(super) struct FrozenCameras {
    views: Vec<FrozenView>,
    frame: u64,
}

struct FrozenView {
    path: Vec<usize>,
    generation: u64,
    scope: [Option<RowKey>; 2],
    id: Option<String>,
    camera: SceneCamera,
    frame: u64,
    committed: bool,
}

impl FrozenView {
    fn matches(&self, owner: Owner<'_>) -> bool {
        self.path == owner.path
            && self.generation == owner.generation
            && self.scope == owner.scope
            && self.id.as_deref() == owner.id
    }
}

impl FrozenCameras {
    pub fn begin_frame(&mut self) {
        if !self.views.is_empty() {
            self.frame = self.frame.wrapping_add(1);
            self.views
                .retain(|view| self.frame.wrapping_sub(view.frame) <= 120);
        }
    }

    pub fn visit(&mut self, owner: Owner<'_>, enabled: bool) {
        if let Some(index) = self.views.iter().position(|view| view.matches(owner)) {
            if enabled {
                self.views[index].frame = self.frame;
            } else {
                self.views.swap_remove(index);
            }
        }
    }

    pub fn reference(
        &mut self,
        owner: Owner<'_>,
        enabled: bool,
        camera: SceneCamera,
    ) -> SceneCamera {
        self.visit(owner, enabled);
        if !enabled {
            return camera;
        }
        if let Some(view) = self.views.iter().find(|view| view.matches(owner)) {
            return view.camera;
        }
        self.views.push(FrozenView {
            path: owner.path.to_vec(),
            generation: owner.generation,
            scope: owner.scope,
            id: owner.id.map(str::to_owned),
            camera,
            frame: self.frame,
            committed: false,
        });
        camera
    }

    pub fn finish_frame(&mut self, submitted: bool) {
        if submitted {
            for view in &mut self.views {
                view.committed = true;
            }
        } else {
            self.views.retain(|view| view.committed);
        }
    }
}

pub(super) type Corners = [[f32; 3]; 8];
pub(super) const CASCADES: [[u8; 4]; 4] = [
    [91, 226, 247, 245],
    [101, 217, 132, 245],
    [255, 207, 100, 245],
    [213, 142, 245, 245],
];
pub(super) const LIGHT_VOLUME: [u8; 4] = [227, 138, 255, 200];
pub(super) const CASTER: [u8; 4] = [255, 119, 77, 200];
pub(super) const RECEIVER: [u8; 4] = [100, 155, 255, 190];
pub(super) const INTERSECTION: [u8; 4] = [139, 243, 144, 210];

const FACES: [[usize; 4]; 6] = [
    [0, 2, 6, 4],
    [1, 3, 7, 5],
    [0, 1, 5, 4],
    [2, 3, 7, 6],
    [0, 1, 3, 2],
    [4, 5, 7, 6],
];

fn segment(lines: &mut Vec<PhysicsDebugVertex>, a: [f32; 3], b: [f32; 3], color: [u8; 4]) {
    if a.iter().chain(&b).all(|value| value.is_finite()) && a != b {
        lines.extend([a, b].map(|position| PhysicsDebugVertex { position, color }));
    }
}

pub(super) fn outline(lines: &mut Vec<PhysicsDebugVertex>, corners: &Corners, color: [u8; 4]) {
    for index in 0..8 {
        for axis in [1, 2, 4] {
            if index & axis == 0 {
                segment(lines, corners[index], corners[index | axis], color);
            }
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn cross([x, y, z]: [f32; 3], [a, b, c]: [f32; 3]) -> [f32; 3] {
    [y * c - z * b, z * a - x * c, x * b - y * a]
}

pub(super) struct ClipVolume {
    corners: Corners,
    planes: [([f32; 3], f32); 6],
}

impl ClipVolume {
    pub fn new(corners: Corners) -> Option<Self> {
        let center = std::array::from_fn(|axis| corners.iter().map(|p| p[axis] * 0.125).sum());
        let mut planes = [([0.0; 3], 0.0); 6];
        for (plane, [i, j, k, _]) in planes.iter_mut().zip(FACES) {
            let a = corners[i];
            let n = cross(sub(corners[j], a), sub(corners[k], a));
            let length = dot(n, n).sqrt();
            if !length.is_finite() || length <= 1e-12 {
                return None;
            }
            let sign = if dot(n, sub(center, a)) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let n = n.map(|value| value * sign / length);
            *plane = (n, -dot(n, a));
        }
        Some(Self { corners, planes })
    }

    pub fn intersect(&self, bounds: &Corners, lines: &mut Vec<PhysicsDebugVertex>) {
        if self
            .planes
            .iter()
            .any(|&(n, offset)| bounds.iter().all(|&point| dot(n, point) + offset < 0.0))
        {
            return;
        }
        // Clip faces of both hulls. A planar receiver still contributes
        // its clipped polygon even when it has no volumetric half-spaces.
        Self::clip_faces(bounds, self, lines);
        if let Some(other) = Self::new(*bounds) {
            Self::clip_faces(&self.corners, &other, lines);
        }
    }

    fn clip_faces(corners: &Corners, clip: &Self, lines: &mut Vec<PhysicsDebugVertex>) {
        let mut polygon = Vec::with_capacity(10);
        let mut next = Vec::with_capacity(10);
        for face in FACES {
            polygon.clear();
            polygon.extend(face.map(|i| corners[i]));
            for &(n, offset) in &clip.planes {
                let Some(&last) = polygon.last() else { break };
                let mut previous = last;
                let mut distance = dot(n, previous) + offset;
                next.clear();
                for &point in &polygon {
                    let current = dot(n, point) + offset;
                    if (current >= 0.0) != (distance >= 0.0) {
                        let t = distance / (distance - current);
                        next.push(std::array::from_fn(|i| {
                            previous[i] + (point[i] - previous[i]) * t
                        }));
                    }
                    if current >= 0.0 {
                        next.push(point);
                    }
                    previous = point;
                    distance = current;
                }
                std::mem::swap(&mut polygon, &mut next);
            }
            if polygon.len() >= 3 {
                for (&a, &b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
                    segment(lines, a, b, INTERSECTION);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_includes_face_crossings() {
        let box_corners = |max: [f32; 3]| {
            std::array::from_fn(|i| {
                std::array::from_fn(|axis| {
                    max[axis] * if i & (1 << axis) == 0 { -1.0 } else { 1.0 }
                })
            })
        };
        let volume = ClipVolume::new(box_corners([2.0, 1.0, 1.0])).unwrap();
        let mut lines = Vec::new();
        volume.intersect(&box_corners([1.0, 2.0, 0.5]), &mut lines);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|v| v.position[0].abs() <= 1.0
            && v.position[1].abs() <= 1.0
            && v.position[2].abs() <= 0.5));
        assert!(lines.iter().any(|v| v.position == [1.0, 1.0, 0.5]));
    }
}
