fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let right = select(vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), input.parameters.x > 0.5);
    result.base_color = select(vec3(1.0, 0.0, 0.0), right, input.world_position.x >= 0.0);
    return result;
}
