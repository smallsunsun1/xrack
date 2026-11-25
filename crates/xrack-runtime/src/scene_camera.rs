//! Camera projections shared by rendering, shadow cascades and viewport picking.
use std::{cell::RefCell, collections::VecDeque};

use crate::scene_visibility::Frustum;
use nalgebra::{Matrix4, UnitQuaternion, Vector3, Vector4};

use crate::{
    Length, Node, Point2D, RecordValue, Value, VecModel, Vector3D,
    scene_bounds::Bounds3D,
    scene_hierarchy::SceneModel,
    scene_rotation::look_at_basis,
    scene3d::{
        Hit3D, Ray3D, RayRows, SceneObject, SceneRay, SceneTextureHit, direction, trace_id,
        trace_surfaces,
    },
};

const NEAR: f32 = 0.1;
const FAR: f32 = 100.0;
const ORBIT_FOCAL: f32 = 2.414_213_7; // 45 degree vertical field of view

#[doc(hidden)]
pub const CAMERA_MATRIX_FIELDS: [[&str; 4]; 4] = [
    ["m11", "m12", "m13", "m14"],
    ["m21", "m22", "m23", "m24"],
    ["m31", "m32", "m33", "m34"],
    ["m41", "m42", "m43", "m44"],
];

/// A position in 3D scene coordinates, measured in world units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3D {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(crate) fn narrow(self) -> Option<[f32; 3]> {
        let point = [self.x as f32, self.y as f32, self.z as f32];
        point.into_iter().all(f32::is_finite).then_some(point)
    }
}

/// The projected logical-pixel position and its positive camera-space depth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProjectedPoint3D {
    pub point: Point2D,
    pub depth: f64,
    pub valid: bool,
    pub inside: bool,
}

/// Projection settings authored on a scene camera node.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneCameraSettings {
    projection: Projection,
    aspect: Option<f64>,
    near: f32,
    far: f32,
    depth_tilt: [f32; 2],
    reverse_depth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Projection {
    Perspective(f32),
    Orthographic(f32),
    FixedLens {
        lens: [f32; 4],
        orthographic: bool,
    },
    OrthographicWindow {
        horizontal: [Option<f64>; 2],
        vertical: [f32; 2],
    },
    Custom {
        rows: [[f32; 4]; 4],
        reverse_row: [f32; 4],
        viewer: [f32; 4],
        reverses_winding: bool,
    },
}

impl Projection {
    const fn is_orthographic(self) -> bool {
        matches!(
            self,
            Self::Orthographic(_)
                | Self::OrthographicWindow { .. }
                | Self::FixedLens {
                    orthographic: true,
                    ..
                }
        ) || matches!(self, Self::Custom { viewer, .. } if viewer[3] == 0.0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ClipSpace {
    WebGpu,
    OpenGl,
}

impl ClipSpace {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "" | "webgpu" => Some(Self::WebGpu),
            "opengl" => Some(Self::OpenGl),
            _ => None,
        }
    }
}

const MATRIX_CACHE_CAPACITY: usize = 16;

#[derive(Default)]
pub(crate) struct CameraMatrixCache(RefCell<VecDeque<CachedCameraMatrix>>);

struct CachedCameraMatrix {
    coefficients: [[u64; 4]; 4],
    clip_space: ClipSpace,
    settings: Option<SceneCameraSettings>,
}

impl CameraMatrixCache {
    pub(crate) const fn new() -> Self {
        Self(RefCell::new(VecDeque::new()))
    }

    pub(crate) fn get(&self, rows: [[f64; 4]; 4], clip_space: &str) -> Option<SceneCameraSettings> {
        let clip_space = ClipSpace::parse(clip_space)?;
        let coefficients = rows.map(|row| row.map(f64::to_bits));
        let mut matrices = self.0.borrow_mut();
        if let Some(index) = matrices.iter().position(|matrix| {
            matrix.coefficients == coefficients && matrix.clip_space == clip_space
        }) && let Some(entry) = matrices.remove(index)
        {
            let settings = entry.settings;
            matrices.push_front(entry);
            return settings;
        }
        let settings = SceneCameraSettings::prepare_custom(rows, clip_space);
        matrices.truncate(MATRIX_CACHE_CAPACITY - 1);
        matrices.push_front(CachedCameraMatrix {
            coefficients,
            clip_space,
            settings,
        });
        settings
    }
}

fn lens_axis(lower: f64, upper: f64, reference: f64, extent_depth: f32) -> Option<[f32; 2]> {
    let span = upper - lower;
    let scale = (2.0 * reference / span) as f32;
    let shift = (-(upper + lower) / span) as f32;
    (lower < upper
        && scale.is_finite()
        && scale > 0.0
        && shift.is_finite()
        && (((1.0 + shift.abs()) / scale) * extent_depth).is_finite())
    .then_some([scale, shift])
}

impl SceneCameraSettings {
    pub(crate) fn custom_from_record(
        matrix: &RecordValue,
        clip_space: &str,
        cache: &CameraMatrixCache,
    ) -> Option<Self> {
        let mut rows = [[0.0; 4]; 4];
        for (row, fields) in rows.iter_mut().zip(CAMERA_MATRIX_FIELDS) {
            for (dst, field) in row.iter_mut().zip(fields) {
                let Value::Float(value) = matrix.field(field)? else {
                    return None;
                };
                *dst = *value;
            }
        }
        cache.get(rows, clip_space)
    }

    /// An authored camera-space projection, specified in row-major order.
    #[doc(hidden)]
    pub fn custom(rows: [[f64; 4]; 4], clip_space: &str) -> Option<Self> {
        Self::prepare_custom(rows, ClipSpace::parse(clip_space)?)
    }

    fn prepare_custom(mut rows: [[f64; 4]; 4], clip_space: ClipSpace) -> Option<Self> {
        if !rows.as_flattened().iter().all(|value| value.is_finite()) {
            return None;
        }
        // Homogeneous coordinates are invariant under positive global scale.
        // Bound the coefficients before inversion and narrowing to GPU f32.
        let magnitude = rows
            .as_flattened()
            .iter()
            .map(|value| value.abs())
            .fold(0.0, f64::max);
        if magnitude == 0.0 {
            return None;
        }
        // XR camera records look down -Z. The internal view coordinates use
        // positive forward depth.
        for row in &mut rows {
            for value in row.iter_mut() {
                *value /= magnitude;
            }
            row[2] = -row[2];
        }
        if clip_space == ClipSpace::OpenGl {
            rows[2] = std::array::from_fn(|i| rows[2][i].midpoint(rows[3][i]));
        }
        let inverse = Matrix4::from_row_slice(rows.as_flattened()).try_inverse()?;
        let reverse_row = if custom_depth_order(&inverse)? == DepthOrder::Reverse {
            let authored = rows[2];
            rows[2] = std::array::from_fn(|i| rows[3][i] - rows[2][i]);
            authored
        } else {
            std::array::from_fn(|i| rows[3][i] - rows[2][i])
        }
        .map(|value| value as f32);
        let rows = rows.map(|row| row.map(|value| value as f32));
        if !rows.as_flattened().iter().all(|value| value.is_finite()) {
            return None;
        }
        // Validate the coefficients which will actually reach the GPU.
        let uploaded = Matrix4::from_row_slice(rows.as_flattened()).cast::<f64>();
        let inverse = uploaded.try_inverse()?;
        if !inverse.iter().all(|value| (*value as f32).is_finite()) {
            return None;
        }
        if custom_depth_order(&inverse)? != DepthOrder::Forward {
            return None;
        }
        let [near, far] = custom_depth_range(&inverse)?;
        // At the viewport center the lateral Jacobian has this sign.
        // A validated forward projection has no lateral singularity within
        // the viewport, so the sign applies to the entire image.
        let reverses_winding =
            uploaded[(0, 0)] * uploaded[(1, 1)] < uploaded[(0, 1)] * uploaded[(1, 0)];
        let center = inverse.column(2);
        let viewer = if center.w == 0.0 {
            let direction = (center.xyz() * center.z.signum()).try_normalize(0.0)?;
            [
                direction.x as f32,
                direction.y as f32,
                direction.z as f32,
                0.0,
            ]
        } else {
            let eye = center.xyz() / center.w;
            [
                eye.x as f32,
                eye.y as f32,
                eye.z as f32,
                -center.w.signum() as f32,
            ]
        };
        viewer
            .iter()
            .all(|value| value.is_finite())
            .then_some(Self {
                projection: Projection::Custom {
                    rows,
                    reverse_row,
                    viewer,
                    reverses_winding,
                },
                aspect: None,
                near,
                far,
                depth_tilt: [0.0; 2],
                reverse_depth: false,
            })
    }

