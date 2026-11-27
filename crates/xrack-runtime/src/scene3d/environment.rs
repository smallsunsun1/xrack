//! Preconditioned equirectangular scene lighting shared by AOT and the interpreter.
use std::{io::Read, sync::Arc};

use crate::{
    Node, RecordList, Value, VecModel,
    scene3d::sky::SkySettings,
    scene3d::{Point3D, Vector3D},
};

pub fn environment_resource_key(url: &str) -> String {
    format!("xrack:environment:{url}")
}

/// Linear RGB9E5 cubemap, mip-major and face-major, with nine convolved
/// spherical harmonic coefficients for diffuse irradiance divided by pi.
pub struct EnvironmentProbe {
    diffuse: [[f32; 3]; 9],
    upper_diffuse: [[f32; 3]; 9],
    texels: Box<[u8]>,
}

impl EnvironmentProbe {
    pub const FACE_SIZE: u32 = 128;
    pub const LEVELS: u32 = Self::FACE_SIZE.ilog2() + 1;
    pub const BYTE_LEN: usize = ((Self::FACE_SIZE * Self::FACE_SIZE * 4 - 1) / 3 * 6 * 4) as usize;

    #[doc(hidden)]
    pub fn new(
        diffuse: [[f32; 3]; 9],
        upper_diffuse: [[f32; 3]; 9],
        texels: Vec<u8>,
    ) -> Option<Self> {
        (texels.len() == Self::BYTE_LEN
            && diffuse
                .iter()
                .chain(&upper_diffuse)
                .flatten()
                .all(|value| value.is_finite()))
        .then(|| Self {
            diffuse,
            upper_diffuse,
            texels: texels.into_boxed_slice(),
        })
    }

    #[doc(hidden)]
    pub fn from_packed(
        diffuse: [[f32; 3]; 9],
        upper_diffuse: [[f32; 3]; 9],
        compressed: &[u8],
    ) -> Option<Self> {
        let mut texels = Vec::with_capacity(Self::BYTE_LEN);
        flate2::read::ZlibDecoder::new(compressed)
            .take(Self::BYTE_LEN as u64 + 1)
            .read_to_end(&mut texels)
            .ok()?;
        Self::new(diffuse, upper_diffuse, texels)
    }

    pub fn diffuse(&self) -> &[[f32; 3]; 9] {
        &self.diffuse
    }

    #[doc(hidden)]
    pub fn upper_diffuse(&self) -> &[[f32; 3]; 9] {
        &self.upper_diffuse
    }

    pub fn texels(&self) -> &[u8] {
        &self.texels
    }
}

impl std::fmt::Debug for EnvironmentProbe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentProbe")
            .field("face_size", &Self::FACE_SIZE)
            .finish()
    }
}

impl PartialEq for EnvironmentProbe {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// The environment selected by a SceneView3D, shared by rendering and baking.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ViewportEnvironment<'a> {
    pub source: EnvironmentSource<'a>,
    pub intensity: f32,
    pub orientation: EnvironmentOrientation,
    pub horizon: f32,
}

/// One world-to-probe convention for skyboxes, IBL and baking.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvironmentOrientation([f32; 3]);

impl EnvironmentOrientation {
    pub fn degrees(yaw_pitch_roll: [f64; 3]) -> Self {
        Self(yaw_pitch_roll.map(|angle| {
            if angle.is_finite() {
                angle.rem_euclid(360.0).to_radians() as f32
            } else {
                0.0
            }
        }))
    }

    #[doc(hidden)]
    pub fn from_radians(yaw_pitch_roll: [f32; 3]) -> Self {
        Self(yaw_pitch_roll)
    }

    #[doc(hidden)]
    pub fn radians(self) -> [f32; 3] {
        self.0
    }

    #[doc(hidden)]
    pub fn rows(self) -> [[f32; 3]; 3] {
        let [yaw, pitch, roll] = self.0;
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let (sr, cr) = roll.sin_cos();
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|[x, y, z]| {
            let (x, z) = (cy * x - sy * z, sy * x + cy * z);
            let (y, z) = (cp * y + sp * z, -sp * y + cp * z);
            [cr * x + sr * y, -sr * x + cr * y, z]
        });
        std::array::from_fn(|row| [axes[0][row], axes[1][row], axes[2][row]])
    }
}

