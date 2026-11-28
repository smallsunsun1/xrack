//! Materialize Draco primitives as ordinary glTF accessors.

use draco_core::{
    DecodeLimits, DecoderBuffer, Mesh, MeshDecoder, PointIndex, draco_types::DataType,
};
use gltf::json::{
    Index, Root,
    accessor::{Accessor, ComponentType, GenericComponentType, Type},
    buffer::{Buffer, Stride, View},
    mesh::{Mode, Semantic},
    validation::{Checked, USize64},
};
use serde::Deserialize;
use std::{collections::BTreeMap, ops::Range};
use xrack_assets::MAX_ASSET_BYTES;
use xrack_runtime::scene3d::MAX_IMPORTED_VERTICES;
use xrack_syntax::gltf_metadata::{DRACO_COMPRESSION, GltfMetadata, MESH_GPU_INSTANCING};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrimitiveDecode {
    buffer_view: usize,
    attributes: BTreeMap<Checked<Semantic>, u32>,
}

pub(super) struct DracoDecode {
    primitives: BTreeMap<(usize, usize), PrimitiveDecode>,
    fallback_buffers: Vec<bool>,
    replaced_accessors: Vec<bool>,
}

impl DracoDecode {
    pub fn prepare(root: &Root, metadata: &GltfMetadata) -> Result<Option<Self>, String> {
        let used = metadata.uses_draco_compression();
        let required = root
            .extensions_required
            .iter()
            .any(|name| name == DRACO_COMPRESSION);
        if required && !used {
            return Err(format!(
                "glTF must declare {DRACO_COMPRESSION} in extensionsUsed"
            ));
        }
        let mut primitives = BTreeMap::new();
        for (mesh, model) in root.meshes.iter().enumerate() {
            for (part, primitive) in model.primitives.iter().enumerate() {
                let Some(raw) = primitive
                    .extensions
                    .as_ref()
                    .and_then(|extensions| extensions.others.get(DRACO_COMPRESSION))
                else {
                    continue;
                };
                let context =
                    |error: &str| format!("glTF mesh #{mesh} primitive #{part} Draco: {error}");
                if !used {
                    return Err(context("missing declaration in extensionsUsed"));
                }
                let spec = PrimitiveDecode::deserialize(raw)
                    .map_err(|error| context(&error.to_string()))?;
                if spec.buffer_view >= root.buffer_views.len() || spec.attributes.is_empty() {
                    return Err(context("missing or invalid buffer view or attribute map"));
                }
                if !matches!(
                    primitive.mode,
                    Checked::Valid(Mode::Triangles | Mode::TriangleStrip)
                ) {
                    return Err(context("requires triangles or a triangle strip"));
                }
                for semantic in spec.attributes.keys() {
                    if *semantic == Checked::Invalid || !primitive.attributes.contains_key(semantic)
                    {
                        return Err(context(
                            "compressed attribute does not have a corresponding accessor",
                        ));
                    }
                }
                for accessor in primitive
                    .attributes
                    .values()
                    .copied()
                    .chain(primitive.indices)
                {
                    if accessor.value() >= root.accessors.len() {
                        return Err(context("accessor index is out of bounds"));
                    }
                }
                primitives.insert((mesh, part), spec);
            }
        }
        if primitives.is_empty() {
            return Ok(None);
        }
        let (fallback_buffers, replaced_accessors) = fallback_inputs(root, metadata, &primitives);
        Ok(Some(Self {
            primitives,
            fallback_buffers,
            replaced_accessors,
        }))
    }

    pub fn fallback_only(&self, buffer: usize) -> bool {
        self.fallback_buffers.get(buffer).copied().unwrap_or(false)
    }

    pub fn decode(self, mut root: Root, buffers: &mut Vec<Vec<u8>>) -> Result<Root, String> {
        let mut output = DecodedBuffer {
            buffer: Index::new(buffers.len() as u32),
            bytes: Vec::new(),
            aliases: Vec::new(),
        };
        for ((mesh, part), spec) in self.primitives {
            let context = |error| format!("glTF mesh #{mesh} primitive #{part} Draco: {error}");
            normalize_primitive(&mut root, mesh, part, &spec, buffers, &mut output)
                .map_err(context)?;
        }
        // gltf-rs validates even the now-unreferenced accessor declarations.
        // Give Draco-only placeholders backing data. Keep the per-primitive
        // copies because the same declaration can describe distinct streams.
        for (original, decoded) in output.aliases {
            let entry = &root.accessors[original.value()];
            if self.replaced_accessors[original.value()]
                && entry.buffer_view.is_none()
                && entry.sparse.is_none()
            {
                root.accessors[original.value()] = root.accessors[decoded.value()].clone();
            }
        }
        root.buffers.push(Buffer {
            byte_length: USize64(output.bytes.len() as u64),
            uri: None,
            extensions: None,
            extras: Default::default(),
        });
        buffers.push(output.bytes);
        Ok(root)
    }
}

