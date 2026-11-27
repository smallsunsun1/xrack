use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use nalgebra::{Vector2, Vector3};
use xrack_runtime::scene3d::{
    MAX_IMPORTED_VERTICES, Mesh3D, MeshAlphaMode, MeshAsset, MeshImage, MeshMaterial,
    MeshMaterialSlot, MeshPart, MeshPbr, MeshTexture, MeshVertex, TextureColorSpace, TextureFilter,
    TextureSampling, TextureTransform, TextureWrap,
};
use xrack_syntax::wavefront::directives;

use xrack_assets::files::{asset_path, local_path, read_asset};

const MAX_FACE_CORNERS: usize = 64;
const MAX_LIBRARIES: usize = 32;
const MAX_MATERIALS: usize = 1024;
const TEXTURE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Corner {
    position: usize,
    uv: Option<usize>,
    normal: Option<usize>,
}

struct Triangle {
    corners: [Corner; 3],
    normal: Vector3<f64>,
    smoothing: u32,
    material: usize,
}

#[derive(Default)]
struct Geometry<'a> {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<Triangle>,
    smooth: HashMap<(u32, usize), Vector3<f64>>,
    materials: Vec<Option<&'a str>>,
    slots: HashMap<Option<&'a str>, usize>,
    libraries: Vec<(usize, &'a str)>,
}

pub(super) fn load(
    path: &Path,
    declared: Option<&BTreeSet<PathBuf>>,
) -> Result<(MeshAsset, BTreeSet<PathBuf>), String> {
    let bytes = read_asset(path, "OBJ")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "OBJ must be UTF-8")?;
    let geometry = Geometry::parse(text).map_err(|err| format!("{}:{err}", path.display()))?;
    let mut loader = MaterialLoader {
        declared,
        inputs: BTreeSet::from([path.to_owned()]),
        definitions: BTreeMap::new(),
        images: BTreeMap::new(),
        texture_bytes: 0,
    };
    let base = path.parent().ok_or("OBJ has no parent directory")?;
    for (line, library) in &geometry.libraries {
        loader.read_library(base, library).map_err(|err| {
            format!(
                "{}:{line}: material library {library:?}: {err}",
                path.display()
            )
        })?;
    }
    let materials = geometry
        .materials
        .iter()
        .map(|slot| loader.material(*slot))
        .collect::<Result<Vec<_>, _>>()?;
    let mesh = geometry.finish(materials)?;
    Ok((MeshAsset::Static(mesh), loader.inputs))
}

impl<'a> Geometry<'a> {
    fn parse(text: &'a str) -> Result<Self, String> {
        let mut geometry = Self::default();
        let mut material = None;
        let mut smoothing = 0;
        let mut corners = Vec::with_capacity(4);
        for (line, command, value) in directives(text) {
            let read = || format!("{line}: invalid OBJ {command} directive");
            match command {
                "v" => geometry.positions.push(numbers(value).ok_or_else(read)?),
                "vt" => {
                    let mut values = value.split_whitespace();
                    let u = number(values.next()).ok_or_else(read)?;
                    let v = values
                        .next()
                        .map_or(Some(0.0), |v| number(Some(v)))
                        .ok_or_else(read)?;
                    geometry.uvs.push([u, 1.0 - v]);
                }
                "vn" => geometry.normals.push(
                    Vector3::from(numbers::<3>(value).ok_or_else(read)?)
                        .cast::<f64>()
                        .try_normalize(0.0)
                        .ok_or_else(read)?
                        .cast::<f32>()
                        .into(),
                ),
                "f" => geometry
                    .face(value, smoothing, material, &mut corners)
                    .map_err(|err| format!("{line}: {err}"))?,
                "usemtl" => material = Some(value).filter(|value| !value.is_empty()),
                "mtllib" => {
                    for library in value.split_whitespace() {
                        if geometry.libraries.len() == MAX_LIBRARIES {
                            return Err(format!("{line}: OBJ exceeds the material library limit"));
                        }
                        geometry.libraries.push((line, library));
                    }
                }
                "s" => {
                    smoothing = match value {
                        "off" => 0,
                        "on" => 1,
                        _ => value.parse().map_err(|_| read())?,
                    }
                }
                _ => {}
            }
            if [
                geometry.positions.len(),
                geometry.uvs.len(),
                geometry.normals.len(),
            ]
            .into_iter()
            .any(|count| count > MAX_IMPORTED_VERTICES)
            {
                return Err(format!("{line}: OBJ exceeds the source vertex limit"));
            }
        }
        if geometry.triangles.is_empty() {
            return Err("OBJ contains no polygon faces".into());
        }
        Ok(geometry)
    }

