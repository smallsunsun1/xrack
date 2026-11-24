fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    result.base_color = vec3(0.1, 0.35, 0.65);
    result.metallic = input.parameters.x;
    result.emissive = vec3(0.02, 0.08, 0.18);
    result.roughness = 0.4;
    return result;
}
