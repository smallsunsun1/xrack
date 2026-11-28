use crate::{
    mesh_animation,
    mesh_attributes::{Attribute, AttributeSource},
    mesh_compression::BufferDecode,
    mesh_draco::DracoDecode,
    mesh_instances, mesh_lights,
    mesh_materials::{MaterialImages, Materials, NormalCoordinates, prepare_texture_sources},
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
use xrack_assets::{
    MAX_ASSET_BYTES,
    files::{asset_path, local_path, read_asset},
};
use xrack_runtime::scene3d::{
    AnimatedMesh, MAX_ANIMATION_NODES, MAX_IMPORTED_VERTICES, MAX_MORPH_DELTAS, MAX_MORPH_TARGETS,
    Mesh3D, MeshAnimationNode, MeshAsset, MeshCamera, MeshCameraProjection, MeshInfluence,
    MeshInstances, MeshLight, MeshLighting, MeshNodeCatalog, MeshNodeTransform, MeshPart,
    MeshTangent, MeshTopology, MeshVertex, TextureTransform,
};
use xrack_runtime::{MeshNode3D, Point3D, Vector3D};
use xrack_syntax::gltf_metadata::{
    DRACO_COMPRESSION, EXT_MESHOPT_COMPRESSION, EXTENDED_MATERIALS, GltfMetadata,
    KHR_MESHOPT_COMPRESSION, LIGHTS_PUNCTUAL, MATERIALS_VARIANTS, MESH_GPU_INSTANCING,
    MESH_QUANTIZATION, MaterialVariantMappings, TEXTURE_BASIS, TEXTURE_TRANSFORM, TEXTURE_WEBP,
    json_chunk,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MeshSource {
    Gltf,
    Heightfield,
    Obj,
}

impl MeshSource {
    pub fn resolve(
        base: &Path,
        url: &str,
        declared: Option<&BTreeSet<PathBuf>>,
    ) -> Result<(PathBuf, Self), String> {
        let relative = local_path(url)?;
        let source = Self::from_path(relative)?;
        let path = asset_path(base, relative, declared, "mesh")?;
        Ok((path, source))
    }

    pub(crate) fn from_path(path: &Path) -> Result<Self, String> {
        use xrack_syntax::builtins::{
            HEIGHTFIELD_EXTENSION, MESH_SOURCE_EXTENSIONS, OBJ_EXTENSION,
        };
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        let source = if extension.eq_ignore_ascii_case(HEIGHTFIELD_EXTENSION) {
            Self::Heightfield
        } else if extension.eq_ignore_ascii_case(OBJ_EXTENSION) {
            Self::Obj
        } else if MESH_SOURCE_EXTENSIONS
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
        {
            Self::Gltf
        } else {
            return Err(
                "expected a .gltf, .glb, or .obj mesh or a grayscale PNG heightfield".into(),
            );
        };
        Ok(source)
    }

    pub fn load(
        self,
        path: &Path,
        declared: Option<&BTreeSet<PathBuf>>,
    ) -> Result<(MeshAsset, BTreeSet<PathBuf>), String> {
        match self {
            Self::Gltf => load_mesh(path, declared),
            Self::Obj => crate::mesh_obj::load(path, declared),
            Self::Heightfield => {
                let mesh = crate::heightfield::load(path)?;
                Ok((mesh, BTreeSet::from([path.to_owned()])))
            }
        }
    }

    pub fn generate_lightmap_uv(self, mesh: MeshAsset) -> Result<MeshAsset, String> {
        match self {
            Self::Gltf | Self::Obj => crate::lightmap_unwrap::generate(mesh),
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
    let mut blob = parsed.blob;
    let meshopt = BufferDecode::prepare(&mut root, blob.is_some())?;
    prepare_texture_sources(&mut root)?;
    let draco = DracoDecode::prepare(&root, &metadata)?;
    // These extensions are checked by the resource loader itself. Run all
    // glTF validation, including rejection of other unsupported requirements.
    root.extensions_required.retain(|name| {
        name != TEXTURE_TRANSFORM
            && name != DRACO_COMPRESSION
            && name != TEXTURE_BASIS
            && name != TEXTURE_WEBP
            && name != MESH_QUANTIZATION
            && name != EXT_MESHOPT_COMPRESSION
            && name != KHR_MESHOPT_COMPRESSION
            && name != LIGHTS_PUNCTUAL
            && name != MATERIALS_VARIANTS
            && name != MESH_GPU_INSTANCING
            && !EXTENDED_MATERIALS.contains(&name.as_str())
    });
    let document = if draco.is_some() {
        // Required Draco accessors lack buffer views until decompression.
        gltf::Document::from_json_without_validation(root)
    } else {
        gltf::Document::from_json(root).map_err(|error| error.to_string())?
    };
    let base = path
        .parent()
        .ok_or("mesh has no parent directory")?
        .to_owned();
    let mut inputs = BTreeSet::from([path.to_owned()]);
    let mut buffers = Vec::new();
    let mut total_bytes = 0;
    let skipped_draco_fallback = |index| {
        draco
            .as_ref()
            .is_some_and(|decode| decode.fallback_only(index))
            && meshopt
                .as_ref()
                .is_none_or(|decode| !decode.reads_compressed_source(index))
    };
    for buffer in document.buffers() {
        if skipped_draco_fallback(buffer.index())
            || meshopt
                .as_ref()
                .is_some_and(|decode| !decode.reads_buffer(buffer.index()))
        {
            buffers.push(Vec::new());
            continue;
        }
        let data = match buffer.source() {
            Source::Bin if meshopt.is_none() || buffer.index() == 0 => {
                blob.take().ok_or("missing GLB BIN chunk")?
            }
            Source::Bin => return Err("glTF meshopt source buffer has no URI or BIN data".into()),
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
    if let Some(decode) = &meshopt {
        decode.decode(&mut buffers)?;
    }
    for view in &document.as_json().buffer_views {
        let index = view.buffer.value();
        let buffer = document
            .as_json()
            .buffers
            .get(index)
            .ok_or("invalid glTF view buffer")?;
        let end = view
            .byte_offset
            .map_or(0, |value| value.0)
            .checked_add(view.byte_length.0);
        if end.is_none_or(|end| {
            end > buffer.byte_length.0
                || (!skipped_draco_fallback(index) && end > buffers[index].len() as u64)
        }) {
            return Err("glTF buffer view is out of bounds".into());
        }
    }
    let document = if let Some(decode) = draco {
        let root = decode.decode(document.into_json(), &mut buffers)?;
        gltf::Document::from_json(root).map_err(|error| error.to_string())?
    } else {
        document
    };
    let addresses = metadata.scene_node_addresses()?;
    let source_nodes: Vec<_> = document.nodes().collect();
    let mut transforms: Vec<Matrix4<f32>> = Vec::with_capacity(addresses.len());
    let mut nodes = Vec::with_capacity(addresses.len());
    for address in &addresses {
        let node = source_nodes
            .get(address.source)
            .ok_or("invalid glTF node index")?;
        let columns = node.transform().matrix();
        let local = Matrix4::from_fn(|row, column| columns[column][row]);
        let world = address
            .parent
            .map_or(local, |parent| transforms[parent] * local);
        if !world.iter().copied().all(f32::is_finite) {
            return Err(format!(
                "non-finite world transform on glTF node #{}",
                address.source
            ));
        }
        transforms.push(world);
        let source_index = i64::try_from(address.source)
            .map_err(|_| "glTF node index exceeds the supported range")?;
        let parent_index = address
            .parent
            .map_or(-1, |parent| addresses[parent].source as i64);
        let vector = |column| {
            Vector3D::new(
                f64::from(world[(0, column)]),
                f64::from(world[(1, column)]),
                f64::from(world[(2, column)]),
            )
        };
        let origin = vector(3);
        nodes.push(MeshNode3D {
            source_index,
            parent_index,
            name: address.name.to_owned(),
            path: address.path.clone(),
            origin: Point3D::new(origin.x, origin.y, origin.z),
            axis_x: vector(0),
            axis_y: vector(1),
            axis_z: vector(2),
            has_mesh: address.has_mesh,
            has_camera: address.has_camera,
            has_light: address.has_light,
        });
    }
    let hierarchy = ImportHierarchy {
        nodes: addresses
            .iter()
            .zip(transforms)
            .map(|(address, transform)| ImportedNode {
                node: source_nodes[address.source].clone(),
                parent: address.parent,
                transform,
            })
            .collect(),
        lights: mesh_lights::attachments(&document)?,
        instances: mesh_instances::read(&document, &metadata, &buffers)?,
    };
    let source = ImportData {
        document: &document,
        metadata: &metadata,
        attributes: AttributeSource {
            buffers: &buffers,
            quantized: metadata.requires_mesh_quantization(),
        },
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
        prepare_mesh(source, &mut materials, &hierarchy, variant, primary)
    };
    let primary = prepare(None, None)?;
    let primary = if nodes.is_empty() {
        primary
    } else {
        let catalog = MeshNodeCatalog::new(nodes);
        match primary {
            MeshAsset::Static(mesh) => MeshAsset::Static(mesh.with_node_catalog(catalog)),
            MeshAsset::Animated(rig) => MeshAsset::Animated(Arc::new(
                Arc::unwrap_or_clone(rig).with_node_catalog(catalog),
            )),
            asset => asset,
        }
    };
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

struct ImportedNode<'a> {
    node: gltf::Node<'a>,
    parent: Option<usize>,
    transform: Matrix4<f32>,
}

struct ImportHierarchy<'a> {
    nodes: Vec<ImportedNode<'a>>,
    lights: BTreeMap<usize, xrack_runtime::scene3d::SceneLight>,
    instances: BTreeMap<usize, MeshInstances>,
}

#[derive(Clone, Copy)]
pub(super) struct ImportData<'a> {
    pub document: &'a gltf::Document,
    pub metadata: &'a GltfMetadata,
    pub attributes: AttributeSource<'a>,
}

fn prepare_mesh(
    source: ImportData<'_>,
    materials: &mut Materials<'_>,
    hierarchy: &ImportHierarchy<'_>,
    variant: Option<&MaterialVariantMappings>,
    primary: Option<&MeshAsset>,
) -> Result<MeshAsset, String> {
    let document = source.document;
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
        let mesh = animated_mesh(source, materials, hierarchy, variant, rig)?;
        return Ok(MeshAsset::Animated(Arc::new(mesh)));
    }
    let mut geometry = MeshGeometry::default();
    if !hierarchy.nodes.is_empty() || document.scenes().next().is_some() {
        for authored in &hierarchy.nodes {
            if let Some(mesh) = authored.node.mesh() {
                append_mesh(
                    source,
                    mesh,
                    &authored.transform,
                    &mut geometry,
                    materials,
                    variant,
                )?;
            }
        }
    } else {
        // Mesh-only assets can be exported without a scene or nodes.
        for mesh in document.meshes() {
            append_mesh(
                source,
                mesh,
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
    source: ImportData<'_>,
    materials: &mut Materials<'_>,
    hierarchy: &ImportHierarchy<'_>,
    variant: Option<&MaterialVariantMappings>,
    primary: Option<&AnimatedMesh>,
) -> Result<AnimatedMesh, String> {
    use gltf::scene::Transform;
    let ImportData {
        document,
        metadata,
        attributes,
    } = source;
    let buffers = attributes.buffers;
    let names: BTreeMap<_, _> = metadata
        .camera_nodes()
        .into_iter()
        .filter_map(|(node, _, name)| Some((node, name?)))
        .collect();

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
    if hierarchy.nodes.len() > MAX_ANIMATION_NODES {
        return Err("oversized animated glTF node graph".into());
    }
    for (index, authored) in hierarchy.nodes.iter().enumerate() {
        let node = &authored.node;
        let parent = authored.parent;
        indices.insert(node.index(), index);
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
                        source,
                        mesh,
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
                    .with_morph_uvs(
                        morph.count,
                        names.into(),
                        morph.offsets,
                        morph.deltas,
                        morph.uvs,
                    )
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
    source: ImportData<'_>,
    mesh: gltf::Mesh<'_>,
    transform: &Matrix4<f32>,
    geometry: &mut MeshGeometry,
    materials: &mut Materials<'_>,
    variant: Option<&MaterialVariantMappings>,
) -> Result<(), String> {
    for primitive in mesh.primitives() {
        let index = variant.and_then(|mapping| mapping.get(&(mesh.index(), primitive.index())));
        let material = match index {
            Some(&index) => source
                .document
                .materials()
                .nth(index)
                .ok_or("invalid glTF variant material")?,
            None => primitive.material(),
        };
        let material = materials.get(material)?;
        let start = geometry.vertices.len();
        let topology = match primitive.mode() {
            Mode::Points => MeshTopology::Points,
            Mode::Lines | Mode::LineLoop | Mode::LineStrip => MeshTopology::Lines,
            Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan => MeshTopology::Triangles,
        };
        let has_normals = primitive.get(&Semantic::Normals).is_some();
        let tangents = append_primitive(
            source,
            mesh.index(),
            primitive,
            transform,
            (material.uv_sets, material.normal_coordinates),
            geometry,
        )?;
        let lighting = match (topology, has_normals, tangents.is_some()) {
            (MeshTopology::Triangles, _, _) | (_, true, true) => MeshLighting::Material,
            (_, true, false) => MeshLighting::VertexNormal,
            (_, false, _) => MeshLighting::BaseAndEmission,
        };
        let end = geometry.vertices.len();
        if end > start {
            if let Some(tangents) = tangents {
                geometry.tangents.resize(start, [0; 4]);
                geometry.tangents.extend(tangents);
            } else if !geometry.tangents.is_empty() {
                geometry.tangents.resize(end, [0; 4]);
            }
            let volume_inverse = if lighting != MeshLighting::BaseAndEmission
                && materials.has_volume(material.index)
            {
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
                && last.topology == topology
                && last.lighting == lighting
                && last.material == material.index
                && last.volume_inverse == volume_inverse
                && !materials.is_blended(material.index)
            {
                last.vertices.end = end;
            } else {
                geometry.parts.push(MeshPart {
                    lighting,
                    topology,
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
    source: ImportData<'_>,
    mesh: usize,
    primitive: Primitive<'_>,
    transform: &Matrix4<f32>,
    (uv_sets, normal_coordinates): ([Option<u32>; 2], Option<NormalCoordinates>),
    geometry: &mut MeshGeometry,
) -> Result<Option<Vec<MeshTangent>>, String> {
    let mode = primitive.mode();
    let attributes = source.attributes;
    let buffers = attributes.buffers;
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

    let vertex_count = position.count();
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let positions: Vec<_> = attributes
        .read::<3>(position, vertex_count, Attribute::Position)?
        .map(|point| transform.transform_point(&Point3::from(point)).coords)
        .collect();
    let read_uv = |set| {
        let accessor = primitive
            .get(&Semantic::TexCoords(set))
            .ok_or_else(|| format!("glTF texture requires TEXCOORD_{set}"))?;
        Ok::<_, String>(
            attributes
                .read::<2>(accessor, vertex_count, Attribute::Texcoord)?
                .collect::<Vec<_>>(),
        )
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
    let source_morph = morph
        .as_ref()
        .map(|morph| {
            mesh_morph::read_morph(source, mesh, &primitive, morph.count, vertex_count, uv_sets)
        })
        .transpose()?;
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
    let normals = primitive
        .get(&Semantic::Normals)
        .map(|accessor| {
            let normals = attributes.read::<3>(accessor, vertex_count, Attribute::Direction)?;
            let inverse = transform
                .fixed_view::<3, 3>(0, 0)
                .try_inverse()
                .ok_or("glTF node has a singular normal transform")?
                .transpose();
            let normals: Vec<_> = normals
                .map(|normal| (inverse * Vector3::from(normal)).try_normalize(1e-8))
                .collect();
            if normals.len() != positions.len() {
                return Err("glTF normal and position counts differ".into());
            }
            Ok::<_, String>(normals)
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
    let start = vertices.len();
    let mut append_vertex = |index: usize, point: Vector3<f32>, flat_normal| {
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
    };
    let expanded = match mode {
        Mode::Points => Some(indices.len()),
        Mode::Lines if indices.len().is_multiple_of(2) => Some(indices.len()),
        Mode::LineStrip if indices.len() >= 2 => Some((indices.len() - 1) * 2),
        Mode::LineLoop if indices.len() >= 2 => Some(indices.len() * 2),
        Mode::Lines | Mode::LineStrip | Mode::LineLoop => {
            return Err("glTF primitive contains incomplete lines".into());
        }
        Mode::Triangles | Mode::TriangleFan | Mode::TriangleStrip => None,
    };
    if let Some(count) = expanded {
        if count == 0 || start + count > MAX_IMPORTED_VERTICES {
            return Err("glTF point or line primitive has an invalid expanded vertex count".into());
        }
        let source_index = |vertex: usize| {
            let offset = match mode {
                Mode::LineStrip | Mode::LineLoop => vertex.div_ceil(2) % indices.len(),
                _ => vertex,
            };
            indices[offset]
        };
        for index in (0..count).map(source_index) {
            let point = *positions
                .get(index)
                .ok_or("glTF mesh index is out of bounds")?;
            if !point.iter().all(|value| value.is_finite()) {
                return Err("glTF mesh contains an invalid position".into());
            }
            if let Some((morph, source)) = morph.as_mut().zip(source_morph.as_ref()) {
                morph.append_source_vertex(source, index)?;
            }
            append_vertex(index, point, Vector3::z());
        }
        return mesh_tangents::preserve_authored(
            &primitive,
            attributes,
            transform,
            normal_coordinates,
            positions.len(),
            (0..count).map(source_index),
            &vertices[start..],
        );
    }
    if indices.len() < 3 || mode == Mode::Triangles && !indices.len().is_multiple_of(3) {
        return Err("glTF primitive contains incomplete triangles".into());
    }
    let count = if mode == Mode::Triangles {
        indices.len() / 3
    } else {
        indices.len() - 2
    };
    if start + count * 3 > MAX_IMPORTED_VERTICES {
        return Err("glTF mesh exceeds the triangle limit".into());
    }
    let orientation = transform.fixed_view::<3, 3>(0, 0).determinant().signum();
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
            append_vertex(index, point, flat_normal);
        }
    }
    tangent_faces
        .map(|(slot, faces)| {
            mesh_tangents::prepare(
                &primitive,
                attributes,
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
