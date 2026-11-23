//! Lightweight scene asset metadata shared by asset loading and editor completion.
use serde::Deserialize;
use std::{collections::BTreeMap, ops::Range};

pub const TEXTURE_TRANSFORM: &str = "KHR_texture_transform";
pub const LIGHTS_PUNCTUAL: &str = "KHR_lights_punctual";
pub const CLEARCOAT: &str = "KHR_materials_clearcoat";
pub const EMISSIVE_STRENGTH: &str = "KHR_materials_emissive_strength";
pub const IOR: &str = "KHR_materials_ior";
pub const SPECULAR: &str = "KHR_materials_specular";
pub const TRANSMISSION: &str = "KHR_materials_transmission";
pub const VOLUME: &str = "KHR_materials_volume";
pub const DISPERSION: &str = "KHR_materials_dispersion";
pub const EXTENDED_MATERIALS: [&str; 7] = [
    CLEARCOAT,
    EMISSIVE_STRENGTH,
    IOR,
    SPECULAR,
    TRANSMISSION,
    VOLUME,
    DISPERSION,
];

#[derive(Deserialize)]
pub struct GltfMetadata {
    #[serde(default)]
    pub animations: Vec<GltfAnimation>,
    #[serde(default)]
    meshes: Vec<GltfMesh>,
    #[serde(default)]
    materials: Vec<GltfMaterial>,
    #[serde(default)]
    textures: Vec<GltfTexture>,
    #[serde(default)]
    nodes: Vec<GltfNode>,
    #[serde(default)]
    cameras: Vec<GltfCamera>,
    #[serde(default)]
    samplers: Vec<GltfSampler>,
    #[serde(default)]
    extensions: GltfExtensions,
    #[serde(default, rename = "extensionsUsed")]
    extensions_used: Vec<String>,
    #[serde(default, rename = "extensionsRequired")]
    extensions_required: Vec<String>,
}

#[derive(Deserialize)]
struct GltfNode {
    camera: Option<usize>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GltfCamera {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
}

#[derive(Default, Deserialize)]
struct GltfExtensions {
    #[serde(rename = "KHR_lights_punctual")]
    punctual: Option<GltfLightLibrary>,
}

#[derive(Deserialize)]
struct GltfLightLibrary {
    lights: Vec<GltfLight>,
}

#[derive(Deserialize)]
struct GltfLight {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GltfTexture {
    sampler: Option<usize>,
}

#[derive(Deserialize)]
struct GltfSampler {
    #[serde(rename = "minFilter")]
    min_filter: Option<u32>,
}

#[derive(Deserialize)]
struct GltfMaterial {
    #[serde(default)]
    extensions: BTreeMap<String, serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
pub struct GltfAnimation {
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct GltfMesh {
    extras: Option<MeshExtras>,
    #[serde(default)]
    primitives: Vec<GltfPrimitive>,
}

#[derive(Deserialize)]
struct GltfPrimitive {
    #[serde(default)]
    targets: Vec<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MeshExtras {
    Named {
        #[serde(rename = "targetNames")]
        target_names: Vec<String>,
    },
    Other(serde::de::IgnoredAny),
}

impl GltfMetadata {
    pub fn uses_texture_transform(&self) -> bool {
        self.extensions_used
            .iter()
            .chain(&self.extensions_required)
            .any(|name| name == TEXTURE_TRANSFORM)
    }

    /// Imported camera selectors use the original glTF node index. A unique
    /// name may be used alongside that stable selector.
    pub fn camera_nodes(&self) -> Vec<(usize, &'_ str, Option<&'_ str>)> {
        let candidates: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let camera = self.cameras.get(node.camera?)?;
                let alias = node
                    .name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .or(camera.name.as_deref())
                    .filter(|name| {
                        !name.is_empty()
                            && !name.starts_with('#')
                            && !name.contains('/')
                            && !name.chars().any(char::is_control)
                    });
                Some((index, camera.kind.as_str(), alias))
            })
            .collect();
        let mut counts = BTreeMap::new();
        for alias in candidates.iter().filter_map(|(_, _, alias)| *alias) {
            *counts.entry(alias).or_insert(0usize) += 1;
        }
        candidates
            .into_iter()
            .map(|(index, kind, alias)| {
                (
                    index,
                    kind,
                    alias.filter(|alias| counts.get(alias) == Some(&1)),
                )
            })
            .collect()
    }

