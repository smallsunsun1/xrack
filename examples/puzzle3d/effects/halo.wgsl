fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let source = scene_original_color(uv);
    let blurred = scene_color(uv);
    let halo = max(blurred.a - source.a, 0.0) * 0.86;
    let coverage = source.a + halo;
    let tint = vec3(0.05, 0.63, 0.94);
    let color = source.rgb * source.a + tint * halo;
    return vec4(color / max(coverage, 0.000001), coverage);
}
