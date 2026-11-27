const CUBE_DIRECTIONS = array(
    vec3(1.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0),
    vec3(0.0, 1.0, 0.0), vec3(0.0, -1.0, 0.0),
    vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0),
);

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let parameters = scene_effect_parameters();
    if parameters.z != 0.0 {
        return vec4(scene_buffer_color(0u, vec2(0.35, 0.5)).rgb, 1.0);
    }
    if !scene_has_cubemap(1u) || any(scene_buffer_size(1u) != vec2(16.0)) {
        return vec4(vec3(0.025), 1.0);
    }
    let direction = CUBE_DIRECTIONS[min(u32(parameters.x), 5u)];
    let color = scene_buffer_cubemap_lod(1u, direction, parameters.y);
    return vec4(color.rgb * color.a, 1.0);
}
