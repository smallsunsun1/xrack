use xrack_runtime::{Node, Value, scene3d::SceneCamera};

pub(super) const MAX_CASCADES: usize = 4;

#[derive(Clone, Copy)]
pub(super) struct CascadeRequest {
    pub camera: SceneCamera,
    pub count: usize,
    distance: Option<f32>,
    blend: f32,
    balance: f32,
}

impl PartialEq for CascadeRequest {
    fn eq(&self, other: &Self) -> bool {
        self.camera.same_frustum(&other.camera)
            && self.count == other.count
            && self.distance == other.distance
            && self.blend == other.blend
            && self.balance == other.balance
    }
}

impl CascadeRequest {
    pub fn new(node: &Node, camera: SceneCamera) -> Option<Self> {
        let Some(Value::Int(count)) = node.properties.get("shadow-cascades") else {
            return None;
        };
        let count = (*count).clamp(1, MAX_CASCADES as i64) as usize;
        (count > 1).then(|| Self {
            camera,
            count,
            distance: node
                .float("shadow-distance")
                .filter(|distance| *distance > 0.0),
            blend: node
                .float("shadow-cascade-blend")
                .unwrap_or(0.1)
                .clamp(0.0, 1.0),
            balance: node
                .float("shadow-split-balance")
                .unwrap_or(0.5)
                .clamp(0.0, 1.0),
        })
    }

    pub fn splits(self, scene_corners: &[[f32; 3]]) -> Option<CascadeSplits> {
        let [near, camera_far] = self.camera.depth_limits();
        let far = self
            .distance
            .unwrap_or_else(|| {
                scene_corners
                    .iter()
                    .map(|point| self.camera.view_depth(*point))
                    .fold(0.0, f32::max)
                    + 0.005
            })
            .min(camera_far);
        if !far.is_finite() || far <= near + 0.001 {
            return None;
        }
        let ends = std::array::from_fn(|i| {
            if i + 1 >= self.count {
                return far;
            }
            let t = (i + 1) as f32 / self.count as f32;
            let linear = near + (far - near) * t;
            let logarithmic = if near > 0.0 {
                near * (far / near).powf(t)
            } else {
                linear
            };
            linear + (logarithmic - linear) * self.balance
        });
        Some(CascadeSplits {
            near,
            far,
            ends,
            count: self.count,
            blend: self.blend,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct CascadeSplits {
    pub near: f32,
    pub far: f32,
    pub ends: [f32; MAX_CASCADES],
    pub count: usize,
    pub blend: f32,
}

impl CascadeSplits {
    fn edge(self, index: usize) -> f32 {
        if index == 0 {
            self.near
        } else {
            self.ends[index - 1]
        }
    }

    // Keep adjacent transitions disjoint and fit both maps over each transition.
    // Match the selection and weight calculation in scene.wgsl.
    fn overlap(self, edge: usize) -> f32 {
        if edge == 0 || edge == self.count {
            return 0.0;
        }
        let center = self.edge(edge);
        0.5 * self.blend * (center - self.edge(edge - 1)).min(self.edge(edge + 1) - center)
    }

    pub fn range(self, index: usize) -> [f32; 2] {
        [
            self.edge(index) - self.overlap(index),
            self.edge(index + 1) + self.overlap(index + 1),
        ]
    }
}