    pub fn punctual_lights(&self) -> impl Iterator<Item = (usize, &str, Option<&str>)> + '_ {
        self.extensions
            .punctual
            .as_ref()
            .into_iter()
            .flat_map(|library| {
                library
                    .lights
                    .iter()
                    .enumerate()
                    .filter_map(|(index, light)| {
                        let kind = light.kind.as_str();
                        matches!(kind, "directional" | "point" | "spot").then_some((
                            index,
                            kind,
                            light.name.as_deref(),
                        ))
                    })
            })
    }

    pub fn texture_min_filters(&self) -> impl Iterator<Item = (usize, &'static str)> + '_ {
        self.textures
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                let filter = match texture.sampler {
                    Some(sampler) => self.samplers.get(sampler)?.min_filter,
                    None => None,
                };
                let description = match filter {
                    None => "LINEAR_MIPMAP_LINEAR (XRack default)",
                    Some(9728) => "NEAREST",
                    Some(9729) => "LINEAR",
                    Some(9984) => "NEAREST_MIPMAP_NEAREST",
                    Some(9985) => "LINEAR_MIPMAP_NEAREST",
                    Some(9986) => "NEAREST_MIPMAP_LINEAR",
                    Some(9987) => "LINEAR_MIPMAP_LINEAR",
                    _ => return None,
                };
                Some((index, description))
            })
    }

    pub fn material_extensions(&self) -> impl Iterator<Item = &'static str> + '_ {
        EXTENDED_MATERIALS.into_iter().filter(|name| {
            self.materials
                .iter()
                .any(|material| material.extensions.contains_key(*name))
        })
    }

    pub fn mesh_morph_counts(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.meshes.iter().enumerate().filter_map(|(index, mesh)| {
            let count = mesh.primitives.first()?.targets.len();
            (count != 0).then_some((index, count))
        })
    }

    /// In glTF exporters' extras.targetNames convention each name refers to
    /// the same ordinal as its primitive's morph target.
    pub fn morph_target_names(&self, mesh: usize, count: usize) -> Vec<String> {
        let Some(MeshExtras::Named { target_names }) =
            self.meshes.get(mesh).and_then(|mesh| mesh.extras.as_ref())
        else {
            return Vec::new();
        };
        if target_names.len() != count {
            return Vec::new();
        }
        target_names
            .iter()
            .map(|name| {
                if !name.is_empty()
                    && target_names.iter().filter(|other| *other == name).count() == 1
                {
                    name.clone()
                } else {
                    String::new()
                }
            })
            .collect()
    }

    /// Unique names are preferred. Unnamed and ambiguous names use source indices.
    pub fn animation_selectors(&self) -> Vec<String> {
        let mut counts = BTreeMap::new();
        for name in self
            .animations
            .iter()
            .filter_map(|clip| clip.name.as_deref())
        {
            *counts.entry(name).or_insert(0usize) += 1;
        }
        self.animations
            .iter()
            .enumerate()
            .map(|(index, clip)| {
                clip.name
                    .as_deref()
                    .filter(|name| !name.is_empty() && counts.get(name) == Some(&1))
                    .filter(|name| {
                        !name
                            .strip_prefix('#')
                            .is_some_and(|index| index.parse::<usize>().is_ok())
                    })
                    .map_or_else(|| format!("#{index}"), str::to_owned)
            })
            .collect()
    }
}

/// Return the source JSON or the JSON chunk of a GLB 2.0 envelope.
/// The asset loader validates the full glTF document and buffers separately.
pub fn json_chunk(data: &[u8]) -> Option<&[u8]> {
    data.get(json_range(data, data.len())?)
}

/// Locate metadata using only the file header so editors can skip the BIN chunk.
pub fn json_range(header: &[u8], size: usize) -> Option<Range<usize>> {
    if !header.starts_with(b"glTF") {
        return Some(0..size);
    }
    let u32_at = |offset| {
        Some(u32::from_le_bytes(
            header.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    if u32_at(4)? != 2 || u32_at(8)? as usize != size || header.get(16..20)? != b"JSON" {
        return None;
    }
    let end = 20usize.checked_add(u32_at(12)? as usize)?;
    (end <= size).then_some(20..end)
}