#[derive(Clone, Copy)]
pub enum EnvironmentSource<'a> {
    Panorama(&'a Arc<EnvironmentProbe>),
    Sky(SkySettings),
}

impl<'a> EnvironmentSource<'a> {
    #[doc(hidden)]
    pub fn from_node(node: &'a Node, sky_field: &str, image_field: &str) -> Option<Self> {
        let sky = match node.properties.get(sky_field) {
            Some(Value::Record(record)) => SkySettings::from_record(record),
            _ => None,
        };
        sky.map(EnvironmentSource::Sky).or_else(|| {
            let Some(Value::SceneEnvironment(Some(probe))) = node.properties.get(image_field)
            else {
                return None;
            };
            Some(EnvironmentSource::Panorama(probe))
        })
    }

    #[doc(hidden)]
    pub fn last_mip(self) -> u32 {
        match self {
            Self::Panorama(_) => EnvironmentProbe::LEVELS - 1,
            Self::Sky(sky) => sky.resolution().ilog2(),
        }
    }
}

impl<'a> ViewportEnvironment<'a> {
    pub fn from_node(node: &'a Node) -> Option<Self> {
        let source = EnvironmentSource::from_node(node, "environment-sky", "environment")?;
        Some(Self {
            source,
            intensity: node
                .number("environment-intensity")
                .filter(|value| !value.is_nan())
                .unwrap_or(1.0)
                .clamp(0.0, 100.0) as f32,
            orientation: EnvironmentOrientation::degrees(
                ["environment-yaw", "environment-pitch", "environment-roll"]
                    .map(|field| node.number(field).unwrap_or(0.0)),
            ),
            horizon: node
                .number("environment-horizon")
                .filter(|value| !value.is_nan())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0) as f32,
        })
    }

    pub fn contributes_light(&self) -> bool {
        self.intensity > 0.0
    }
}

