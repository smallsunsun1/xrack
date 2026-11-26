//! Validate glTF punctual emitters before embedding them in an asset.
use std::collections::BTreeMap;

use serde::Deserialize;
use xrack_runtime::scene3d::{MAX_ANIMATION_NODES, SceneLight, SceneLightSource};
use xrack_syntax::gltf_metadata::LIGHTS_PUNCTUAL;

#[derive(Default, Deserialize)]
struct LightLibrary {
    lights: Vec<Light>,
}

#[derive(Deserialize)]
struct LightReference {
    light: usize,
}

#[derive(Deserialize)]
struct Light {
    #[serde(rename = "type")]
    kind: LightKind,
    #[serde(default = "white")]
    color: [f32; 3],
    #[serde(default = "one")]
    intensity: f32,
    range: Option<f64>,
    spot: Option<Cone>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum LightKind {
    Directional,
    Point,
    Spot,
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Cone {
    inner_cone_angle: f64,
    outer_cone_angle: f64,
}

impl Default for Cone {
    fn default() -> Self {
        Self {
            inner_cone_angle: 0.0,
            outer_cone_angle: std::f64::consts::FRAC_PI_4,
        }
    }
}

fn white() -> [f32; 3] {
    [1.0; 3]
}

fn one() -> f32 {
    1.0
}

impl Light {
    fn prepare(&self) -> Result<SceneLight, String> {
        if !self.intensity.is_finite()
            || self.intensity < 0.0
            || self
                .color
                .into_iter()
                .any(|value| !(0.0..=1.0).contains(&value))
        {
            return Err(
                "requires linear colors in 0..=1 and a finite nonnegative intensity".into(),
            );
        }
        let inverse_range_squared = if let Some(range) = self.range {
            let inverse = range.recip().powi(2) as f32;
            if range <= 0.0 || !range.is_finite() || !inverse.is_finite() {
                return Err(
                    "requires a finite positive range representable in the renderer".into(),
                );
            }
            if matches!(self.kind, LightKind::Directional) {
                return Err("does not allow a range on a directional light".into());
            }
            inverse
        } else {
            0.0
        };
        let toward_light = [0.0, 0.0, 1.0];
        let source = match self.kind {
            LightKind::Directional => SceneLightSource::Directional { toward_light },
            LightKind::Point => SceneLightSource::Point {
                position: [0.0; 3],
                inverse_range_squared,
            },
            LightKind::Spot => {
                let cone = self.spot.as_ref().ok_or("requires spot cone data")?;
                let inner = cone.inner_cone_angle;
                let outer = cone.outer_cone_angle;
                if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&outer)
                    || !(0.0..outer).contains(&inner)
                {
                    return Err("requires 0 <= inner cone < outer cone <= π/2".into());
                }
                let outer_cos = outer.cos().max(0.0) as f32;
                let cone_scale = (inner.cos() as f32 - outer_cos).max(1e-6).recip();
                SceneLightSource::Spot {
                    position: [0.0; 3],
                    inverse_range_squared,
                    toward_light,
                    cone_scale,
                    cone_offset: -outer_cos * cone_scale,
                    outer_cos,
                }
            }
        };
        Ok(SceneLight {
            source,
            radiance: self.color.map(|channel| channel * self.intensity),
            casts_shadow: true,
            shadow: Default::default(),
            shadow_projection: Default::default(),
            bake_mode: Default::default(),
            layers: xrack_runtime::scene3d::MAIN_LAYER,
            scope: Default::default(),
        })
    }
}

pub(super) fn attachments(
    document: &gltf::Document,
) -> Result<BTreeMap<usize, SceneLight>, String> {
    let library = document
        .extension_value(LIGHTS_PUNCTUAL)
        .map(LightLibrary::deserialize)
        .transpose()
        .map_err(|error| format!("glTF {LIGHTS_PUNCTUAL}: {error}"))?
        .unwrap_or_default();
    if library.lights.len() > MAX_ANIMATION_NODES {
        return Err("glTF exceeds the light definition limit".into());
    }
    let lights = library
        .lights
        .iter()
        .enumerate()
        .map(|(index, light)| {
            light
                .prepare()
                .map_err(|error| format!("glTF light #{index} {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut attached = BTreeMap::new();
    for node in document.nodes() {
        let Some(extension) = node.extension_value(LIGHTS_PUNCTUAL) else {
            continue;
        };
        let reference = LightReference::deserialize(extension)
            .map_err(|error| format!("glTF light on node #{}: {error}", node.index()))?;
        let light = lights.get(reference.light).ok_or_else(|| {
            format!(
                "glTF node #{} references missing light #{}",
                node.index(),
                reference.light
            )
        })?;
        attached.insert(node.index(), *light);
    }
    Ok(attached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_punctual_parameters_and_missing_light_references() -> Result<(), String> {
        for value in [
            serde_json::json!({"type":"point","range":0}),
            serde_json::json!({"type":"directional","range":2}),
            serde_json::json!({"type":"spot","spot":{"innerConeAngle":1.2,"outerConeAngle":0.4}}),
        ] {
            let light = Light::deserialize(value).map_err(|error| error.to_string())?;
            assert!(light.prepare().is_err());
        }
        let file = br#"{"asset":{"version":"2.0"},"nodes":[{"extensions":{"KHR_lights_punctual":{"light":2}}}]}"#;
        let parsed =
            gltf::Gltf::from_slice_without_validation(file).map_err(|error| error.to_string())?;
        assert!(attachments(&parsed.document).is_err());
        Ok(())
    }
}