    fn face(
        &mut self,
        source: &str,
        smoothing: u32,
        material: Option<&'a str>,
        corners: &mut Vec<Corner>,
    ) -> Result<(), String> {
        corners.clear();
        for value in source.split_whitespace() {
            if corners.len() == MAX_FACE_CORNERS {
                return Err("OBJ faces are limited to 64 corners".into());
            }
            let mut fields = value.split('/');
            let position = index(fields.next(), self.positions.len())
                .ok_or("OBJ position index is out of bounds")?;
            let uv = optional_index(fields.next(), self.uvs.len())
                .ok_or("OBJ texture coordinate index is out of bounds")?;
            let normal = optional_index(fields.next(), self.normals.len())
                .ok_or("OBJ normal index is out of bounds")?;
            if fields.next().is_some() {
                return Err("expected an OBJ position/uv/normal reference".into());
            }
            corners.push(Corner {
                position,
                uv,
                normal,
            });
        }
        let count = corners.len();
        if count < 3 {
            return Err("OBJ face requires at least three corners".into());
        }
        if (self.triangles.len() + count - 2) * 3 > MAX_IMPORTED_VERTICES {
            return Err("OBJ exceeds the expanded vertex limit".into());
        }
        let slot = *self.slots.entry(material).or_insert_with(|| {
            let slot = self.materials.len();
            self.materials.push(material);
            slot
        });
        if slot >= MAX_MATERIALS {
            return Err("OBJ exceeds the material slot limit".into());
        }
        let point = |corner: &Corner| Vector3::from(self.positions[corner.position]).cast::<f64>();
        let origin = point(&corners[0]);
        let normal = corners[1..]
            .windows(2)
            .map(|edge| (point(&edge[0]) - origin).cross(&(point(&edge[1]) - origin)))
            .sum::<Vector3<f64>>();
        let face_normal = normal.try_normalize(0.0).ok_or("degenerate OBJ face")?;
        if smoothing != 0 {
            for corner in corners.iter() {
                *self.smooth.entry((smoothing, corner.position)).or_default() += normal;
            }
        }
        let mut emit = |indices: [usize; 3]| {
            self.triangles.push(Triangle {
                corners: indices.map(|index| corners[index]),
                normal: face_normal,
                smoothing,
                material: slot,
            })
        };
        if count == 3 {
            emit([0, 1, 2]);
        } else {
            triangulate(corners.iter().map(point), normal, emit)?;
        }
        Ok(())
    }

    fn finish(&self, materials: Vec<MeshMaterial>) -> Result<Mesh3D, String> {
        let mut parts: Vec<MeshPart> = Vec::new();
        let mut vertices = Vec::with_capacity(self.triangles.len() * 3);
        for triangle in &self.triangles {
            if let Some(part) = parts
                .last_mut()
                .filter(|part| part.material == triangle.material)
            {
                part.vertices.end += 3;
            } else {
                parts.push(MeshPart {
                    vertices: vertices.len()..vertices.len() + 3,
                    material: triangle.material,
                    volume_inverse: None,
                });
            }
            vertices.extend(triangle.corners.map(|corner| {
                let normal = corner.normal.map_or_else(
                    || {
                        self.smooth
                            .get(&(triangle.smoothing, corner.position))
                            .and_then(|smooth| smooth.try_normalize(0.0))
                            .unwrap_or(triangle.normal)
                            .cast::<f32>()
                            .into()
                    },
                    |index| self.normals[index],
                );
                MeshVertex {
                    position: self.positions[corner.position],
                    normal,
                    uv: corner.uv.map_or([0.0; 2], |index| self.uvs[index]),
                    uv1: [0.0; 2],
                    color: [1.0; 4],
                }
            }));
        }
        let mut next = 0;
        let slots = self
            .materials
            .iter()
            .map(|name| {
                let source_index = name.map(|_| {
                    let index = next;
                    next += 1;
                    index
                });
                MeshMaterialSlot {
                    source_index,
                    name: name.map(str::to_owned),
                }
            })
            .collect::<Arc<[_]>>();
        Mesh3D::imported(vertices, Vec::new(), parts, materials)
            .map_err(str::to_owned)?
            .with_material_slots(slots)
            .ok_or_else(|| "could not assign OBJ material slots".into())
    }
}

