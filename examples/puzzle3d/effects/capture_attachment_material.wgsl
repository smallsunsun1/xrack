fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let data = material_texture(0u, input.uv0);
    if !material_has_texture(0u) {
        result.base_color = vec3(0.035, 0.055, 0.07);
    } else if input.parameters.x > 0.5 {
        result.base_color = select(vec3(0.035, 0.055, 0.07),
            data.xyz * 0.5 + 0.5, dot(data.xyz, data.xyz) > 0.5);
    } else {
        result.base_color = vec3(data.x);
    }
    return result;
}
