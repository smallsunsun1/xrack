//! Numeric vertex attributes from glTF, including KHR_mesh_quantization.

use gltf::{
    Accessor,
    accessor::{DataType, Dimensions, Item, Iter},
};

use crate::mesh::validate_accessor_with_sparse;

#[derive(Clone, Copy)]
pub(super) struct AttributeSource<'a> {
    pub buffers: &'a [Vec<u8>],
    pub quantized: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Attribute {
    Position,
    Direction,
    Texcoord,
    MorphPosition,
    MorphTexcoord,
}

impl Attribute {
    fn permits(self, kind: DataType, normalized: bool, quantized: bool) -> bool {
        if kind == DataType::F32 {
            return !normalized;
        }
        let signed = matches!(kind, DataType::I8 | DataType::I16);
        let unsigned = matches!(kind, DataType::U8 | DataType::U16);
        match self {
            Self::Position => quantized && (signed || unsigned),
            Self::Direction => quantized && signed && normalized,
            Self::Texcoord => (quantized && signed) || (unsigned && (normalized || quantized)),
            Self::MorphPosition => quantized && signed,
            Self::MorphTexcoord => quantized && signed && !normalized,
        }
    }
}

impl<'a> AttributeSource<'a> {
    pub fn read<const N: usize>(
        self,
        accessor: Accessor<'_>,
        count: usize,
        attribute: Attribute,
    ) -> Result<Box<dyn ExactSizeIterator<Item = [f32; N]> + 'a>, String>
    where
        [f32; N]: Item,
        [i8; N]: Item,
        [i16; N]: Item,
        [u8; N]: Item,
        [u16; N]: Item,
    {
        let normalized = accessor.normalized();
        let kind = accessor.data_type();
        let dimensions = match N {
            2 => Dimensions::Vec2,
            3 => Dimensions::Vec3,
            4 => Dimensions::Vec4,
            _ => return Err("unsupported glTF vertex attribute dimension".into()),
        };
        if accessor.count() != count
            || accessor.dimensions() != dimensions
            || !attribute.permits(kind, normalized, self.quantized)
        {
            return Err(format!("invalid glTF {attribute:?} accessor type or count"));
        }
        validate_accessor_with_sparse(&accessor, self.buffers)?;
        let data = |buffer: gltf::Buffer<'_>| self.buffers.get(buffer.index()).map(Vec::as_slice);
        let unreadable = || format!("unable to read glTF {attribute:?} accessor");
        macro_rules! integers {
            ($type:ty, $max:expr) => {{
                let values = Iter::<[$type; N]>::new(accessor, data).ok_or_else(unreadable)?;
                Box::new(values.map(move |vector| {
                    vector.map(|integer| {
                        let value = f32::from(integer);
                        if normalized {
                            (value / $max).max(-1.0)
                        } else {
                            value
                        }
                    })
                }))
            }};
        }
        Ok(match kind {
            DataType::F32 => {
                Box::new(Iter::<[f32; N]>::new(accessor, data).ok_or_else(unreadable)?)
            }
            DataType::I8 => integers!(i8, 127.0),
            DataType::I16 => integers!(i16, 32767.0),
            DataType::U8 => integers!(u8, 255.0),
            DataType::U16 => integers!(u16, 65535.0),
            _ => return Err(unreadable()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_formats_are_scoped_to_their_semantic_and_extension() {
        use Attribute::*;
        assert!(Position.permits(DataType::I16, false, true));
        assert!(Position.permits(DataType::U16, true, true));
        assert!(!Position.permits(DataType::I16, false, false));
        assert!(Direction.permits(DataType::I8, true, true));
        assert!(!Direction.permits(DataType::I8, false, true));
        assert!(!Direction.permits(DataType::U8, true, true));
        assert!(Texcoord.permits(DataType::U8, true, false));
        assert!(!Texcoord.permits(DataType::U8, false, false));
        assert!(Texcoord.permits(DataType::U8, false, true));
        assert!(Texcoord.permits(DataType::I16, true, true));
        assert!(MorphPosition.permits(DataType::I16, true, true));
        assert!(!MorphPosition.permits(DataType::U16, false, true));
        assert!(MorphTexcoord.permits(DataType::I8, false, true));
        assert!(!MorphTexcoord.permits(DataType::I8, true, true));
        assert!(!Position.permits(DataType::U32, false, true));
    }

    #[test]
    fn signed_normalization_clamps_the_negative_endpoint_and_honors_stride() {
        let gltf = gltf::Gltf::from_slice_without_validation(br#"{
            "asset":{"version":"2.0"},
            "buffers":[{"byteLength":8}],
            "bufferViews":[{"buffer":0,"byteLength":8,"byteStride":4}],
            "accessors":[{"bufferView":0,"componentType":5120,"normalized":true,"count":2,"type":"VEC3"}]
        }"#).unwrap();
        let accessor = gltf.document.accessors().next().unwrap();
        let buffers = [vec![128, 0, 127, 51, 129, 127, 0, 77]];
        let source = AttributeSource {
            buffers: &buffers,
            quantized: true,
        };
        let normals: Vec<_> = source
            .read::<3>(accessor.clone(), 2, Attribute::Direction)
            .unwrap()
            .collect();
        assert_eq!(normals, [[-1.0, 0.0, 1.0], [-1.0, 1.0, 0.0]]);
        let unextended = AttributeSource {
            quantized: false,
            ..source
        };
        assert!(
            unextended
                .read::<3>(accessor, 2, Attribute::Direction)
                .is_err()
        );
    }
}