fn number(value: Option<&str>) -> Option<f32> {
    let value: f32 = value?.parse().ok()?;
    value.is_finite().then_some(value)
}

fn numbers<const N: usize>(value: &str) -> Option<[f32; N]> {
    let mut words = value.split_whitespace();
    let mut result = [0.0; N];
    for element in &mut result {
        *element = number(words.next())?;
    }
    Some(result)
}

fn index(value: Option<&str>, count: usize) -> Option<usize> {
    let value: i64 = value?.parse().ok()?;
    let index = if value < 0 {
        count as i64 + value
    } else {
        value.checked_sub(1)?
    };
    usize::try_from(index).ok().filter(|&index| index < count)
}

fn optional_index(value: Option<&str>, count: usize) -> Option<Option<usize>> {
    match value {
        None | Some("") => Some(None),
        value => index(value, count).map(Some),
    }
}

// Project onto the plane with maximal signed area, then remove ears in file
// winding order. An interior notch must remain empty for rendering and picking.
fn triangulate(
    points: impl ExactSizeIterator<Item = Vector3<f64>>,
    normal: Vector3<f64>,
    mut emit: impl FnMut([usize; 3]),
) -> Result<(), String> {
    let axis = normal.iamax();
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let orientation = normal[axis].signum();
    let mut projected = [Vector2::<f64>::zeros(); MAX_FACE_CORNERS];
    let count = points.len();
    for (output, point) in projected.iter_mut().zip(points) {
        *output = Vector2::new(point[u], point[v]);
    }
    let turn = |a: usize, b: usize, c: usize| {
        let ab = projected[b] - projected[a];
        let ac = projected[c] - projected[a];
        orientation * (ab.x * ac.y - ab.y * ac.x)
    };
    let mut ring: Vec<_> = (0..count).collect();
    while ring.len() > 3 {
        let count = ring.len();
        let ear = (0..count)
            .find_map(|index| {
                let a = ring[(index + count - 1) % count];
                let b = ring[index];
                let c = ring[(index + 1) % count];
                (turn(a, b, c) > 0.0
                    && !ring.iter().copied().any(|p| {
                        p != a
                            && p != b
                            && p != c
                            && turn(a, b, p) >= 0.0
                            && turn(b, c, p) >= 0.0
                            && turn(c, a, p) >= 0.0
                    }))
                .then_some((index, [a, b, c]))
            })
            .ok_or("could not triangulate OBJ face")?;
        emit(ear.1);
        ring.remove(ear.0);
    }
    let face = [ring[0], ring[1], ring[2]];
    if turn(face[0], face[1], face[2]) <= 0.0 {
        return Err("degenerate OBJ polygon".into());
    }
    emit(face);
    Ok(())
}

struct MaterialLoader<'a> {
    declared: Option<&'a BTreeSet<PathBuf>>,
    inputs: BTreeSet<PathBuf>,
    definitions: BTreeMap<String, Material>,
    images: BTreeMap<PathBuf, (Arc<MeshImage>, bool)>,
    texture_bytes: usize,
}

struct Material {
    color: [f32; 3],
    opacity: f32,
    roughness: f32,
    emissive: [f32; 3],
    unlit: bool,
    map: Option<DiffuseMap>,
}

#[derive(Clone)]
struct DiffuseMap {
    directory: PathBuf,
    source: String,
    clamp: bool,
    transform: TextureTransform,
}

