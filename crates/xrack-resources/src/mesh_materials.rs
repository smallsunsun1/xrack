use crate::{
    assets::{asset_path, local_path, read_asset},
    mesh::{decode_uri, inline_data},
};
use flate2::{Compression, write::ZlibEncoder};
use gltf::{
    Material,
    image::Source,
    material::AlphaMode,
    texture::{MagFilter, MinFilter, WrappingMode},
};
use jpeg_decoder::{Decoder, PixelFormat};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use xrack_runtime::scene3d::{
    MeshAlphaMode, MeshAnisotropy, MeshClearcoat, MeshCullMode, MeshDielectric, MeshImage,
    MeshMaterial, MeshMaterialSlot, MeshNormalMap, MeshPbr, MeshTexture, MeshTransmission,
    MeshVolume, MeshWorkflow, TextureColorSpace, TextureFilter, TextureTransform, TextureWrap,
};
use xrack_syntax::gltf_metadata::{
    ANISOTROPY, CLEARCOAT, DISPERSION, EMISSIVE_STRENGTH, EXTENDED_MATERIALS, GltfMetadata, IOR,
    SPECULAR, SPECULAR_GLOSSINESS, TEXTURE_TRANSFORM, TRANSMISSION, VOLUME,
};

const MAX_DECODED_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_MIP_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct PreparedMaterial {
    pub index: usize,
    pub uv_sets: [Option<u32>; 2],
    pub normal_coordinates: Option<NormalCoordinates>,
}

#[derive(Clone, Copy)]
pub(super) struct NormalCoordinates {
    pub slot: u8,
    pub overrides_source_set: bool,
    pub transform: TextureTransform,
}

struct PreparedClearcoat {
    material: MeshClearcoat,
    authored_normal_uv: Option<u32>,
}

#[derive(Default)]
pub(super) struct MaterialImages {
    images: BTreeMap<usize, Arc<MeshImage>>,
    decoded_bytes: usize,
    mip_bytes: usize,
}

pub(super) struct Materials<'a> {
    document: &'a gltf::Document,
    base: &'a Path,
    declared: Option<&'a BTreeSet<PathBuf>>,
    buffers: &'a [Vec<u8>],
    inputs: &'a mut BTreeSet<PathBuf>,
    images: &'a mut MaterialImages,
    material_indices: BTreeMap<Option<usize>, PreparedMaterial>,
    materials: Vec<MeshMaterial>,
    slots: Vec<MeshMaterialSlot>,
    metadata: &'a GltfMetadata,
}

impl<'a> Materials<'a> {
    pub(super) fn new(
        document: &'a gltf::Document,
        metadata: &'a GltfMetadata,
        base: &'a Path,
        declared: Option<&'a BTreeSet<PathBuf>>,
        buffers: &'a [Vec<u8>],
        inputs: &'a mut BTreeSet<PathBuf>,
        images: &'a mut MaterialImages,
    ) -> Self {
        Self {
            document,
            base,
            declared,
            buffers,
            inputs,
            images,
            material_indices: BTreeMap::new(),
            materials: Vec::new(),
            slots: Vec::new(),
            metadata,
        }
    }

    pub(super) fn finish(&mut self) -> (Vec<MeshMaterial>, Vec<MeshMaterialSlot>) {
        (
            std::mem::take(&mut self.materials),
            std::mem::take(&mut self.slots),
        )
    }

    pub(super) fn is_blended(&self, index: usize) -> bool {
        self.materials[index].is_blended()
    }

    pub(super) fn has_volume(&self, index: usize) -> bool {
        self.materials[index]
            .pbr
            .as_ref()
            .and_then(MeshPbr::volume)
            .is_some()
    }