    pub fn from_record(
        kind: &str,
        fov: f64,
        height: f64,
        near: f64,
        far: f64,
        bounds: [Option<f64>; 4],
        width: Option<f64>,
    ) -> Option<Self> {
        if !(0.001..=100_000.0).contains(&near) || !(near + 0.001..=100_000.0).contains(&far) {
            return None;
        }
        let projection = match kind {
            "perspective" if (1.0..=175.0).contains(&fov) => {
                Projection::Perspective((fov.to_radians() * 0.5).tan().recip() as f32)
            }
            "orthographic" => {
                let [left, right, bottom, top] = bounds;
                if (bottom.is_none() || top.is_none()) && !(0.001..=100_000.0).contains(&height) {
                    return None;
                }
                if (left.is_none() || right.is_none())
                    && width.is_some_and(|width| !(0.001..=100_000.0).contains(&width))
                {
                    return None;
                }
                if bounds.iter().all(Option::is_none) && width.is_none() {
                    Projection::Orthographic((2.0 / height) as f32)
                } else {
                    let half_height = height * 0.5;
                    let vertical = lens_axis(
                        bottom.unwrap_or(-half_height),
                        top.unwrap_or(half_height),
                        1.0,
                        1.0,
                    )?;
                    let horizontal = [
                        left.or_else(|| width.map(|width| -width * 0.5)),
                        right.or_else(|| width.map(|width| width * 0.5)),
                    ];
                    if let [Some(left), Some(right)] = horizontal {
                        let [sx, dx] = lens_axis(left, right, 1.0, 1.0)?;
                        let [sy, dy] = vertical;
                        Projection::FixedLens {
                            lens: [sx, sy, dx, dy],
                            orthographic: true,
                        }
                    } else {
                        Projection::OrthographicWindow {
                            horizontal,
                            vertical,
                        }
                    }
                }
            }
            "frustum" => {
                let half = near / f64::from(ORBIT_FOCAL);
                let [left, right, bottom, top] = std::array::from_fn(|i| {
                    bounds[i].unwrap_or(if i % 2 == 0 { -half } else { half })
                });
                let [sx, dx] = lens_axis(left, right, near, far as f32)?;
                let [sy, dy] = lens_axis(bottom, top, near, far as f32)?;
                Projection::FixedLens {
                    lens: [sx, sy, dx, dy],
                    orthographic: false,
                }
            }
            _ => return None,
        };
        let (near, far) = (near as f32, far as f32);
        (near < far).then_some(Self {
            projection,
            aspect: None,
            near,
            far,
            depth_tilt: [0.0; 2],
            reverse_depth: false,
        })
    }

    #[doc(hidden)]
    pub const fn with_reverse_depth(mut self, reverse_depth: bool) -> Self {
        self.reverse_depth = reverse_depth;
        self
    }

    /// Tilt the depth planes around their intersections with the optical axis.
    /// The normal is expressed in camera-local coordinates, with forward along -Z.
    #[doc(hidden)]
    pub fn with_clip_normal(mut self, normal: Option<Vector3D>) -> Option<Self> {
        if let Some(normal) = normal {
            if !normal.z.is_finite() || normal.z >= 0.0 {
                return None;
            }
            self.depth_tilt = [(normal.x / -normal.z) as f32, (normal.y / -normal.z) as f32];
            if self.depth_tilt.iter().any(|value| !value.is_finite()) {
                return None;
            }
        }
        Some(self)
    }

    /// A glTF perspective camera can specify its sensor aspect or omit the
    /// far clipping plane. In that case the depth projection is truly infinite.
    pub fn perspective(
        yfov: f32,
        aspect: Option<f32>,
        near: f32,
        far: Option<f32>,
    ) -> Option<Self> {
        if !(0.0..std::f32::consts::PI).contains(&yfov)
            || yfov == 0.0
            || aspect.is_some_and(|ratio| !ratio.is_finite() || ratio <= 0.0)
            || !near.is_finite()
            || near <= 0.0
            || far.is_some_and(|far| !far.is_finite() || far <= near)
        {
            return None;
        }
        let scale = (yfov * 0.5).tan().recip();
        (scale.is_finite() && scale > 0.0).then_some(Self {
            projection: Projection::Perspective(scale),
            aspect: aspect.map(f64::from),
            near,
            far: far.unwrap_or(f32::INFINITY),
            depth_tilt: [0.0; 2],
            reverse_depth: false,
        })
    }

