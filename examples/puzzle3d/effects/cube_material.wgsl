const CUBE_DIRECTIONS = array(
    vec3(1.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0),
    vec3(0.0, 1.0, 0.0), vec3(0.0, -1.0, 0.0),
    vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0),
);

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    if input.parameters.z != 0.0 {
        result.base_color = material_texture(0u, vec2(0.35, 0.5)).rgb;
    } else if material_has_cubemap(1u) && all(material_texture_size(1u) == vec2(16u)) {
        let direction = CUBE_DIRECTIONS[min(u32(input.parameters.x), 5u)];
        let color = material_cubemap_lod(1u, direction, input.parameters.y);
        result.base_color = color.rgb * color.a;
    } else {
        result.base_color = vec3(0.025);
    }
    return result;
}
