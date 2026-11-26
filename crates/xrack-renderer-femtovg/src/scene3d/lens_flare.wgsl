struct Settings {
    // Ghost count, ghost spacing, halo radius, halo gain.
    ghosts: vec4<f32>,
    // Chromatic shift in source texels, aspect correction, sine, cosine.
    optics: vec4<f32>,
    // Output width and height, viewport aspect.
    extent: vec4<f32>,
    maps: vec4<f32>,
}

@group(0) @binding(0) var highlights: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;
@group(0) @binding(3) var lens_colors: texture_2d<f32>;
@group(0) @binding(4) var dirt: texture_2d<f32>;
@group(0) @binding(5) var starburst: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let point = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(point * 2.0 - 1.0, 0.0, 1.0);
}

fn read_highlight(uv: vec2<f32>) -> vec3<f32> {
    let edge = min(uv, 1.0 - uv);
    let weight = smoothstep(0.0, 0.025, min(edge.x, edge.y));
    return textureSampleLevel(highlights, filtering, uv, 0.0).rgb * weight;
}

fn read_optics(uv: vec2<f32>, split: vec2<f32>) -> vec3<f32> {
    let center = read_highlight(uv);
    if settings.optics.x <= 0.0 {
        return center;
    }
    return vec3<f32>(read_highlight(uv + split).r, center.g, read_highlight(uv - split).b);
}

fn tint(radial: f32) -> vec3<f32> {
    if settings.maps.x < 0.5 {
        return vec3<f32>(1.0);
    }
    let encoded = textureSampleLevel(lens_colors, filtering, vec2<f32>(radial, 0.5), 0.0).rgb;
    return select(pow((encoded + 0.055) / 1.055, vec3<f32>(2.4)),
        encoded / 12.92, encoded <= vec3<f32>(0.04045));
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / settings.extent.xy;
    let aspect = select(1.0, settings.extent.z, settings.optics.y > 0.5);
    let axes = vec2<f32>(aspect, 1.0);
    let offset = uv - vec2<f32>(0.5);
    let radial = offset * axes;
    let direction = radial / max(length(radial), 0.00001);
    let split = direction * settings.optics.x / vec2<f32>(textureDimensions(highlights));
    let mirror = vec2<f32>(0.5) - offset;
    let ghost_count = u32(settings.ghosts.x);
    var color = vec3<f32>(0.0);
    for (var ghost = 0u; ghost < ghost_count; ghost += 1u) {
        let index = f32(ghost);
        let at = mirror - offset * (settings.ghosts.y * index);
        let radius = clamp(length(at - 0.5) * 1.414214, 0.0, 1.0);
        let aperture = (1.0 - radius) * (1.0 - radius);
        color += read_optics(at, split) * tint(radius) * aperture / (1.0 + 0.52 * index);
    }
    color /= sqrt(max(f32(ghost_count), 1.0));
    if settings.ghosts.w > 0.0 {
        let at = mirror + direction / axes * settings.ghosts.z;
        let radius = clamp(length(at - 0.5) * 1.414214, 0.0, 1.0);
        let mask = smoothstep(0.0, 0.08, length(radial)) * (1.0 - radius);
        color += read_optics(at, split) * tint(radius) * (settings.ghosts.w * mask);
    }
    if settings.maps.y > 0.5 {
        color *= textureSampleLevel(dirt, filtering, uv, 0.0).rgb;
    }
    if settings.maps.z > 0.5 {
        let sin = settings.optics.z;
        let cos = settings.optics.w;
        let rotated = vec2<f32>(cos * radial.x - sin * radial.y, sin * radial.x + cos * radial.y);
        let at = rotated / axes + 0.5;
        color *= textureSampleLevel(starburst, filtering, at, 0.0).rgb;
    }
    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
