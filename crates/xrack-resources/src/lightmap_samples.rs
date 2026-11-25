//! Streaming offline format for linear irradiance and its denoising guides.
use std::{
    collections::BTreeSet,
    io::{Read, Write},
};

use nalgebra::Vector3;
use xrack_runtime::scene3d::LightmapMode;

use crate::lightmap_bake::{
    AtlasBuilder, BakeError, BakeOptions, BakeTile, BakedAtlas, MAX_DIMENSION, MAX_RESOLUTION,
    MIN_RESOLUTION, PADDING, ShadedTexel,
};

const MAGIC: &[u8; 8] = b"XRLM\x01\0\0\0";
const EMPTY_TEXEL: u32 = u32::MAX;
const GUIDE_BYTES: usize = 13 * 4;
const MAX_ID_BYTES: usize = xrack_assets::MAX_ASSET_BYTES as usize;

pub(crate) fn write_header(
    writer: &mut dyn Write,
    [width, height]: [u32; 2],
    count: usize,
    options: BakeOptions,
) -> Result<(), BakeError> {
    let mode = match options.mode {
        LightmapMode::Indirect => 0,
        LightmapMode::Diffuse => 1,
        LightmapMode::Mixed => 2,
    };
    writer.write_all(MAGIC)?;
    write_words(
        writer,
        &[
            width,
            height,
            count as u32,
            mode,
            options.samples,
            u32::from(options.bounces),
            options.bias.to_bits(),
            u32::from(options.denoise_passes),
            options.indirect_factor.to_bits(),
        ],
    )?;
    Ok(())
}

pub(crate) fn write_tile(
    writer: &mut dyn Write,
    tile: &BakeTile,
    shaded: &[Option<ShadedTexel>],
) -> Result<(), BakeError> {
    if tile.id.is_empty() || tile.id.len() > MAX_ID_BYTES {
        return Err(BakeError::InvalidSamples(
            "receiver ID has an unsupported length",
        ));
    }
    write_words(
        writer,
        &[
            tile.id.len() as u32,
            tile.origin[0],
            tile.origin[1],
            tile.resolution,
        ],
    )?;
    writer.write_all(tile.id.as_bytes())?;
    for texel in shaded {
        let Some(value) = texel else {
            writer.write_all(&EMPTY_TEXEL.to_le_bytes())?;
            continue;
        };
        let object = u32::try_from(value.object)
            .ok()
            .filter(|&object| object != EMPTY_TEXEL)
            .ok_or(BakeError::InvalidSamples("object index is out of range"))?;
        let [px, py, pz] = value.position;
        let [nx, ny, nz] = value.normal;
        let [dr, dg, db] = value.direct.into();
        let [ir, ig, ib] = value.indirect.into();
        let words = [px, py, pz, nx, ny, nz, value.radius, dr, dg, db, ir, ig, ib];
        validate_guide(words)?;
        let mut bytes = [0; GUIDE_BYTES + 4];
        bytes[..4].copy_from_slice(&object.to_le_bytes());
        for (chunk, word) in bytes[4..].chunks_exact_mut(4).zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        writer.write_all(&bytes)?;
    }
    Ok(())
}

/// Rebuild an atlas from unfiltered samples and the requested postprocessing settings.
pub fn replay_lightmap_samples(
    reader: &mut dyn Read,
    denoise_passes: Option<u8>,
    indirect_factor: Option<f32>,
) -> Result<(BakedAtlas, BakeOptions), BakeError> {
    let (size, count, mut options) = read_header(reader)?;
    let had_indirect = options.indirect_factor > 0.0;
    options.denoise_passes = denoise_passes.unwrap_or(options.denoise_passes);
    options.indirect_factor = indirect_factor.unwrap_or(options.indirect_factor);
    options.validate()?;
    if !had_indirect && options.indirect_factor > 0.0 {
        return Err(BakeError::MissingIndirectSamples);
    }
    let mut atlas = AtlasBuilder::new(size, count);
    let mut occupied = vec![false; (size[0] * size[1]) as usize];
    let mut ids = BTreeSet::new();
    for _ in 0..count {
        let tile = read_tile(reader, size, &mut occupied)?;
        if !ids.insert(tile.id.clone()) {
            return Err(BakeError::InvalidSamples("duplicate receiver ID"));
        }
        let side = tile.resolution + 2 * PADDING;
        let shaded = (0..side * side)
            .map(|_| read_texel(reader))
            .collect::<Result<Vec<_>, _>>()?;
        atlas.place(tile, &shaded, options)?;
    }
    if reader.read(&mut [0; 1])? != 0 {
        return Err(BakeError::InvalidSamples("trailing data"));
    }
    Ok((atlas.finish(options.mode)?, options))
}