const MAX_REFLECTION_PROBES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionRefresh {
    FirstFrame,
    OnChange,
    EveryFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionSlicing {
    None,
    IndividualFaces,
    AllFacesAtOnce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReflectionCapture {
    pub size: u32,
    pub refresh: ReflectionRefresh,
    pub revision: i64,
    pub slicing: ReflectionSlicing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReflectionSource {
    Panorama(Arc<EnvironmentProbe>),
    Scene(ReflectionCapture),
}

impl ReflectionSource {
    #[doc(hidden)]
    pub fn resolve(
        key: &str,
        size: i64,
        refresh: &str,
        revision: i64,
        slicing: &str,
        load: impl FnOnce(&str) -> Option<Arc<EnvironmentProbe>>,
    ) -> Option<Self> {
        if !key.is_empty() {
            return load(key).map(Self::Panorama);
        }
        let refresh = match refresh {
            "first-frame" => ReflectionRefresh::FirstFrame,
            "on-change" | "" => ReflectionRefresh::OnChange,
            "every-frame" => ReflectionRefresh::EveryFrame,
            _ => return None,
        };
        let slicing = match slicing {
            "none" | "" => ReflectionSlicing::None,
            "individual-faces" => ReflectionSlicing::IndividualFaces,
            "all-faces-at-once" => ReflectionSlicing::AllFacesAtOnce,
            _ => return None,
        };
        Some(Self::Scene(ReflectionCapture {
            size: size.clamp(32, 256) as u32,
            refresh,
            revision,
            slicing,
        }))
    }
}

/// An authored HDR reflection source with a world-space influence box.
#[derive(Clone, Debug, PartialEq)]
pub struct ReflectionProbe {
    pub source: ReflectionSource,
    pub position: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub orientation: EnvironmentOrientation,
    pub intensity: f32,
    pub parallax: bool,
    pub debug_view: bool,
    pub layers: u64,
}

impl ReflectionProbe {
    #[doc(hidden)]
    pub fn prepare(
        source: ReflectionSource,
        position: Point3D,
        size: Vector3D,
        offset: Vector3D,
        rotation: [f64; 3],
        intensity: f64,
        parallax: bool,
    ) -> Option<Self> {
        let position = position.narrow()?;
        let size = [size.x, size.y, size.z].map(|axis| axis as f32);
        let offset = [offset.x, offset.y, offset.z].map(|axis| axis as f32);
        let mut min = [0.0; 3];
        let mut max = [0.0; 3];
        for axis in 0..3 {
            let center = position[axis] + offset[axis];
            min[axis] = center - size[axis] * 0.5;
            max[axis] = center + size[axis] * 0.5;
            if !min[axis].is_finite() || !max[axis].is_finite() || min[axis] >= max[axis] {
                return None;
            }
        }
        if !rotation.into_iter().all(f64::is_finite) || !intensity.is_finite() {
            return None;
        }
        Some(Self {
            source,
            position,
            min,
            max,
            orientation: EnvironmentOrientation::degrees(rotation),
            intensity: intensity.clamp(0.0, 100.0) as f32,
            parallax,
            debug_view: false,
            layers: crate::scene3d::MAIN_LAYER,
        })
    }

    #[doc(hidden)]
    pub fn with_layers(mut self, layers: u64) -> Self {
        self.layers = layers;
        self
    }

    #[doc(hidden)]
    pub fn with_debug_view(mut self, debug_view: bool) -> Self {
        self.debug_view = debug_view;
        self
    }

    /// Debug presentation is independent of probe assignment and sampling.
    #[doc(hidden)]
    pub fn same_lighting(&self, other: &Self) -> bool {
        if self.debug_view == other.debug_view {
            self == other
        } else {
            self.clone().with_debug_view(other.debug_view) == *other
        }
    }

    /// Selection uses the model origin, so LOD geometry can share the same probe.
    #[doc(hidden)]
    pub fn nearest(probes: &[Self], point: [f32; 3], layers: u64) -> Option<usize> {
        probes
            .iter()
            .enumerate()
            .filter_map(|(index, probe)| {
                if probe.layers & layers == 0
                    || !(0..3).all(|i| (probe.min[i]..=probe.max[i]).contains(&point[i]))
                {
                    return None;
                }
                let distance = probe
                    .position
                    .into_iter()
                    .zip(point)
                    .map(|(origin, value)| (f64::from(origin) - f64::from(value)).powi(2))
                    .sum::<f64>();
                Some((index, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index)
    }
}

#[doc(hidden)]
pub fn model_reflections<T: Clone + 'static>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<ReflectionProbe>,
) -> Arc<[ReflectionProbe]> {
    (0..model.len().min(MAX_REFLECTION_PROBES))
        .filter_map(|index| project(model.row(index)?))
        .collect()
}

#[doc(hidden)]
pub fn record_reflections(
    list: &RecordList,
    environment: impl Fn(&str) -> Option<Arc<EnvironmentProbe>>,
) -> Arc<[ReflectionProbe]> {
    model_reflections(list.model(), |row| {
        if row.field("visible") == Some(&Value::Bool(false)) {
            return None;
        }
        let text = |name| match row.field(name) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        let integer = |name, default| match row.field(name) {
            Some(Value::Int(value)) => *value,
            _ => default,
        };
        let Value::Point3D(position) = row.field("position")? else {
            return None;
        };
        let Value::Vector3D(size) = row.field("box-size")? else {
            return None;
        };
        let offset = match row.field("box-offset") {
            Some(Value::Vector3D(offset)) => *offset,
            _ => Vector3D::default(),
        };
        let number = |name, default| match row.field(name) {
            Some(Value::Float(number)) => *number,
            _ => default,
        };
        ReflectionProbe::prepare(
            ReflectionSource::resolve(
                text("source"),
                integer("capture-size", 128),
                text("refresh-mode"),
                integer("capture-revision", 0),
                text("time-slicing"),
                &environment,
            )?,
            *position,
            *size,
            offset,
            ["yaw", "pitch", "roll"].map(|field| number(field, 0.0)),
            number("intensity", 1.0),
            row.field("parallax-correction") == Some(&Value::Bool(true)),
        )
        .map(|probe| probe.with_debug_view(row.field("debug-view") == Some(&Value::Bool(true))))
        .map(|probe| match row.field("layers") {
            Some(Value::Int(layers)) => probe.with_layers(*layers as u64),
            _ => probe,
        })
    })
}