fn fallback_inputs(
    root: &Root,
    metadata: &GltfMetadata,
    primitives: &BTreeMap<(usize, usize), PrimitiveDecode>,
) -> (Vec<bool>, Vec<bool>) {
    let mut replaced = vec![false; root.accessors.len()];
    for (&(mesh, part), spec) in primitives {
        let primitive = &root.meshes[mesh].primitives[part];
        for index in spec
            .attributes
            .keys()
            .map(|name| primitive.attributes[name])
            .chain(primitive.indices)
        {
            replaced[index.value()] = true;
        }
    }

    // A replaced accessor can still be shared with an ordinary primitive, a
    // morph target, an animation, a skin, or an instance attribute.
    let mut keep_accessor = |index: usize| {
        if let Some(flag) = replaced.get_mut(index) {
            *flag = false;
        }
    };
    for (mesh, model) in root.meshes.iter().enumerate() {
        for (part, primitive) in model.primitives.iter().enumerate() {
            let decode = primitives.get(&(mesh, part));
            for (name, index) in &primitive.attributes {
                if decode.is_none_or(|spec| !spec.attributes.contains_key(name)) {
                    keep_accessor(index.value());
                }
            }
            if decode.is_none()
                && let Some(index) = primitive.indices
            {
                keep_accessor(index.value());
            }
            for (target_index, target) in primitive.targets.iter().flatten().enumerate() {
                for index in [target.positions, target.normals, target.tangents]
                    .into_iter()
                    .flatten()
                {
                    keep_accessor(index.value());
                }
                for (_, index) in metadata.morph_texcoord_accessors(mesh, part, target_index) {
                    keep_accessor(index);
                }
            }
        }
    }
    for animation in &root.animations {
        for sampler in &animation.samplers {
            keep_accessor(sampler.input.value());
            keep_accessor(sampler.output.value());
        }
    }
    for skin in &root.skins {
        if let Some(index) = skin.inverse_bind_matrices {
            keep_accessor(index.value());
        }
    }
    for node in &root.nodes {
        let attributes = node
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.others.get(MESH_GPU_INSTANCING))
            .and_then(|extension| extension.get("attributes"))
            .and_then(serde_json::Value::as_object);
        for index in attributes.into_iter().flat_map(|map| map.values()) {
            if let Some(index) = index.as_u64().and_then(|value| usize::try_from(value).ok()) {
                keep_accessor(index);
            }
        }
    }

    // Use two flags because multiple accessors may share an interleaved view.
    let mut views = vec![(false, false); root.buffer_views.len()];
    let mut mark_view = |index: usize, fallback: bool| {
        if let Some(flags) = views.get_mut(index) {
            if fallback {
                flags.0 = true;
            } else {
                flags.1 = true;
            }
        }
    };
    for (accessor, &fallback) in root.accessors.iter().zip(&replaced) {
        if let Some(view) = accessor.buffer_view {
            mark_view(view.value(), fallback);
        }
        if let Some(sparse) = &accessor.sparse {
            mark_view(sparse.indices.buffer_view.value(), fallback);
            mark_view(sparse.values.buffer_view.value(), fallback);
        }
    }
    for image in &root.images {
        if let Some(view) = image.buffer_view {
            mark_view(view.value(), false);
        }
    }
    for spec in primitives.values() {
        mark_view(spec.buffer_view, false);
    }
    let mut buffers = vec![(false, false); root.buffers.len()];
    for (view, flags) in root.buffer_views.iter().zip(views) {
        if let Some(buffer) = buffers.get_mut(view.buffer.value()) {
            if flags == (true, false) {
                buffer.0 = true;
            } else {
                buffer.1 = true;
            }
        }
    }
    let fallback_buffers = buffers
        .into_iter()
        .map(|(fallback, live)| fallback && !live)
        .collect();
    (fallback_buffers, replaced)
}