fn read_header(reader: &mut dyn Read) -> Result<([u32; 2], usize, BakeOptions), BakeError> {
    let mut magic = [0; MAGIC.len()];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(BakeError::InvalidSamples("unknown format or version"));
    }
    let width = read_word(reader)?;
    let height = read_word(reader)?;
    let count = read_word(reader)?;
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
        return Err(BakeError::InvalidSamples(
            "atlas dimensions are out of range",
        ));
    }
    let minimum = (MIN_RESOLUTION + 2 * PADDING).pow(2);
    if count == 0 || count > width * height / minimum {
        return Err(BakeError::InvalidSamples("invalid receiver count"));
    }
    let mode = match read_word(reader)? {
        0 => LightmapMode::Indirect,
        1 => LightmapMode::Diffuse,
        2 => LightmapMode::Mixed,
        _ => return Err(BakeError::InvalidSamples("unknown lighting mode")),
    };
    let options = BakeOptions {
        mode,
        samples: read_word(reader)?,
        bounces: u8::try_from(read_word(reader)?)
            .map_err(|_| BakeError::InvalidSamples("invalid bounce count"))?,
        bias: f32::from_bits(read_word(reader)?),
        denoise_passes: u8::try_from(read_word(reader)?)
            .map_err(|_| BakeError::InvalidSamples("invalid denoise count"))?,
        indirect_factor: f32::from_bits(read_word(reader)?),
        ..BakeOptions::default()
    };
    options.validate()?;
    Ok(([width, height], count as usize, options))
}

fn read_tile(
    reader: &mut dyn Read,
    [width, height]: [u32; 2],
    occupied: &mut [bool],
) -> Result<BakeTile, BakeError> {
    let length = read_word(reader)? as usize;
    let x = read_word(reader)?;
    let y = read_word(reader)?;
    let resolution = read_word(reader)?;
    if length == 0 || length > MAX_ID_BYTES {
        return Err(BakeError::InvalidSamples("invalid receiver ID length"));
    }
    if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&resolution) {
        return Err(BakeError::InvalidSamples(
            "receiver resolution is out of range",
        ));
    }
    let side = resolution + 2 * PADDING;
    if x.checked_add(side).is_none_or(|right| right > width)
        || y.checked_add(side).is_none_or(|bottom| bottom > height)
    {
        return Err(BakeError::InvalidSamples("receiver lies outside the atlas"));
    }
    for row in y..y + side {
        let start = (row * width + x) as usize;
        let span = &mut occupied[start..start + side as usize];
        if span.iter().any(|used| *used) {
            return Err(BakeError::InvalidSamples("overlapping receiver regions"));
        }
        span.fill(true);
    }
    let mut id = vec![0; length];
    reader.read_exact(&mut id)?;
    let id =
        String::from_utf8(id).map_err(|_| BakeError::InvalidSamples("receiver ID is not UTF-8"))?;
    Ok(BakeTile {
        id,
        origin: [x, y],
        resolution,
    })
}

fn read_texel(reader: &mut dyn Read) -> Result<Option<ShadedTexel>, BakeError> {
    let object = read_word(reader)?;
    if object == EMPTY_TEXEL {
        return Ok(None);
    }
    let mut bytes = [0; GUIDE_BYTES];
    reader.read_exact(&mut bytes)?;
    let words = bytes.as_chunks::<4>().0;
    let guide = std::array::from_fn(|i| f32::from_le_bytes(words[i]));
    validate_guide(guide)?;
    let [px, py, pz, nx, ny, nz, radius, dr, dg, db, ir, ig, ib] = guide;
    Ok(Some(ShadedTexel {
        position: [px, py, pz],
        normal: [nx, ny, nz],
        object: object as usize,
        radius,
        direct: Vector3::new(dr, dg, db),
        indirect: Vector3::new(ir, ig, ib),
    }))
}

fn validate_guide(guide: [f32; 13]) -> Result<(), BakeError> {
    if guide.iter().any(|value| !value.is_finite())
        || guide[6] <= 0.0
        || guide[7..].iter().any(|&value| value < 0.0)
    {
        return Err(BakeError::InvalidSamples(
            "invalid geometric guide or irradiance",
        ));
    }
    Ok(())
}

fn write_words(writer: &mut dyn Write, words: &[u32]) -> Result<(), BakeError> {
    for word in words {
        writer.write_all(&word.to_le_bytes())?;
    }
    Ok(())
}

fn read_word(reader: &mut dyn Read) -> Result<u32, BakeError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_rejects_out_of_bounds_and_overlapping_tiles() -> Result<(), BakeError> {
        let options = BakeOptions::default();
        let side = MIN_RESOLUTION + 2 * PADDING;
        let shaded = vec![
            Some(ShadedTexel {
                position: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
                object: 0,
                radius: 1.0,
                direct: Vector3::repeat(0.25),
                indirect: Vector3::zeros(),
            });
            (side * side) as usize
        ];
        for (origin, error) in [
            ([1, 0], "overlapping receiver regions"),
            ([u32::MAX, 0], "receiver lies outside the atlas"),
        ] {
            let mut bytes = Vec::new();
            write_header(&mut bytes, [side * 2, side], 2, options)?;
            write_tile(
                &mut bytes,
                &BakeTile {
                    id: "first".into(),
                    origin: [0, 0],
                    resolution: MIN_RESOLUTION,
                },
                &shaded,
            )?;
            write_tile(
                &mut bytes,
                &BakeTile {
                    id: "second".into(),
                    origin,
                    resolution: MIN_RESOLUTION,
                },
                &shaded,
            )?;
            assert!(matches!(
                replay_lightmap_samples(&mut bytes.as_slice(), None, None),
                Err(BakeError::InvalidSamples(reason)) if reason == error,
            ));
        }
        Ok(())
    }
}
