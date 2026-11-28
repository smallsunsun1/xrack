//! Lightweight scene asset metadata shared by asset loading and editor completion.
use serde::Deserialize;
use std::{collections::BTreeMap, ops::Range};

pub const TEXTURE_TRANSFORM: &str = "KHR_texture_transform";
pub const TEXTURE_WEBP: &str = "EXT_texture_webp";
pub const TEXTURE_BASIS: &str = "KHR_texture_basisu";
pub const MESH_QUANTIZATION: &str = "KHR_mesh_quantization";
pub const EXT_MESHOPT_COMPRESSION: &str = "EXT_meshopt_compression";
pub const KHR_MESHOPT_COMPRESSION: &str = "KHR_meshopt_compression";
pub const DRACO_COMPRESSION: &str = "KHR_draco_mesh_compression";
pub const MESH_GPU_INSTANCING: &str = "EXT_mesh_gpu_instancing";
pub const INSTANCE_COLOR_ATTRIBUTE: &str = "_COLOR_0";
pub const INSTANCE_COLOR_ALIAS: &str = "_COLOR";
pub const INSTANCE_DATA_ATTRIBUTE: &str = "_INSTANCE_DATA";
pub const MATERIALS_VARIANTS: &str = "KHR_materials_variants";
pub const LIGHTS_PUNCTUAL: &str = "KHR_lights_punctual";
pub const CLEARCOAT: &str = "KHR_materials_clearcoat";
pub const EMISSIVE_STRENGTH: &str = "KHR_materials_emissive_strength";
pub const IOR: &str = "KHR_materials_ior";
pub const SPECULAR: &str = "KHR_materials_specular";
pub const TRANSMISSION: &str = "KHR_materials_transmission";
pub const VOLUME: &str = "KHR_materials_volume";
pub const DISPERSION: &str = "KHR_materials_dispersion";
pub const ANISOTROPY: &str = "KHR_materials_anisotropy";
pub const SPECULAR_GLOSSINESS: &str = "KHR_materials_pbrSpecularGlossiness";
pub const IRIDESCENCE: &str = "KHR_materials_iridescence";
pub const SHEEN: &str = "KHR_materials_sheen";
pub const EXTENDED_MATERIALS: [&str; 11] = [
    SPECULAR_GLOSSINESS,
    CLEARCOAT,
    EMISSIVE_STRENGTH,
    IOR,
    SPECULAR,
    TRANSMISSION,
    VOLUME,
    DISPERSION,
    ANISOTROPY,
    IRIDESCENCE,
    SHEEN,
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
    scenes: Vec<GltfScene>,
    scene: Option<usize>,
    #[serde(default)]
    accessors: Vec<GltfAccessor>,
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
    mesh: Option<usize>,
    #[serde(default)]
    children: Vec<usize>,
    name: Option<String>,
    #[serde(default)]
    extensions: GltfNodeExtensions,
}

#[derive(Deserialize)]
struct GltfScene {
    #[serde(default)]
    nodes: Vec<usize>,
}

/// The original node address and a path in the selected scene.
pub struct GltfSceneNode<'a> {
    pub source: usize,
    /// Index of the parent in the returned traversal.
    pub parent: Option<usize>,
    pub name: &'a str,
    pub path: String,
    pub has_mesh: bool,
    pub has_camera: bool,
    pub has_light: bool,
}

#[derive(Deserialize)]
struct GltfAccessor {
    count: Option<usize>,
}

#[derive(Default, Deserialize)]
struct GltfNodeExtensions {
    #[serde(rename = "EXT_mesh_gpu_instancing")]
    instances: Option<GltfInstances>,
    #[serde(rename = "KHR_lights_punctual")]
    punctual: Option<GltfNodeLight>,
}

#[derive(Deserialize)]
struct GltfNodeLight {
    light: Option<usize>,
}

#[derive(Deserialize)]
struct GltfInstances {
    attributes: BTreeMap<String, usize>,
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
    #[serde(rename = "KHR_materials_variants")]
    material_variants: Option<GltfVariantLibrary>,
}

#[derive(Deserialize)]
struct GltfVariantLibrary {
    variants: Vec<GltfVariant>,
}

#[derive(Deserialize)]
struct GltfVariant {
    name: String,
}

#[derive(Default, Deserialize)]
struct GltfPrimitiveExtensions {
    #[serde(rename = "KHR_materials_variants")]
    material_variants: Option<GltfVariantMappings>,
}

#[derive(Deserialize)]
struct GltfVariantMappings {
    mappings: Vec<GltfVariantMapping>,
}

#[derive(Deserialize)]
struct GltfVariantMapping {
    material: usize,
    variants: Vec<usize>,
}

#[doc(hidden)]
pub type MaterialVariantMappings = BTreeMap<(usize, usize), usize>;

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
    name: Option<String>,
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
    attributes: GltfPrimitiveAttributes,
    mode: Option<usize>,
    material: Option<usize>,
    #[serde(default)]
    targets: Vec<BTreeMap<String, usize>>,
    #[serde(default)]
    extensions: GltfPrimitiveExtensions,
}

