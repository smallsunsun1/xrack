//! Renderer-independent light data projected from SceneView3D record models.
use crate::{
    Color, Node, RecordList, Value, VecModel,
    scene3d::{MAIN_LAYER, SceneCamera, SceneModel, direction},
};
use nalgebra::{UnitQuaternion, Vector3};
use std::sync::Arc;

pub const MAX_SCENE_LIGHTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLight {
    pub source: SceneLightSource,
    pub radiance: [f32; 3],
    pub casts_shadow: bool,
    pub shadow: ShadowSampling,
    pub shadow_projection: ShadowProjectionSettings,
    pub bake_mode: LightBakeMode,
    pub layers: u64,
    pub scope: SceneLightScope,
}

/// Inclusive preorder interval in a prepared scene. Zero matches the whole scene.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneLightScope(u32);

impl SceneLightScope {
    pub(crate) fn subtree(first: u16, last: u16) -> Self {
        Self(u32::from(first) | (u32::from(last) << 16))
    }

    pub fn packed(self) -> u32 {
        self.0
    }

    pub fn contains(self, index: u16) -> bool {
        let index = u32::from(index);
        self.0 == 0 || index >= (self.0 & 0xffff) && index <= (self.0 >> 16)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightBakeMode {
    #[default]
    Disabled,
    Indirect,
    All,
}

impl LightBakeMode {
    fn from_name(name: &str) -> Self {
        match name {
            "indirect" => Self::Indirect,
            "all" => Self::All,
            _ => Self::Disabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ShadowFilter {
    Hard,
    #[default]
    Bilinear,
    Pcf4,
    Pcf8,
    Pcf16,
    Pcf32,
    Pcf64,
}

impl ShadowFilter {
    pub fn taps(self) -> u32 {
        match self {
            Self::Hard => 0,
            Self::Bilinear => 1,
            Self::Pcf4 => 4,
            Self::Pcf8 => 8,
            Self::Pcf16 => 16,
            Self::Pcf32 => 32,
            Self::Pcf64 => 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShadowRadius {
    Texels(f32),
    World(f32),
}

/// Receiver-side settings. Shadow map projection and depth caching are independent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowSampling {
    pub filter: ShadowFilter,
    pub strength: f32,
    pub radius: ShadowRadius,
    pub bias: Option<f32>,
}

impl Default for ShadowSampling {
    fn default() -> Self {
        Self {
            filter: ShadowFilter::Bilinear,
            strength: Self::DEFAULT_STRENGTH as f32,
            radius: ShadowRadius::Texels(Self::DEFAULT_RADIUS as f32),
            bias: None,
        }
    }
}

impl ShadowSampling {
    pub const DEFAULT_STRENGTH: f64 = 1.0;
    pub const DEFAULT_RADIUS: f64 = 2.0;

    fn prepare(
        filter: &str,
        strength: f64,
        radius: f64,
        world_radius: Option<f64>,
        bias: Option<f64>,
    ) -> Self {
        let default = Self::default();
        let bounded =
            |value: f64, maximum: f64| value.is_finite().then(|| value.clamp(0.0, maximum) as f32);
        Self {
            filter: match filter {
                "hard" => ShadowFilter::Hard,
                "pcf4" => ShadowFilter::Pcf4,
                "pcf8" => ShadowFilter::Pcf8,
                "pcf16" => ShadowFilter::Pcf16,
                "pcf32" => ShadowFilter::Pcf32,
                "pcf64" => ShadowFilter::Pcf64,
                _ => default.filter,
            },
            strength: bounded(strength, 1.0).unwrap_or(default.strength),
            radius: world_radius
                .filter(|value| *value >= 0.0)
                .map(|value| value as f32)
                .filter(|value| value.is_finite())
                .map(ShadowRadius::World)
                .or_else(|| bounded(radius, 16.0).map(ShadowRadius::Texels))
                .unwrap_or(default.radius),
            bias: bias.and_then(|value| bounded(value, 0.05)),
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShadowMapPrecision {
    #[default]
    Depth32,
    Depth16,
}

impl ShadowMapPrecision {
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "depth16" => Some(Self::Depth16),
            "depth32" => Some(Self::Depth32),
            _ => None,
        }
    }

    pub fn bytes(self) -> u32 {
        match self {
            Self::Depth32 => 4,
            Self::Depth16 => 2,
        }
    }
}

/// Per-light projection overrides. Receiver sampling is configured independently.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShadowProjectionSettings {
    pub far: Option<f32>,
    pub resolution: Option<u32>,
    pub precision: Option<ShadowMapPrecision>,
    pub cascades: Option<u8>,
    pub splits: [Option<f32>; 3],
    pub blend: Option<f32>,
    pub balance: Option<f32>,
    pub lock_texels: Option<bool>,
}

impl ShadowProjectionSettings {
    pub fn new(
        far: Option<f64>,
        resolution: Option<i64>,
        precision: &str,
        cascades: Option<i64>,
        splits: [Option<f64>; 3],
        blend: Option<f64>,
        balance: Option<f64>,
        lock_texels: Option<bool>,
    ) -> Self {
        let finite = |value: Option<f64>| value.map(|value| value as f32).filter(|v| v.is_finite());
        Self {
            far: finite(far).filter(|value| *value > 0.0),
            resolution: resolution
                .filter(|v| *v > 0)
                .map(|v| v.clamp(128, 4096) as u32),
            precision: ShadowMapPrecision::from_name(precision),
            cascades: cascades.map(|value| value.clamp(1, 4) as u8),
            splits: splits.map(|value| finite(value).filter(|v| *v > 0.0 && *v < 1.0)),
            blend: finite(blend).map(|value| value.clamp(0.0, 1.0)),
            balance: finite(balance).map(|value| value.clamp(0.0, 1.0)),
            lock_texels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SceneLightSource {
    Directional {
        toward_light: [f32; 3],
    },
    Point {
        position: [f32; 3],
        inverse_range_squared: f32,
    },
    Spot {
        position: [f32; 3],
        inverse_range_squared: f32,
        toward_light: [f32; 3],
        cone_scale: f32,
        cone_offset: f32,
        outer_cos: f32,
    },
}

/// Typed input shared by the interpreter and generated Rust bindings.
#[doc(hidden)]
pub struct SceneLightDefinition<'a> {
    pub kind: &'a str,
    pub position: [f64; 3],
    pub yaw: f64,
    pub elevation: f64,
    pub intensity: f64,
    pub color: Color,
    pub range: f64,
    pub inner_angle: f64,
    pub outer_angle: f64,
    pub casts_shadow: bool,
    pub shadow_filter: &'a str,
    pub shadow_strength: f64,
    pub shadow_radius: f64,
    pub shadow_world_radius: Option<f64>,
    pub shadow_bias: Option<f64>,
    pub shadow_projection: ShadowProjectionSettings,
    pub bake_mode: &'a str,
    pub layers: u64,
}

impl SceneLightDefinition<'_> {
    pub fn prepare(self) -> Option<SceneLight> {
        let intensity = self.intensity as f32;
        if !intensity.is_finite() || intensity <= 0.0 {
            return None;
        }
        let radiance = linear_color(self.color).map(|channel| channel * intensity);
        if radiance == [0.0; 3] {
            return None;
        }
        let orientation = || {
            (self.yaw.is_finite() && self.elevation.is_finite())
                .then(|| direction(self.yaw, self.elevation))
        };
        let punctual = || {
            let position = self.position.map(|coordinate| coordinate as f32);
            if !position.into_iter().all(f32::is_finite)
                || !self.range.is_finite()
                || self.range < 0.0
            {
                return None;
            }
            // Zero represents an unbounded light. Store the reciprocal so the
            // shader's finite-range test needs no square root or division.
            let inverse_range_squared = if self.range > 0.0 {
                self.range.recip().powi(2) as f32
            } else {
                0.0
            };
            inverse_range_squared
                .is_finite()
                .then_some((position, inverse_range_squared))
        };
        let source = match self.kind {
            "directional" => SceneLightSource::Directional {
                toward_light: orientation()?,
            },
            "point" => {
                let (position, inverse_range_squared) = punctual()?;
                SceneLightSource::Point {
                    position,
                    inverse_range_squared,
                }
            }
            "spot" => {
                let (position, inverse_range_squared) = punctual()?;
                let (inner, outer) = (self.inner_angle, self.outer_angle);
                if !(0.0..=90.0).contains(&outer) || !(0.0..outer).contains(&inner) {
                    return None;
                }
                let outer_cos = outer.to_radians().cos() as f32;
                let inner_cos = inner.to_radians().cos() as f32;
                let cone_scale = (inner_cos - outer_cos).max(1e-6).recip();
                SceneLightSource::Spot {
                    position,
                    inverse_range_squared,
                    toward_light: orientation()?,
                    cone_scale,
                    cone_offset: -outer_cos * cone_scale,
                    outer_cos,
                }
            }
            _ => return None,
        };
        Some(SceneLight {
            source,
            radiance,
            casts_shadow: self.casts_shadow,
            shadow: ShadowSampling::prepare(
                self.shadow_filter,
                self.shadow_strength,
                self.shadow_radius,
                self.shadow_world_radius,
                self.shadow_bias,
            ),
            shadow_projection: self.shadow_projection,
            bake_mode: LightBakeMode::from_name(self.bake_mode),
            layers: self.layers,
            scope: SceneLightScope::default(),
        })
    }
}

/// Convert the RGB portion of an XRack color to linear light.
pub fn linear_color(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.0.to_be_bytes();
    [r, g, b].map(|encoded| linear_srgb_channel(f32::from(encoded) / 255.0))
}

pub(crate) fn linear_srgb_channel(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

impl SceneLight {
    fn mounted(self, model: &SceneModel, parent: &str) -> Option<Self> {
        let frame = model.node_frame(parent)?;
        let position = |local| {
            let world = frame.origin + frame.linear * Vector3::from(local);
            world
                .iter()
                .all(|value| value.is_finite())
                .then(|| world.into())
        };
        let direction = |local| {
            model
                .rotate_node_direction(frame, Vector3::from(local))
                .into()
        };
        self.transformed(position, direction)
    }

    pub(crate) fn placed(
        self,
        origin: Vector3<f32>,
        rotation: UnitQuaternion<f32>,
    ) -> Option<Self> {
        if !origin.iter().all(|value| value.is_finite()) {
            return None;
        }
        self.transformed(
            |local| Some((origin + rotation * Vector3::from(local)).into()),
            |local| {
                rotation
                    .transform_vector(&Vector3::from(local))
                    .normalize()
                    .into()
            },
        )
    }

    pub(crate) fn mounted_on(
        self,
        origin: Vector3<f32>,
        linear: nalgebra::Matrix3<f32>,
        rotation: UnitQuaternion<f32>,
    ) -> Option<Self> {
        self.transformed(
            |local| {
                let world = origin + linear * Vector3::from(local);
                world
                    .iter()
                    .all(|value| value.is_finite())
                    .then(|| world.into())
            },
            |local| {
                rotation
                    .transform_vector(&Vector3::from(local))
                    .normalize()
                    .into()
            },
        )
    }

    pub(crate) fn is_valid(self) -> bool {
        self.radiance
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
            && match self.source {
                SceneLightSource::Directional { toward_light } => {
                    toward_light.into_iter().all(f32::is_finite) && toward_light != [0.0; 3]
                }
                SceneLightSource::Point {
                    position,
                    inverse_range_squared,
                } => {
                    position.into_iter().all(f32::is_finite)
                        && inverse_range_squared.is_finite()
                        && inverse_range_squared >= 0.0
                }
                SceneLightSource::Spot {
                    position,
                    inverse_range_squared,
                    toward_light,
                    cone_scale,
                    cone_offset,
                    outer_cos,
                } => {
                    position
                        .into_iter()
                        .chain(toward_light)
                        .chain([inverse_range_squared, cone_scale, cone_offset, outer_cos])
                        .all(f32::is_finite)
                        && inverse_range_squared >= 0.0
                        && toward_light != [0.0; 3]
                        && cone_scale > 0.0
                        && (0.0..=1.0).contains(&outer_cos)
                }
            }
    }

    fn transformed(
        self,
        position: impl Fn([f32; 3]) -> Option<[f32; 3]>,
        direction: impl Fn([f32; 3]) -> [f32; 3],
    ) -> Option<Self> {
        let source = match self.source {
            SceneLightSource::Point {
                position: local,
                inverse_range_squared,
            } => SceneLightSource::Point {
                position: position(local)?,
                inverse_range_squared,
            },
            SceneLightSource::Directional { toward_light } => SceneLightSource::Directional {
                toward_light: direction(toward_light),
            },
            SceneLightSource::Spot {
                position: local,
                inverse_range_squared,
                toward_light,
                cone_scale,
                cone_offset,
                outer_cos,
            } => SceneLightSource::Spot {
                position: position(local)?,
                inverse_range_squared,
                toward_light: direction(toward_light),
                cone_scale,
                cone_offset,
                outer_cos,
            },
        };
        Some(Self { source, ..self })
    }
}

/// Light records projected once by both backends. Attached lights stay in
/// local space until the renderer has the viewport's resolved scene model.
#[derive(Debug, PartialEq)]
pub struct SceneLightModel {
    lights: Arc<[SceneLight]>,
    targets: Vec<(usize, String, String)>,
}

impl SceneLightModel {
    pub fn resolve(&self, model: Option<&SceneModel>) -> Arc<[SceneLight]> {
        if self.targets.is_empty() {
            return self.lights.clone();
        }
        let mut targets = self.targets.iter().peekable();
        self.lights
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(index, mut light)| {
                if targets
                    .peek()
                    .is_some_and(|(target, _, _)| *target == index)
                {
                    let (_, parent, scope) = targets.next()?;
                    let model = model?;
                    if !parent.is_empty() {
                        light = light.mounted(model, parent)?;
                    }
                    if !scope.is_empty() {
                        light.scope = model.light_scope(scope)?;
                    }
                }
                Some(light)
            })
            .collect()
    }
}

#[doc(hidden)]
pub fn model_lights<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<(SceneLight, String, String)>,
) -> Arc<SceneLightModel> {
    let mut lights = Vec::new();
    let mut targets = Vec::new();
    for (light, parent, scope) in
        (0..model.len().min(MAX_SCENE_LIGHTS)).filter_map(|index| project(model.row(index)?))
    {
        if !parent.is_empty() || !scope.is_empty() {
            targets.push((lights.len(), parent, scope));
        }
        lights.push(light);
    }
    Arc::new(SceneLightModel {
        lights: lights.into(),
        targets,
    })
}

#[doc(hidden)]
pub fn record_lights(list: &RecordList) -> Arc<SceneLightModel> {
    model_lights(list.model(), |row| {
        if row.field("visible") == Some(&Value::Bool(false)) {
            return None;
        }
        let number = |name| match row.field(name)? {
            Value::Float(value) => Some(*value),
            _ => None,
        };
        let Value::String(kind) = row.field("kind")? else {
            return None;
        };
        let Value::Color(color) = row.field("color")? else {
            return None;
        };
        let text = |field| match row.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        SceneLightDefinition {
            kind,
            position: [number("x")?, number("y")?, number("z")?],
            yaw: number("yaw")?,
            elevation: number("elevation")?,
            intensity: number("intensity")?,
            color: *color,
            range: number("range")?,
            inner_angle: number("inner-angle")?,
            outer_angle: number("outer-angle")?,
            casts_shadow: row.field("casts-shadow") != Some(&Value::Bool(false)),
            shadow_filter: text("shadow-filter"),
            shadow_strength: number("shadow-strength").unwrap_or(ShadowSampling::DEFAULT_STRENGTH),
            shadow_radius: number("shadow-radius").unwrap_or(ShadowSampling::DEFAULT_RADIUS),
            shadow_world_radius: number("shadow-world-radius"),
            shadow_bias: number("shadow-bias"),
            shadow_projection: ShadowProjectionSettings::new(
                number("shadow-map-far"),
                match row.field("shadow-map-size") {
                    Some(Value::Int(size)) => Some(*size),
                    _ => None,
                },
                text("shadow-map-precision"),
                match row.field("shadow-cascades") {
                    Some(Value::Int(count)) => Some(*count),
                    _ => None,
                },
                ["shadow-split-1", "shadow-split-2", "shadow-split-3"].map(number),
                number("shadow-cascade-blend"),
                number("shadow-split-balance"),
                match row.field("shadow-lock-texels") {
                    Some(Value::Bool(lock)) => Some(*lock),
                    _ => None,
                },
            ),
            bake_mode: text("bake-mode"),
            layers: match row.field("layers") {
                Some(Value::Int(layers)) => *layers as u64,
                _ => MAIN_LAYER,
            },
        }
        .prepare()
        .map(|light| (light, text("parent").to_owned(), text("scope").to_owned()))
    })
}

/// The light parameters that affect the contents of a shadow map.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq)]
pub enum ShadowLight {
    Directional([f32; 3]),
    Point {
        position: [f32; 3],
        inverse_range_squared: f32,
    },
    Spot {
        position: [f32; 3],
        toward_light: [f32; 3],
        inverse_range_squared: f32,
        outer_cos: f32,
    },
}

impl ShadowLight {
    pub fn for_light(light: &SceneLight) -> Option<Self> {
        if !light.casts_shadow || light.scope.packed() != 0 {
            return None;
        }
        match light.source {
            SceneLightSource::Directional { toward_light } => Some(Self::Directional(toward_light)),
            SceneLightSource::Spot {
                position,
                toward_light,
                inverse_range_squared,
                outer_cos,
                ..
            } => Some(Self::Spot {
                position,
                toward_light,
                inverse_range_squared,
                outer_cos,
            }),
            SceneLightSource::Point {
                position,
                inverse_range_squared,
            } => Some(Self::Point {
                position,
                inverse_range_squared,
            }),
        }
    }

    /// World-to-light clip rows. Fit the map to scene bounds independently of
    /// the viewing camera so orbiting the viewport reuses the same depth.
    pub fn planar_projection(
        self,
        corners: &[[f32; 3]],
        shadow_far: Option<f32>,
    ) -> Option<[[f32; 4]; 4]> {
        let toward_light = match self {
            Self::Directional(axis) => axis,
            Self::Spot { toward_light, .. } => toward_light,
            Self::Point { .. } => return None,
        };
        let forward = -Vector3::from(toward_light);
        let [right, up] = light_axes(forward);
        match self {
            Self::Directional(_) => {
                let space = DirectionalShadowSpace([right, up, forward]);
                let [mut min, mut max] = space.bounds(corners)?;
                for axis in 0..3 {
                    let padding = ((max[axis] - min[axis]) * 0.02).max(0.005);
                    min[axis] -= padding;
                    max[axis] += padding;
                }
                Some(space.orthographic(min, max))
            }
            Self::Spot {
                position,
                inverse_range_squared,
                outer_cos,
                ..
            } => {
                let eye = Vector3::from(position);
                let mut far = corners
                    .iter()
                    .map(|point| forward.dot(&(Vector3::from(*point) - eye)))
                    .fold(0.0, f32::max)
                    * 1.02;
                if inverse_range_squared > 0.0 {
                    far = far.min(inverse_range_squared.sqrt().recip());
                }
                let far = far.min(shadow_far.unwrap_or(f32::INFINITY));
                if !far.is_finite() || far <= 0.0 {
                    return None;
                }
                let near = (far * 0.005).min(0.02);
                let cosine = outer_cos.clamp(0.001, 0.9999999);
                let focal = cosine / (1.0 - cosine * cosine).sqrt();
                Some(perspective_rows(
                    eye,
                    [right, up, forward],
                    near,
                    far,
                    focal,
                ))
            }
            Self::Point { .. } => None,
        }
    }
}

/// Recover the exact fitted volume from the world-to-shadow clip rows.
#[doc(hidden)]
pub fn shadow_clip_corners(rows: [[f32; 4]; 4]) -> Option<[[f32; 3]; 8]> {
    let matrix = nalgebra::Matrix4::from_row_slice(rows.as_flattened()).cast::<f64>();
    let inverse = matrix.try_inverse()?;
    let corners = std::array::from_fn(|i| {
        let point = inverse
            * nalgebra::Vector4::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { 0.0 } else { 1.0 },
                1.0,
            );
        [point.x, point.y, point.z].map(|value| (value / point.w) as f32)
    });
    corners
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(corners)
}

fn light_axes(forward: Vector3<f32>) -> [Vector3<f32>; 2] {
    let reference = if forward.y.abs() < 0.95 {
        Vector3::y()
    } else {
        Vector3::z()
    };
    let right = forward.cross(&reference).normalize();
    [right, right.cross(&forward)]
}

fn row(axis: Vector3<f32>, shift: f32) -> [f32; 4] {
    [axis.x, axis.y, axis.z, shift]
}

fn perspective_rows(
    eye: Vector3<f32>,
    [right, up, forward]: [Vector3<f32>; 3],
    near: f32,
    far: f32,
    focal: f32,
) -> [[f32; 4]; 4] {
    let depth = far / (far - near);
    let local_row = |axis: Vector3<f32>| row(axis, -axis.dot(&eye));
    let mut z = local_row(forward * depth);
    z[3] -= near * depth;
    [
        local_row(right * focal),
        local_row(up * focal),
        z,
        local_row(forward),
    ]
}

/// Light-space coordinates shared by the whole-scene map and cascades.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DirectionalShadowSpace([Vector3<f32>; 3]);

impl DirectionalShadowSpace {
    pub fn new(toward_light: [f32; 3]) -> Self {
        let forward = -Vector3::from(toward_light);
        let [right, up] = light_axes(forward);
        Self([right, up, forward])
    }

    fn project(self, point: [f32; 3]) -> [f32; 3] {
        let point = Vector3::from(point);
        self.0.map(|axis| axis.dot(&point))
    }

    pub fn bounds(self, points: &[[f32; 3]]) -> Option<[[f32; 3]; 2]> {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for point in points {
            let value = self.project(*point);
            for axis in 0..3 {
                min[axis] = min[axis].min(value[axis]);
                max[axis] = max[axis].max(value[axis]);
            }
        }
        min.into_iter()
            .chain(max)
            .all(f32::is_finite)
            .then_some([min, max])
    }

    fn orthographic(self, min: [f32; 3], max: [f32; 3]) -> [[f32; 4]; 4] {
        let [right, up, forward] = self.0;
        let xy = |axis: Vector3<f32>, i: usize| {
            let width = max[i] - min[i];
            row(axis * (2.0 / width), -(max[i] + min[i]) / width)
        };
        let depth = (max[2] - min[2]).recip();
        [
            xy(right, 0),
            xy(up, 1),
            row(forward * depth, -min[2] * depth),
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    pub fn cascade(
        self,
        camera: &SceneCamera,
        [near, far]: [f32; 2],
        resolution: u32,
        lock_texels: bool,
    ) -> Option<DirectionalShadowCascade> {
        let (corners, center, radius) = camera.frustum_slice(near, far);
        let [mut min, mut max] = self.bounds(&corners)?;
        let resolution = resolution as f32;
        let texel = if lock_texels {
            // A sphere keeps the projection scale unchanged as the camera rotates.
            let radius = radius * (resolution / (resolution - 2.0));
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            let texel = 2.0 * radius / resolution;
            let [x, y, _] = self
                .project(center)
                .map(|value| (value / texel).round() * texel);
            [min[0], min[1]] = [x - radius, y - radius];
            [max[0], max[1]] = [x + radius, y + radius];
            Some(texel)
        } else {
            for axis in 0..2 {
                let padding = ((max[axis] - min[axis]) / (resolution - 2.0)).max(0.005);
                min[axis] -= padding;
                max[axis] += padding;
            }
            None
        };
        Some(DirectionalShadowCascade {
            space: self,
            min,
            max,
            texel,
        })
    }
}

#[doc(hidden)]
pub struct DirectionalShadowCascade {
    space: DirectionalShadowSpace,
    min: [f32; 3],
    max: [f32; 3],
    texel: Option<f32>,
}

impl DirectionalShadowCascade {
    /// Include occluders upstream of the camera slice, including off-screen ones.
    pub fn include_caster(&mut self, bounds: [[f32; 3]; 2]) -> bool {
        if self.overlaps_caster(bounds) {
            self.min[2] = self.min[2].min(bounds[0][2]);
            true
        } else {
            false
        }
    }

    pub fn overlaps_caster(&self, [min, max]: [[f32; 3]; 2]) -> bool {
        min[2] <= self.max[2]
            && (0..2).all(|axis| min[axis] <= self.max[axis] && max[axis] >= self.min[axis])
    }

    pub fn rows(mut self) -> [[f32; 4]; 4] {
        if let Some(texel) = self.texel {
            self.min[2] = ((self.min[2] / texel).floor() - 1.0) * texel;
            self.max[2] = ((self.max[2] / texel).ceil() + 1.0) * texel;
        } else {
            let padding = ((self.max[2] - self.min[2]) * 0.005).max(0.005);
            self.min[2] -= padding;
            self.max[2] += padding;
        }
        self.space.orthographic(self.min, self.max)
    }
}

/// Point light projections share near and far planes across all six faces.
#[doc(hidden)]
pub struct PointShadowProjection {
    position: [f32; 3],
    near: f32,
    far: f32,
}

impl PointShadowProjection {
    pub const FACE_COUNT: usize = 6;
    // Leave room for both taps of the comparison filter at a face seam,
    // including at the minimum 128-pixel shadow resolution.
    const FOCAL: f32 = 63.0 / 64.0;

    pub fn new(
        position: [f32; 3],
        inverse_range_squared: f32,
        scene_corners: &[[f32; 3]],
        shadow_far: Option<f32>,
        caster_bounds: impl Iterator<Item = [[f32; 3]; 2]>,
    ) -> Option<Self> {
        let extent = scene_corners.iter().fold(0.0_f32, |extent, point| {
            (0..3).fold(extent, |extent, axis| {
                extent.max((point[axis] - position[axis]).abs())
            })
        }) * 1.02;
        let range = inverse_range_squared.sqrt().recip();
        let far = extent.min(range).min(shadow_far.unwrap_or(f32::INFINITY));
        if !far.is_finite() || far <= 0.0 {
            return None;
        }
        let closest = caster_bounds
            .map(|[min, max]| {
                (0..3)
                    .map(|axis| {
                        (min[axis] - position[axis])
                            .max(position[axis] - max[axis])
                            .max(0.0)
                    })
                    .fold(0.0_f32, f32::max)
            })
            .fold(f32::INFINITY, f32::min);
        let near = (closest * 0.5).clamp(far * 0.0001, far * 0.1);
        (near > 0.0 && near < far).then_some(Self {
            position,
            near,
            far,
        })
    }

    pub fn near_far(&self) -> [f32; 2] {
        [self.near, self.far]
    }

    /// Faces use +X, -X, +Y, -Y, +Z, -Z order in the CPU and WGSL.
    pub fn rows(&self, face: usize) -> [[f32; 4]; 4] {
        let mut forward = Vector3::zeros();
        forward[face / 2] = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let [right, up] = light_axes(forward);
        perspective_rows(
            self.position.into(),
            [right, up, forward],
            self.near,
            self.far,
            Self::FOCAL,
        )
    }

    /// Test the world AABB against the pyramid and both depth planes.
    pub fn overlaps(&self, face: usize, [min, max]: [[f32; 3]; 2]) -> bool {
        let axis = face / 2;
        let [start, end] = if face.is_multiple_of(2) {
            [
                min[axis] - self.position[axis],
                max[axis] - self.position[axis],
            ]
        } else {
            [
                self.position[axis] - max[axis],
                self.position[axis] - min[axis],
            ]
        };
        let extent = end.min(self.far);
        extent >= self.near
            && start <= self.far
            && (0..3).all(|i| {
                i == axis
                    || (min[i] - self.position[i]).max(self.position[i] - max[i]) * Self::FOCAL
                        <= extent
            })
    }
}

/// Resolve the viewport's explicit and imported lights using the render path's
/// precedence, layer mask and defaults. Used by both drawing and offline baking.
#[doc(hidden)]
pub fn viewport_lighting(
    node: &Node,
    model: Option<&SceneModel>,
    layers: u64,
) -> (Option<Arc<[SceneLight]>>, SceneLight, [f32; 3]) {
    let filter = |lights: Arc<[SceneLight]>| {
        if lights.iter().all(|light| light.layers & layers != 0) {
            lights
        } else {
            lights
                .iter()
                .copied()
                .filter(|light| light.layers & layers != 0)
                .collect()
        }
    };
    let explicit = match node.properties.get("lights") {
        Some(Value::SceneLights(lights)) => Some(filter(lights.resolve(model))),
        _ => None,
    };
    let imported = node
        .boolean("imported-lights", true)
        .then(|| model.and_then(SceneModel::imported_lights))
        .flatten()
        .map(filter);
    let lights = match (explicit, imported) {
        (Some(explicit), Some(imported)) => {
            if explicit.is_empty() {
                Some(imported)
            } else if imported.is_empty() || explicit.len() >= MAX_SCENE_LIGHTS {
                Some(explicit)
            } else {
                Some(
                    explicit
                        .iter()
                        .chain(imported.iter())
                        .copied()
                        .take(MAX_SCENE_LIGHTS)
                        .collect(),
                )
            }
        }
        (explicit, imported) => explicit.or(imported),
    };
    let fallback_direction = if lights.is_some() {
        [0.0; 3]
    } else {
        let angle = |property, default| {
            node.number(property)
                .filter(|angle| angle.is_finite())
                .unwrap_or(default)
        };
        direction(angle("light-yaw", 35.0), angle("light-elevation", 55.0))
    };
    let ambient_intensity = node
        .number("ambient-intensity")
        .map(|intensity| intensity as f32)
        .filter(|intensity| intensity.is_finite())
        .unwrap_or(1.0)
        .max(0.0);
    let fallback = SceneLight {
        source: SceneLightSource::Directional {
            toward_light: fallback_direction,
        },
        radiance: [2.4; 3],
        casts_shadow: true,
        shadow: ShadowSampling::default(),
        shadow_projection: ShadowProjectionSettings::default(),
        bake_mode: LightBakeMode::Disabled,
        layers: u64::MAX,
        scope: SceneLightScope::default(),
    };
    let ambient = linear_color(node.color("ambient-color").unwrap_or(Color(0xffffffff)))
        .map(|channel| channel * ambient_intensity);
    (lights, fallback, ambient)
}
