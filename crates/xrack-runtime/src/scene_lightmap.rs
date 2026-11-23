//! Per-instance static irradiance and its atlas region.
use crate::{Image, ImageAsset, ImageFormat, ImageIdentity, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightmapMode {
    /// Replace the diffuse environment contribution. Keep punctual lighting.
    Indirect,
    /// Replace ambient and direct diffuse lighting. Keep live specular.
    Diffuse,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakedLightmap {
    image: Image,
    identity: ImageIdentity,
    atlas: [f32; 4],
    intensity: f32,
    mode: LightmapMode,
}

impl BakedLightmap {
    /// Atlas coordinates are [offset_x, offset_y, scale_x, scale_y], top-left origin.
    pub fn new(image: Image, mode: &str, intensity: f64, atlas: [f64; 4]) -> Option<Self> {
        let asset = ImageAsset::from(image.clone());
        let pixels = asset.pixels()?;
        if pixels.format != ImageFormat::Png || pixels.size.contains(&0) {
            return None;
        }
        let intensity = intensity.clamp(0.0, 1024.0) as f32;
        let [x, y, w, h] = atlas.map(|value| value as f32);
        if !(intensity > 0.0
            && x >= 0.0
            && y >= 0.0
            && w > 0.0
            && h > 0.0
            && x + w <= 1.0
            && y + h <= 1.0)
        {
            return None;
        }
        let mode = match mode {
            "" | "indirect" => LightmapMode::Indirect,
            "diffuse" => LightmapMode::Diffuse,
            _ => return None,
        };
        Some(Self {
            image,
            identity: pixels.identity,
            atlas: [x, y, w, h],
            intensity,
            mode,
        })
    }

    #[doc(hidden)]
    pub fn from_value(value: &Value, mode: &str, intensity: f64, atlas: [f64; 4]) -> Option<Self> {
        let image = match value {
            Value::Image(image) | Value::ImageAsset(ImageAsset::Embedded(image)) => image.clone(),
            Value::ImageAsset(ImageAsset::Loaded(image)) => Image::Loaded(image.clone()),
            _ => return None,
        };
        Self::new(image, mode, intensity, atlas)
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn identity(&self) -> ImageIdentity {
        self.identity
    }

    #[doc(hidden)]
    pub fn instance_words(&self) -> [f32; 8] {
        let [x, y, w, h] = self.atlas;
        let full_diffuse = f32::from(u8::from(self.mode == LightmapMode::Diffuse));
        [x, y, w, h, self.intensity, full_diffuse, 0.0, 0.0]
    }
}

// A world-space hierarchy and each mesh's existing pick tree provide the
// two levels of acceleration used by the static bake.
#[doc(hidden)]
pub struct LightmapTrace<'a> {
    objects: &'a [crate::scene3d::SceneObject],
    tree: crate::scene_bounds::BoundsTree<usize>,
}

impl<'a> LightmapTrace<'a> {
    pub fn new(objects: &'a [crate::scene3d::SceneObject]) -> Self {
        let candidates = objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.world_bounds(), index))
            .collect();
        Self {
            objects,
            tree: crate::scene_bounds::BoundsTree::new(candidates, 4),
        }
    }

    pub fn occluded(&self, ray: crate::scene3d::Ray3D, distance: f32, layers: u64) -> bool {
        let mut blocked = false;
        crate::scene_pick::visit_bounds(
            &self.tree,
            ray.origin(),
            ray.direction,
            0.0,
            distance,
            |index, closest| {
                let object = &self.objects[*index];
                if !blocked
                    && object.casts_shadow
                    && object.layers & layers != 0
                    && object.bake_occludes(ray, closest)
                {
                    blocked = true;
                    return 0.0;
                }
                closest
            },
        );
        blocked
    }

    pub fn trace(
        &self,
        ray: crate::scene3d::Ray3D,
        distance: f32,
        layers: u64,
    ) -> Option<crate::scene3d::BakedSurface> {
        let mut result = None;
        crate::scene_pick::visit_bounds(
            &self.tree,
            ray.origin(),
            ray.direction,
            0.0,
            distance,
            |index, closest| {
                let object = &self.objects[*index];
                if object.layers & layers == 0 {
                    return closest;
                }
                if let Some((hit, surface)) = object.bake_hit(ray, 0.0, closest) {
                    if hit < closest
                        || result
                            .as_ref()
                            .is_none_or(|(previous, _)| *index < *previous)
                    {
                        result = Some((*index, surface));
                        return hit;
                    }
                }
                closest
            },
        );
        result.map(|(_, surface)| surface)
    }
}