#[derive(Default, Deserialize)]
struct GltfPrimitiveAttributes {
    #[serde(rename = "NORMAL")]
    normal: Option<usize>,
    #[serde(rename = "TANGENT")]
    tangent: Option<usize>,
}

#[derive(Default)]
pub struct GltfPrimitiveSummary {
    /// Number of primitive groups for each of the seven glTF draw modes.
    pub modes: [usize; 7],
    pub point_line_unlit: usize,
    pub point_line_normals: usize,
    pub point_line_tangents: usize,
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
    pub fn primitive_summary(&self) -> GltfPrimitiveSummary {
        let mut summary = GltfPrimitiveSummary::default();
        for primitive in self.meshes.iter().flat_map(|mesh| &mesh.primitives) {
            let mode = primitive.mode.unwrap_or(4);
            if let Some(count) = summary.modes.get_mut(mode) {
                *count += 1;
            }
            if mode < 4 {
                match (primitive.attributes.normal, primitive.attributes.tangent) {
                    (None, _) => summary.point_line_unlit += 1,
                    (Some(_), None) => summary.point_line_normals += 1,
                    (Some(_), Some(_)) => summary.point_line_tangents += 1,
                }
            }
        }
        summary
    }

    pub fn gpu_instance_attributes(
        &self,
    ) -> impl Iterator<Item = (usize, &BTreeMap<String, usize>)> {
        self.nodes.iter().enumerate().filter_map(|(index, node)| {
            Some((index, &node.extensions.instances.as_ref()?.attributes))
        })
    }

    pub fn requires_gpu_instancing(&self) -> bool {
        self.extensions_required
            .iter()
            .any(|name| name == MESH_GPU_INSTANCING)
    }

    pub fn gpu_instance_groups(
        &self,
    ) -> impl Iterator<Item = (usize, Option<&str>, Option<usize>, &BTreeMap<String, usize>)> {
        self.gpu_instance_attributes().map(|(index, attributes)| {
            let count = attributes
                .values()
                .next()
                .and_then(|&index| self.accessors.get(index)?.count)
                .filter(|&count| {
                    attributes
                        .values()
                        .all(|&i| self.accessors.get(i).and_then(|a| a.count) == Some(count))
                });
            (index, self.nodes[index].name.as_deref(), count, attributes)
        })
    }

    pub fn material_variant_names(&self) -> impl ExactSizeIterator<Item = &str> + Clone {
        self.extensions
            .material_variants
            .as_ref()
            .map_or(&[][..], |library| library.variants.as_slice())
            .iter()
            .map(|variant| variant.name.as_str())
    }

    pub fn material_variant_selectors(&self) -> Vec<String> {
        let names = self.material_variant_names();
        let mut counts = BTreeMap::new();
        for name in names.clone() {
            *counts.entry(name).or_insert(0usize) += 1;
        }
        let mut selectors = Vec::with_capacity(2 * names.len().min(64));
        for (index, name) in names.take(64).enumerate() {
            selectors.push(format!("#{index}"));
            if counts.get(name) == Some(&1) && !name.is_empty() && !name.starts_with('#') {
                selectors.push(name.to_owned());
            }
        }
        selectors
    }

