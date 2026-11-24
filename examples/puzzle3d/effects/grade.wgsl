fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let color = scene_color(uv);
    let settings = scene_effect_parameters();
    return vec4(mix(color.rgb, vec3(settings.y, 0.36, settings.z), settings.x), color.a);
}
