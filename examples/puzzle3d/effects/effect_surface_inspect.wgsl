fn inspect(uv: vec2<f32>) -> XrEffectSurface {
    return scene_surface(uv);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let surface = inspect(uv);
    if !surface.valid {
        return scene_color(uv);
    }
    let mode = scene_effect_parameters().x;
    if mode == 1.0 {
        let reduced = scene_buffer_color(0u, uv).r;
        let agrees = abs(reduced - surface.roughness) <= 0.003;
        return vec4(surface.roughness, reduced, select(1.0, 0.0, agrees), 1.0);
    }
    if mode == 2.0 {
        let location = surface.position * vec3(0.16, 0.16, 0.5) + vec3(0.5, 0.5, 0.2);
        return vec4(clamp(location, vec3(0.0), vec3(1.0)), 1.0);
    }
    return vec4(surface.normal * 0.5 + 0.5, 1.0);
}