    pub fn orthographic(xmag: f32, ymag: f32, near: f32, far: f32) -> Option<Self> {
        let scale = ymag.recip();
        if xmag <= 0.0
            || !xmag.is_finite()
            || ymag <= 0.0
            || !ymag.is_finite()
            || !scale.is_finite()
            || !near.is_finite()
            || near < 0.0
            || !far.is_finite()
            || far <= near
        {
            return None;
        }
        Some(Self {
            projection: Projection::Orthographic(scale),
            aspect: Some(f64::from(xmag) / f64::from(ymag)),
            near,
            far,
            depth_tilt: [0.0; 2],
            reverse_depth: false,
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DepthOrder {
    Forward,
    Reverse,
}

fn custom_depth_order(inverse: &Matrix4<f64>) -> Option<DepthOrder> {
    let mut order = None;
    for [x, y] in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
        let point = inverse * Vector4::new(x, y, 0.5, 1.0);
        let derivative = inverse[(2, 2)] * point.w - inverse[(3, 2)] * point.z;
        if !point.iter().all(|value| value.is_finite()) || point.w <= 0.0 || !derivative.is_finite()
        {
            return None;
        }
        let direction = if derivative > 0.0 {
            DepthOrder::Forward
        } else if derivative < 0.0 {
            DepthOrder::Reverse
        } else {
            return None;
        };
        if order.is_some_and(|previous| previous != direction) {
            return None;
        }
        order = Some(direction);
    }
    order
}

fn custom_depth_range(inverse: &Matrix4<f64>) -> Option<[f32; 2]> {
    let mut near = f64::INFINITY;
    let mut far: f64 = 0.0;
    for [x, y] in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
        let first = inverse * Vector4::new(x, y, 0.0, 1.0);
        let last = inverse * Vector4::new(x, y, 1.0, 1.0);
        if first.w <= 0.0 || last.w < 0.0 || first.z <= 0.0 || last.z <= 0.0 {
            return None;
        }
        let start = first.xyz() / first.w;
        if !start.iter().all(|value| (*value as f32).is_finite()) {
            return None;
        }
        near = near.min(start.z);
        if last.w == 0.0 {
            far = f64::INFINITY;
        } else {
            let end = last.xyz() / last.w;
            if !end.iter().all(|value| (*value as f32).is_finite()) {
                return None;
            }
            far = far.max(end.z);
        }
    }
    let [near, far] = [near as f32, far as f32];
    (near > 0.0 && near < far).then_some([near, far])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraNode {
    pub eye: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub settings: SceneCameraSettings,
    pub lod_bias: f64,
    pub layers: u64,
}

impl CameraNode {
    pub(crate) fn aim_at(&mut self, target: Point3D, up: Option<Vector3D>) {
        if target.narrow() == Some(self.eye.into()) {
            return;
        }
        let authored_right = self.rotation * Vector3::x();
        let up = up.unwrap_or_else(|| {
            let authored_up = self.rotation * Vector3::y();
            let [x, y, z] = <[f32; 3]>::from(authored_up).map(f64::from);
            Vector3D::new(x, y, z)
        });
        let [x, y, z] = <[f32; 3]>::from(self.eye).map(f64::from);
        let eye = Point3D::new(x, y, z);
        if let Some(basis) = look_at_basis(eye, target, up, Some(authored_right.cast())) {
            self.rotation = UnitQuaternion::from_rotation_matrix(&basis.cast());
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct SceneCamera {
    eye: Vector3<f32>,
    right: Vector3<f32>,
    up: Vector3<f32>,
    forward: Vector3<f32>,
    lens: [f32; 4],
    content: [f64; 4],
    pub(crate) lod_bias: f64,
    layers: u64,
    settings: SceneCameraSettings,
    depth_range: [f32; 2],
    clip_offset: [f32; 2],
    clip_viewport: Option<[f32; 4]>,
}

impl SceneCamera {
    #[doc(hidden)]
    pub fn for_view(node: &Node, viewport: [f64; 2]) -> Option<Self> {
        let camera_id = node.text("camera").unwrap_or_default();
        let camera = if camera_id.is_empty() {
            Self::new(
                node.number("camera-yaw").unwrap_or(25.0),
                node.number("camera-pitch").unwrap_or(24.0),
                node.number("camera-distance").unwrap_or(10.0),
                viewport,
            )
        } else {
            match node.properties.get("model") {
                Some(Value::SceneObjects(model)) => model.camera(camera_id, viewport),
                _ => None,
            }
        };
        camera.map(|camera| camera.with_layer_mask(node.int("layer-mask")))
    }

    #[doc(hidden)]
    pub fn item_texture_at(
        &self,
        model: &SceneModel,
        point: Point2D,
    ) -> Option<Option<SceneTextureHit>> {
        self.scene_ray(point)?.item_texture(&model.geometry(self))
    }

    /// A cube face follows the GPU's cubemap orientation. It reverses raster winding.
    #[doc(hidden)]
    pub fn reflection_face(origin: [f32; 3], face: usize, layers: u64) -> Option<Self> {
        let axes = [
            [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            [[0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        ];
        let [forward, right, up] = axes.get(face)?.map(Vector3::from);
        Some(Self {
            eye: Vector3::from(origin),
            forward,
            right,
            up,
            lens: [1.0, 1.0, 0.0, 0.0],
            content: [0.0, 0.0, 1.0, 1.0],
            lod_bias: 0.0,
            layers,
            clip_offset: [0.0; 2],
            clip_viewport: None,
            depth_range: [0.05, FAR],
            settings: SceneCameraSettings {
                projection: Projection::Perspective(1.0),
                aspect: None,
                near: 0.05,
                far: FAR,
                depth_tilt: [0.0; 2],
                reverse_depth: false,
            },
        })
    }

    /// The orbit camera also used when a view has an empty `camera` binding.
    pub fn new(yaw: f64, pitch: f64, distance: f64, viewport: [f64; 2]) -> Option<Self> {
        if ![yaw, pitch, distance].into_iter().all(f64::is_finite) {
            return None;
        }
        let radial = Vector3::from(direction(yaw, pitch.clamp(-80.0, 80.0)));
        let forward = -radial;
        let right = forward.cross(&Vector3::y()).normalize();
        let up = right.cross(&forward);
        let settings = SceneCameraSettings {
            projection: Projection::Perspective(ORBIT_FOCAL),
            aspect: None,
            near: NEAR,
            far: FAR,
            depth_tilt: [0.0; 2],
            reverse_depth: false,
        };
        Some(Self {
            eye: radial * distance.clamp(2.0, 60.0) as f32,
            forward,
            right,
            up,
            lens: camera_frame(settings, viewport)?.0,
            content: [0.0, 0.0, viewport[0], viewport[1]],
            lod_bias: 1.0,
            layers: crate::scene3d::MAIN_LAYER,
            settings,
            depth_range: [NEAR, FAR],
            clip_offset: [0.0; 2],
            clip_viewport: None,
        })
    }

    pub(crate) fn from_node(node: &CameraNode, viewport: [f64; 2]) -> Option<Self> {
        let (lens, content) = camera_frame(node.settings, viewport)?;
        let depth_range = depth_range(node.settings, lens)?;
        Some(Self {
            eye: node.eye,
            right: node.rotation * Vector3::x(),
            up: node.rotation * Vector3::y(),
            forward: node.rotation * -Vector3::z(),
            lens,
            content,
            lod_bias: node.lod_bias,
            layers: node.layers,
            settings: node.settings,
            depth_range,
            clip_offset: [0.0; 2],
            clip_viewport: None,
        })
    }

    /// Select the camera's matching objects using a viewport-specific mask.
    pub fn with_layer_mask(mut self, layers: Option<i64>) -> Self {
        if let Some(layers) = layers {
            self.layers = layers as u64;
        }
        self
    }

    pub const fn layer_mask(&self) -> u64 {
        self.layers
    }

    /// Compare the world-space frustum independently of the viewport's logical size.
    #[doc(hidden)]
    pub fn same_frustum(&self, other: &Self) -> bool {
        self.eye == other.eye
            && self.right == other.right
            && self.up == other.up
            && self.forward == other.forward
            && self.lens == other.lens
            && self.settings == other.settings
            && self.clip_offset == other.clip_offset
            && self.clip_viewport == other.clip_viewport
    }

    /// Offset a raster sample in NDC, leaving the camera pose and depth range intact.
    #[doc(hidden)]
    pub fn with_clip_offset(mut self, offset: [f32; 2]) -> Self {
        self.clip_offset = offset;
        self
    }

    /// Map the projection into a normalized framebuffer rectangle without changing depth.
    #[doc(hidden)]
    pub fn with_clip_viewport(mut self, bounds: [f32; 4]) -> Self {
        self.clip_viewport = Some(bounds);
        self
    }

    /// Rows of the homogeneous world-to-clip transform (wgpu depth 0..1).
    pub fn clip_rows(&self) -> [[f32; 4]; 4] {
        if let Some(rows) = self.custom_view_clip_rows() {
            return rows.map(|[x, y, depth, offset]| {
                self.row(self.right * x + self.up * y + self.forward * depth, offset)
            });
        }
        let [near, far] = [self.settings.near, self.settings.far];
        let (depth_scale, w) = if self.is_orthographic() {
            ((far - near).recip(), [0.0, 0.0, 0.0, 1.0])
        } else if far.is_infinite() {
            (1.0, self.depth_row())
        } else {
            (far / (far - near), self.depth_row())
        };
        let [sx, sy, dx, dy] = self.clip_lens();
        let lateral = |axis, scale, shift| {
            if self.is_orthographic() {
                self.row(axis * scale, shift)
            } else {
                self.row(axis * scale + self.forward * shift, 0.0)
            }
        };
        // Form the reversed coefficients directly. Subtracting the *evaluated*
        // forward depth from W would discard the distant depth distinctions.
        let reversed = self.reverse_depth();
        let (forward_scale, depth_offset) = if reversed {
            let (near, far) = (f64::from(near), f64::from(far));
            if self.is_orthographic() {
                (-depth_scale, (far / (far - near)) as f32)
            } else if far.is_infinite() {
                (0.0, near as f32)
            } else {
                (
                    (-near / (far - near)) as f32,
                    (near * far / (far - near)) as f32,
                )
            }
        } else {
            (depth_scale, -near * depth_scale)
        };
        let mut depth_axis = self.forward * forward_scale;
        if self.has_oblique_depth() {
            let [x, y] = self.settings.depth_tilt;
            let lateral_scale = if reversed { -depth_scale } else { depth_scale };
            depth_axis += self.right * (x * lateral_scale) + self.up * (y * lateral_scale);
        }
        [
            lateral(self.right, sx, dx),
            lateral(self.up, sy, dy),
            self.row(depth_axis, depth_offset),
            w,
        ]
    }

    /// Rows of the clip-to-world transform for reconstruction from depth.
    #[doc(hidden)]
    pub fn inverse_clip_rows(&self) -> Option<[[f32; 4]; 4]> {
        if self.has_custom_projection() {
            return inverse_rows(self.clip_rows());
        }
        let inverse = Matrix4::from_row_slice(self.clip_rows().as_flattened()).try_inverse()?;
        inverse
            .iter()
            .all(|value| value.is_finite())
            .then(|| std::array::from_fn(|row| std::array::from_fn(|col| inverse[(row, col)])))
    }

    #[doc(hidden)]
    pub const fn has_custom_projection(&self) -> bool {
        matches!(self.settings.projection, Projection::Custom { .. })
    }

    #[doc(hidden)]
    pub const fn reverse_depth(&self) -> bool {
        self.settings.reverse_depth
    }

    #[doc(hidden)]
    pub const fn depth_clear(&self) -> f32 {
        if self.reverse_depth() { 0.0 } else { 1.0 }
    }

    #[doc(hidden)]
    pub const fn reverses_winding(&self) -> bool {
        matches!(
            self.settings.projection,
            Projection::Custom {
                reverses_winding: true,
                ..
            }
        )
    }

    /// Camera-space rows, in right/up/positive-depth coordinates.
    #[doc(hidden)]
    pub fn custom_view_clip_rows(&self) -> Option<[[f32; 4]; 4]> {
        let Projection::Custom {
            mut rows,
            reverse_row,
            ..
        } = self.settings.projection
        else {
            return None;
        };
        if self.reverse_depth() {
            rows[2] = reverse_row;
        }
        let [width, height, x, y] =
            self.clip_viewport
                .map_or([1.0, 1.0, 0.0, 0.0], |[left, top, right, bottom]| {
                    [
                        right - left,
                        bottom - top,
                        left + right - 1.0,
                        1.0 - top - bottom,
                    ]
                });
        let [dx, dy] = self.clip_offset;
        rows[0] = std::array::from_fn(|i| width * rows[0][i] + (x + dx * width) * rows[3][i]);
        rows[1] = std::array::from_fn(|i| height * rows[1][i] + (y + dy * height) * rows[3][i]);
        Some(rows)
    }

    #[doc(hidden)]
    pub fn custom_inverse_view_clip_rows(&self) -> Option<[[f32; 4]; 4]> {
        inverse_rows(self.custom_view_clip_rows()?)
    }

    /// Projection center in camera space, or a forward direction when w is zero.
    #[doc(hidden)]
    pub const fn custom_viewer(&self) -> Option<[f32; 4]> {
        match self.settings.projection {
            Projection::Custom { viewer, .. } => Some(viewer),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn custom_world_viewer(&self) -> Option<[f32; 4]> {
        let [x, y, z, kind] = self.custom_viewer()?;
        let mut viewer = self.world_vector(Vector3::new(x, y, z));
        if kind != 0.0 {
            viewer += self.eye;
        }
        Some([viewer.x, viewer.y, viewer.z, kind])
    }

    #[doc(hidden)]
    pub fn lateral_axes(&self) -> [[f32; 3]; 2] {
        [self.right.into(), self.up.into()]
    }

    fn world_vector(&self, point: Vector3<f32>) -> Vector3<f32> {
        self.right * point.x + self.up * point.y + self.forward * point.z
    }

    fn custom_view_point(&self, ndc: [f32; 2], depth: f32) -> Option<Vector3<f32>> {
        let rows = self.custom_view_clip_rows()?.map(|row| row.map(f64::from));
        // At a fixed positive depth, x/w and y/w give two linear equations
        // for the lateral position. No inverse is needed on the picking path.
        let [px, py] = ndc.map(f64::from);
        let z = f64::from(depth);
        let [a, b, c, d] = std::array::from_fn(|i| rows[0][i] - px * rows[3][i]);
        let [e, f, g, h] = std::array::from_fn(|i| rows[1][i] - py * rows[3][i]);
        let det = a * f - b * e;
        let (r, s) = (-(c * z + d), -(g * z + h));
        let x = ((r * f - b * s) / det) as f32;
        let y = ((a * s - r * e) / det) as f32;
        (x.is_finite() && y.is_finite()).then(|| Vector3::new(x, y, depth))
    }

    /// The lateral clip scales and NDC offsets before the depth divide.
    #[doc(hidden)]
    pub fn clip_lens(&self) -> [f32; 4] {
        let [sx, sy, dx, dy] = self.lens;
        let (dx, dy) = (dx + self.clip_offset[0], dy + self.clip_offset[1]);
        if let Some([left, top, right, bottom]) = self.clip_viewport {
            let width = right - left;
            let height = bottom - top;
            [
                sx * width,
                sy * height,
                dx * width + left + right - 1.0,
                dy * height + 1.0 - top - bottom,
            ]
        } else {
            [sx, sy, dx, dy]
        }
    }

    fn row(&self, axis: Vector3<f32>, offset: f32) -> [f32; 4] {
        [axis.x, axis.y, axis.z, offset - axis.dot(&self.eye)]
    }

    pub fn clip_position(&self, [x, y, z]: [f32; 3]) -> [f32; 4] {
        self.clip_rows()
            .map(|[a, b, c, offset]| a * x + b * y + c * z + offset)
    }

    pub fn view_depth(&self, point: [f32; 3]) -> f32 {
        self.forward.dot(&(Vector3::from(point) - self.eye))
    }

    pub fn depth_row(&self) -> [f32; 4] {
        self.row(self.forward, 0.0)
    }

    /// The centred active picture in viewport coordinates, without letterbox bars.
    pub fn content_rect(&self) -> [f64; 4] {
        self.content
    }

    pub const fn depth_limits(&self) -> [f32; 2] {
        self.depth_range
    }

    #[doc(hidden)]
    pub fn has_oblique_depth(&self) -> bool {
        self.settings.depth_tilt != [0.0; 2]
    }

    /// Reference depths on the optical axis and camera-local depth slopes.
    #[doc(hidden)]
    pub const fn depth_projection(&self) -> [f32; 4] {
        let [x, y] = self.settings.depth_tilt;
        [self.settings.near, self.settings.far, x, y]
    }

    pub const fn is_orthographic(&self) -> bool {
        self.settings.projection.is_orthographic()
    }

    #[doc(hidden)]
    pub fn frustum_slice(&self, near: f32, far: f32) -> ([[f32; 3]; 8], [f32; 3], f32) {
        let [sx, sy, dx, dy] = self.lens;
        if matches!(
            self.settings.projection,
            Projection::FixedLens { .. }
                | Projection::OrthographicWindow { .. }
                | Projection::Custom { .. }
        ) {
            let corners: [Vector3<f32>; 8] = std::array::from_fn(|index| {
                let depth = if index & 4 == 0 { near } else { far };
                let x = if index & 1 == 0 { -1.0 } else { 1.0 };
                let y = if index & 2 == 0 { -1.0 } else { 1.0 };
                if self.has_custom_projection() {
                    return self
                        .custom_view_point([x, y], depth)
                        .unwrap_or_else(|| Vector3::new(0.0, 0.0, depth));
                }
                let scale = if self.is_orthographic() { 1.0 } else { depth };
                Vector3::new((x - dx) / sx * scale, (y - dy) / sy * scale, depth)
            });
            let (min, max) = corners
                .iter()
                .fold((corners[0], corners[0]), |(min, max), p| {
                    (min.inf(p), max.sup(p))
                });
            let center = min.zip_map(&max, f32::midpoint);
            let radius = corners
                .iter()
                .map(|p| (p - center).norm())
                .fold(0.0, f32::max);
            let world = |p: Vector3<f32>| {
                (self.eye + self.right * p.x + self.up * p.y + self.forward * p.z).into()
            };
            return (corners.map(world), world(center), radius);
        }
        let middle = (near + far) * 0.5;
        let center = self.eye + self.forward * middle;
        let extent = |depth| {
            let size = if self.is_orthographic() { 1.0 } else { depth };
            [size / sx, size / sy]
        };
        let corners = std::array::from_fn(|index| {
            let depth = if index & 4 == 0 { near } else { far };
            let [width, height] = extent(depth);
            let x = if index & 1 == 0 { -width } else { width };
            let y = if index & 2 == 0 { -height } else { height };
            (self.eye + self.forward * depth + self.right * x + self.up * y).into()
        });
        // The far corners are farthest from the slice center. Compute
        // the radius without using camera orientation or position.
        let [width, height] = extent(far);
        let radius = Vector3::new(width, height, far - middle).norm();
        (corners, center.into(), radius)
    }

    pub(crate) fn toward_viewer(&self, position: Vector3<f32>) -> Vector3<f32> {
        if let Some([x, y, z, kind]) = self.custom_world_viewer() {
            let viewer = Vector3::new(x, y, z);
            return if kind == 0.0 {
                -viewer
            } else {
                (viewer - position) * kind
            };
        }
        if self.is_orthographic() {
            -self.forward
        } else {
            self.eye - position
        }
    }

    pub fn eye_position(&self) -> [f32; 3] {
        self.eye.into()
    }

    /// Conservative size of the authored reference geometry in logical pixels.
    pub(crate) fn lod_size(&self, bounds: Bounds3D) -> f64 {
        if !bounds.is_finite() {
            return f64::INFINITY;
        }
        if self.has_custom_projection() {
            let rows = self.clip_rows();
            let mut minimum = [f64::INFINITY; 2];
            let mut maximum = [f64::NEG_INFINITY; 2];
            for index in 0..8 {
                let [x, y, z] = std::array::from_fn(|axis| {
                    if index & (1 << axis) == 0 {
                        bounds.min[axis]
                    } else {
                        bounds.max[axis]
                    }
                });
                let [x, y, _, w] = rows.map(|[a, b, c, d]| a * x + b * y + c * z + d);
                if w <= 0.0 || ![x, y, w].into_iter().all(f32::is_finite) {
                    return f64::INFINITY;
                }
                let ndc = [x, y].map(|value| f64::from(value) / f64::from(w));
                for axis in 0..2 {
                    minimum[axis] = minimum[axis].min(ndc[axis]);
                    maximum[axis] = maximum[axis].max(ndc[axis]);
                }
            }
            let extent =
                |axis: usize| (maximum[axis] - minimum[axis]) * self.content[axis + 2] * 0.5;
            return extent(0).max(extent(1));
        }
        let center = std::array::from_fn(|axis| f32::midpoint(bounds.min[axis], bounds.max[axis]));
        let radius = Vector3::from(std::array::from_fn(|axis| {
            (f64::from(bounds.max[axis]) - f64::from(bounds.min[axis])) * 0.5
        }))
        .norm();
        let [sx, sy, ..] = self.lens;
        let scale = self.content[3] * f64::from(sy);
        if self.is_orthographic() {
            return scale.max(self.content[2] * f64::from(sx)) * radius;
        }
        let nearest = f64::from(self.view_depth(center)) - radius;
        if nearest <= f64::from(self.depth_range[0]) {
            return f64::INFINITY;
        }
        if let Projection::FixedLens { .. } = self.settings.projection {
            let delta = Vector3::from(center) - self.eye;
            let depth = f64::from(self.forward.dot(&delta));
            // A sphere's change in depth also increases its lateral screen extent.
            let extent = |axis: Vector3<f32>, pixels: f64, clip_scale: f32| {
                let slope = f64::from(axis.dot(&delta)) / depth;
                pixels * f64::from(clip_scale) * slope.hypot(1.0)
            };
            return extent(self.right, self.content[2], sx).max(extent(
                self.up,
                self.content[3],
                sy,
            )) * (radius / nearest);
        }
        scale * radius / nearest
    }

    /// Project into the camera viewport in logical pixels.
    /// The depth is measured along the forward axis from the camera origin.
    pub fn project(&self, point: Point3D) -> ProjectedPoint3D {
        let Some(point) = point.narrow() else {
            return ProjectedPoint3D::default();
        };
        let [x, y, z, w] = self.clip_position(point);
        let depth = self.view_depth(point);
        if depth <= 0.0 || w <= 0.0 || ![x, y, w, depth].into_iter().all(f32::is_finite) {
            return ProjectedPoint3D::default();
        }
        let x = f64::from(x) / f64::from(w);
        let y = f64::from(y) / f64::from(w);
        let point = Point2D::new(
            Length(self.content[0] + (0.5 + x * 0.5) * self.content[2]),
            Length(self.content[1] + (0.5 - y * 0.5) * self.content[3]),
        );
        if ![point.x.0, point.y.0].into_iter().all(f64::is_finite) {
            return ProjectedPoint3D::default();
        }
        let [near, far] = self.depth_limits();
        ProjectedPoint3D {
            point,
            depth: f64::from(depth),
            valid: true,
            inside: (-1.0..=1.0).contains(&x)
                && (-1.0..=1.0).contains(&y)
                && if self.has_oblique_depth() || self.has_custom_projection() {
                    (0.0..=w).contains(&z)
                } else {
                    (near..=far).contains(&depth)
                },
        }
    }

    /// Unproject a logical-pixel point at a positive forward-axis depth.
    /// Screen points outside the viewport still map into world space.
    pub fn unproject(&self, point: Point2D, depth: f64) -> Option<Point3D> {
        let depth = depth as f32;
        if !(depth > 0.0 && depth.is_finite()) {
            return None;
        }
        if self.has_custom_projection() {
            let local = self.custom_view_point(self.screen_ndc(point), depth)?;
            let world = self.eye + self.world_vector(local);
            return world
                .iter()
                .all(|value| value.is_finite())
                .then(|| Point3D::new(f64::from(world.x), f64::from(world.y), f64::from(world.z)));
        }
        let offset = self.screen_offset(point);
        let lateral = if self.is_orthographic() {
            offset
        } else {
            offset * depth
        };
        let world = self.eye + self.forward * depth + lateral;
        world
            .iter()
            .all(|value| value.is_finite())
            .then(|| Point3D::new(f64::from(world.x), f64::from(world.y), f64::from(world.z)))
    }

    fn screen_ndc(&self, point: Point2D) -> [f32; 2] {
        [
            ((point.x.0 - self.content[0]) / self.content[2] * 2.0 - 1.0) as f32,
            (1.0 - (point.y.0 - self.content[1]) / self.content[3] * 2.0) as f32,
        ]
    }

    fn screen_offset(&self, point: Point2D) -> Vector3<f32> {
        let [x, y] = self.screen_ndc(point);
        let [sx, sy, dx, dy] = self.clip_lens();
        ((x - dx) / sx) * self.right + ((y - dy) / sy) * self.up
    }

    fn pointer_ray(&self, point: Point2D) -> Option<(Ray3D, f32, f32)> {
        let [x, y, width, height] = self.content;
        if !(x..x + width).contains(&point.x.0) || !(y..y + height).contains(&point.y.0) {
            return None;
        }
        let (origin, direction) = if self.has_custom_projection() {
            let ndc = self.screen_ndc(point);
            let first = self.custom_view_point(ndc, 0.0)?;
            let last = self.custom_view_point(ndc, 1.0)?;
            (
                self.eye + self.world_vector(first),
                self.world_vector(last - first),
            )
        } else {
            let offset = self.screen_offset(point);
            if self.is_orthographic() {
                (self.eye + offset, self.forward)
            } else {
                (self.eye, self.forward + offset)
            }
        };
        let ray = Ray3D::new(origin.into(), direction.into())?;
        if self.has_oblique_depth() || self.has_custom_projection() {
            let [_, _, z, w] = self.clip_rows();
            let far = std::array::from_fn(|index| w[index] - z[index]);
            let mut start: f32 = 0.0;
            let mut end = f32::INFINITY;
            for [x, y, z, offset] in [z, far] {
                let normal = Vector3::new(x, y, z);
                let rate = normal.dot(&ray.direction);
                let value = normal.dot(&origin) + offset;
                if rate > 0.0 {
                    start = start.max(-value / rate);
                } else if rate < 0.0 {
                    end = end.min(-value / rate);
                } else if value < 0.0 {
                    return None;
                }
            }
            return (start < end).then_some((ray, start, end));
        }
        let depth_at_origin = self.forward.dot(&(origin - self.eye));
        let depth_per_unit = self.forward.dot(&ray.direction);
        let [near, far] = self.depth_limits();
        Some((
            ray,
            (near - depth_at_origin) / depth_per_unit,
            (far - depth_at_origin) / depth_per_unit,
        ))
    }
}

fn depth_range(settings: SceneCameraSettings, lens: [f32; 4]) -> Option<[f32; 2]> {
    let [tx, ty] = settings.depth_tilt;
    if [tx, ty] == [0.0; 2] {
        return Some([settings.near, settings.far]);
    }
    let [sx, sy, dx, dy] = lens.map(f64::from);
    let bound = |tilt: f32, scale: f64, shift: f64| {
        let a = f64::from(tilt) * (-1.0 - shift) / scale;
        let b = f64::from(tilt) * (1.0 - shift) / scale;
        [a.min(b), a.max(b)]
    };
    let [xmin, xmax] = bound(tx, sx, dx);
    let [ymin, ymax] = bound(ty, sy, dy);
    let min = xmin + ymin;
    let max = xmax + ymax;
    let near = f64::from(settings.near);
    let far = f64::from(settings.far);
    let [start, end] = if settings.projection.is_orthographic() {
        [near - max, far - min]
    } else {
        if min <= -1.0 {
            return None;
        }
        let far_denominator = min + near / far;
        [
            near / (1.0 + max),
            if far_denominator > 0.0 {
                near / far_denominator
            } else {
                f64::INFINITY
            },
        ]
    };
    let (start, end) = (start as f32, end as f32);
    (start > 0.0 && start.is_finite() && end > start).then_some([start, end])
}

fn inverse_rows(rows: [[f32; 4]; 4]) -> Option<[[f32; 4]; 4]> {
    let matrix = Matrix4::from_row_slice(rows.as_flattened()).cast::<f64>();
    let inverse = matrix.try_inverse()?.cast::<f32>();
    inverse
        .iter()
        .all(|value| value.is_finite())
        .then(|| std::array::from_fn(|row| std::array::from_fn(|col| inverse[(row, col)])))
}

fn camera_frame(
    settings: SceneCameraSettings,
    [width, height]: [f64; 2],
) -> Option<([f32; 4], [f64; 4])> {
    if ![width, height]
        .into_iter()
        .all(|size| size.is_finite() && size > 0.0)
    {
        return None;
    }
    let aspect = settings.aspect.unwrap_or(width / height);
    let (frame_width, frame_height) = match settings.aspect {
        Some(_) => (width.min(height * aspect), height.min(width / aspect)),
        None => (width, height),
    };
    let lens = match settings.projection {
        Projection::FixedLens { lens, .. } => lens,
        Projection::Custom { rows, .. } => [
            rows[0][0].hypot(rows[0][1]),
            rows[1][0].hypot(rows[1][1]),
            0.0,
            0.0,
        ],
        Projection::Perspective(sy) | Projection::Orthographic(sy) => {
            let sx = if settings.aspect.is_some() {
                (f64::from(sy) / aspect) as f32
            } else {
                sy * (height / width) as f32
            };
            [sx, sy, 0.0, 0.0]
        }
        Projection::OrthographicWindow {
            horizontal: [left, right],
            vertical: [sy, dy],
        } => {
            // Match the resolved vertical span when the author leaves an edge open.
            let half_width = (width / height) / f64::from(sy);
            let [sx, dx] = lens_axis(
                left.unwrap_or(-half_width),
                right.unwrap_or(half_width),
                1.0,
                1.0,
            )?;
            [sx, sy, dx, dy]
        }
    };
    (frame_width > 0.0 && frame_height > 0.0 && lens[0].is_finite() && lens[0] > 0.0).then_some((
        lens,
        [
            (width - frame_width) * 0.5,
            (height - frame_height) * 0.5,
            frame_width,
            frame_height,
        ],
    ))
}

fn pick(geometry: &[SceneObject], camera: Option<SceneCamera>, point: Point2D) -> String {
    camera
        .and_then(|camera| camera.scene_ray(point))
        .and_then(|query| trace_id(geometry, query))
        .map_or_else(String::new, str::to_owned)
}

#[doc(hidden)]
pub fn pick_objects(
    geometry: &[SceneObject],
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
) -> String {
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0]);
    pick(geometry, camera, point)
}

#[doc(hidden)]
pub fn pick_model(
    model: &SceneModel,
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> String {
    SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0]).map_or_else(
        String::new,
        |camera| {
            let camera = camera.with_layer_mask(layers);
            camera
                .scene_ray(point)
                .map_or_else(String::new, |ray| ray.id(&model.geometry(&camera)))
        },
    )
}

#[doc(hidden)]
pub fn pick_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> String {
    model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map_or_else(String::new, |camera| {
            let camera = camera.with_layer_mask(layers);
            camera
                .scene_ray(point)
                .map_or_else(String::new, |ray| ray.id(&model.geometry(&camera)))
        })
}

fn hit(geometry: &[SceneObject], camera: Option<SceneCamera>, point: Point2D) -> Hit3D {
    camera
        .and_then(|camera| camera.scene_ray(point))
        .and_then(|query| trace_surfaces(geometry, query))
        .map_or_else(Hit3D::default, Hit3D::from)
}

#[doc(hidden)]
pub fn hit_objects(
    geometry: &[SceneObject],
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
) -> Hit3D {
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0]);
    hit(geometry, camera, point)
}

#[doc(hidden)]
pub fn hit_model(
    model: &SceneModel,
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> Hit3D {
    SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0]).map_or_else(
        Hit3D::default,
        |camera| {
            let camera = camera.with_layer_mask(layers);
            camera.scene_ray(point).map_or_else(Hit3D::default, |ray| {
                ray.first(&model.geometry(&camera), &RayRows::All)
            })
        },
    )
}

#[doc(hidden)]
pub fn hit_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> Hit3D {
    model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map_or_else(Hit3D::default, |camera| {
            let camera = camera.with_layer_mask(layers);
            camera.scene_ray(point).map_or_else(Hit3D::default, |ray| {
                ray.first(&model.geometry(&camera), &RayRows::All)
            })
        })
}

fn pick_rect_rows(
    model: &SceneModel,
    camera: Option<SceneCamera>,
    corners: [Point2D; 2],
) -> VecModel<i64> {
    let Some(camera) = camera.filter(|camera| camera.layer_mask() != 0) else {
        return VecModel::default();
    };
    let Some(region) = Frustum::selection(&camera, corners) else {
        return VecModel::default();
    };
    let geometry = model.geometry(&camera);
    let eye = camera.eye_position();
    let mut rows = Vec::new();
    geometry.visit_selection(&region, |index, planes| {
        let object = &geometry[index];
        if object.layers & camera.layer_mask() != 0
            && object.visible_at_distance(eye)
            && let Some(row) = object.scene_row()
            && object.intersects_selection(&camera, &region, planes)
        {
            rows.push(i64::from(row));
        }
    });
    rows.sort_unstable();
    rows.dedup();
    rows.into()
}

/// Geometry intersecting a view rectangle, reported in authored row order.
#[doc(hidden)]
pub fn pick_in_rect(
    model: &SceneModel,
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    corners: [Point2D; 2],
    layers: Option<i64>,
) -> VecModel<i64> {
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    pick_rect_rows(model, camera, corners)
}

#[doc(hidden)]
pub fn pick_in_rect_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    corners: [Point2D; 2],
    layers: Option<i64>,
) -> VecModel<i64> {
    let camera = model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    pick_rect_rows(model, camera, corners)
}

impl SceneCamera {
    fn scene_ray(&self, point: Point2D) -> Option<SceneRay> {
        let (ray, min, max) = self.pointer_ray(point)?;
        Some(SceneRay::new(ray, min, max, self.layer_mask()).with_camera(self.eye_position()))
    }
}

fn hit_all(
    model: &SceneModel,
    camera: Option<SceneCamera>,
    point: Point2D,
    rows: &RayRows,
) -> VecModel<Hit3D> {
    camera
        .and_then(|camera| {
            let ray = camera.scene_ray(point)?;
            Some(ray.all(&model.geometry(&camera), rows))
        })
        .unwrap_or_default()
}

#[doc(hidden)]
pub fn hit_row(
    model: &SceneModel,
    orbit: [f64; 3],
    viewport: Point2D,
    point: Point2D,
    row: i64,
    layers: Option<i64>,
) -> Hit3D {
    let [yaw, pitch, distance] = orbit;
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_filtered(model, camera, point, &RayRows::Row(row))
}

#[doc(hidden)]
pub fn hit_row_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    row: i64,
    layers: Option<i64>,
) -> Hit3D {
    let camera = model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_filtered(model, camera, point, &RayRows::Row(row))
}

#[doc(hidden)]
pub fn hit_instance(
    model: &SceneModel,
    orbit: [f64; 3],
    viewport: Point2D,
    point: Point2D,
    address: [i64; 3],
    layers: Option<i64>,
) -> Hit3D {
    let Some(rows) = RayRows::instance(address) else {
        return Hit3D::default();
    };
    let [yaw, pitch, distance] = orbit;
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_filtered(model, camera, point, &rows)
}

#[doc(hidden)]
pub fn hit_instance_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    address: [i64; 3],
    layers: Option<i64>,
) -> Hit3D {
    let Some(rows) = RayRows::instance(address) else {
        return Hit3D::default();
    };
    let camera = model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_filtered(model, camera, point, &rows)
}

fn hit_filtered(
    model: &SceneModel,
    camera: Option<SceneCamera>,
    point: Point2D,
    rows: &RayRows,
) -> Hit3D {
    camera
        .and_then(|camera| {
            let ray = camera.scene_ray(point)?;
            Some(ray.first(&model.geometry(&camera), rows))
        })
        .unwrap_or_default()
}

#[doc(hidden)]
pub fn hit_subset(
    model: &SceneModel,
    orbit: [f64; 3],
    viewport: Point2D,
    point: Point2D,
    rows: &[i64],
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let [yaw, pitch, distance] = orbit;
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_all(model, camera, point, &RayRows::subset(rows))
}

#[doc(hidden)]
pub fn hit_subset_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    rows: &[i64],
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let camera = model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_all(model, camera, point, &RayRows::subset(rows))
}

#[doc(hidden)]
pub fn hit_all_objects(
    model: &SceneModel,
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let camera = SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_all(model, camera, point, &RayRows::All)
}

#[doc(hidden)]
pub fn hit_all_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    let camera = model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map(|camera| camera.with_layer_mask(layers));
    hit_all(model, camera, point, &RayRows::All)
}

#[doc(hidden)]
pub fn project(
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point3D,
) -> ProjectedPoint3D {
    SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .map_or_else(ProjectedPoint3D::default, |camera| camera.project(point))
}

#[doc(hidden)]
pub fn project_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point3D,
) -> ProjectedPoint3D {
    model
        .camera(id, [viewport.x.0, viewport.y.0])
        .map_or_else(ProjectedPoint3D::default, |camera| camera.project(point))
}