fn normalize_primitive(
    root: &mut Root,
    mesh: usize,
    part: usize,
    spec: &PrimitiveDecode,
    buffers: &[Vec<u8>],
    output: &mut DecodedBuffer,
) -> Result<(), String> {
    let view = &root.buffer_views[spec.buffer_view];
    let offset = view.byte_offset.map_or(0, |value| value.0);
    let range = offset
        .checked_add(view.byte_length.0)
        .and_then(|end| Some(usize::try_from(offset).ok()?..usize::try_from(end).ok()?))
        .ok_or("compressed buffer view range is out of bounds")?;
    let payload = buffers
        .get(view.buffer.value())
        .and_then(|buffer| buffer.get(range))
        .ok_or("compressed buffer view is out of bounds")?;
    let limits = DecodeLimits::default()
        .with_max_points(MAX_IMPORTED_VERTICES as u64)
        .with_max_faces(MAX_IMPORTED_VERTICES as u64 / 3)
        .with_max_decoded_bytes(MAX_ASSET_BYTES);
    let mut decoded = Mesh::new();
    MeshDecoder::new()
        .decode(
            &mut DecoderBuffer::new(payload).with_limits(limits),
            &mut decoded,
        )
        .map_err(|error| error.to_string())?;
    let points = decoded.num_points();
    if !(1..=MAX_IMPORTED_VERTICES).contains(&points) || decoded.num_faces() == 0 {
        return Err("decoded mesh has an invalid vertex or face count".into());
    }

    let primitive = &mut root.meshes[mesh].primitives[part];
    for (semantic, &unique_id) in &spec.attributes {
        let index = primitive
            .attributes
            .get_mut(semantic)
            .ok_or("missing attribute accessor")?;
        let accessor = root
            .accessors
            .get(index.value())
            .ok_or("invalid attribute accessor")?
            .clone();
        let attribute = decoded
            .attribute_by_unique_id(unique_id)
            .ok_or_else(|| format!("missing compressed attribute with unique ID {unique_id}"))?;
        if accessor.count.0 != points as u64 {
            return Err(format!(
                "attribute {semantic:?} declares {} vertices but Draco decoded {points}",
                accessor.count.0
            ));
        }
        let (kind, bytes) = component(accessor.component_type)?;
        let columns = match accessor.type_ {
            Checked::Valid(Type::Mat2) => 2,
            Checked::Valid(Type::Mat3) => 3,
            Checked::Valid(Type::Mat4) => 4,
            _ => 1,
        };
        let per_column = match accessor.type_ {
            Checked::Valid(Type::Scalar) => 1,
            Checked::Valid(Type::Vec2 | Type::Mat2) => 2,
            Checked::Valid(Type::Vec3 | Type::Mat3) => 3,
            Checked::Valid(Type::Vec4 | Type::Mat4) => 4,
            _ => return Err("invalid accessor shape".into()),
        };
        if kind != attribute.data_type()
            || columns * per_column != attribute.num_components() as usize
        {
            return Err(format!(
                "compressed attribute {semantic:?} does not match its accessor type"
            ));
        }
        let column_bytes = per_column * bytes;
        let column_stride = if columns == 1 {
            column_bytes
        } else {
            column_bytes.next_multiple_of(4)
        };
        let stride = (columns * column_stride).next_multiple_of(4);
        let source_stride = usize::try_from(attribute.byte_stride())
            .ok()
            .filter(|&size| size >= columns * column_bytes)
            .ok_or("invalid compressed attribute stride")?;
        let range = output.reserve(points * stride)?;
        let input = attribute.buffer().data();
        for (point, vertex) in output.bytes[range.clone()]
            .chunks_exact_mut(stride)
            .enumerate()
        {
            let mapped = attribute.mapped_index(PointIndex(point as u32)).0 as usize;
            let end = mapped
                .checked_mul(source_stride)
                .and_then(|start| start.checked_add(columns * column_bytes))
                .ok_or("invalid compressed attribute mapping")?;
            let value = input
                .get(end - columns * column_bytes..end)
                .ok_or("compressed attribute mapping is out of bounds")?;
            for (column, bytes) in value.chunks_exact(column_bytes).enumerate() {
                let start = column * column_stride;
                vertex[start..start + column_bytes].copy_from_slice(bytes);
            }
        }
        let original = *index;
        *index = output.accessor(
            &mut root.accessors,
            &mut root.buffer_views,
            accessor,
            range,
            Some(stride),
        );
        output.aliases.push((original, *index));
    }

    let faces = decoded.faces();
    let count = faces.len() * 3;
    let mut indices = if let Some(index) = primitive.indices {
        let accessor = root
            .accessors
            .get(index.value())
            .ok_or("invalid index accessor")?
            .clone();
        if primitive.mode == Checked::Valid(Mode::Triangles) && accessor.count.0 != count as u64 {
            return Err("decoded face indices do not match the triangle accessor count".into());
        }
        accessor
    } else {
        serde_json::from_value(
            serde_json::json!({ "componentType": 5125, "count": count, "type": "SCALAR" }),
        )
        .map_err(|error| error.to_string())?
    };
    let (kind, size) = component(indices.component_type)?;
    if !matches!(kind, DataType::Uint8 | DataType::Uint16 | DataType::Uint32)
        || indices.type_ != Checked::Valid(Type::Scalar)
        || indices.normalized
    {
        return Err("invalid Draco index type".into());
    }
    let range = output.reserve(count * size)?;
    for (point, bytes) in faces
        .iter()
        .flatten()
        .zip(output.bytes[range.clone()].chunks_exact_mut(size))
    {
        let index = point.0;
        if index as usize >= points || (size < 4 && index >= 1u32 << (size * 8)) {
            return Err("decoded index exceeds the vertex count or accessor type".into());
        }
        bytes.copy_from_slice(&index.to_le_bytes()[..size]);
    }
    indices.count = USize64(count as u64);
    let indices = output.accessor(
        &mut root.accessors,
        &mut root.buffer_views,
        indices,
        range,
        None,
    );
    if let Some(original) = primitive.indices.replace(indices) {
        output.aliases.push((original, indices));
    }
    primitive.mode = Checked::Valid(Mode::Triangles);
    Ok(())
}