impl DiffuseMap {
    fn parse(directory: &Path, mut source: &str) -> Result<Self, String> {
        let mut scale = [1.0; 3];
        let mut offset = [0.0; 3];
        let mut clamp = false;
        while source.starts_with('-') {
            let (option, rest) = word(source);
            source = rest;
            match option {
                "-clamp" => {
                    let (value, rest) = word(source);
                    clamp = match value {
                        "on" => true,
                        "off" => false,
                        _ => return Err("MTL -clamp requires on or off".into()),
                    };
                    source = rest;
                }
                "-s" | "-o" => {
                    let output = if option == "-s" {
                        &mut scale
                    } else {
                        &mut offset
                    };
                    let mut count = 0;
                    for coordinate in output {
                        let (value, rest) = word(source);
                        let Some(value) = number(Some(value)) else {
                            break;
                        };
                        *coordinate = value;
                        source = rest;
                        count += 1;
                    }
                    if count == 0 {
                        return Err(format!("MTL {option} requires texture coordinates"));
                    }
                }
                _ => return Err(format!("unsupported MTL map_Kd option {option:?}")),
            }
        }
        let source = source.trim_matches('"');
        if source.is_empty() {
            return Err("MTL map_Kd requires a texture path".into());
        }
        // OBJ's V coordinate runs from the bottom of an image.
        let offset_v = 1.0 - scale[1] - offset[1];
        if !offset_v.is_finite() {
            return Err("invalid MTL V offset".into());
        }
        Ok(Self {
            directory: directory.to_owned(),
            source: source.to_owned(),
            clamp,
            transform: TextureTransform {
                rows: [[scale[0], 0.0, offset[0]], [0.0, scale[1], offset_v]],
            },
        })
    }
}

fn word(source: &str) -> (&str, &str) {
    let (head, rest) = source
        .split_once(char::is_whitespace)
        .unwrap_or((source, ""));
    (head, rest.trim_start())
}

impl Default for Material {
    fn default() -> Self {
        Self {
            color: [1.0; 3],
            opacity: 1.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            unlit: false,
            map: None,
        }
    }
}