#[doc(hidden)]
pub fn unproject(
    yaw: f64,
    pitch: f64,
    distance: f64,
    viewport: Point2D,
    point: Point2D,
    depth: f64,
    fallback: Point3D,
) -> Point3D {
    SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
        .and_then(|camera| camera.unproject(point, depth))
        .unwrap_or(fallback)
}

#[doc(hidden)]
pub fn unproject_camera(
    model: &SceneModel,
    id: &str,
    viewport: Point2D,
    point: Point2D,
    depth: f64,
    fallback: Point3D,
) -> Point3D {
    model
        .camera(id, [viewport.x.0, viewport.y.0])
        .and_then(|camera| camera.unproject(point, depth))
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_depth_keeps_distant_values_separate_and_preserves_infinite_matrix_rows() {
        let settings = SceneCameraSettings::from_record(
            "perspective",
            45.0,
            10.0,
            0.001,
            100_000.0,
            [None; 4],
            None,
        )
        .expect("valid perspective");
        let mut node = CameraNode {
            eye: Vector3::new(0.0, 0.0, 100.0),
            rotation: UnitQuaternion::identity(),
            settings,
            lod_bias: 1.0,
            layers: 1,
        };
        let viewport = [432.0, 252.0];
        let values = |camera: SceneCamera| {
            [0.0, 0.02].map(|z| {
                let clip = camera.clip_position([0.0, 0.0, z]);
                clip[2] / clip[3]
            })
        };
        let [far, near] = values(SceneCamera::from_node(&node, viewport).expect("valid viewport"));
        assert_eq!(far, near);
        node.settings = settings.with_reverse_depth(true);
        let reversed = SceneCamera::from_node(&node, viewport).expect("valid reversed viewport");
        let [far, near] = values(reversed);
        assert!(near > far);
        assert_eq!(reversed.depth_clear(), 0.0);

        node.settings = SceneCameraSettings::custom(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.001],
                [0.0, 0.0, -1.0, 0.0],
            ],
            "webgpu",
        )
        .expect("infinite reversed matrix")
        .with_reverse_depth(true);
        let custom = SceneCamera::from_node(&node, viewport).expect("valid custom viewport");
        assert!(custom.depth_limits()[1].is_infinite());
        let [far, near] = values(custom);
        assert!(near > far);
    }

    #[test]
    fn camera_matrix_cache_reuses_depth_aliases_and_bounds_live_matrix_edits() {
        let cache = CameraMatrixCache::new();
        let matrix = [
            [1.4, 0.2, 0.0, 0.0],
            [0.0, 1.6, 0.0, 0.0],
            [0.0, 0.0, -1.25, -1.25],
            [0.0, 0.0, -1.0, 0.0],
        ];
        let original = cache.get(matrix, "").expect("valid camera");
        assert_eq!(cache.get(matrix, "webgpu"), Some(original));
        assert_eq!(cache.0.borrow().len(), 1);
        assert_ne!(cache.get(matrix, "opengl"), Some(original));
        assert_eq!(cache.0.borrow().len(), 2);
        let mut invalid = matrix;
        invalid[0][0] = f64::NAN;
        assert!(cache.get(invalid, "webgpu").is_none());
        assert!(cache.get(invalid, "webgpu").is_none());
        assert_eq!(cache.0.borrow().len(), 3);
        for step in 1..=MATRIX_CACHE_CAPACITY + 1 {
            let mut edited = matrix;
            edited[0][1] += step as f64 * 0.0025;
            assert!(cache.get(edited, "webgpu").is_some());
        }
        assert_eq!(cache.0.borrow().len(), MATRIX_CACHE_CAPACITY);
        assert_eq!(cache.get(matrix, "webgpu"), Some(original));
    }

    #[test]
    fn a_custom_camera_round_trips_homogeneous_projection_through_pose_and_capture_rect() {
        let source = [
            [1.44, 0.35, 0.06, 0.12],
            [0.12, 1.6, -0.02, 0.08],
            [0.01, -0.02, -1.25, -1.25],
            [0.045, -0.035, -1.0, 0.0],
        ];
        let reference = Matrix4::from_row_slice(source.as_flattened());
        let point = Vector3::new(0.9_f32, 0.5, 3.0);
        let expected = reference
            * Vector4::new(
                f64::from(point.x),
                f64::from(point.y),
                -f64::from(point.z),
                1.0,
            );
        let viewport = [432.0, 272.0];
        let rect = [0.1, 0.2, 0.8, 0.9];
        for (reverse, gl, scale) in [
            (false, false, 1.0),
            (true, false, 1.0),
            (false, true, 1e-140),
            (true, true, 1e140),
        ] {
            let mut rows = source.map(|row| row.map(|value| value * scale));
            if reverse {
                rows[2] = std::array::from_fn(|i| rows[3][i] - rows[2][i]);
            }
            if gl {
                rows[2] = std::array::from_fn(|i| 2.0 * rows[2][i] - rows[3][i]);
            }
            let settings = SceneCameraSettings::custom(rows, if gl { "opengl" } else { "webgpu" })
                .expect("valid matrix");
            let node = CameraNode {
                eye: Vector3::new(3.0, -2.0, 8.0),
                rotation: UnitQuaternion::from_euler_angles(0.15, -0.23, 0.12),
                settings,
                lod_bias: 1.0,
                layers: 1,
            };
            let camera = SceneCamera::from_node(&node, viewport).expect("valid viewport");
            let world = camera.eye + camera.world_vector(point);
            let [x, y, z] = <[f32; 3]>::from(world).map(f64::from);
            let world_point = Point3D::new(x, y, z);
            let base = camera.project(world_point);
            assert!(base.valid && base.inside);
            assert!(
                (base.point.x.0 - viewport[0] * (0.5 + 0.5 * expected.x / expected.w)).abs() < 1e-3
            );
            assert!(
                (base.point.y.0 - viewport[1] * (0.5 - 0.5 * expected.y / expected.w)).abs() < 1e-3
            );
            let clipped = camera.with_clip_viewport(rect);
            let projected = clipped.project(world_point);
            let [left, top, right, bottom] = rect.map(f64::from);
            assert!(
                (projected.point.x.0 - (left * viewport[0] + (right - left) * base.point.x.0))
                    .abs()
                    < 1e-3
            );
            assert!(
                (projected.point.y.0 - (top * viewport[1] + (bottom - top) * base.point.y.0)).abs()
                    < 1e-3
            );
            let restored = clipped
                .unproject(projected.point, projected.depth)
                .expect("invertible ray");
            assert!(
                (restored.x - x)
                    .abs()
                    .max((restored.y - y).abs())
                    .max((restored.z - z).abs())
                    < 1e-4
            );
            let (ray, start, end) = clipped.pointer_ray(projected.point).expect("viewport ray");
            let delta = world - ray.origin();
            assert!(delta.cross(&ray.direction).norm() < 1e-4);
            assert!((start..=end).contains(&delta.dot(&ray.direction)));
            let actual = clipped.clip_position(world.into());
            assert!((f64::from(actual[2] / actual[3]) - expected.z / expected.w).abs() < 1e-5);
        }
        assert!(SceneCameraSettings::custom([[0.0; 4]; 4], "webgpu").is_none());
        let mut behind = source;
        behind[3] = [0.0, 0.0, 1.0, 0.0];
        assert!(SceneCameraSettings::custom(behind, "webgpu").is_none());
    }

    #[test]
    fn oblique_depth_planes_clip_the_same_world_points_as_projection_and_rays() {
        let tilt = Vector3D::new(0.5, 0.0, -1.0);
        for kind in ["orthographic", "perspective"] {
            let settings =
                SceneCameraSettings::from_record(kind, 45.0, 5.0, 7.5, 12.0, [None; 4], Some(8.0))
                    .and_then(|settings| settings.with_clip_normal(Some(tilt)))
                    .expect("valid oblique camera");
            let node = CameraNode {
                eye: Vector3::new(0.0, 0.0, 8.0),
                rotation: UnitQuaternion::identity(),
                settings,
                lod_bias: 1.0,
                layers: 1,
            };
            let camera = SceneCamera::from_node(&node, [432.0, 252.0]).expect("valid viewport");
            let [start, end] = camera.depth_limits();
            assert!(start < 7.5 && end > 12.0);
            let inverse = Matrix4::from_row_slice(
                camera
                    .inverse_clip_rows()
                    .expect("invertible projection")
                    .as_flattened(),
            );
            for ([x, z], inside) in [
                ([-2.0, 0.0], false),
                ([2.0, 0.0], true),
                ([-2.0, -3.8], true),
                ([2.0, -3.8], false),
            ] {
                let point = Point3D::new(x, 0.0, z);
                let projected = camera.project(point);
                assert!(projected.valid);
                assert_eq!(projected.inside, inside);
                let (ray, near, far) = camera.pointer_ray(projected.point).expect("valid ray");
                let world = Vector3::new(x as f32, 0.0, z as f32);
                let distance = (world - ray.origin()).dot(&ray.direction);
                assert_eq!((near..=far).contains(&distance), inside);
                let clip = camera.clip_position(world.into());
                let restored = inverse * nalgebra::Vector4::from(clip);
                assert!((restored.xyz() / restored.w - world).norm() < 1e-4);
            }
        }
    }

    #[test]
    fn partial_orthographic_edges_use_the_resolved_vertical_span_on_resize() {
        let settings = SceneCameraSettings::from_record(
            "orthographic",
            45.0,
            10.0,
            0.1,
            100.0,
            [Some(-2.0), None, Some(-1.0), Some(3.0)],
            None,
        )
        .expect("valid authored bounds");
        let node = CameraNode {
            eye: Vector3::new(0.0, 0.0, 8.0),
            rotation: UnitQuaternion::identity(),
            settings,
            lod_bias: 1.0,
            layers: 1,
        };
        for (viewport, [center_x, right]) in
            [([400.0, 200.0], [1.0, 4.0]), ([200.0, 200.0], [0.0, 2.0])]
        {
            let camera = SceneCamera::from_node(&node, viewport).expect("valid viewport");
            let center = camera.project(Point3D::new(center_x, 1.0, 0.0));
            assert!(center.inside);
            assert!((center.point.x.0 - viewport[0] * 0.5).abs() < 1e-4);
            assert!((center.point.y.0 - viewport[1] * 0.5).abs() < 1e-4);
            let edge = camera.project(Point3D::new(right, 1.0, 0.0));
            assert!((edge.point.x.0 - viewport[0]).abs() < 1e-4);
            let ray = camera
                .pointer_ray(center.point)
                .expect("center defines a parallel ray");
            assert!((f64::from(ray.0.origin().x) - center_x).abs() < 1e-5);
            assert_eq!(ray.0.direction, -Vector3::z());
        }
    }
}