    /// Validate the source indices once before preparing the different materials.
    #[doc(hidden)]
    pub fn material_variant_mappings(&self) -> Result<Vec<MaterialVariantMappings>, &'static str> {
        let names = self.material_variant_names();
        if names.len() > 64 {
            return Err("glTF exceeds the 64 material variant limit");
        }
        if self.extensions.material_variants.is_some()
            && (names.len() == 0 || names.clone().any(str::is_empty))
        {
            return Err("glTF material variants require nonempty names");
        }
        if names.len() == 0
            && self
                .extensions_required
                .iter()
                .any(|name| name == MATERIALS_VARIANTS)
        {
            return Err("glTF requires a material variant library");
        }
        let mut variants = vec![BTreeMap::new(); names.len()];
        for (mesh, source) in self.meshes.iter().enumerate() {
            for (primitive, part) in source.primitives.iter().enumerate() {
                let Some(extension) = &part.extensions.material_variants else {
                    continue;
                };
                if extension.mappings.is_empty() {
                    return Err("glTF material variant mappings must be nonempty");
                }
                for mapping in &extension.mappings {
                    if mapping.material >= self.materials.len() || mapping.variants.is_empty() {
                        return Err("invalid glTF material variant mapping");
                    }
                    for &index in &mapping.variants {
                        let variant = variants
                            .get_mut(index)
                            .ok_or("invalid glTF material variant index")?;
                        if variant
                            .insert((mesh, primitive), mapping.material)
                            .is_some()
                        {
                            return Err("glTF primitive maps a material variant more than once");
                        }
                    }
                }
            }
        }
        Ok(variants)
    }

    pub fn material_name(&self, index: usize) -> Option<&str> {
        self.materials
            .get(index)?
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn uses_default_material(&self) -> bool {
        self.meshes
            .iter()
            .flat_map(|mesh| &mesh.primitives)
            .any(|part| part.material.is_none())
    }

    pub fn meshopt_extensions(&self) -> impl Iterator<Item = &'static str> + '_ {
        [EXT_MESHOPT_COMPRESSION, KHR_MESHOPT_COMPRESSION]
            .into_iter()
            .filter(|name| {
                self.extensions_used
                    .iter()
                    .chain(&self.extensions_required)
                    .any(|entry| entry == name)
            })
    }

    pub fn uses_draco_compression(&self) -> bool {
        self.extensions_used
            .iter()
            .any(|name| name == DRACO_COMPRESSION)
    }

    pub fn uses_webp_textures(&self) -> bool {
        self.extensions_used.iter().any(|name| name == TEXTURE_WEBP)
    }

    pub fn uses_basis_textures(&self) -> bool {
        self.extensions_used
            .iter()
            .any(|name| name == TEXTURE_BASIS)
    }

    pub fn requires_mesh_quantization(&self) -> bool {
        self.extensions_required
            .iter()
            .any(|name| name == MESH_QUANTIZATION)
    }

    pub fn morph_texcoord_accessors(
        &self,
        mesh: usize,
        primitive: usize,
        target: usize,
    ) -> impl Iterator<Item = (u32, usize)> + '_ {
        self.meshes
            .get(mesh)
            .and_then(|mesh| mesh.primitives.get(primitive))
            .and_then(|primitive| primitive.targets.get(target))
            .into_iter()
            .flatten()
            .filter_map(|(name, &accessor)| {
                Some((name.strip_prefix("TEXCOORD_")?.parse().ok()?, accessor))
            })
    }

    pub fn uses_texture_transform(&self) -> bool {
        self.extensions_used
            .iter()
            .chain(&self.extensions_required)
            .any(|name| name == TEXTURE_TRANSFORM)
    }

    /// Imported camera selectors use the original glTF node index. A unique
    /// name may be used alongside that stable selector.
    /// Visit only the default scene, the first scene, or the parentless nodes
    /// of an asset without a scene. Parents precede their children.
    pub fn scene_node_addresses(&self) -> Result<Vec<GltfSceneNode<'_>>, &'static str> {
        let roots = if self.scenes.is_empty() {
            let mut children = vec![false; self.nodes.len()];
            for node in &self.nodes {
                for &child in &node.children {
                    *children.get_mut(child).ok_or("invalid glTF child index")? = true;
                }
            }
            (0..self.nodes.len())
                .filter(|&index| !children[index])
                .collect()
        } else {
            self.scenes
                .get(self.scene.unwrap_or(0))
                .ok_or("invalid glTF default scene")?
                .nodes
                .clone()
        };
        let unscened = self.scenes.is_empty();
        let mut pending: Vec<(usize, Option<usize>)> =
            roots.into_iter().rev().map(|root| (root, None)).collect();
        let mut seen = vec![false; self.nodes.len()];
        let mut addresses: Vec<GltfSceneNode<'_>> = Vec::new();
        let mut path_bytes = 0usize;
        while let Some((source, parent)) = pending.pop() {
            let node = self.nodes.get(source).ok_or("invalid glTF node index")?;
            if std::mem::replace(&mut seen[source], true) {
                return Err("invalid recursive or multiply parented glTF node");
            }
            let name = node.name.as_deref().unwrap_or_default();
            // Bound the expanded paths while preserving numeric selectors for
            // every node, including unusually deep or heavily named scenes.
            let prefix = parent.map_or(Some(""), |parent| {
                let path = addresses.get(parent).map(|parent| parent.path.as_str())?;
                (!path.is_empty()).then_some(path)
            });
            let mut path = String::new();
            if let Some(prefix) = prefix.filter(|prefix| prefix.len() + name.len() <= 4096) {
                let segment = if name.is_empty() {
                    format!("#{source}")
                } else {
                    name.replace('~', "~0").replace('/', "~1")
                };
                let length = prefix.len() + 1 + segment.len();
                if length <= 4096 && path_bytes + length <= 16 * 1024 * 1024 {
                    path = format!("{prefix}/{segment}");
                    path_bytes += length;
                }
            }
            let ordinal = addresses.len();
            addresses.push(GltfSceneNode {
                source,
                parent,
                name,
                path,
                has_mesh: node.mesh.is_some(),
                has_camera: node.camera.is_some(),
                has_light: node
                    .extensions
                    .punctual
                    .as_ref()
                    .is_some_and(|light| light.light.is_some()),
            });
            pending.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|&child| (child, Some(ordinal))),
            );
        }
        if unscened && seen.into_iter().any(|seen| !seen) {
            return Err("invalid recursive glTF node graph");
        }
        Ok(addresses)
    }

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
