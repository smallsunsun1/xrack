use std::fmt::Write;

use super::material_shader::MAX_TEXTURES;

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyTextureKind {
    Image,
    Cubemap,
    Volume,
}

#[doc(hidden)]
pub fn sky_texture_interface(kinds: [Option<SkyTextureKind>; MAX_TEXTURES]) -> String {
    let mut source = String::new();
    for (slot, kind) in kinds.into_iter().enumerate() {
        let dimension = match kind {
            Some(SkyTextureKind::Image) => "2d",
            Some(SkyTextureKind::Cubemap) => "cube",
            Some(SkyTextureKind::Volume) => "3d",
            None => continue,
        };
        let _ = writeln!(
            source,
            "@group(1) @binding({}) var xr_sky_input_{slot}: texture_{dimension}<f32>;\n@group(1) @binding({}) var xr_sky_sampler_{slot}: sampler;",
            2 * slot,
            2 * slot + 1,
        );
    }
    for (name, dimension, kind, coordinate) in [
        ("texture", 2, SkyTextureKind::Image, "uv"),
        ("cubemap", 3, SkyTextureKind::Cubemap, "direction"),
        ("volume", 3, SkyTextureKind::Volume, "uvw"),
    ] {
        let available = kinds
            .into_iter()
            .enumerate()
            .filter(|(_, slot)| *slot == Some(kind))
            .fold(0u32, |bitmap, (index, _)| bitmap | 1 << index);
        let _ = writeln!(
            source,
            "fn sky_has_{name}(index: u32) -> bool {{ return index < {MAX_TEXTURES}u && ({available}u & (1u << index)) != 0u; }}\n\
            fn sky_{name}(index: u32, {coordinate}: vec{dimension}<f32>) -> vec4<f32> {{ return sky_{name}_lod(index, {coordinate}, 0.0); }}\n\
            fn sky_{name}_lod(index: u32, {coordinate}: vec{dimension}<f32>, lod: f32) -> vec4<f32> {{\n            if !all(abs({coordinate}) <= vec{dimension}(1e20)) || !(abs(lod) <= 1e20) {{ return vec4(0.0); }}\n            switch index {{"
        );
        for (slot, shape) in kinds.into_iter().enumerate() {
            if shape == Some(kind) {
                let _ = writeln!(
                    source,
                    "case {slot}u: {{ return textureSampleLevel(xr_sky_input_{slot}, xr_sky_sampler_{slot}, {coordinate}, lod); }}"
                );
            }
        }
        source.push_str("default: { return vec4(0.0); }\n}\n}\n");
    }
    source.push_str("fn sky_texture_size(index: u32) -> vec2<u32> {\nswitch index {\n");
    for (slot, kind) in kinds.into_iter().enumerate() {
        if kind.is_some() {
            let _ = writeln!(
                source,
                "case {slot}u: {{ return textureDimensions(xr_sky_input_{slot}).xy; }}"
            );
        }
    }
    source.push_str("default: { return vec2(0u); }\n}\n}\n");
    source
}
