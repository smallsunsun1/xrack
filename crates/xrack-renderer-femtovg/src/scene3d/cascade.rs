use xrack_runtime::{
    Node, Value,
    scene3d::{SceneCamera, ShadowProjectionSettings},
};

pub(super) const MAX_CASCADES: usize = 4;

#[derive(Clone, Copy)]
pub(super) struct CascadeRequest {
    pub camera: SceneCamera,
    pub count: usize,
    distance: Option<f32>,
    blend: f32,
    balance: f32,
    splits: [Option<f32>; MAX_CASCADES - 1],
    pub lock_texels: bool,
}

impl PartialEq for CascadeRequest {
    fn eq(&self, other: &Self) -> bool {
        self.camera.same_frustum(&other.camera)
            && self.count == other.count
            && self.distance == other.distance
            && self.blend == other.blend
            && self.balance == other.balance
            && self.splits == other.splits
            && self.lock_texels == other.lock_texels
    }
}

impl CascadeRequest {
    pub fn new(node: &Node, camera: SceneCamera, light: ShadowProjectionSettings) -> Option<Self> {
        let count = light.cascades.map(usize::from).unwrap_or_else(|| {
            match node.properties.get("shadow-cascades") {
                Some(Value::Int(count)) => (*count).clamp(1, MAX_CASCADES as i64) as usize,
                _ => 1,
            }
        });
        let number = |name, fallback| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(fallback)
        };
        let distance = light.far.or_else(|| {
            let distance = number("shadow-distance", 0.0);
            (count > 1 && distance > 0.0).then_some(distance)
        });
        (count > 1 || distance.is_some()).then(|| Self {
            camera,
            count,
            distance,
            blend: if count > 1 {
                light
                    .blend
                    .unwrap_or_else(|| number("shadow-cascade-blend", 0.1).clamp(0.0, 1.0))
            } else {
                0.0
            },
            balance: if count > 1 {
                light
                    .balance
                    .unwrap_or_else(|| number("shadow-split-balance", 0.5).clamp(0.0, 1.0))
            } else {
                0.0
            },
            splits: std::array::from_fn(|i| if i + 1 < count { light.splits[i] } else { None }),
            lock_texels: light.lock_texels.unwrap_or(true),
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
        let ends = authored_ends(near, far, self.count, ends, self.splits);
        Some(CascadeSplits {
            near,
            far,
            ends,
            count: self.count,
            blend: self.blend,
        })
    }
}

/// Interpolate the automatic distribution between authored split anchors.
fn authored_ends(
    near: f32,
    far: f32,
    count: usize,
    automatic: [f32; MAX_CASCADES],
    splits: [Option<f32>; MAX_CASCADES - 1],
) -> [f32; MAX_CASCADES] {
    if splits.iter().all(Option::is_none) {
        return automatic;
    }
    let mut ends = automatic;
    let mut start = near;
    let mut base_start = near;
    let mut lower = 0;
    let authored = splits.into_iter().enumerate().filter_map(|(i, fraction)| {
        (i + 1 < count)
            .then_some(fraction?)
            .map(|t| (i, near + (far - near) * t))
    });
    for (upper, end) in authored.chain([(count - 1, far)]) {
        let base_end = automatic[upper];
        if end <= start || base_end <= base_start {
            return automatic;
        }
        for i in lower..upper {
            let fraction = (automatic[i] - base_start) / (base_end - base_start);
            ends[i] = start + (end - start) * fraction;
        }
        ends[upper] = end;
        lower = upper + 1;
        start = end;
        base_start = base_end;
    }
    let strictly_ordered = ends[0] > near && ends[..count].windows(2).all(|pair| pair[0] < pair[1]);
    if strictly_ordered { ends } else { automatic }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_splits_anchor_the_automatic_distribution() {
        let automatic = [2.0, 4.0, 8.0, 10.0];
        assert_eq!(
            authored_ends(0.0, 10.0, 4, automatic, [Some(0.3), None, Some(0.75)]),
            [3.0, 4.5, 7.5, 10.0],
        );
        assert_eq!(
            authored_ends(0.0, 10.0, 4, automatic, [Some(0.7), Some(0.3), None]),
            automatic,
        );
    }
}
