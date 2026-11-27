struct XrTextureFootprint {
    step: vec2<f32>,
    lod: f32,
    taps: u32,
}

// The eigenvectors of J * transpose(J) give the footprint axes in texel space.
fn xr_anisotropic_footprint(
    dx: vec2<f32>, dy: vec2<f32>, size: vec2<f32>, levels: u32, maximum: u32,
) -> XrTextureFootprint {
    let x = clamp(dx, vec2(-1e12), vec2(1e12)) * size;
    let y = clamp(dy, vec2(-1e12), vec2(1e12)) * size;
    let components = max(abs(x), abs(y));
    let scale = max(max(components.x, components.y), 1e-10);
    let nx = x / scale;
    let ny = y / scale;
    let a = nx.x * nx.x + ny.x * ny.x;
    let b = nx.x * nx.y + ny.x * ny.y;
    let c = nx.y * nx.y + ny.y * ny.y;
    let major_squared = 0.5 * (a + c + length(vec2(a - c, 2.0 * b)));
    let normalized_major = sqrt(major_squared);
    let major = normalized_major * scale;
    if major <= 1.0 {
        return XrTextureFootprint(vec2(0.0), 0.0, 1u);
    }

    // The determinant gives the smaller singular value without subtracting
    // almost equal eigenvalues in a very elongated footprint.
    let minor = abs(nx.x * ny.y - nx.y * ny.x) * scale / normalized_major;
    let width = max(max(minor, 1.0), major / f32(maximum));
    let lod = log2(width);
    if major <= width * 1.01 || (levels > 1u && lod >= f32(levels - 1u)) {
        return XrTextureFootprint(vec2(0.0), lod, 1u);
    }

    let taps = min(u32(ceil(major / width)), maximum);
    let axis = normalize(select(vec2(b, major_squared - a), vec2(major_squared - c, b), a >= c));
    return XrTextureFootprint((axis / size) * (major / f32(taps)), lod, taps);
}

fn xr_footprint_uv(footprint: XrTextureFootprint, uv: vec2<f32>, tap: u32) -> vec2<f32> {
    return uv + (f32(tap) + 0.5 - 0.5 * f32(footprint.taps)) * footprint.step;
}

fn xr_sample_texture_footprint(image: texture_2d<f32>, filtering: sampler, uv: vec2<f32>,
                             dx: vec2<f32>, dy: vec2<f32>, maximum: u32) -> vec4<f32> {
    if maximum > 1u {
        let footprint = xr_anisotropic_footprint(dx, dy, vec2<f32>(textureDimensions(image)),
            textureNumLevels(image), maximum);
        if footprint.taps > 1u {
            var accumulated = vec4(0.0);
            for (var tap = 0u; tap < footprint.taps; tap += 1u) {
                accumulated += textureSampleLevel(image, filtering,
                    xr_footprint_uv(footprint, uv, tap), footprint.lod);
            }
            return accumulated / f32(footprint.taps);
        }
    }
    return textureSampleGrad(image, filtering, uv, dx, dy);
}
