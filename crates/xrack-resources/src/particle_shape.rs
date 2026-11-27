//! Decoder for version 1 of the Qt Quick 3D particle point file format.
use xrack_runtime::scene3d::{MAX_PARTICLE_POINT_COUNT, ParticlePointCloud};

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> Option<&[u8]> {
        let (value, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(value)
    }

    // A missing length denotes an indefinite-length CBOR container.
    fn head(&mut self) -> Option<(u8, Option<u64>)> {
        let head = *self.bytes(1)?.first()?;
        let value = match head & 31 {
            n @ 0..=23 => Some(u64::from(n)),
            24 => Some(u64::from(*self.bytes(1)?.first()?)),
            25 => Some(u64::from(u16::from_be_bytes(
                self.bytes(2)?.try_into().ok()?,
            ))),
            26 => Some(u64::from(u32::from_be_bytes(
                self.bytes(4)?.try_into().ok()?,
            ))),
            27 => Some(u64::from_be_bytes(self.bytes(8)?.try_into().ok()?)),
            31 => None,
            _ => return None,
        };
        Some((head, value))
    }

    fn array(&mut self) -> Option<Option<u64>> {
        let (head, length) = self.head()?;
        (head >> 5 == 4).then_some(length)
    }

    fn end_array(&mut self, length: Option<u64>) -> Option<()> {
        if length.is_none() {
            (self.bytes(1)? == [0xff]).then_some(())?;
        }
        Some(())
    }

    fn coordinate(&mut self) -> Option<f32> {
        let (tag, value) = self.head()?;
        let value = value?;
        let value = match tag {
            0xf9 => {
                let bits = value as u16;
                let exponent = (bits >> 10) & 31;
                if exponent == 31 {
                    return None;
                }
                let fraction = f32::from(bits & 1023) / 1024.0;
                let magnitude = if exponent == 0 {
                    fraction * 2.0_f32.powi(-14)
                } else {
                    (1.0 + fraction) * 2.0_f32.powi(i32::from(exponent) - 15)
                };
                if bits & 0x8000 == 0 {
                    magnitude
                } else {
                    -magnitude
                }
            }
            0xfa => f32::from_bits(value as u32),
            0xfb => f64::from_bits(value) as f32,
            _ if tag >> 5 == 0 => value as f32,
            _ if tag >> 5 == 1 => -1.0 - value as f32,
            _ => return None,
        };
        (-1e8..=1e8).contains(&value).then_some(value)
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<ParticlePointCloud, String> {
    let decode = || {
        let mut reader = Reader(bytes);
        let root = reader.array()?;
        if root.is_some_and(|size| size != 3) {
            return None;
        }
        let (tag, size) = reader.head()?;
        if tag >> 5 != 3 || size != Some(10) || reader.bytes(10)? != b"QQ3D_SHAPE" {
            return None;
        }
        let (tag, version) = reader.head()?;
        if tag >> 5 != 0 || version != Some(1) {
            return None;
        }
        let length = reader.array()?;
        let declared_count = match length {
            Some(n) => {
                let count = usize::try_from(n / 3).ok()?;
                if n % 3 != 0 || !(1..=MAX_PARTICLE_POINT_COUNT).contains(&count) {
                    return None;
                }
                Some(count)
            }
            None => None,
        };
        // Do not reserve from an untrusted count larger than the input.
        let mut points = Vec::with_capacity(declared_count.unwrap_or(0).min(reader.0.len() / 3));
        while declared_count.map_or_else(
            || reader.0.first() != Some(&0xff),
            |count| points.len() < count,
        ) {
            if points.len() == MAX_PARTICLE_POINT_COUNT {
                return None;
            }
            points.push([
                reader.coordinate()?,
                reader.coordinate()?,
                reader.coordinate()?,
            ]);
        }
        reader.end_array(length)?;
        reader.end_array(root)?;
        reader.0.is_empty().then_some(())?;
        ParticlePointCloud::new(points)
    };
    decode().ok_or_else(|| {
        format!(
            "invalid QQ3D_SHAPE v1 CBOR; expected 1–{MAX_PARTICLE_POINT_COUNT} finite XYZ positions"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_shape_loads_qt_streamed_arrays_and_rejects_truncated_or_invalid_files() {
        let mut bytes = b"\x9f\x6aQQ3D_SHAPE\x01\x9f".to_vec();
        for value in [0.25_f32, -1.5, 2.0] {
            bytes.push(0xfa);
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend([0xff, 0xff]);
        assert_eq!(
            decode(&bytes).expect("valid Qt shape").points(),
            &[[0.25, -1.5, 2.0]]
        );
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut wrong_version = bytes.clone();
        wrong_version[12] = 2;
        assert!(decode(&wrong_version).is_err());
        bytes[15..19].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(decode(&bytes).is_err());
    }
}