impl MaterialLoader<'_> {
    fn read_library(&mut self, base: &Path, name: &str) -> Result<(), String> {
        let path = asset_path(
            base,
            local_path(name)?,
            self.declared,
            "OBJ material library",
        )?;
        if !self.inputs.insert(path.clone()) {
            return Ok(());
        }
        let bytes = read_asset(&path, "MTL")?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "MTL must be UTF-8")?;
        let directory = path.parent().ok_or("MTL has no parent directory")?;
        let mut current = None;
        for (line, command, value) in directives(text) {
            if command == "newmtl" {
                if self.definitions.len() == MAX_MATERIALS || value.is_empty() {
                    return Err(format!(
                        "{}:{line}: invalid or excess material definition",
                        path.display()
                    ));
                }
                self.definitions
                    .insert(value.to_owned(), Material::default());
                current = Some(value);
                continue;
            }
            let Some(material) = current.and_then(|name| self.definitions.get_mut(name)) else {
                continue;
            };
            let error = || format!("{}:{line}: invalid MTL {command} value", path.display());
            let scalar = || number(value.split_whitespace().next()).ok_or_else(error);
            match command {
                "Kd" => {
                    material.color = numbers::<3>(value)
                        .ok_or_else(error)?
                        .map(|c| c.clamp(0.0, 1.0))
                }
                "d" => material.opacity = scalar()?.clamp(0.0, 1.0),
                "Tr" => material.opacity = 1.0 - scalar()?.clamp(0.0, 1.0),
                "Ke" => {
                    material.emissive = numbers::<3>(value).ok_or_else(error)?.map(|c| c.max(0.0))
                }
                "Ns" => material.roughness = (2.0 / (scalar()?.max(0.0) + 2.0)).sqrt(),
                "illum" => material.unlit = value == "0",
                "map_Kd" => {
                    material.map = Some(
                        DiffuseMap::parse(directory, value)
                            .map_err(|error| format!("{}:{line}: {error}", path.display()))?,
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn material(&mut self, slot: Option<&str>) -> Result<MeshMaterial, String> {
        let default = Material::default();
        let material = slot
            .and_then(|slot| self.definitions.get(slot))
            .unwrap_or(&default);
        let [r, g, b] = material.color;
        let base_color = [r, g, b, material.opacity];
        let pbr = (!material.unlit).then(|| MeshPbr {
            workflow: Default::default(),
            anisotropy: None,
            fresnel: Default::default(),
            metallic: 0.0,
            roughness: material.roughness,
            emissive: material.emissive,
            occlusion_strength: 1.0,
            surface_texture: None,
            separate_surface_maps: None,
            specular_reflection: None,
            height_map: None,
            emissive_texture: None,
            occlusion_texture: None,
            normal_map: None,
            clearcoat: None,
            dielectric: None,
            transmission: None,
        });
        let map = material.map.clone();
        let (base_texture, image_transparent) = if let Some(map) = map {
            let image = self.image(&map.directory, &map.source)?;
            let wrap = if map.clamp {
                TextureWrap::Clamp
            } else {
                TextureWrap::Repeat
            };
            (
                Some(MeshTexture {
                    image: image.0,
                    sampling: TextureSampling {
                        wrap: [wrap; 2],
                        min_filter: TextureFilter::Linear,
                        mag_filter: TextureFilter::Linear,
                        mip_filter: Some(TextureFilter::Linear),
                        max_anisotropy: 1,
                    },
                    uv_slot: 0,
                    transform: map.transform,
                }),
                image.1,
            )
        } else {
            (None, false)
        };
        let transparent = base_color[3] < 1.0 || image_transparent;
        Ok(MeshMaterial {
            opacity: 1.0,
            opacity_texture: None,
            invert_opacity_map_value: false,
            base_color,
            base_texture,
            base_item: None,
            channels: Default::default(),
            vertex_colors: false,
            vertex_masks: Default::default(),
            alpha_mode: if transparent {
                MeshAlphaMode::Blend
            } else {
                MeshAlphaMode::Opaque
            },
            blend_mode: Default::default(),
            depth_draw_mode: Default::default(),
            cull_mode: Default::default(),
            pbr,
            shader: None,
        })
    }

    fn image(&mut self, base: &Path, source: &str) -> Result<(Arc<MeshImage>, bool), String> {
        let load = || {
            asset_path(
                base,
                local_path(source)?,
                self.declared,
                "OBJ diffuse texture",
            )
        };
        let error = |err: String| format!("OBJ diffuse texture {source:?}: {err}");
        let path = load().map_err(error)?;
        if let Some(image) = self.images.get(&path) {
            return Ok(image.clone());
        }
        let bytes = read_asset(&path, "OBJ diffuse texture").map_err(error)?;
        let image = Arc::new(crate::mesh_materials::decode(&bytes, None).map_err(error)?);
        self.texture_bytes += image.rgba().len() + image.mip_byte_len();
        if self.texture_bytes > TEXTURE_BYTES {
            return Err("OBJ diffuse textures exceed 64 MiB of prepared pixels".into());
        }
        image.prepare_mips(TextureColorSpace::Srgb);
        self.inputs.insert(path.clone());
        let transparent = image
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] < 255);
        let ready = (image, transparent);
        self.images.insert(path, ready.clone());
        Ok(ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_references_and_degenerate_faces_keep_the_obj_line() {
        let coordinates = "v 0 0 0\nv 1 0 0\nv 0 1 0\n";
        for (face, expected) in [
            ("f 0 2 3", "position index"),
            ("f -4 2 3", "position index"),
            ("f 1/1 2 3", "texture coordinate index"),
            ("f 1//1 2 3", "normal index"),
            ("f 1 1 3", "degenerate OBJ face"),
        ] {
            let source = format!("{coordinates}{face}");
            let error = Geometry::parse(&source).err().expect("invalid face");
            assert!(
                error.starts_with("4:") && error.contains(expected),
                "{error}"
            );
        }
    }

    #[test]
    fn flat_and_smooth_faces_resolve_distinct_normals_on_the_same_edge() {
        let source = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\ns 1\nf 1 2 3\nf 1 4 2\ns off\nf 1 3 4";
        let parsed = Geometry::parse(source).expect("valid OBJ");
        let material = MaterialLoader {
            declared: None,
            inputs: BTreeSet::new(),
            definitions: BTreeMap::new(),
            images: BTreeMap::new(),
            texture_bytes: 0,
        }
        .material(None)
        .expect("default material");
        let mesh = parsed
            .finish(vec![material])
            .expect("complete OBJ triangles");
        let vertices = mesh.vertices();
        assert_eq!(vertices[0].normal, [0.0, 0.5f32.sqrt(), 0.5f32.sqrt()]);
        assert_eq!(vertices[3].normal, vertices[0].normal);
        assert_eq!(vertices[6].normal, [1.0, 0.0, 0.0]);
    }
}
