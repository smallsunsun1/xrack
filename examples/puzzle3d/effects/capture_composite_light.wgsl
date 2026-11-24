fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    // The lighting rectangle covers the entire orthographic camera image.
    let uv = (input.uv0 - 0.5) * 1.05 + 0.5;
    let point = material_texture_world_position(0u, uv);
    if input.parameters.z < -1.5 {
        result.base_color = vec3(
            material_texture(0u, uv).x,
            select(0.2, 0.8, point.valid),
            select(0.1, 0.6, point.valid),
        );
        return result;
    }
    if !point.valid {
        result.base_color = vec3(0.012, 0.027, 0.05);
        return result;
    }
    if input.parameters.z < 0.0 {
        result.base_color = point.position * 0.125 + vec3(0.5, 0.5, 0.0);
        return result;
    }
    let normal_roughness = material_texture(1u, uv);
    let base_metalness = material_texture(2u, uv);
    let emission_occlusion = material_texture(3u, uv);
    let light_delta = input.parameters - point.position;
    let distance_squared = dot(light_delta, light_delta);
    let light = normalize(light_delta);
    let halfway = normalize(light + vec3(0.0, 0.0, 1.0));
    let diffuse = base_metalness.rgb * (1.0 - base_metalness.a);
    let specular = mix(vec3(0.04), base_metalness.rgb, base_metalness.a);
    let gloss = pow(max(dot(normal_roughness.rgb, halfway), 0.0), mix(80.0, 4.0, normal_roughness.a));
    let direct = (diffuse + specular * gloss) * max(dot(normal_roughness.rgb, light), 0.0) * 9.0 / (1.0 + distance_squared);
    result.base_color = base_metalness.rgb * (0.24 * emission_occlusion.a) + direct + emission_occlusion.rgb;
    return result;
}