    pub(super) fn get(&mut self, material: Material<'_>) -> Result<PreparedMaterial, String> {
        let key = material.index();
        if let Some(&index) = self.material_indices.get(&key) {
            return Ok(index);
        }
        let alpha_mode = match material.alpha_mode() {
            AlphaMode::Opaque => MeshAlphaMode::Opaque,
            AlphaMode::Mask => MeshAlphaMode::Mask(material.alpha_cutoff().unwrap_or(0.5)),
            AlphaMode::Blend => MeshAlphaMode::Blend,
        };
        let pbr = material.pbr_metallic_roughness();
        let mut glossy = extension::<SpecularGlossiness>(&material, SPECULAR_GLOSSINESS)?;
        let mut uv_sets = [None, None];
        let (base_color, base_texture) = if let Some(glossy) = &mut glossy {
            if !glossy
                .diffuse_factor
                .into_iter()
                .chain(glossy.specular_factor)
                .chain([glossy.glossiness_factor])
                .all(|value| (0.0..=1.0).contains(&value))
            {
                return Err("glTF specular-glossiness factors must be in 0..=1".into());
            }
            let texture = glossy
                .diffuse_texture
                .take()
                .map(|info| self.texture_info(info, &mut uv_sets, TextureColorSpace::Srgb))
                .transpose()?;
            (glossy.diffuse_factor, texture)
        } else {
            let texture = pbr
                .base_color_texture()
                .map(|info| {
                    self.texture(
                        info.texture(),
                        (info.tex_coord(), info.extension_value(TEXTURE_TRANSFORM)),
                        &mut uv_sets,
                        TextureColorSpace::Srgb,
                    )
                })
                .transpose()?;
            (pbr.base_color_factor(), texture)
        };
        let mut authored_normal_uv = None;
        let shading = if material.unlit() {
            if EXTENDED_MATERIALS
                .into_iter()
                .any(|name| material.extension_value(name).is_some())
            {
                return Err("glTF unlit material cannot use lighting material extensions".into());
            }
            None
        } else {
            let (workflow, metallic, roughness, surface_texture, legacy_dielectric) =
                if let Some(glossy) = glossy {
                    let packed = glossy
                        .specular_glossiness_texture
                        .map(|info| self.texture_info(info, &mut uv_sets, TextureColorSpace::Srgb))
                        .transpose()?;
                    let dielectric = MeshDielectric {
                        ior: MeshDielectric::DEFAULT_IOR,
                        specular: 1.0,
                        color: glossy.specular_factor,
                        specular_texture: None,
                        color_texture: packed.clone(),
                    };
                    (
                        MeshWorkflow::SpecularGlossiness,
                        0.0,
                        1.0 - glossy.glossiness_factor,
                        packed,
                        Some(dielectric),
                    )
                } else {
                    let texture = pbr
                        .metallic_roughness_texture()
                        .map(|info| {
                            self.texture(
                                info.texture(),
                                (info.tex_coord(), info.extension_value(TEXTURE_TRANSFORM)),
                                &mut uv_sets,
                                TextureColorSpace::Linear,
                            )
                        })
                        .transpose()?;
                    (
                        MeshWorkflow::MetallicRoughness,
                        pbr.metallic_factor(),
                        pbr.roughness_factor(),
                        texture,
                        None,
                    )
                };
            let emissive_texture = material
                .emissive_texture()
                .map(|info| {
                    self.texture(
                        info.texture(),
                        (info.tex_coord(), info.extension_value(TEXTURE_TRANSFORM)),
                        &mut uv_sets,
                        TextureColorSpace::Srgb,
                    )
                })
                .transpose()?;
            let occlusion = material.occlusion_texture();
            let occlusion_texture = occlusion
                .as_ref()
                .map(|info| {
                    self.texture(
                        info.texture(),
                        (info.tex_coord(), info.extension_value(TEXTURE_TRANSFORM)),
                        &mut uv_sets,
                        TextureColorSpace::Linear,
                    )
                })
                .transpose()?;
            let normal_map = material
                .normal_texture()
                .map(|info| {
                    authored_normal_uv = Some(info.tex_coord());
                    Ok::<_, String>(MeshNormalMap {
                        texture: self.texture(
                            info.texture(),
                            (info.tex_coord(), info.extension_value(TEXTURE_TRANSFORM)),
                            &mut uv_sets,
                            TextureColorSpace::Linear,
                        )?,
                        scale: info.scale(),
                    })
                })
                .transpose()?;
            let clearcoat = self.clearcoat(&material, &mut uv_sets)?.map(|coat| {
                authored_normal_uv = authored_normal_uv.or(coat.authored_normal_uv);
                coat.material
            });
            Some(MeshPbr {
                anisotropy: self.anisotropy(&material, &mut uv_sets)?.map(Box::new),
                fresnel: Default::default(),
                workflow,
                metallic,
                roughness,
                emissive: emission(&material)?,
                occlusion_strength: occlusion.map_or(1.0, |info| info.strength()),
                surface_texture,
                emissive_texture,
                occlusion_texture,
                normal_map,
                clearcoat,
                dielectric: legacy_dielectric
                    .or(self.dielectric(&material, &mut uv_sets)?)
                    .map(Box::new),
                transmission: self.transmission(&material, &mut uv_sets)?.map(Box::new),
            })
        };
        let index = self.materials.len();
        let normal_coordinates =
            shading
                .as_ref()
                .and_then(MeshPbr::tangent_normal_map)
                .map(|normal| {
                    let slot = normal.texture.uv_slot;
                    NormalCoordinates {
                        slot,
                        overrides_source_set: uv_sets[usize::from(slot)] != authored_normal_uv,
                        transform: normal.texture.transform,
                    }
                });
        let cull_mode =
            if material.double_sided() && shading.as_ref().and_then(MeshPbr::volume).is_none() {
                MeshCullMode::None
            } else {
                MeshCullMode::Back
            };
        self.materials.push(MeshMaterial {
            shader: None,
            channels: Default::default(),
            vertex_colors: true,
            vertex_masks: Default::default(),
            cull_mode,
            depth_draw_mode: Default::default(),
            blend_mode: Default::default(),
            base_color,
            base_texture,
            base_item: None,
            alpha_mode,
            pbr: shading,
        });
        let source_index = key
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "glTF material index exceeds u32")?;
        self.slots.push(MeshMaterialSlot {
            source_index,
            name: key
                .and_then(|index| self.metadata.material_name(index))
                .map(str::to_owned),
        });
        let entry = PreparedMaterial {
            index,
            uv_sets,
            normal_coordinates,
        };
        self.material_indices.insert(key, entry);
        Ok(entry)
    }

    fn clearcoat(
        &mut self,
        material: &Material<'_>,
        uv_sets: &mut [Option<u32>; 2],
    ) -> Result<Option<PreparedClearcoat>, String> {
        let Some(clearcoat) = extension::<Clearcoat>(material, CLEARCOAT)? else {
            return Ok(None);
        };
        if !(0.0..=1.0).contains(&clearcoat.clearcoat_factor)
            || !(0.0..=1.0).contains(&clearcoat.clearcoat_roughness_factor)
        {
            return Err("glTF clearcoat factors must be between 0 and 1".into());
        }
        if clearcoat.clearcoat_factor == 0.0 {
            return Ok(None);
        }
        let mut map = |info| self.texture_info(info, uv_sets, TextureColorSpace::Linear);
        let amount_texture = clearcoat.clearcoat_texture.map(&mut map).transpose()?;
        let roughness_texture = clearcoat
            .clearcoat_roughness_texture
            .map(&mut map)
            .transpose()?;
        let authored_normal_uv = clearcoat
            .clearcoat_normal_texture
            .as_ref()
            .map(|info| info.texture.tex_coord);
        let normal_map = clearcoat
            .clearcoat_normal_texture
            .map(|info| {
                let scale = info.scale.unwrap_or(1.0);
                if !scale.is_finite() {
                    return Err("glTF clearcoat normal scale must be finite".into());
                }
                Ok::<_, String>(MeshNormalMap {
                    texture: map(info.texture)?,
                    scale,
                })
            })
            .transpose()?;
        Ok(Some(PreparedClearcoat {
            material: MeshClearcoat {
                fresnel: Default::default(),
                amount: clearcoat.clearcoat_factor,
                roughness: clearcoat.clearcoat_roughness_factor,
                amount_texture,
                roughness_texture,
                normal_map,
            },
            authored_normal_uv,
        }))
    }

    fn anisotropy(
        &mut self,
        material: &Material<'_>,
        uv_sets: &mut [Option<u32>; 2],
    ) -> Result<Option<MeshAnisotropy>, String> {
        let Some(term) = extension::<Anisotropy>(material, ANISOTROPY)? else {
            return Ok(None);
        };
        if !(0.0..=1.0).contains(&term.anisotropy_strength) || !term.anisotropy_rotation.is_finite()
        {
            return Err("glTF anisotropy requires strength in 0..=1 and finite rotation".into());
        }
        if material.extension_value(SPECULAR_GLOSSINESS).is_some() {
            return Err("glTF anisotropy requires a metallic-roughness material".into());
        }
        let texture = term
            .anisotropy_texture
            .map(|info| self.texture_info(info, uv_sets, TextureColorSpace::Linear))
            .transpose()?;
        Ok(Some(MeshAnisotropy {
            strength: term.anisotropy_strength,
            rotation: term.anisotropy_rotation.rem_euclid(std::f32::consts::TAU),
            texture,
        }))
    }

    fn dielectric(
        &mut self,
        material: &Material<'_>,
        uv_sets: &mut [Option<u32>; 2],
    ) -> Result<Option<MeshDielectric>, String> {
        let ior = extension::<IndexOfRefraction>(material, IOR)?;
        let specular = extension::<Specular>(material, SPECULAR)?;
        if ior.is_none() && specular.is_none() {
            return Ok(None);
        }
        if material.extension_value(SPECULAR_GLOSSINESS).is_some() {
            return Err(
                "glTF IOR and specular extensions require a metallic-roughness material".into(),
            );
        }
        let ior = ior.unwrap_or_default().ior;
        if !MeshDielectric::valid_ior(ior) {
            return Err("glTF KHR_materials_ior requires ior >= 1 or the special value 0".into());
        }
        let specular = specular.unwrap_or_default();
        if !(0.0..=1.0).contains(&specular.factor)
            || specular
                .color
                .into_iter()
                .any(|value| !value.is_finite() || value < 0.0)
        {
            return Err("glTF KHR_materials_specular requires a factor in 0..=1 and finite nonnegative color channels".into());
        }
        let specular_texture = specular
            .texture
            .map(|info| self.texture_info(info, uv_sets, TextureColorSpace::Linear))
            .transpose()?;
        let color_texture = specular
            .color_texture
            .map(|info| self.texture_info(info, uv_sets, TextureColorSpace::Srgb))
            .transpose()?;
        Ok(Some(MeshDielectric {
            ior,
            specular: specular.factor,
            color: specular.color,
            specular_texture,
            color_texture,
        }))
    }

    fn transmission(
        &mut self,
        material: &Material<'_>,
        uv_sets: &mut [Option<u32>; 2],
    ) -> Result<Option<MeshTransmission>, String> {
        let volume = extension::<Volume>(material, VOLUME)?;
        let dispersion = extension::<Dispersion>(material, DISPERSION)?;
        if let Some(term) = &dispersion {
            if !term.dispersion.is_finite() || term.dispersion < 0.0 {
                return Err(
                    "glTF KHR_materials_dispersion requires a finite nonnegative dispersion".into(),
                );
            }
            if volume.is_none() {
                return Err("glTF KHR_materials_dispersion requires KHR_materials_volume".into());
            }
        }
        let dispersion = dispersion.map_or(0.0, |term| term.dispersion);
        let Some(transmission) = extension::<Transmission>(material, TRANSMISSION)? else {
            if volume.is_some() {
                return Err(
                    "glTF KHR_materials_volume requires a supported transmission material".into(),
                );
            }
            return Ok(None);
        };
        if let Some(volume) = &volume
            && (!volume.thickness_factor.is_finite()
                || volume.thickness_factor < 0.0
                || volume
                    .attenuation_distance
                    .is_some_and(|v| !v.is_finite() || v <= 0.0)
                || !volume
                    .attenuation_color
                    .into_iter()
                    .all(|v| (0.0..=1.0).contains(&v)))
        {
            return Err("glTF KHR_materials_volume requires nonnegative thickness, a positive attenuation distance and attenuation colors in 0..=1".into());
        }
        if !(0.0..=1.0).contains(&transmission.transmission_factor) {
            return Err(
                "glTF KHR_materials_transmission requires a transmissionFactor in 0..=1".into(),
            );
        }
        if material.extension_value(SPECULAR_GLOSSINESS).is_some() {
            return Err("glTF transmission requires a metallic-roughness material".into());
        }
        if transmission.transmission_factor == 0.0 {
            return Ok(None);
        }
        let texture = transmission
            .transmission_texture
            .map(|info| self.texture_info(info, uv_sets, TextureColorSpace::Linear))
            .transpose()?;
        let volume = volume
            .filter(|volume| volume.thickness_factor > 0.0)
            .map(|volume| {
                let texture = volume
                    .thickness_texture
                    .map(|info| self.texture_info(info, uv_sets, TextureColorSpace::Linear))
                    .transpose()?;
                Ok::<_, String>(Box::new(MeshVolume {
                    thickness: volume.thickness_factor,
                    thickness_in_prepared_space: false,
                    dispersion,
                    texture,
                    attenuation_distance: volume.attenuation_distance,
                    attenuation_color: volume.attenuation_color,
                }))
            })
            .transpose()?;
        Ok(Some(MeshTransmission {
            factor: transmission.transmission_factor,
            texture,
            volume,
        }))
    }

    fn texture_info(
        &mut self,
        info: TextureInfo,
        uv_sets: &mut [Option<u32>; 2],
        color: TextureColorSpace,
    ) -> Result<MeshTexture, String> {
        let texture = self.document.textures().nth(info.index).ok_or_else(|| {
            format!(
                "glTF material texture index {} is out of bounds",
                info.index
            )
        })?;
        self.texture(
            texture,
            (info.tex_coord, info.extensions.get(TEXTURE_TRANSFORM)),
            uv_sets,
            color,
        )
    }

    fn texture(
        &mut self,
        texture: gltf::Texture<'_>,
        (tex_coords, raw_transform): (u32, Option<&serde_json::Value>),
        uv_sets: &mut [Option<u32>; 2],
        color: TextureColorSpace,
    ) -> Result<MeshTexture, String> {
        let (tex_coords, transform) = texture_coordinates(tex_coords, raw_transform)?;
        let slot = uv_sets
            .iter()
            .position(|set| *set == Some(tex_coords))
            .or_else(|| uv_sets.iter().position(Option::is_none))
            .ok_or("glTF material uses more than two texture coordinate sets")?;
        uv_sets[slot] = Some(tex_coords);
        let sampler = texture.sampler();
        let (min_filter, mip_filter) = match sampler.min_filter() {
            Some(MinFilter::Nearest) => (TextureFilter::Nearest, None),
            Some(MinFilter::Linear) => (TextureFilter::Linear, None),
            Some(MinFilter::NearestMipmapNearest) => {
                (TextureFilter::Nearest, Some(TextureFilter::Nearest))
            }
            Some(MinFilter::LinearMipmapNearest) => {
                (TextureFilter::Linear, Some(TextureFilter::Nearest))
            }
            Some(MinFilter::NearestMipmapLinear) => {
                (TextureFilter::Nearest, Some(TextureFilter::Linear))
            }
            Some(MinFilter::LinearMipmapLinear) | None => {
                (TextureFilter::Linear, Some(TextureFilter::Linear))
            }
        };
        let image = self.image(texture.source())?;
        if mip_filter.is_some() && image.size() != [1, 1] && image.prepared_mips(color).is_none() {
            self.images.mip_bytes += image.mip_byte_len();
            if self.images.mip_bytes > MAX_MIP_BYTES {
                return Err(
                    "glTF material textures exceed 64 MiB of prepared mipmap pixels".into(),
                );
            }
            image.prepare_mips(color);
        }
        Ok(MeshTexture {
            image,
            wrap: [wrap(sampler.wrap_s()), wrap(sampler.wrap_t())],
            min_filter,
            mip_filter,
            mag_filter: match sampler.mag_filter() {
                Some(MagFilter::Nearest) => TextureFilter::Nearest,
                _ => TextureFilter::Linear,
            },
            uv_slot: slot as u8,
            transform,
        })
    }

    fn image(&mut self, image: gltf::Image<'_>) -> Result<Arc<MeshImage>, String> {
        let index = image.index();
        if let Some(image) = self.images.images.get(&index) {
            return Ok(image.clone());
        }
        let pixels = match image.source() {
            Source::View { view, mime_type } => {
                let start = view.offset();
                let end = start + view.length();
                decode(
                    &self.buffers[view.buffer().index()][start..end],
                    Some(mime_type),
                )?
            }
            Source::Uri { uri, mime_type } if uri.starts_with("data:") => {
                decode(&inline_data(uri)?, mime_type)?
            }
            Source::Uri { uri, mime_type } => {
                let error_at_texture = |error| format!("texture {uri:?}: {error}");
                let relative = decode_uri(uri).map_err(error_at_texture)?;
                let path = asset_path(
                    self.base,
                    local_path(&relative).map_err(error_at_texture)?,
                    self.declared,
                    "mesh texture",
                )
                .map_err(error_at_texture)?;
                let bytes = read_asset(&path, "mesh texture").map_err(error_at_texture)?;
                let pixels = decode(&bytes, mime_type).map_err(error_at_texture)?;
                self.inputs.insert(path);
                pixels
            }
        };
        self.images.decoded_bytes += pixels.rgba().len();
        if self.images.decoded_bytes > MAX_DECODED_TEXTURE_BYTES {
            return Err("glTF material textures exceed 64 MiB of decoded pixels".into());
        }
        let pixels = Arc::new(pixels);
        self.images.images.insert(index, pixels.clone());
        Ok(pixels)
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Clearcoat {
    clearcoat_factor: f32,
    clearcoat_roughness_factor: f32,
    clearcoat_texture: Option<TextureInfo>,
    clearcoat_roughness_texture: Option<TextureInfo>,
    clearcoat_normal_texture: Option<NormalTextureInfo>,
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TextureTransformExtension {
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
    tex_coord: Option<u32>,
}

impl Default for TextureTransformExtension {
    fn default() -> Self {
        Self {
            offset: [0.0; 2],
            rotation: 0.0,
            scale: [1.0; 2],
            tex_coord: None,
        }
    }
}

fn texture_coordinates(
    set: u32,
    raw: Option<&serde_json::Value>,
) -> Result<(u32, TextureTransform), String> {
    let Some(raw) = raw else {
        return Ok((set, TextureTransform::IDENTITY));
    };
    let spec = TextureTransformExtension::deserialize(raw)
        .map_err(|error| format!("glTF texture extension {TEXTURE_TRANSFORM}: {error}"))?;
    let [sx, sy] = spec.scale;
    let [u, v] = spec.offset;
    let (sin, cos) = spec.rotation.sin_cos();
    let rows = [[cos * sx, -sin * sy, u], [sin * sx, cos * sy, v]];
    if !rows.into_iter().flatten().all(f32::is_finite) {
        return Err(format!("glTF {TEXTURE_TRANSFORM} values must be finite"));
    }
    Ok((spec.tex_coord.unwrap_or(set), TextureTransform { rows }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextureInfo {
    index: usize,
    #[serde(default)]
    tex_coord: u32,
    #[serde(default)]
    extensions: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct NormalTextureInfo {
    #[serde(flatten)]
    texture: TextureInfo,
    scale: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Dispersion {
    dispersion: f32,
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Volume {
    thickness_factor: f32,
    thickness_texture: Option<TextureInfo>,
    attenuation_distance: Option<f32>,
    attenuation_color: [f32; 3],
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            thickness_factor: 0.0,
            thickness_texture: None,
            attenuation_distance: None,
            attenuation_color: [1.0; 3],
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Transmission {
    transmission_factor: f32,
    transmission_texture: Option<TextureInfo>,
}

#[derive(Deserialize)]
#[serde(default)]
struct IndexOfRefraction {
    ior: f32,
}

impl Default for IndexOfRefraction {
    fn default() -> Self {
        Self {
            ior: MeshDielectric::DEFAULT_IOR,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SpecularGlossiness {
    diffuse_factor: [f32; 4],
    diffuse_texture: Option<TextureInfo>,
    specular_factor: [f32; 3],
    glossiness_factor: f32,
    specular_glossiness_texture: Option<TextureInfo>,
}

impl Default for SpecularGlossiness {
    fn default() -> Self {
        Self {
            diffuse_factor: [1.0; 4],
            diffuse_texture: None,
            specular_factor: [1.0; 3],
            glossiness_factor: 1.0,
            specular_glossiness_texture: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Anisotropy {
    anisotropy_strength: f32,
    anisotropy_rotation: f32,
    anisotropy_texture: Option<TextureInfo>,
}

#[derive(Deserialize)]
#[serde(default)]
struct Specular {
    #[serde(rename = "specularFactor")]
    factor: f32,
    #[serde(rename = "specularColorFactor")]
    color: [f32; 3],
    #[serde(rename = "specularTexture")]
    texture: Option<TextureInfo>,
    #[serde(rename = "specularColorTexture")]
    color_texture: Option<TextureInfo>,
}

impl Default for Specular {
    fn default() -> Self {
        Self {
            factor: 1.0,
            color: [1.0; 3],
            texture: None,
            color_texture: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmissiveStrength {
    #[serde(default = "default_emissive_strength")]
    emissive_strength: f32,
}

fn default_emissive_strength() -> f32 {
    1.0
}

fn extension<T: serde::de::DeserializeOwned>(
    material: &Material<'_>,
    name: &str,
) -> Result<Option<T>, String> {
    material
        .extension_value(name)
        .map(T::deserialize)
        .transpose()
        .map_err(|error| format!("glTF material extension {name}: {error}"))
}

fn emission(material: &Material<'_>) -> Result<[f32; 3], String> {
    let strength = extension::<EmissiveStrength>(material, EMISSIVE_STRENGTH)?
        .map(|extension| extension.emissive_strength)
        .unwrap_or(1.0);
    if !strength.is_finite() || strength < 0.0 {
        return Err("glTF emissive strength must be finite and nonnegative".into());
    }
    let factor = material.emissive_factor();
    if !factor
        .into_iter()
        .all(|channel| (0.0..=1.0).contains(&channel))
    {
        return Err("glTF core emissive factors must be between 0 and 1".into());
    }
    Ok(factor.map(|channel| channel * strength))
}

fn wrap(wrap: WrappingMode) -> TextureWrap {
    match wrap {
        WrappingMode::ClampToEdge => TextureWrap::Clamp,
        WrappingMode::Repeat => TextureWrap::Repeat,
        WrappingMode::MirroredRepeat => TextureWrap::Mirror,
    }
}

pub(crate) fn prepare_scene_texture(bytes: &[u8]) -> Result<MeshImage, String> {
    let image = decode(bytes, None)
        .map_err(|error| error.replace("glTF material textures", "3D textures"))?;
    for color in TextureColorSpace::ALL {
        image.prepare_mips(color);
    }
    Ok(image)
}

fn decode(bytes: &[u8], mime: Option<&str>) -> Result<MeshImage, String> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mime = mime.or_else(|| {
        if bytes.starts_with(PNG_SIGNATURE) {
            Some("image/png")
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            Some("image/jpeg")
        } else {
            None
        }
    });
    let (size, rgba) = match mime {
        Some("image/png") => {
            let pixels = xrack_assets::decode_png(bytes)?;
            (pixels.size, pixels.rgba)
        }
        Some("image/jpeg") => decode_jpeg(bytes)?,
        _ => return Err("glTF material textures require PNG or JPEG".into()),
    };
    MeshImage::new(size, rgba).ok_or_else(|| "invalid glTF texture dimensions".into())
}

fn decode_jpeg(bytes: &[u8]) -> Result<([u32; 2], Vec<u8>), String> {
    let mut decoder = Decoder::new(bytes);
    decoder.set_max_decoding_buffer_size(MAX_DECODED_TEXTURE_BYTES);
    decoder.read_info().map_err(|error| error.to_string())?;
    let info = decoder.info().ok_or("missing JPEG dimensions")?;
    let size = xrack_assets::validate_image_size([u32::from(info.width), u32::from(info.height)])?;
    let pixels = decoder.decode().map_err(|error| error.to_string())?;
    let rgba = match info.pixel_format {
        PixelFormat::L8 => pixels.into_iter().flat_map(|l| [l, l, l, 255]).collect(),
        PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return Err("glTF JPEG textures require RGB or grayscale samples".into()),
    };
    Ok((size, rgba))
}

/// Pack the prepared pixels instead of decoding and filtering source images at AOT startup.
pub fn pack_mesh_image(image: &MeshImage) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    let levels = TextureColorSpace::ALL.map(|color| image.prepared_mips(color));
    let flags = u8::from(levels[0].is_some()) | (u8::from(levels[1].is_some()) << 1);
    encoder
        .write_all(&[flags])
        .and_then(|()| encoder.write_all(image.rgba()))
        .map_err(|error| error.to_string())?;
    for mipmaps in levels.into_iter().flatten() {
        encoder
            .write_all(mipmaps)
            .map_err(|error| error.to_string())?;
    }
    encoder.finish().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dielectric_reflectance_accepts_the_gltf_domain() {
        let mut material = MeshDielectric {
            ior: MeshDielectric::DEFAULT_IOR,
            specular: 1.0,
            color: [1.0; 3],
            specular_texture: None,
            color_texture: None,
        };
        assert_eq!(material.normal_reflectance(), MeshDielectric::DEFAULT_F0);
        material.ior = 1.0;
        assert_eq!(material.normal_reflectance(), 0.0);
        material.ior = 2.42;
        assert!((material.normal_reflectance() - 0.1724).abs() < 0.0001);
        material.ior = 0.0;
        assert!(MeshDielectric::valid_ior(material.ior));
        assert_eq!(material.normal_reflectance(), 1.0);
        for invalid in [0.99, -1.0, f32::INFINITY, f32::NAN] {
            assert!(!MeshDielectric::valid_ior(invalid));
        }
    }

    #[test]
    fn gltf_texture_transform_overrides_uv_and_rejects_invalid_arguments() -> Result<(), String> {
        let transform = serde_json::json!({
            "texCoord": 1, "offset": [0.95, 0.95], "scale": [-0.9, 0.9],
            "rotation": std::f32::consts::FRAC_PI_2,
        });
        let (set, transform) = texture_coordinates(2, Some(&transform))?;
        assert_eq!(set, 1);
        let [x, y] = transform.map([0.25, 0.75]);
        assert!((x - 0.275).abs() < 1e-6 && (y - 0.725).abs() < 1e-6);
        let malformed = serde_json::json!({"offset":[0]});
        assert!(texture_coordinates(0, Some(&malformed)).is_err());
        Ok(())
    }

    #[test]
    fn mipmaps_filter_odd_edges_and_pack_each_color_space_once() -> Result<(), String> {
        let source = [0, 0, 0, 0, 0, 0, 0, 128, 255, 255, 255, 255];
        let image = MeshImage::new([3, 1], source.to_vec()).ok_or("invalid mesh image")?;
        assert_eq!(image.mip_extents().collect::<Vec<_>>(), [[1, 1]]);
        assert_eq!(
            image.prepare_mips(TextureColorSpace::Linear),
            [85, 85, 85, 128]
        );
        let color = image.prepare_mips(TextureColorSpace::Srgb);
        assert_eq!(color, [156, 156, 156, 128]);
        assert!(std::ptr::eq(
            color,
            image.prepare_mips(TextureColorSpace::Srgb)
        ));

        let packed = pack_mesh_image(&image)?;
        let unpacked =
            MeshImage::from_packed(image.size(), &packed).ok_or("invalid packed pixels")?;
        assert_eq!(unpacked.rgba(), source);
        for color in TextureColorSpace::ALL {
            assert_eq!(unpacked.prepared_mips(color), image.prepared_mips(color));
        }
        Ok(())
    }
}
