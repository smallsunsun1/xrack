//! Decode meshopt buffer views before handing them to glTF's ordinary accessors.

use std::ops::Range;

use gltf::json::{Index, Root, validation::USize64};
use serde::Deserialize;
use serde_json::{Map, Value};
use xrack_assets::MAX_ASSET_BYTES;
use xrack_syntax::gltf_metadata::{EXT_MESHOPT_COMPRESSION, KHR_MESHOPT_COMPRESSION};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Compression {
    Ext,
    Khr,
}

impl Compression {
    fn name(self) -> &'static str {
        match self {
            Self::Ext => EXT_MESHOPT_COMPRESSION,
            Self::Khr => KHR_MESHOPT_COMPRESSION,
        }
    }

    fn header(self, mode: Mode, bytes: &[u8]) -> bool {
        match mode {
            Mode::Attributes => {
                bytes.first() == Some(&0xa0) || self == Self::Khr && bytes.first() == Some(&0xa1)
            }
            Mode::Triangles => bytes.first() == Some(&0xe1),
            Mode::Indices => bytes.first() == Some(&0xd1),
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Mode {
    Attributes,
    Triangles,
    Indices,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Filter {
    #[default]
    None,
    Octahedral,
    Quaternion,
    Exponential,
    Color,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Description {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    byte_stride: usize,
    count: usize,
    mode: Mode,
    #[serde(default)]
    filter: Filter,
}

impl Description {
    fn output_size(&self, compression: Compression, view_stride: Option<usize>) -> Option<usize> {
        let stride = self.byte_stride;
        let mode_valid = match self.mode {
            Mode::Attributes => (4..=256).contains(&stride) && stride.is_multiple_of(4),
            Mode::Triangles => matches!(stride, 2 | 4) && self.count.is_multiple_of(3),
            Mode::Indices => matches!(stride, 2 | 4),
        };
        let filter_valid = if matches!(self.mode, Mode::Attributes) {
            match self.filter {
                Filter::None | Filter::Exponential => true,
                Filter::Octahedral => matches!(stride, 4 | 8),
                Filter::Quaternion => stride == 8,
                Filter::Color => compression == Compression::Khr && matches!(stride, 4 | 8),
            }
        } else {
            matches!(self.filter, Filter::None)
        };
        if self.count == 0
            || self.byte_length == 0
            || !mode_valid
            || !filter_valid
            || view_stride.is_some_and(|view_stride| view_stride != stride)
        {
            return None;
        }
        self.count.checked_mul(stride)
    }
}

struct Block {
    view: usize,
    compression: Compression,
    description: Description,
    input: Range<usize>,
    output: Range<usize>,
}

impl Block {
    fn decode(&self, source: &[u8], destination: &mut [u8]) -> Result<(), &'static str> {
        let Description {
            mode,
            filter,
            count,
            byte_stride: stride,
            ..
        } = self.description;
        if !self.compression.header(mode, source) {
            return Err("unexpected meshopt bitstream version or mode");
        }
        let decoded = match mode {
            Mode::Attributes => rmeshopt::decode_vertex_buffer(destination, count, stride, source),
            Mode::Indices | Mode::Triangles => {
                let decode = match mode {
                    Mode::Triangles => rmeshopt::decode_index_buffer,
                    _ => rmeshopt::decode_index_sequence,
                };
                if stride == 4 {
                    decode(destination, count, 4, source)
                } else {
                    // Check representability before narrowing indices to the view's width.
                    let mut wide = vec![0; count * 4];
                    if !decode(&mut wide, count, 4, source) {
                        return Err("invalid meshopt index data");
                    }
                    for (&bytes, target) in wide
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(destination.chunks_exact_mut(2))
                    {
                        let index = u16::try_from(u32::from_le_bytes(bytes))
                            .map_err(|_| "meshopt index exceeds the 16-bit view")?;
                        target.copy_from_slice(&index.to_le_bytes());
                    }
                    true
                }
            }
        };
        if !decoded {
            return Err("invalid meshopt compressed data");
        }
        match filter {
            Filter::None => {}
            Filter::Octahedral => rmeshopt::decode_filter_oct(destination, count, stride),
            Filter::Quaternion => rmeshopt::decode_filter_quat(destination, count, stride),
            Filter::Exponential => rmeshopt::decode_filter_exp(destination, count, stride),
            Filter::Color => rmeshopt::decode_filter_color(destination, count, stride),
        }
        Ok(())
    }
}

pub(super) struct BufferDecode {
    used: Vec<bool>,
    blocks: Vec<Block>,
    decoded_size: usize,
}

impl BufferDecode {
    pub fn prepare(root: &mut Root, has_bin: bool) -> Result<Option<Self>, String> {
        if !root
            .extensions_used
            .iter()
            .chain(&root.extensions_required)
            .any(|name| {
                matches!(
                    name.as_str(),
                    EXT_MESHOPT_COMPRESSION | KHR_MESHOPT_COMPRESSION
                )
            })
        {
            return Ok(None);
        }
        let fallbacks = root
            .buffers
            .iter()
            .map(|buffer| {
                let Some((codec, value)) =
                    extension(buffer.extensions.as_ref().map(|ext| &ext.others))?
                else {
                    return Ok(None);
                };
                #[derive(Deserialize)]
                struct Fallback {
                    #[serde(default)]
                    fallback: bool,
                }
                let marker = Fallback::deserialize(value)
                    .map_err(|error| format!("invalid {} buffer marker: {error}", codec.name()))?;
                Ok(marker.fallback.then_some(codec))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let original_count = root.buffers.len();
        let decoded_buffer = u32::try_from(original_count).map_err(|_| "too many glTF buffers")?;
        let mut plan = Self {
            used: vec![false; original_count],
            blocks: Vec::new(),
            decoded_size: 0,
        };
        for (index, view) in root.buffer_views.iter_mut().enumerate() {
            let parent = view.buffer.value();
            let parent_data = root
                .buffers
                .get(parent)
                .ok_or_else(|| format!("glTF buffer view #{index} has an invalid parent buffer"))?;
            let start = view.byte_offset.map_or(0, |value| value.0);
            if start
                .checked_add(view.byte_length.0)
                .is_none_or(|end| end > parent_data.byte_length.0)
            {
                return Err(format!("glTF buffer view #{index} is out of bounds"));
            }
            let encoding = extension(view.extensions.as_ref().map(|ext| &ext.others))?;
            let Some((codec, value)) = encoding else {
                if fallbacks[parent].is_some() {
                    return Err(format!(
                        "glTF buffer view #{index} references a meshopt fallback without compressed data"
                    ));
                }
                plan.used[parent] = true;
                continue;
            };
            if fallbacks[parent].is_some_and(|fallback| fallback != codec) {
                return Err(format!(
                    "glTF buffer view #{index} uses a different extension than its fallback buffer"
                ));
            }
            if parent_data.uri.is_none()
                && !(parent == 0 && has_bin)
                && !root
                    .extensions_required
                    .iter()
                    .any(|name| name == codec.name())
            {
                return Err(format!(
                    "glTF buffer view #{index} has no fallback storage and requires {}",
                    codec.name()
                ));
            }
            let description = Description::deserialize(value).map_err(|error| {
                format!("invalid {} buffer view #{index}: {error}", codec.name())
            })?;
            let source = root.buffers.get(description.buffer).ok_or_else(|| {
                format!("glTF buffer view #{index} has an invalid meshopt source buffer")
            })?;
            if fallbacks[description.buffer].is_some() {
                return Err(format!(
                    "glTF buffer view #{index} uses a meshopt fallback as compressed input"
                ));
            }
            let input_end = description
                .byte_offset
                .checked_add(description.byte_length)
                .filter(|&end| end as u64 <= source.byte_length.0)
                .ok_or_else(|| format!("compressed glTF buffer view #{index} is out of bounds"))?;
            let length = description.output_size(codec, view.byte_stride.as_ref().map(|stride| stride.0))
                .filter(|&length| length as u64 == view.byte_length.0)
                .ok_or_else(|| format!("invalid meshopt mode, stride, filter, or output length for buffer view #{index}"))?;
            let start = plan.decoded_size.next_multiple_of(4);
            let end = start
                .checked_add(length)
                .filter(|&end| end as u64 <= MAX_ASSET_BYTES)
                .ok_or("decoded glTF meshopt buffer views exceed 16 MiB")?;
            plan.used[description.buffer] = true;
            plan.decoded_size = end;
            plan.blocks.push(Block {
                view: index,
                compression: codec,
                input: description.byte_offset..input_end,
                description,
                output: start..end,
            });
            // Only the backing storage changes. Accessor and image view indices,
            // offsets within the view, and declared stride stay the same.
            view.buffer = Index::new(decoded_buffer);
            view.byte_offset = Some(USize64(start as u64));
        }
        if !plan.blocks.is_empty() {
            let decoded =
                serde_json::from_value(serde_json::json!({ "byteLength": plan.decoded_size }))
                    .map_err(|error| error.to_string())?;
            root.buffers.push(decoded);
        }
        Ok(Some(plan))
    }

    pub fn reads_buffer(&self, index: usize) -> bool {
        self.used.get(index).copied().unwrap_or(false)
    }

    pub fn reads_compressed_source(&self, index: usize) -> bool {
        self.blocks
            .iter()
            .any(|block| block.description.buffer == index)
    }

    pub fn decode(&self, buffers: &mut [Vec<u8>]) -> Result<(), String> {
        if self.blocks.is_empty() {
            return Ok(());
        }
        let mut decoded = vec![0; self.decoded_size];
        for block in &self.blocks {
            let source = buffers
                .get(block.description.buffer)
                .and_then(|buffer| buffer.get(block.input.clone()))
                .ok_or_else(|| {
                    format!(
                        "compressed glTF buffer view #{} is out of bounds",
                        block.view
                    )
                })?;
            block
                .decode(source, &mut decoded[block.output.clone()])
                .map_err(|error| format!("glTF buffer view #{}: {error}", block.view))?;
        }
        *buffers
            .get_mut(self.used.len())
            .ok_or("missing decoded glTF buffer slot")? = decoded;
        Ok(())
    }
}

fn extension(values: Option<&Map<String, Value>>) -> Result<Option<(Compression, &Value)>, String> {
    let Some(values) = values else {
        return Ok(None);
    };
    match (
        values.get(EXT_MESHOPT_COMPRESSION),
        values.get(KHR_MESHOPT_COMPRESSION),
    ) {
        (Some(_), Some(_)) => {
            Err("glTF EXT and KHR meshopt compression cannot share a buffer or view".into())
        }
        (Some(value), None) => Ok(Some((Compression::Ext, value))),
        (None, Some(value)) => Ok(Some((Compression::Khr, value))),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document(encoded: usize, count: usize, stride: usize, mode: &str) -> Value {
        json!({
            "asset": {"version": "2.0"},
            "extensionsUsed": [KHR_MESHOPT_COMPRESSION],
            "buffers": [
                {"byteLength": encoded, "uri": "input.bin"},
                {"byteLength": count * stride, "uri": "fallback.bin",
                    "extensions": {KHR_MESHOPT_COMPRESSION: {"fallback": true}}}
            ],
            "bufferViews": [
                {"buffer": 1, "byteLength": count * stride, "extensions": {
                    KHR_MESHOPT_COMPRESSION: {
                        "buffer": 0, "byteLength": encoded, "byteStride": stride,
                        "count": count, "mode": mode
                    }
                }}
            ]
        })
    }

    #[test]
    fn decoding_retargets_the_view_without_loading_the_fallback() {
        let raw: Vec<_> = [0.5_f32, 1.25, -3.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let bound = rmeshopt::encode_vertex_buffer_bound(3, 4);
        let mut compressed = vec![0; bound];
        let length = rmeshopt::encode_vertex_buffer(&mut compressed, &raw, 3, 4);
        assert!(length > 0);
        compressed.truncate(length);
        let mut root: Root = serde_json::from_value(document(length, 3, 4, "ATTRIBUTES")).unwrap();

        let plan = BufferDecode::prepare(&mut root, false).unwrap().unwrap();
        assert!(plan.reads_buffer(0));
        assert!(!plan.reads_buffer(1));
        assert_eq!(root.buffer_views[0].buffer.value(), 2);
        let mut buffers = [compressed, Vec::new(), Vec::new()];
        plan.decode(&mut buffers).unwrap();
        assert_eq!(buffers[2], raw);

        buffers[0].pop();
        assert!(plan.decode(&mut buffers).is_err());
    }

    #[test]
    fn declared_expansion_and_fallbacks_are_checked_before_allocating() {
        let excessive = MAX_ASSET_BYTES as usize / 4 + 1;
        let mut root = serde_json::from_value(document(1, excessive, 4, "ATTRIBUTES")).unwrap();
        assert!(matches!(
            BufferDecode::prepare(&mut root, false),
            Err(error) if error.contains("decoded glTF meshopt buffer views exceed")
        ));

        let mut optional = document(4, 1, 4, "ATTRIBUTES");
        optional["buffers"][1]["uri"] = Value::Null;
        let mut root = serde_json::from_value(optional).unwrap();
        assert!(matches!(
            BufferDecode::prepare(&mut root, false),
            Err(error) if error.contains("no fallback storage")
        ));

        let mut fallback = document(4, 1, 4, "ATTRIBUTES");
        fallback["bufferViews"][0]["extensions"] = json!({});
        let mut root = serde_json::from_value(fallback).unwrap();
        assert!(matches!(
            BufferDecode::prepare(&mut root, false),
            Err(error) if error.contains("fallback without compressed data")
        ));

        let mut root = serde_json::from_value(document(4, 4, 2, "TRIANGLES")).unwrap();
        assert!(matches!(
            BufferDecode::prepare(&mut root, false),
            Err(error) if error.contains("invalid meshopt mode")
        ));
    }

    #[test]
    fn indices_outside_the_declared_width_are_rejected() {
        let mut compressed = vec![0; rmeshopt::encode_index_sequence_bound(1, 70_001)];
        let length = rmeshopt::encode_index_sequence(&mut compressed, &[70_000]);
        assert!(length > 0);
        compressed.truncate(length);
        let mut root = serde_json::from_value(document(length, 1, 2, "INDICES")).unwrap();

        let plan = BufferDecode::prepare(&mut root, false).unwrap().unwrap();
        let mut buffers = [compressed, Vec::new(), Vec::new()];
        assert!(matches!(
            plan.decode(&mut buffers),
            Err(error) if error.contains("index exceeds the 16-bit view")
        ));
    }
}
