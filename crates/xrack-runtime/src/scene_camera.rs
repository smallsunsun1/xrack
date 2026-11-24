//! Camera projections shared by rendering, shadow cascades and viewport picking.
use crate::scene_visibility::Frustum;
use nalgebra::{Matrix4, UnitQuaternion, Vector3};

use crate::{
    Length, Node, Point2D, Value, VecModel,
    scene_bounds::Bounds3D,
    scene_hierarchy::SceneModel,
    scene3d::{
        Hit3D, Ray3D, RayRows, SceneObject, SceneRay, SceneTextureHit, direction, trace_id,
        trace_surfaces,
    },
};

const NEAR: f32 = 0.1;
const FAR: f32 = 100.0;
const ORBIT_FOCAL: f32 = 2.414_213_7; // 45 degree vertical field of view

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Projection {
    Perspective(f32),
    Orthographic(f32),
    Frustum { scale: [f32; 2], shift: [f32; 2] },
}

impl Projection {
    fn vertical_scale(self) -> f32 {
        match self {
            Self::Perspective(focal) | Self::Orthographic(focal) => focal,
            Self::Frustum { scale, .. } => scale[1],
        }
    }
}

impl SceneCameraSettings {
    pub fn from_record(
        kind: &str,
        fov: f64,
        height: f64,
        near: f64,
        far: f64,
        bounds: [Option<f64>; 4],
    ) -> Option<Self> {
        if !(0.001..=100_000.0).contains(&near) || !(near + 0.001..=100_000.0).contains(&far) {
            return None;
        }
        let projection = match kind {
            "perspective" if (1.0..=175.0).contains(&fov) => {
                Projection::Perspective((fov.to_radians() * 0.5).tan().recip() as f32)
            }
            "orthographic" if (0.001..=100_000.0).contains(&height) => {
                Projection::Orthographic((2.0 / height) as f32)
            }
            "frustum" => {
                let half = near / f64::from(ORBIT_FOCAL);
                let [left, right, bottom, top] = std::array::from_fn(|i| {
                    bounds[i].unwrap_or(if i % 2 == 0 { -half } else { half })
                });
                let axis = |min: f64, max: f64| {
                    let span = max - min;
                    let scale = (2.0 * near / span) as f32;
                    let shift = (-(max + min) / span) as f32;
                    (min < max
                        && scale.is_finite()
                        && scale > 0.0
                        && shift.is_finite()
                        && (((1.0 + shift.abs()) / scale) * far as f32).is_finite())
                    .then_some((scale, shift))
                };
                let (sx, dx) = axis(left, right)?;
                let (sy, dy) = axis(bottom, top)?;
                Projection::Frustum {
                    scale: [sx, sy],
                    shift: [dx, dy],
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
        })
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
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraNode {
    pub eye: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub target: Option<Vector3<f32>>,
    pub settings: SceneCameraSettings,
    pub lod_bias: f64,
    pub layers: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub struct SceneCamera {
    eye: Vector3<f32>,
    right: Vector3<f32>,
    up: Vector3<f32>,
    forward: Vector3<f32>,
    horizontal_scale: f32,
    content: [f64; 4],
    pub(crate) lod_bias: f64,
    layers: u64,
    settings: SceneCameraSettings,
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
            horizontal_scale: 1.0,
            content: [0.0, 0.0, 1.0, 1.0],
            lod_bias: 0.0,
            layers,
            clip_offset: [0.0; 2],
            clip_viewport: None,
            settings: SceneCameraSettings {
                projection: Projection::Perspective(1.0),
                aspect: None,
                near: 0.05,
                far: FAR,
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
        };
        Some(Self {
            eye: radial * distance.clamp(2.0, 60.0) as f32,
            forward,
            right,
            up,
            horizontal_scale: camera_frame(settings, viewport)?.0,
            content: [0.0, 0.0, viewport[0], viewport[1]],
            lod_bias: 1.0,
            layers: crate::scene3d::MAIN_LAYER,
            settings,
            clip_offset: [0.0; 2],
            clip_viewport: None,
        })
    }

    pub(crate) fn from_node(node: &CameraNode, viewport: [f64; 2]) -> Option<Self> {
        let (horizontal_scale, content) = camera_frame(node.settings, viewport)?;
        let mut camera = Self {
            eye: node.eye,
            right: node.rotation * Vector3::x(),
            up: node.rotation * Vector3::y(),
            forward: node.rotation * -Vector3::z(),
            horizontal_scale,
            content,
            lod_bias: node.lod_bias,
            layers: node.layers,
            settings: node.settings,
            clip_offset: [0.0; 2],
            clip_viewport: None,
        };
        if let Some(forward) = node
            .target
            .and_then(|target| (target - node.eye).try_normalize(1e-6))
        {
            let right = forward
                .cross(&camera.up)
                .try_normalize(1e-4)
                // At the pole, project the authored right vector instead.
                .unwrap_or_else(|| {
                    (camera.right - forward * camera.right.dot(&forward)).normalize()
                });
            camera.forward = forward;
            camera.right = right;
            camera.up = right.cross(&forward);
        }
        Some(camera)
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
            && self.horizontal_scale == other.horizontal_scale
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
        let [near, far] = self.depth_limits();
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
        [
            lateral(self.right, sx, dx),
            lateral(self.up, sy, dy),
            self.row(self.forward * depth_scale, -near * depth_scale),
            w,
        ]
    }

    /// Rows of the clip-to-world transform for reconstruction from depth.
    #[doc(hidden)]
    pub fn inverse_clip_rows(&self) -> Option<[[f32; 4]; 4]> {
        let inverse = Matrix4::from_row_slice(self.clip_rows().as_flattened()).try_inverse()?;
        inverse
            .iter()
            .all(|value| value.is_finite())
            .then(|| std::array::from_fn(|row| std::array::from_fn(|col| inverse[(row, col)])))
    }

    /// The lateral clip scales and NDC offsets before the depth divide.
    #[doc(hidden)]
    pub fn clip_lens(&self) -> [f32; 4] {
        let [dx, dy] = match self.settings.projection {
            Projection::Frustum { shift, .. } => shift,
            _ => [0.0; 2],
        };
        let (sx, sy) = (
            self.horizontal_scale,
            self.settings.projection.vertical_scale(),
        );
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
        [self.settings.near, self.settings.far]
    }

    pub const fn is_orthographic(&self) -> bool {
        matches!(self.settings.projection, Projection::Orthographic(_))
    }

    #[doc(hidden)]
    pub fn frustum_slice(&self, near: f32, far: f32) -> ([[f32; 3]; 8], [f32; 3], f32) {
        if let Projection::Frustum { scale, shift } = self.settings.projection {
            let corners: [Vector3<f32>; 8] = std::array::from_fn(|index| {
                let depth = if index & 4 == 0 { near } else { far };
                let x = if index & 1 == 0 { -1.0 } else { 1.0 };
                let y = if index & 2 == 0 { -1.0 } else { 1.0 };
                Vector3::new(
                    (x - shift[0]) / scale[0] * depth,
                    (y - shift[1]) / scale[1] * depth,
                    depth,
                )
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
            [
                size / self.horizontal_scale,
                size / self.settings.projection.vertical_scale(),
            ]
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
        let center = std::array::from_fn(|axis| f32::midpoint(bounds.min[axis], bounds.max[axis]));
        let radius = Vector3::from(std::array::from_fn(|axis| {
            (f64::from(bounds.max[axis]) - f64::from(bounds.min[axis])) * 0.5
        }))
        .norm();
        let scale = self.content[3] * f64::from(self.settings.projection.vertical_scale());
        if self.is_orthographic() {
            return scale * radius;
        }
        let nearest = f64::from(self.view_depth(center)) - radius;
        if nearest <= f64::from(self.settings.near) {
            return f64::INFINITY;
        }
        if let Projection::Frustum { scale, .. } = self.settings.projection {
            let delta = Vector3::from(center) - self.eye;
            let depth = f64::from(self.forward.dot(&delta));
            // A sphere's change in depth also increases its lateral screen extent.
            let extent = |axis: Vector3<f32>, pixels: f64, clip_scale: f32| {
                let slope = f64::from(axis.dot(&delta)) / depth;
                pixels * f64::from(clip_scale) * slope.hypot(1.0)
            };
            return extent(self.right, self.content[2], scale[0]).max(extent(
                self.up,
                self.content[3],
                scale[1],
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
        let [x, y, _, w] = self.clip_position(point);
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
                && (near..=far).contains(&depth),
        }
    }

    /// Unproject a logical-pixel point at a positive forward-axis depth.
    /// Screen points outside the viewport still map into world space.
    pub fn unproject(&self, point: Point2D, depth: f64) -> Option<Point3D> {
        let depth = depth as f32;
        if !(depth > 0.0 && depth.is_finite()) {
            return None;
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

    fn screen_offset(&self, point: Point2D) -> Vector3<f32> {
        let x = ((point.x.0 - self.content[0]) / self.content[2] * 2.0 - 1.0) as f32;
        let y = (1.0 - (point.y.0 - self.content[1]) / self.content[3] * 2.0) as f32;
        let [sx, sy, dx, dy] = self.clip_lens();
        ((x - dx) / sx) * self.right + ((y - dy) / sy) * self.up
    }

    fn pointer_ray(&self, point: Point2D) -> Option<(Ray3D, f32, f32)> {
        let [x, y, width, height] = self.content;
        if !(x..x + width).contains(&point.x.0) || !(y..y + height).contains(&point.y.0) {
            return None;
        }
        let offset = self.screen_offset(point);
        let (origin, direction) = if self.is_orthographic() {
            (self.eye + offset, self.forward)
        } else {
            (self.eye, self.forward + offset)
        };
        let ray = Ray3D::new(origin.into(), direction.into())?;
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

fn camera_frame(
    settings: SceneCameraSettings,
    [width, height]: [f64; 2],
) -> Option<(f32, [f64; 4])> {
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
    let scale = if let Projection::Frustum { scale, .. } = settings.projection {
        scale[0]
    } else if settings.aspect.is_some() {
        (f64::from(settings.projection.vertical_scale()) / aspect) as f32
    } else {
        settings.projection.vertical_scale() * (height / width) as f32
    };
    (frame_width > 0.0 && frame_height > 0.0 && scale.is_finite() && scale > 0.0).then_some((
        scale,
        [
            (width - frame_width) * 0.5,
            (height - frame_height) * 0.5,
            frame_width,
            frame_height,
        ],
    ))
}

fn pick(geometry: &[SceneObject], camera: Option<SceneCamera>, point: Point2D) -> String {
    let Some(camera) = camera else {
        return String::new();
    };
    let Some((ray, min, max)) = camera.pointer_ray(point) else {
        return String::new();
    };
    trace_id(geometry, ray, min, max, camera.layer_mask()).map_or_else(String::new, str::to_owned)
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
        .and_then(|camera| {
            let (ray, min, max) = camera.pointer_ray(point)?;
            trace_surfaces(geometry, ray, min, max, camera.layer_mask())
        })
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
    let mut rows = Vec::new();
    geometry.visit_selection(&region, |index, planes| {
        let object = &geometry[index];
        if object.layers & camera.layer_mask() != 0
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
        Some(SceneRay::new(ray, min, max, self.layer_mask()))
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
