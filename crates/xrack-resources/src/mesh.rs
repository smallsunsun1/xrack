use crate::{
    assets::{asset_path, local_path, read_asset},
    mesh_animation, mesh_instances, mesh_lights,
    mesh_materials::{MaterialImages, Materials, NormalCoordinates},
    mesh_morph::{self, MorphData},
    mesh_skin, mesh_tangents,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gltf::{
    Accessor, Primitive,
    accessor::{DataType, Dimensions},
    buffer::Source,
    mesh::{Mode, Semantic},
};
use nalgebra::{Matrix4, Point3, Vector3};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use xrack_assets::MAX_ASSET_BYTES;
use xrack_runtime::scene3d::{
    AnimatedMesh, MAX_ANIMATION_NODES, MAX_IMPORTED_VERTICES, MAX_MORPH_DELTAS, MAX_MORPH_TARGETS,
    Mesh3D, MeshAnimationNode, MeshAsset, MeshCamera, MeshCameraProjection, MeshInfluence,
    MeshInstances, MeshLight, MeshNodeTransform, MeshPart, MeshTangent, MeshVertex,
    TextureTransform,
};
use xrack_syntax::gltf_metadata::{
    EXTENDED_MATERIALS, GltfMetadata, LIGHTS_PUNCTUAL, MATERIALS_VARIANTS, MESH_GPU_INSTANCING,
    MaterialVariantMappings, TEXTURE_TRANSFORM, json_chunk,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MeshSource {
    Gltf,
    Heightfield,
}

impl MeshSource {
    pub(super) fn resolve(
        base: &Path,
        url: &str,
        declared: Option<&BTreeSet<PathBuf>>,
    ) -> Result<(PathBuf, Self), String> {
        use xrack_syntax::builtins::{HEIGHTFIELD_EXTENSION, MESH_SOURCE_EXTENSIONS};
        let relative = local_path(url)?;
        let extension = relative
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        let source = if extension.eq_ignore_ascii_case(HEIGHTFIELD_EXTENSION) {
            Self::Heightfield
        } else if MESH_SOURCE_EXTENSIONS
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
        {
            Self::Gltf
        } else {
            return Err("expected a .gltf or .glb mesh or a grayscale PNG heightfield".into());
        };
        let path = asset_path(base, relative, declared, "mesh")?;
        Ok((path, source))
    }

    pub(super) fn load(
        self,
        path: &Path,
        declared: Option<&BTreeSet<PathBuf>>,
    ) -> Result<(MeshAsset, BTreeSet<PathBuf>), String> {
        match self {
            Self::Gltf => load_mesh(path, declared),
            Self::Heightfield => {
                let mesh = crate::heightfield::load(path)?;
                Ok((mesh, BTreeSet::from([path.to_owned()])))
            }
        }
    }

    pub(super) fn generate_lightmap_uv(self, mesh: MeshAsset) -> Result<MeshAsset, String> {
        match self {
            Self::Gltf => crate::lightmap_unwrap::generate(mesh),
            Self::Heightfield => crate::heightfield::generate_lightmap_uv(mesh),
        }
    }
}

pub(super) fn load_mesh(
    path: &Path,
    declared: Option<&BTreeSet<PathBuf>>,
) -> Result<(MeshAsset, BTreeSet<PathBuf>), String> {
    let bytes = read_asset(path, "mesh")?;
    let json = json_chunk(&bytes).ok_or("invalid glTF JSON chunk")?;
    let metadata: GltfMetadata = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let variant_mappings = metadata.material_variant_mappings()?;
    let parsed =
        gltf::Gltf::from_slice_without_validation(&bytes).map_err(|error| error.to_string())?;
    let mut root = parsed.document.into_json();
    // These extensions are checked by the resource loader itself. Run all
    // glTF validation, including rejection of other unsupported requirements.
    root.extensions_required.retain(|name| {
        name != TEXTURE_TRANSFORM
            && name != LIGHTS_PUNCTUAL
            && name != MATERIALS_VARIANTS
            && name != MESH_GPU_INSTANCING
            && !EXTENDED_MATERIALS.contains(&name.as_str())
    });
    let document = gltf::Document::from_json(root).map_err(|error| error.to_string())?;
    let mut blob = parsed.blob;
    let base = path
        .parent()
        .ok_or("mesh has no parent directory")?
        .to_owned();
    let mut inputs = BTreeSet::from([path.to_owned()]);
    let mut buffers = Vec::new();
    let mut total_bytes = 0;
    for buffer in document.buffers() {
        let data = match buffer.source() {
            Source::Bin => blob.take().ok_or("missing GLB BIN chunk")?,
            Source::Uri(uri) if uri.starts_with("data:") => inline_data(uri)?,
            Source::Uri(uri) => {
                let relative = decode_uri(uri)?;
                let path = asset_path(&base, local_path(&relative)?, declared, "mesh buffer")?;
                let data = read_asset(&path, "mesh buffer")?;
                inputs.insert(path);
                data
            }
        };
        if buffer.length() > data.len() {
            return Err("glTF buffer is shorter than its declared length".into());
        }
        total_bytes += data.len() as u64;
        if total_bytes > MAX_ASSET_BYTES {
            return Err("glTF buffers exceed 16 MiB".into());
        }
        buffers.push(data);
    }
    for view in document.views() {
        if view.offset().checked_add(view.length()).is_none_or(|end| {
            end > view.buffer().length() || end > buffers[view.buffer().index()].len()
        }) {
            return Err("glTF buffer view is out of bounds".into());
        }
    }
    let hierarchy = ImportHierarchy {
        lights: mesh_lights::attachments(&document)?,
        instances: mesh_instances::read(&document, &metadata, &buffers)?,
    };
    let mut images = MaterialImages::default();
    let mut prepare = |variant, primary| {
        let mut materials = Materials::new(
            &document,
            &metadata,
            &base,
            declared,
            &buffers,
            &mut inputs,
            &mut images,
        );
        prepare_mesh(
            &document,
            &metadata,
            &buffers,
            &mut materials,
            &hierarchy,
            variant,
            primary,
        )
    };
    let primary = prepare(None, None)?;
    let mut expanded_vertices = 0;
    let mut expanded_deltas = 0;
    let mut alternatives = Vec::with_capacity(variant_mappings.len());
    for (name, mappings) in metadata.material_variant_names().zip(&variant_mappings) {
        let alternative = if mappings.is_empty() {
            None
        } else {
            let asset = prepare(Some(mappings), Some(&primary))?;
            for mesh in asset.geometry_meshes() {
                expanded_vertices += mesh.vertices().len();
                expanded_deltas += mesh.morph_geometry().map_or(0, |morph| morph.delta_count());
            }
            if expanded_vertices > 4 * MAX_IMPORTED_VERTICES
                || expanded_deltas > 4 * MAX_MORPH_DELTAS
            {
                return Err("glTF exceeds the expanded material variant geometry limit".into());
            }
            Some(asset)
        };
        alternatives.push((name.to_owned(), alternative));
    }
    Ok((primary.with_material_variants(alternatives), inputs))
}

struct ImportHierarchy {
    lights: BTreeMap<usize, xrack_runtime::scene3d::SceneLight>,
    instances: BTreeMap<usize, MeshInstances>,
}

fn prepare_mesh(
    document: &gltf::Document,
    metadata: &GltfMetadata,
    buffers: &[Vec<u8>],
    materials: &mut Materials<'_>,
    hierarchy: &ImportHierarchy,
    variant: Option<&MaterialVariantMappings>,
    primary: Option<&MeshAsset>,
) -> Result<MeshAsset, String> {
    if !hierarchy.instances.is_empty()
        || !hierarchy.lights.is_empty()
        || document.nodes().any(|node| node.camera().is_some())
        || document.animations().next().is_some()
        || document.skins().next().is_some()
        || document.meshes().any(|mesh| {
            mesh.primitives()
                .any(|primitive| primitive.morph_targets().len() != 0)
        })
    {
        let rig = match primary {
            Some(MeshAsset::Animated(rig)) => Some(rig.as_ref()),
            _ => None,
        };
        let mesh = animated_mesh(
            document, metadata, buffers, materials, hierarchy, variant, rig,
        )?;
        return Ok(MeshAsset::Animated(Arc::new(mesh)));
    }
    let mut geometry = MeshGeometry::default();
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next());
    if let Some(scene) = scene {
        let mut pending: Vec<_> = scene
            .nodes()
            .map(|node| (node, Matrix4::identity()))
            .collect();
        pending.reverse();
        let mut visited = BTreeSet::new();
        while let Some((node, parent)) = pending.pop() {
            if !visited.insert(node.index()) {
                return Err("invalid recursive or multiply parented glTF node".into());
            }
            let columns = node.transform().matrix();
            let transform = parent * Matrix4::from_fn(|row, column| columns[column][row]);
            if let Some(mesh) = node.mesh() {
                append_mesh(
                    document,
                    mesh,
                    buffers,
                    &transform,
                    &mut geometry,
                    materials,
                    variant,
                )?;
            }
            let start = pending.len();
            pending.extend(node.children().map(|child| (child, transform)));
            pending[start..].reverse();
        }
    } else {
        // Mesh-only assets can be exported without a scene or nodes.
        for mesh in document.meshes() {
            append_mesh(
                document,
                mesh,
                buffers,
                &Matrix4::identity(),
                &mut geometry,
                materials,
                variant,
            )?;
        }
    }
    let (materials, slots) = materials.finish();
    let mesh = Mesh3D::imported(
        geometry.vertices,
        geometry.tangents,
        geometry.parts,
        materials,
    )?
    .with_material_slots(slots.into())
    .ok_or("invalid glTF material slots")?
    .with_lightmap_uv(geometry.lightmap_uv)
    .ok_or("invalid glTF lightmap coordinates")?;
    Ok(MeshAsset::Static(mesh))
}

fn animated_mesh(
    document: &gltf::Document,
    metadata: &GltfMetadata,
    buffers: &[Vec<u8>],
    materials: &mut Materials<'_>,
    hierarchy: &ImportHierarchy,
    variant: Option<&MaterialVariantMappings>,
    primary: Option<&AnimatedMesh>,
) -> Result<AnimatedMesh, String> {
    use gltf::scene::Transform;
    let names: BTreeMap<_, _> = metadata
        .camera_nodes()
        .into_iter()
        .filter_map(|(node, _, name)| Some((node, name?)))
        .collect();

    let roots: Vec<_> = if let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        scene.nodes().collect()
    } else {
        let children: BTreeSet<_> = document
            .nodes()
            .flat_map(|node| node.children().map(|child| child.index()))
            .collect();
        document
            .nodes()
            .filter(|node| !children.contains(&node.index()))
            .collect()
    };
    let mut pending: Vec<_> = roots.into_iter().rev().map(|node| (node, None)).collect();
    let mut nodes = Vec::new();
    let mut lights = Vec::new();
    let mut cameras = Vec::new();
    let mut indices = BTreeMap::new();
    let mut mesh_indices = BTreeMap::new();
    let mut skin_indices = BTreeMap::new();
    let mut authored_skins = Vec::new();
    let mut geometry = Vec::new();
    let mut vertex_count = 0;
    let mut delta_count = 0;
    while let Some((node, parent)) = pending.pop() {
        let index = nodes.len();
        if index == MAX_ANIMATION_NODES || indices.insert(node.index(), index).is_some() {
            return Err("invalid or oversized animated glTF node graph".into());
        }
        let skin = node.skin().map(|skin| {
            *skin_indices.entry(skin.index()).or_insert_with(|| {
                let index = authored_skins.len();
                authored_skins.push(skin);
                index
            })
        });
        let (mesh, morph_weights) = if let Some(mesh) = node.mesh() {
            let original = mesh.index();
            let count = mesh
                .primitives()
                .next()
                .map_or(0, |primitive| primitive.morph_targets().len());
            if count > MAX_MORPH_TARGETS {
                return Err("glTF exceeds the morph target limit".into());
            }
            let morph_weights = node
                .weights()
                .or_else(|| mesh.weights())
                .map_or_else(|| vec![0.0; count].into(), Arc::from);
            let mesh = match mesh_indices.entry((original, skin.is_some())) {
                std::collections::btree_map::Entry::Occupied(index) => *index.get(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let index = geometry.len();
                    let mut prepared = MeshGeometry {
                        weights: skin.is_some().then(Vec::new),
                        morph: (count != 0).then(|| MorphData::new(count)),
                        ..MeshGeometry::default()
                    };
                    append_mesh(
                        document,
                        mesh,
                        buffers,
                        &Matrix4::identity(),
                        &mut prepared,
                        materials,
                        variant,
                    )?;
                    vertex_count += prepared.vertices.len();
                    if vertex_count > MAX_IMPORTED_VERTICES {
                        return Err("animated glTF exceeds the vertex limit".into());
                    }
                    delta_count += prepared
                        .morph
                        .as_ref()
                        .map_or(0, |morph| morph.deltas.len());
                    if delta_count > MAX_MORPH_DELTAS {
                        return Err("glTF exceeds the morph displacement limit".into());
                    }
                    geometry.push((prepared, metadata.morph_target_names(original, count)));
                    *entry.insert(index)
                }
            };
            (Some(mesh), morph_weights)
        } else {
            (None, Arc::from([]))
        };
        let transform = match node.transform() {
            Transform::Matrix { matrix } => MeshNodeTransform::Matrix(matrix),
            Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => MeshNodeTransform::Trs {
                translation,
                rotation,
                scale,
            },
        };
        if let Some(&light) = hierarchy.lights.get(&node.index()) {
            lights.push(MeshLight { node: index, light });
        }
        if let Some(camera) = node.camera() {
            let lens = match camera.projection() {
                gltf::camera::Projection::Perspective(lens) => MeshCameraProjection::Perspective {
                    yfov: lens.yfov(),
                    aspect: lens.aspect_ratio(),
                    near: lens.znear(),
                    far: lens.zfar(),
                },
                gltf::camera::Projection::Orthographic(lens) => {
                    MeshCameraProjection::Orthographic {
                        xmag: lens.xmag(),
                        ymag: lens.ymag(),
                        near: lens.znear(),
                        far: lens.zfar(),
                    }
                }
            };
            if lens.settings().is_none() {
                return Err(format!("invalid glTF camera on node #{}", node.index()));
            }
            cameras.push(MeshCamera {
                node: index,
                source_node: node.index(),
                name: names.get(&node.index()).map(|name| (*name).to_owned()),
                lens,
            });
        }
        nodes.push(MeshAnimationNode {
            source_node: u32::try_from(node.index())
                .map_err(|_| "glTF node index exceeds the supported range")?,
            parent,
            mesh,
            skin,
            morph_weights,
            transform,
            instances: hierarchy.instances.get(&node.index()).cloned(),
        });
        let start = pending.len();
        pending.extend(node.children().map(|child| (child, Some(index))));
        pending[start..].reverse();
    }
    let (materials, slots) = materials.finish();
    let materials: Arc<[_]> = materials.into();
    let slots: Arc<[_]> = slots.into();
    let geometry = geometry
        .into_iter()
        .map(|(mesh, names)| {
            let prepared = Mesh3D::imported_shared(
                mesh.vertices,
                mesh.tangents,
                mesh.parts,
                materials.clone(),
            )?
            .with_material_slots(slots.clone())
            .ok_or("invalid glTF material slots")?
            .with_lightmap_uv(mesh.lightmap_uv)
            .ok_or("invalid glTF lightmap coordinates")?;
            let prepared = if let Some(weights) = mesh.weights {
                prepared
                    .with_skin_weights(weights)
                    .ok_or("invalid prepared glTF skin vertices")?
            } else {
                prepared
            };
            if let Some(morph) = mesh.morph {
                prepared
                    .with_morph(morph.count, names.into(), morph.offsets, morph.deltas)
                    .ok_or("invalid prepared glTF morph vertices")
            } else {
                Ok(prepared)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(primary) = primary {
        return Ok(primary.with_meshes(geometry));
    }
    let skins = authored_skins
        .into_iter()
        .map(|skin| mesh_skin::read_skin(skin, &indices, buffers))
        .collect::<Result<Vec<_>, _>>()?;
    let clips = mesh_animation::read_clips(document, &metadata, buffers, &indices, &nodes)?;
    AnimatedMesh::new(geometry, nodes, clips, skins, lights, cameras).map_err(str::to_owned)
}

pub(super) fn inline_data(uri: &str) -> Result<Vec<u8>, String> {
    let (prefix, encoded) = uri.split_once(',').ok_or("invalid glTF data URI")?;
    if !prefix.ends_with(";base64") {
        return Err("glTF data URIs must use base64".into());
    }
    STANDARD
        .decode(encoded)
        .map_err(|error| format!("invalid glTF data buffer: {error}"))
}

pub(super) fn decode_uri(uri: &str) -> Result<String, String> {
    let mut bytes = uri.bytes();
    let mut decoded = Vec::with_capacity(uri.len());
    while let Some(byte) = bytes.next() {
        let value = match byte {
            b'?' | b'#' => return Err("glTF resource must use a relative file URI".into()),
            b'%' => {
                let mut digit = || {
                    bytes
                        .next()
                        .and_then(|byte| char::from(byte).to_digit(16))
                        .ok_or("invalid percent escape in glTF resource URI")
                };
                (digit()? * 16 + digit()?) as u8
            }
            _ => byte,
        };
        if value == 0 {
            return Err("glTF resource URI contains a null byte".into());
        }
        decoded.push(value);
    }
    String::from_utf8(decoded).map_err(|_| "glTF resource URI must be UTF-8".into())
}

pub(super) fn validate_accessor(accessor: &Accessor<'_>) -> Result<(), String> {
    if accessor.sparse().is_some() {
        return Err("sparse glTF accessors are unsupported".into());
    }
    validate_accessor_view(accessor)
}

fn validate_accessor_view(accessor: &Accessor<'_>) -> Result<(), String> {
    let view = accessor
        .view()
        .ok_or("glTF accessor is missing a buffer view")?;
    let size = accessor.size();
    let stride = view.stride().unwrap_or(size);
    let end = accessor
        .count()
        .checked_sub(1)
        .and_then(|count| count.checked_mul(stride))
        .and_then(|span| span.checked_add(size))
        .and_then(|span| span.checked_add(accessor.offset()));
    if stride < size || end.is_none_or(|end| end > view.length()) {
        return Err("glTF accessor is out of bounds".into());
    }
    Ok(())
}

pub(super) fn validate_accessor_with_sparse(
    accessor: &Accessor<'_>,
    buffers: &[Vec<u8>],
) -> Result<(), String> {
    use gltf::accessor::sparse::IndexType;
    let Some(sparse) = accessor.sparse() else {
        return validate_accessor_view(accessor);
    };
    if sparse.count() == 0 || sparse.count() > accessor.count() {
        return Err("invalid glTF sparse accessor count".into());
    }
    if accessor.view().is_some() {
        validate_accessor_view(accessor)?;
    }
    let indices = sparse.indices();
    let kind = indices.index_type();
    let stride = kind.size();
    let values = sparse.values();
    let index_view = indices.view();
    let value_view = values.view();
    if index_view.stride().is_some()
        || value_view.stride().is_some()
        || indices
            .offset()
            .checked_add(sparse.count() * stride)
            .is_none_or(|end| end > index_view.length())
        || values
            .offset()
            .checked_add(sparse.count() * accessor.size())
            .is_none_or(|end| end > value_view.length())
    {
        return Err("glTF sparse accessor data is out of bounds".into());
    }
    let offset = index_view.offset() + indices.offset();
    let bytes = &buffers[index_view.buffer().index()][offset..offset + sparse.count() * stride];
    let mut previous = None;
    for bytes in bytes.chunks_exact(stride) {
        let index = match kind {
            IndexType::U8 => u32::from(bytes[0]),
            IndexType::U16 => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            IndexType::U32 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        };
        if index as usize >= accessor.count() || previous.is_some_and(|previous| index <= previous)
        {
            return Err("invalid glTF sparse accessor indices".into());
        }
        previous = Some(index);
    }
    Ok(())
}

#[derive(Default)]
struct MeshGeometry {
    vertices: Vec<MeshVertex>,
    tangents: Vec<MeshTangent>,
    parts: Vec<MeshPart>,
    lightmap_uv: Vec<[f32; 2]>,
    weights: Option<Vec<MeshInfluence>>,
    morph: Option<MorphData>,
}

fn append_mesh(
    document: &gltf::Document,
    mesh: gltf::Mesh<'_>,
    buffers: &[Vec<u8>],
    transform: &Matrix4<f32>,
    geometry: &mut MeshGeometry,
    materials: &mut Materials<'_>,
    variant: Option<&MaterialVariantMappings>,
) -> Result<(), String> {
    for primitive in mesh.primitives() {
        let index = variant.and_then(|mapping| mapping.get(&(mesh.index(), primitive.index())));
        let material = match index {
            Some(&index) => document
                .materials()
                .nth(index)
                .ok_or("invalid glTF variant material")?,
            None => primitive.material(),
        };
        let material = materials.get(material)?;
        let start = geometry.vertices.len();
        let tangents = append_primitive(
            primitive,
            buffers,
            transform,
            material.uv_sets,
            material.normal_coordinates,
            geometry,
        )?;
        let end = geometry.vertices.len();
        if end > start {
            if let Some(tangents) = tangents {
                geometry.tangents.resize(start, [0; 4]);
                geometry.tangents.extend(tangents);
            } else if !geometry.tangents.is_empty() {
                geometry.tangents.resize(end, [0; 4]);
            }
            let volume_inverse = if materials.has_volume(material.index) {
                let model = transform.fixed_view::<3, 3>(0, 0);
                if model.is_identity(0.0) {
                    None
                } else {
                    let inverse = model
                        .try_inverse()
                        .ok_or("glTF volume requires an invertible node transform")?;
                    Some(std::array::from_fn(|row| {
                        std::array::from_fn(|col| inverse[(row, col)])
                    }))
                }
            } else {
                None
            };
            if let Some(last) = geometry.parts.last_mut()
                && last.material == material.index
                && last.volume_inverse == volume_inverse
                && !materials.is_blended(material.index)
            {
                last.vertices.end = end;
            } else {
                geometry.parts.push(MeshPart {
                    vertices: start..end,
                    material: material.index,
                    volume_inverse,
                });
            }
        }
    }
    Ok(())
}

fn append_primitive(
    primitive: Primitive<'_>,
    buffers: &[Vec<u8>],
    transform: &Matrix4<f32>,
    uv_sets: [Option<u32>; 2],
    normal_coordinates: Option<NormalCoordinates>,
    geometry: &mut MeshGeometry,
) -> Result<Option<Vec<MeshTangent>>, String> {
    let mode = primitive.mode();
    if !matches!(
        mode,
        Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan
    ) {
        return Err(
            "glTF mesh primitives must use triangles, triangle strips or triangle fans".into(),
        );
    }
    let vertices = &mut geometry.vertices;
    let lightmap_uv = &mut geometry.lightmap_uv;
    let weights = &mut geometry.weights;
    let morph = &mut geometry.morph;
    if morph.is_none() && primitive.morph_targets().len() != 0 {
        return Err("glTF primitives have inconsistent morph targets".into());
    }
    let position = primitive
        .get(&Semantic::Positions)
        .ok_or("glTF mesh is missing POSITION")?;
    if position.count() > MAX_IMPORTED_VERTICES {
        return Err("glTF mesh exceeds the vertex limit".into());
    }
    validate_vector(&position)?;
    if let Some(normals) = primitive.get(&Semantic::Normals) {
        validate_vector(&normals)?;
    }
    let indices = primitive.indices();
    if let Some(indices) = &indices {
        if indices.dimensions() != Dimensions::Scalar
            || !matches!(
                indices.data_type(),
                DataType::U8 | DataType::U16 | DataType::U32
            )
            || indices.normalized()
        {
            return Err("invalid glTF index accessor type".into());
        }
        validate_accessor(indices)?;
        if indices.count() > MAX_IMPORTED_VERTICES {
            return Err("glTF mesh exceeds the index limit".into());
        }
    }

    let source_morph = morph
        .as_ref()
        .map(|morph| mesh_morph::read_morph(&primitive, buffers, morph.count, position.count()))
        .transpose()?;
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let positions: Vec<_> = reader
        .read_positions()
        .ok_or("unable to read glTF positions")?
        .map(|point| transform.transform_point(&Point3::from(point)).coords)
        .collect();
    let read_uv = |set| {
        let accessor = primitive
            .get(&Semantic::TexCoords(set))
            .ok_or_else(|| format!("glTF texture requires TEXCOORD_{set}"))?;
        validate_vertex_attribute(&accessor, &[Dimensions::Vec2], positions.len())?;
        let coordinates = reader
            .read_tex_coords(set)
            .ok_or_else(|| "unable to read glTF texture coordinates".to_owned())?;
        Ok::<_, String>(coordinates.into_f32().collect::<Vec<_>>())
    };
    // Retain authored coordinates so a later material override can use them.
    let mut uv_sets = uv_sets;
    for set in [0, 1] {
        if !uv_sets.contains(&Some(set))
            && primitive.get(&Semantic::TexCoords(set)).is_some()
            && let Some(slot) = uv_sets.iter_mut().find(|slot| slot.is_none())
        {
            *slot = Some(set);
        }
    }
    let [uv, uv1] = uv_sets.map(|slot| slot.map(read_uv).transpose());
    let (uv, uv1) = (uv?, uv1?);
    let dedicated =
        if uv_sets.contains(&Some(1)) || primitive.get(&Semantic::TexCoords(1)).is_none() {
            None
        } else {
            Some(read_uv(1)?)
        };
    let chart = match uv_sets {
        [Some(1), _] => uv.as_ref(),
        [_, Some(1)] => uv1.as_ref(),
        _ => dedicated.as_ref(),
    };
    // Keep triangle indices aligned when only some primitive groups have a chart.
    if chart.is_some() && lightmap_uv.is_empty() {
        lightmap_uv.resize(vertices.len(), [-1.0; 2]);
    }
    let colors = primitive
        .get(&Semantic::Colors(0))
        .map(|accessor| {
            validate_vertex_attribute(
                &accessor,
                &[Dimensions::Vec3, Dimensions::Vec4],
                positions.len(),
            )?;
            let colors = reader
                .read_colors(0)
                .ok_or_else(|| "unable to read glTF vertex colors".to_owned())?;
            Ok::<_, String>(colors.into_rgba_f32().collect::<Vec<_>>())
        })
        .transpose()?;
    let normals = reader
        .read_normals()
        .map(|normals| {
            let inverse = transform
                .fixed_view::<3, 3>(0, 0)
                .try_inverse()
                .ok_or("glTF node has a singular normal transform")?
                .transpose();
            let normals: Vec<_> = normals
                .map(|normal| (inverse * Vector3::from(normal)).try_normalize(1e-8))
                .collect();
            if normals.len() != positions.len() {
                return Err("glTF normal and position counts differ");
            }
            Ok(normals)
        })
        .transpose()?;
    let influences = weights
        .as_ref()
        .map(|_| mesh_skin::read_influences(&primitive, buffers, positions.len()))
        .transpose()?;
    let indices: Vec<_> = match reader.read_indices() {
        Some(indices) => indices.into_u32().map(|index| index as usize).collect(),
        None if indices.is_some() => return Err("unable to read glTF indices".into()),
        None => (0..positions.len()).collect(),
    };
    if indices.len() < 3 || mode == Mode::Triangles && !indices.len().is_multiple_of(3) {
        return Err("glTF primitive contains incomplete triangles".into());
    }
    let count = if mode == Mode::Triangles {
        indices.len() / 3
    } else {
        indices.len() - 2
    };
    if vertices.len() + count * 3 > MAX_IMPORTED_VERTICES {
        return Err("glTF mesh exceeds the triangle limit".into());
    }
    let orientation = transform.fixed_view::<3, 3>(0, 0).determinant().signum();
    let start = vertices.len();
    let normal_coordinates = normal_coordinates.or_else(|| {
        (primitive.get(&Semantic::Normals).is_some()
            && primitive.get(&Semantic::Tangents).is_some())
        .then_some(NormalCoordinates {
            slot: 0,
            overrides_source_set: false,
            transform: TextureTransform::IDENTITY,
        })
    });
    let mut tangent_faces = normal_coordinates.map(|coordinates| (coordinates, Vec::new()));
    for triangle in 0..count {
        let offsets = match mode {
            Mode::Triangles => [triangle * 3, triangle * 3 + 1, triangle * 3 + 2],
            Mode::TriangleFan => [0, triangle + 1, triangle + 2],
            _ if triangle.is_multiple_of(2) => [triangle, triangle + 1, triangle + 2],
            _ => [triangle + 1, triangle, triangle + 2],
        };
        let source_indices = offsets.map(|offset| indices[offset]);
        let mut indices = source_indices;
        // The node transform is baked, so mirror the winding along with it.
        if orientation < 0.0 {
            indices.swap(1, 2);
        }
        let [Some(a), Some(b), Some(c)] = indices.map(|index| positions.get(index)) else {
            return Err("glTF mesh index is out of bounds".into());
        };
        if ![a, b, c]
            .iter()
            .all(|point| point.iter().all(|value| value.is_finite()))
        {
            return Err("glTF mesh contains an invalid position".into());
        }
        let Some(flat_normal) = (b - a).cross(&(c - a)).try_normalize(1e-12) else {
            continue;
        };
        if let Some((_, faces)) = &mut tangent_faces {
            faces.push(source_indices);
        }
        if let Some((morph, source)) = morph.as_mut().zip(source_morph.as_ref()) {
            morph.append_triangle(
                source,
                indices,
                [*a, *b, *c],
                normals.is_none().then_some(flat_normal),
            )?;
        }
        for (&index, &point) in indices.iter().zip([a, b, c]) {
            if let Some((weights, influences)) = weights.as_mut().zip(influences.as_ref()) {
                weights.push(influences[index]);
            }
            let normal = normals
                .as_ref()
                .and_then(|normals| normals[index])
                .unwrap_or(flat_normal);
            if chart.is_some() || !lightmap_uv.is_empty() {
                lightmap_uv.push(chart.map_or([-1.0; 2], |coords| coords[index]));
            }
            vertices.push(MeshVertex {
                position: point.into(),
                normal: normal.into(),
                uv: uv.as_ref().map_or([0.0; 2], |coords| coords[index]),
                uv1: uv1.as_ref().map_or([0.0; 2], |coords| coords[index]),
                color: colors.as_ref().map_or([1.0; 4], |colors| colors[index]),
            });
        }
    }
    tangent_faces
        .map(|(slot, faces)| {
            mesh_tangents::prepare(
                &primitive,
                buffers,
                transform,
                slot,
                &faces,
                &vertices[start..],
                positions.len(),
            )
        })
        .transpose()
}

fn validate_vertex_attribute(
    accessor: &Accessor<'_>,
    dimensions: &[Dimensions],
    vertices: usize,
) -> Result<(), String> {
    let supported_type = match accessor.data_type() {
        DataType::F32 => !accessor.normalized(),
        DataType::U8 | DataType::U16 => accessor.normalized(),
        _ => false,
    };
    if !supported_type
        || !dimensions.contains(&accessor.dimensions())
        || accessor.count() != vertices
    {
        return Err("invalid glTF UV or vertex color accessor type or count".into());
    }
    validate_accessor(accessor)
}

fn validate_vector(accessor: &Accessor<'_>) -> Result<(), String> {
    if accessor.dimensions() != Dimensions::Vec3
        || accessor.data_type() != DataType::F32
        || accessor.normalized()
    {
        return Err("glTF positions and normals require unnormalized VEC3 floats".into());
    }
    validate_accessor(accessor)
}
