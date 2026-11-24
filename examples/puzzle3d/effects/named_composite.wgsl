fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let sharp = scene_color(uv);
    let soft = scene_buffer_color(0u, uv);
    let accent_present = scene_has_buffer(1u) && all(scene_buffer_size(1u) > vec2(0.0));
    let accent = scene_buffer_color(1u, uv);
    let accent_tint = mix(vec3(0.04, 0.7, 0.9), accent.rgb, accent.a * 0.35);
    let tint = select(vec3(0.83, 0.25, 0.05), accent_tint, accent_present);
    let halo = max(soft.a - sharp.a, 0.0) * 0.88;
    let coverage = sharp.a + halo;
    return vec4((sharp.rgb * sharp.a + tint * halo) / max(coverage, 0.000001), coverage);
}