fn component(kind: Checked<GenericComponentType>) -> Result<(DataType, usize), String> {
    use ComponentType::*;
    Ok(match kind {
        Checked::Valid(GenericComponentType(I8)) => (DataType::Int8, 1),
        Checked::Valid(GenericComponentType(U8)) => (DataType::Uint8, 1),
        Checked::Valid(GenericComponentType(I16)) => (DataType::Int16, 2),
        Checked::Valid(GenericComponentType(U16)) => (DataType::Uint16, 2),
        Checked::Valid(GenericComponentType(U32)) => (DataType::Uint32, 4),
        Checked::Valid(GenericComponentType(F32)) => (DataType::Float32, 4),
        Checked::Invalid => return Err("invalid component type".into()),
    })
}

struct DecodedBuffer {
    buffer: Index<Buffer>,
    bytes: Vec<u8>,
    aliases: Vec<(Index<Accessor>, Index<Accessor>)>,
}

impl DecodedBuffer {
    fn reserve(&mut self, len: usize) -> Result<Range<usize>, String> {
        let start = self.bytes.len().next_multiple_of(4);
        let end = start
            .checked_add(len)
            .filter(|&end| end as u64 <= MAX_ASSET_BYTES)
            .ok_or("decoded Draco buffers exceed 16 MiB")?;
        self.bytes.resize(end, 0);
        Ok(start..end)
    }

    fn accessor(
        &self,
        accessors: &mut Vec<Accessor>,
        views: &mut Vec<View>,
        mut accessor: Accessor,
        range: Range<usize>,
        stride: Option<usize>,
    ) -> Index<Accessor> {
        accessor.buffer_view = Some(Index::new(views.len() as u32));
        accessor.byte_offset = None;
        accessor.sparse = None;
        // Append accessors. A fallback accessor can still be referenced by
        // another primitive or by an animation.
        let index = Index::new(accessors.len() as u32);
        accessors.push(accessor);
        views.push(View {
            buffer: self.buffer,
            byte_length: USize64(range.len() as u64),
            byte_offset: Some(USize64(range.start as u64)),
            byte_stride: stride.map(Stride),
            target: None,
            extensions: None,
            extras: Default::default(),
        });
        index
    }
}
