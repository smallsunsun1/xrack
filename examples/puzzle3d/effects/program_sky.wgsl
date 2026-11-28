struct SkyUniforms {
    palette: f32,
    sun_angle: f32,
    flat: bool,
    gain: f32,
    tint: vec4<f32>,
    texture_transform: mat3x3<f32>,
    lobe_exponent: u32,
}

fn scene_sky(direction: vec3<f32>, _parameters: vec3<f32>) -> vec3<f32> {
    let controls = sky_uniforms();
    var horizon = mix(vec3(0.63, 0.28, 0.08), vec3(0.06, 0.3, 0.64), controls.palette);
    if sky_has_texture(0u) {
        let uv = vec2(0.5 + 0.49 * direction.x, 0.5 - 0.49 * direction.y);
        let mapped = (controls.texture_transform * vec3(uv, 1.0)).xy;
        horizon = mix(horizon, sky_texture(0u, mapped).rgb, 0.8);
    }
    if sky_has_cubemap(1u) {
        horizon = mix(horizon, sky_cubemap_lod(1u, direction, 1.0).rgb, 0.5);
    }
    if sky_has_volume(2u) {
        horizon = mix(horizon, sky_volume(2u, direction * 0.5 + 0.5).rgb, 0.55);
    }
    let exposure = controls.gain * controls.tint.rgb;
    if controls.flat {
        return horizon * exposure;
    }
    let zenith = mix(vec3(0.19, 0.045, 0.15), vec3(0.02, 0.10, 0.38), controls.palette);
    let ground = horizon * 0.16;
    let fade = abs(direction.y);
    let base = mix(horizon, select(ground, zenith, direction.y > 0.0), sqrt(fade));
    let sun = normalize(vec3(sin(controls.sun_angle), 0.22, -cos(controls.sun_angle)));
    let alignment = max(dot(direction, sun), 0.0);
    let glow = 0.8 * pow(alignment, 24.0) + 5.0 * pow(alignment, f32(max(controls.lobe_exponent, 1u)));
    return (base + vec3(1.0, 0.7, 0.44) * glow) * exposure;
}
