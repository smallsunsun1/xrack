fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if uv.y >= 0.81 {
        return scene_color(uv);
    }
    let scene_uv = vec2(uv.x, uv.y / 0.81);
    if !scene_has_buffer(3u) {
        let emission = scene_buffer_color(0u, scene_uv);
        return vec4(emission.rgb, 1.0);
    }

    let point = scene_buffer_world_position(0u, scene_uv);
    if !point.valid {
        return scene_color(uv);
    }

    let normal_roughness = scene_buffer_color(1u, scene_uv);
    let base_metalness = scene_buffer_color(2u, scene_uv);
    let emission_occlusion = scene_buffer_color(3u, scene_uv);
    let normal = normalize(normal_roughness.xyz);
    let to_light = scene_effect_parameters() - point.position;
    let distance_squared = dot(to_light, to_light);
    let light = to_light * inverseSqrt(max(distance_squared, 0.0001));
    let halfway = normalize(light + vec3(0.0, 0.0, 1.0));
    let facing = max(dot(normal, light), 0.0);
    let diffuse = base_metalness.rgb * (1.0 - base_metalness.a);
    let specular_color = mix(vec3(0.04), base_metalness.rgb, base_metalness.a);
    let highlight = pow(max(dot(normal, halfway), 0.0), mix(80.0, 4.0, normal_roughness.a));
    let ambient = 0.2 * emission_occlusion.a * base_metalness.rgb;
    let direct = (diffuse + specular_color * highlight) * facing * 7.0 / (1.0 + distance_squared);
    return vec4(ambient + direct + emission_occlusion.rgb, 1.0);
}
